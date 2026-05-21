use super::*;
#[test]
fn mcp_agent_save_add_flush_return_unchecked_visibility() {
    let _guard = ENV_LOCK.lock().unwrap();
    let (base_url, handle) = sequenced_request_server(
        vec![
            json!({"data":{"status":"queued","task_id":"task-save-agent"}}),
            json!({"data":{"status":"queued","task_id":"task-add-agent"}}),
            json!({"data":{"status":"success","task_id":"task-flush-agent"}}),
        ],
        500,
    );
    set_env("EVEROS_API_KEY", "test-key");
    set_env("EVEROS_BASE_URL", &base_url);
    set_env("EVEROS_USER_ID", "u1");

    let save_raw = everos_hermes_rust::mcp::call_tool(
        "everos_save_memory",
        json!({"content":"agent raw event","session_id":"sess-agent","scope":"agent","flush":false}),
    )
    .unwrap();
    let add_raw = everos_hermes_rust::mcp::call_tool(
        "everos_add_memories",
        json!({
            "messages":[{"role":"assistant","timestamp":1,"content":"agent batch event"}],
            "session_id":"sess-agent",
            "scope":"agent",
            "flush":true
        }),
    )
    .unwrap();
    let requests = handle.join().unwrap();
    let save: Value = serde_json::from_str(&save_raw).unwrap();
    let add: Value = serde_json::from_str(&add_raw).unwrap();

    assert_eq!(
        save["agent_visibility"]["agent_visibility_status"],
        "unchecked"
    );
    assert_eq!(save["agent_visibility"]["agent_raw_queued"], true);
    assert_eq!(
        parse_http_body(&requests[0])["messages"][0]["role"],
        "assistant"
    );
    assert_eq!(
        add["agent_visibility"]["agent_visibility_status"],
        "unchecked"
    );
    assert_eq!(add["agent_visibility"]["agent_flush"]["status"], "success");
    assert!(requests[2].starts_with("POST /api/v1/memories/agent/flush "));

    remove_env("EVEROS_API_KEY");
    remove_env("EVEROS_BASE_URL");
    remove_env("EVEROS_USER_ID");
}

#[test]
fn mcp_agent_flush_retries_transient_send_error_and_reports_visibility() {
    let _guard = ENV_LOCK.lock().unwrap();
    let (base_url, handle) =
        dropped_then_response_server(json!({"data":{"status":"success","task_id":"task-flush"}}));
    set_env("EVEROS_API_KEY", "test-key");
    set_env("EVEROS_BASE_URL", &base_url);
    set_env("EVEROS_USER_ID", "u1");

    let raw = everos_hermes_rust::mcp::call_tool(
        "everos_flush_memories",
        json!({"session_id":"sess-agent","scope":"agent","timeout":2.0}),
    )
    .unwrap();
    let response: Value = serde_json::from_str(&raw).unwrap();
    let requests = handle.join().unwrap();

    assert_eq!(requests.len(), 2);
    assert!(
        requests
            .iter()
            .all(|raw| raw.starts_with("POST /api/v1/memories/agent/flush "))
    );
    assert_eq!(response["flush"]["ok"], true);
    assert_eq!(response["flush"]["attempt_count"], 2);
    assert_eq!(
        response["agent_visibility"]["agent_visibility_status"],
        "unchecked"
    );

    remove_env("EVEROS_API_KEY");
    remove_env("EVEROS_BASE_URL");
    remove_env("EVEROS_USER_ID");
}

#[test]
fn mcp_save_and_verify_agent_scope_reports_structured_visibility() {
    let _guard = ENV_LOCK.lock().unwrap();
    let (base_url, handle) = sequenced_request_server(
        vec![
            json!({"data":{"status":"queued","task_id":"task-save"}}),
            json!({"data":{"status":"success","task_id":"task-flush"}}),
            json!({"data":{"episodes":[{"id":"ep1","summary":"agent note mirrored as personal search"}]}}),
            json!({"data":{"agent_memory":[]}}),
            json!({"data":{"agent_cases":[]}}),
            json!({"data":{"agent_skills":[]}}),
        ],
        500,
    );
    set_env("EVEROS_API_KEY", "test-key");
    set_env("EVEROS_USER_ID", "u1");
    set_env("EVEROS_BASE_URL", &base_url);

    let raw = everos_hermes_rust::mcp::call_tool(
        "everos_save_and_verify",
        json!({
            "content":"agent note",
            "session_id":"sess-agent",
            "scope":"agent",
            "verification_query":"agent note",
            "flush":true,
            "top_k":3
        }),
    )
    .unwrap();
    let response: Value = serde_json::from_str(&raw).unwrap();
    let requests = handle.join().unwrap();
    assert_eq!(response["status"], "agent_not_visible");
    assert_eq!(response["agent_visibility"]["agent_raw_queued"], true);
    assert_eq!(requests.len(), 6);
    assert!(requests[0].starts_with("POST /api/v1/memories/agent "));
    assert!(requests[1].starts_with("POST /api/v1/memories/agent/flush "));

    remove_env("EVEROS_API_KEY");
    remove_env("EVEROS_USER_ID");
    remove_env("EVEROS_BASE_URL");
}

#[test]
fn client_rejects_invalid_search_and_delete_contracts_before_request() {
    let (base_url, handle) = one_request_server(json!({"data":{"items":[]}}));
    let client = EverOSClient::new("test-key", &base_url, 0.05).unwrap();
    client
        .get_memories(
            Some("u"),
            None,
            None,
            "profile",
            1,
            20,
            "timestamp",
            " DESC ",
        )
        .unwrap();
    assert_eq!(
        parse_http_body(&handle.join().unwrap())["rank_order"],
        "desc"
    );
    assert!(
        client
            .search_memories(
                "q",
                Some("u"),
                None,
                None,
                "hybrid",
                None,
                -2,
                None,
                false,
                false,
                None
            )
            .is_err()
    );
    assert!(
        client
            .delete_memories(Some("mem-1"), Some("u"), None)
            .is_err()
    );
    assert!(client.delete_memories(None, None, Some("sess")).is_err());
}
