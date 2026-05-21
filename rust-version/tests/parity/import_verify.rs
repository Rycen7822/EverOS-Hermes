use super::*;
#[test]
fn mcp_import_and_verify_dry_run_reports_warnings_without_http() {
    let _guard = ENV_LOCK.lock().unwrap();
    set_env("EVEROS_API_KEY", "test-key");
    set_env("EVEROS_USER_ID", "u1");
    remove_env("EVEROS_BASE_URL");

    let raw = everos_hermes_rust::mcp::call_tool(
        "everos_import_and_verify",
        json!({
            "scope":"agent",
            "dry_run":true,
            "messages":[
                {"role":"user","content":"Alpha","timestamp":1},
                {"role":"user","content":"Alpha","timestamp":2},
                {"role":"tool","content":"missing id","timestamp":3},
                {"role":"user","content":"ISO timestamp","timestamp":"2026-05-13T00:00:00Z"}
            ]
        }),
    )
    .unwrap();
    let response: Value = serde_json::from_str(&raw).unwrap();

    assert_eq!(response["status"], "dry_run");
    assert_eq!(response["queued_count"], 0);
    assert!(!response["warnings"].as_array().unwrap().is_empty());

    remove_env("EVEROS_API_KEY");
    remove_env("EVEROS_USER_ID");
}

#[test]
fn mcp_import_and_verify_batches_flushes_and_verifies() {
    let _guard = ENV_LOCK.lock().unwrap();
    let (base_url, handle) = sequenced_status_request_server(
        vec![
            (403, json!({"message":"Forbidden"})),
            (200, json!({"data":{"status":"queued","task_id":"task-1"}})),
            (200, json!({"data":{"status":"queued","task_id":"task-2"}})),
            (200, json!({"data":{"status":"success"}})),
            (
                200,
                json!({"data":{"profiles":[{"id":"p1","profile_data":{"explicit_info":"Alpha"}}]}}),
            ),
        ],
        500,
    );
    set_client_env(&base_url);

    let raw = everos_hermes_rust::mcp::call_tool(
        "everos_import_and_verify",
        json!({
            "session_id":"sess-batch",
            "batch_size":4,
            "flush":true,
            "verification_queries":["Alpha"],
            "messages":[
                {"role":"user","content":"Alpha","timestamp":1},
                {"role":"assistant","content":"Beta","timestamp":2},
                {"role":"user","content":"Gamma","timestamp":3},
                {"role":"assistant","content":"Delta","timestamp":4}
            ]
        }),
    )
    .unwrap();
    let response: Value = serde_json::from_str(&raw).unwrap();
    let requests = handle.join().unwrap();

    assert_eq!(response["status"], "verified");
    assert_eq!(response["queued_count"], 4);
    assert_eq!(response["failed_count"], 0);
    assert_eq!(response["split_count"], 1);
    assert_eq!(requests.len(), 5);
    assert_eq!(
        parse_http_body(&requests[0])["messages"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
    assert_eq!(
        parse_http_body(&requests[1])["messages"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        parse_http_body(&requests[2])["messages"]
            .as_array()
            .unwrap()
            .len(),
        2
    );

    clear_client_env();
}
#[test]
fn mcp_import_and_verify_rejects_invalid_batch_size_and_top_k_before_http() {
    let _guard = ENV_LOCK.lock().unwrap();
    set_client_env("http://127.0.0.1:9");

    for (extra, expected) in [
        (json!({"batch_size":0}), "batch_size"),
        (json!({"top_k":101}), "top_k"),
    ] {
        let mut args = json!({
            "messages":[{"role":"user","content":"Alpha","timestamp":1}],
            "flush":false,
            "verification_queries":[]
        });
        args.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        let raw = everos_hermes_rust::mcp::call_tool("everos_import_and_verify", args).unwrap();
        let response: Value = serde_json::from_str(&raw).unwrap();

        assert_eq!(response["ok"], false);
        assert_eq!(response["error_code"], "validation_failed");
        assert!(response["warnings"].to_string().contains(expected));
    }

    clear_client_env();
}

#[test]
fn mcp_verify_session_ingest_is_read_only_and_reports_misses() {
    let _guard = ENV_LOCK.lock().unwrap();
    let (base_url, handle) = sequenced_request_server(
        vec![
            json!({"data":{"episodes":[{"id":"ep1","summary":"found"}]}}),
            json!({"data":{"episodes":[]}}),
        ],
        500,
    );
    set_client_env(&base_url);

    let raw = everos_hermes_rust::mcp::call_tool(
        "everos_verify_session_ingest",
        json!({"session_id":"sess-readonly","verification_queries":["found","missing"]}),
    )
    .unwrap();
    let response: Value = serde_json::from_str(&raw).unwrap();
    let requests = handle.join().unwrap();

    assert_eq!(response["ok"], true);
    assert_eq!(response["workflow"], "verify_session_ingest");
    assert_eq!(response["status"], "partially_verified");
    assert_eq!(response["verified"], false);
    assert_eq!(requests.len(), 2);
    assert!(
        requests
            .iter()
            .all(|raw| raw.starts_with("POST /api/v1/memories/search "))
    );

    clear_client_env();
}

#[test]
fn mcp_verify_session_ingest_agent_scope_reuses_agent_memory_search_and_compacts_visibility_checks()
{
    let _guard = ENV_LOCK.lock().unwrap();
    let (base_url, handle) = sequenced_request_server(
        vec![
            json!({"data":{"agent_memory":[{"id":"agent-1","content":"found"}]}}),
            json!({"data":{"agent_memory":[{"id":"duplicate-agent-1"}]}}),
            json!({"data":{"agent_cases":[]}}),
            json!({"data":{"agent_skills":[]}}),
        ],
        500,
    );
    set_client_env(&base_url);

    let raw = everos_hermes_rust::mcp::call_tool(
        "everos_verify_session_ingest",
        json!({
            "session_id":"sess-agent",
            "verification_queries":["found"],
            "memory_types":["agent_memory"],
            "scope":"agent"
        }),
    )
    .unwrap();
    let response: Value = serde_json::from_str(&raw).unwrap();
    let requests = handle.join().unwrap();
    let visibility_checks = response["agent_visibility"]["agent_visibility_checks"]
        .as_array()
        .unwrap();

    assert_eq!(requests.len(), 3);
    assert_eq!(
        response["agent_visibility"]["agent_visibility_status"],
        "partial"
    );
    assert_eq!(
        visibility_checks[0]["memory_types"],
        json!(["agent_memory"])
    );

    clear_client_env();
}
