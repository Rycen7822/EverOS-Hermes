use super::*;

#[test]
fn mcp_save_and_verify_queues_flushes_and_searches() {
    let _guard = ENV_LOCK.lock().unwrap();
    let (base_url, handle) = sequenced_request_server(
        vec![
            json!({"data":{"status":"queued","task_id":"task-save"}}),
            json!({"data":{"status":"success","task_id":"task-flush"}}),
            json!({"data":{"episodes":[{"id":"ep1","summary":"espresso preference"}]}}),
        ],
        500,
    );
    set_env("EVEROS_API_KEY", "test-key");
    set_env("EVEROS_USER_ID", "u1");
    set_env("EVEROS_BASE_URL", &base_url);

    let raw = everos_hermes_rust::mcp::call_tool(
        "everos_save_and_verify",
        json!({
            "content":"User prefers espresso.",
            "session_id":"sess-verify",
            "verification_query":"espresso preference",
            "flush":true,
            "top_k":3
        }),
    )
    .unwrap();
    let response: Value = serde_json::from_str(&raw).unwrap();
    let requests = handle.join().unwrap();
    let paths: Vec<&str> = requests
        .iter()
        .map(|raw| raw.lines().next().unwrap_or(""))
        .collect();

    assert_eq!(response["ok"], true);
    assert_eq!(response["workflow"], "save_and_verify");
    assert_eq!(response["status"], "verified");
    assert_eq!(response["save"]["message_queued"], true);
    assert_eq!(response["verification"]["verified"], true);
    assert_eq!(response["verification"]["queries"][0]["hit_count"], 1);
    assert_eq!(
        paths,
        vec![
            "POST /api/v1/memories HTTP/1.1",
            "POST /api/v1/memories/flush HTTP/1.1",
            "POST /api/v1/memories/search HTTP/1.1",
        ]
    );
    assert_eq!(parse_http_body(&requests[2])["top_k"], 3);

    remove_env("EVEROS_API_KEY");
    remove_env("EVEROS_USER_ID");
    remove_env("EVEROS_BASE_URL");
}

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
            ],
            "verification_queries":["Alpha"]
        }),
    )
    .unwrap();
    let response: Value = serde_json::from_str(&raw).unwrap();

    assert_eq!(response["ok"], true);
    assert_eq!(response["workflow"], "import_and_verify");
    assert_eq!(response["status"], "dry_run");
    assert_eq!(response["input_count"], 4);
    assert_eq!(response["queued_count"], 0);
    assert_eq!(response["metrics"]["total_messages"], 4);
    assert_eq!(response["metrics"]["batch_count"], 1);
    assert!(
        response["metrics"]["estimated_payload_bytes"]
            .as_u64()
            .unwrap()
            > 0
    );
    let warnings = response["warnings"].as_array().unwrap();
    assert!(
        warnings
            .iter()
            .any(|warning| warning.as_str().unwrap().contains("duplicate"))
    );
    assert!(
        warnings
            .iter()
            .any(|warning| warning.as_str().unwrap().contains("tool_call_id"))
    );
    assert!(warnings.iter().any(|warning| {
        warning
            .as_str()
            .unwrap()
            .contains("timestamp must be an integer epoch millisecond value")
    }));

    remove_env("EVEROS_API_KEY");
    remove_env("EVEROS_USER_ID");
}

#[test]
fn mcp_import_and_verify_batches_flushes_and_verifies() {
    let _guard = ENV_LOCK.lock().unwrap();
    let (base_url, handle) = sequenced_request_server(
        vec![
            json!({"data":{"status":"queued","task_id":"task-1"}}),
            json!({"data":{"status":"queued","task_id":"task-2"}}),
            json!({"data":{"status":"success"}}),
            json!({"data":{"profiles":[{"id":"p1","profile_data":{"explicit_info":"Alpha"}}]}}),
        ],
        500,
    );
    set_env("EVEROS_API_KEY", "test-key");
    set_env("EVEROS_USER_ID", "u1");
    set_env("EVEROS_BASE_URL", &base_url);

    let raw = everos_hermes_rust::mcp::call_tool(
        "everos_import_and_verify",
        json!({
            "session_id":"sess-batch",
            "batch_size":2,
            "flush":true,
            "verification_queries":["Alpha"],
            "messages":[
                {"role":"user","content":"Alpha","timestamp":1},
                {"role":"assistant","content":"Beta","timestamp":2},
                {"role":"user","content":"Gamma","timestamp":3}
            ]
        }),
    )
    .unwrap();
    let response: Value = serde_json::from_str(&raw).unwrap();
    let requests = handle.join().unwrap();

    assert_eq!(response["ok"], true);
    assert_eq!(response["workflow"], "import_and_verify");
    assert_eq!(response["status"], "verified");
    assert_eq!(response["input_count"], 3);
    assert_eq!(response["queued_count"], 3);
    assert_eq!(response["batches"].as_array().unwrap().len(), 2);
    assert_eq!(requests.len(), 4);
    assert_eq!(
        parse_http_body(&requests[0])["messages"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        parse_http_body(&requests[1])["messages"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    remove_env("EVEROS_API_KEY");
    remove_env("EVEROS_USER_ID");
    remove_env("EVEROS_BASE_URL");
}

#[test]
fn mcp_import_and_verify_preserves_batch_payload_when_verification_has_error() {
    let _guard = ENV_LOCK.lock().unwrap();
    let (base_url, handle) = sequenced_status_request_server(
        vec![
            (
                200,
                json!({"data":{"status":"queued","task_id":"task-import"}}),
            ),
            (
                200,
                json!({"data":{"status":"success","task_id":"task-flush"}}),
            ),
            (
                500,
                json!({"message":"search failed token=import-verify-secret","request_id":"req-import-verify"}),
            ),
        ],
        500,
    );
    set_env("EVEROS_API_KEY", "test-key");
    set_env("EVEROS_USER_ID", "u1");
    set_env("EVEROS_BASE_URL", &base_url);

    let raw = everos_hermes_rust::mcp::call_tool(
        "everos_import_and_verify",
        json!({
            "session_id":"sess-import-verify",
            "batch_size":2,
            "flush":true,
            "verification_queries":["Alpha"],
            "messages":[
                {"role":"user","content":"Alpha","timestamp":1},
                {"role":"assistant","content":"Beta","timestamp":2}
            ]
        }),
    )
    .unwrap();
    let response: Value = serde_json::from_str(&raw).unwrap();
    let rendered = response.to_string();
    let requests = handle.join().unwrap();

    assert_eq!(requests.len(), 3);
    assert_eq!(response["ok"], true);
    assert_eq!(response["status"], "verification_error");
    assert_eq!(response["queued_count"], 2);
    assert_eq!(response["failed_count"], 0);
    assert_eq!(response["verification"]["ok"], false);
    assert_eq!(response["verification"]["status"], "error");
    assert_eq!(response["verification"]["verified"], false);
    assert!(!rendered.contains("import-verify-secret"));
    assert!(rendered.contains("[REDACTED]"));

    remove_env("EVEROS_API_KEY");
    remove_env("EVEROS_USER_ID");
    remove_env("EVEROS_BASE_URL");
}

#[test]
fn mcp_import_and_verify_splits_cloud_403_batches() {
    let _guard = ENV_LOCK.lock().unwrap();
    let (base_url, handle) = sequenced_status_request_server(
        vec![
            (403, json!({"detail":"Forbidden"})),
            (
                200,
                json!({"data":{"status":"queued","task_id":"task-left"}}),
            ),
            (
                200,
                json!({"data":{"status":"queued","task_id":"task-right"}}),
            ),
        ],
        500,
    );
    set_env("EVEROS_API_KEY", "test-key");
    set_env("EVEROS_USER_ID", "u1");
    set_env("EVEROS_BASE_URL", &base_url);

    let raw = everos_hermes_rust::mcp::call_tool(
        "everos_import_and_verify",
        json!({
            "session_id":"sess-split",
            "batch_size":4,
            "flush":false,
            "messages":[
                {"role":"user","content":"Alpha long","timestamp":1},
                {"role":"assistant","content":"Beta long","timestamp":2},
                {"role":"user","content":"Gamma long","timestamp":3},
                {"role":"assistant","content":"Delta long","timestamp":4}
            ]
        }),
    )
    .unwrap();
    let response: Value = serde_json::from_str(&raw).unwrap();
    let requests = handle.join().unwrap();

    assert_eq!(response["ok"], true);
    assert_eq!(response["workflow"], "import_and_verify");
    assert_eq!(response["status"], "queued");
    assert_eq!(response["input_count"], 4);
    assert_eq!(response["queued_count"], 4);
    assert_eq!(response["failed_count"], 0);
    assert_eq!(response["split_count"], 1);
    assert_eq!(response["batches"].as_array().unwrap().len(), 3);
    assert_eq!(response["batches"][0]["split_reason"], "cloud_403");
    assert_eq!(response["batches"][1]["split_from"], 0);
    assert_eq!(response["batches"][2]["split_from"], 0);
    assert!(
        response["batches"]
            .as_array()
            .unwrap()
            .iter()
            .all(|batch| { batch["payload_bytes"].as_u64().unwrap_or(0) > 0 })
    );
    assert!(
        response["suggested_next_actions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|action| {
                action
                    .as_str()
                    .unwrap()
                    .contains("adaptive batch splitting")
            })
    );
    assert_eq!(requests.len(), 3);
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

    remove_env("EVEROS_API_KEY");
    remove_env("EVEROS_USER_ID");
    remove_env("EVEROS_BASE_URL");
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
    set_env("EVEROS_API_KEY", "test-key");
    set_env("EVEROS_USER_ID", "u1");
    set_env("EVEROS_BASE_URL", &base_url);

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

    remove_env("EVEROS_API_KEY");
    remove_env("EVEROS_USER_ID");
    remove_env("EVEROS_BASE_URL");
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
    set_env("EVEROS_API_KEY", "test-key");
    set_env("EVEROS_USER_ID", "u1");
    set_env("EVEROS_BASE_URL", &base_url);

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
    assert_eq!(visibility_checks.len(), 3);
    assert!(
        visibility_checks
            .iter()
            .all(|check| check.get("response").is_none())
    );
    assert_eq!(visibility_checks[0]["kind"], "search");
    assert_eq!(
        visibility_checks[0]["memory_types"],
        json!(["agent_memory"])
    );
    assert_eq!(visibility_checks[0]["hit_count"], 1);

    remove_env("EVEROS_API_KEY");
    remove_env("EVEROS_USER_ID");
    remove_env("EVEROS_BASE_URL");
}
