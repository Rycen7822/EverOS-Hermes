use super::*;

#[test]
fn response_normalization_contract_cases_match_python() {
    use everos_hermes_rust::response_normalization::response_summary;

    let contract = snapshot_json("response_normalization_cases.json");
    for case in contract["cases"].as_array().unwrap() {
        assert_eq!(
            response_summary(&case["response"]),
            case["expected_summary"]
        );
    }
}

#[test]
fn settings_validation_contract_cases_match_python() {
    let contract = snapshot_json("settings_validation_cases.json");
    for case in contract["cases"].as_array().unwrap() {
        let settings = case["settings"].clone();
        let strict = case["strict"].as_bool().unwrap();
        let valid = case["valid"].as_bool().unwrap();
        let (base_url, handle) = if valid {
            sequenced_request_server(vec![json!({"data":{"status":"updated"}})], 150)
        } else {
            sequenced_request_server(vec![json!({"data":{"should_not_send":true}})], 150)
        };
        let client = EverOSClient::new("test-key", &base_url, 0.2).unwrap();
        let result = client.update_settings(settings, strict, false);
        let requests = handle.join().unwrap();
        if valid {
            assert!(result.is_ok());
            assert_eq!(requests.len(), 1);
            assert!(requests[0].starts_with("PUT /api/v1/settings HTTP/1.1"));
            assert_eq!(parse_http_body(&requests[0]), case["settings"]);
        } else {
            assert!(result.is_err());
            assert!(requests.is_empty());
        }
    }
}
#[test]
fn mcp_update_settings_return_diff_gets_before_puts_and_gets_after() {
    let _guard = ENV_LOCK.lock().unwrap();
    let (base_url, handle) = sequenced_request_server(
        vec![
            json!({"data":{"timezone":"UTC","llm_custom_setting":{"temperature":0.2}}}),
            json!({"data":{"status":"updated"}}),
            json!({"data":{"timezone":"Asia/Shanghai","llm_custom_setting":{"temperature":0.2}}}),
        ],
        500,
    );
    set_env("EVEROS_API_KEY", "test-key");
    set_env("EVEROS_BASE_URL", &base_url);

    let raw = everos_hermes_rust::mcp::call_tool(
        "everos_update_settings",
        json!({"settings":{"timezone":"Asia/Shanghai"},"strict":true,"return_diff":true}),
    )
    .unwrap();
    let response: Value = serde_json::from_str(&raw).unwrap();
    let requests = handle.join().unwrap();
    let paths: Vec<&str> = requests
        .iter()
        .map(|raw| raw.lines().next().unwrap_or(""))
        .collect();

    assert_eq!(
        paths,
        vec![
            "GET /api/v1/settings HTTP/1.1",
            "PUT /api/v1/settings HTTP/1.1",
            "GET /api/v1/settings HTTP/1.1",
        ]
    );
    assert_eq!(
        parse_http_body(&requests[1]),
        json!({"timezone":"Asia/Shanghai"})
    );
    assert_eq!(response["diff"]["timezone"]["before"], "UTC");
    assert_eq!(response["diff"]["timezone"]["after"], "Asia/Shanghai");
    assert_eq!(response["updated"]["timezone"], "Asia/Shanghai");

    remove_env("EVEROS_API_KEY");
    remove_env("EVEROS_BASE_URL");
}

#[test]
fn mcp_save_memory_tool_role_requires_tool_call_id_before_http() {
    let _guard = ENV_LOCK.lock().unwrap();
    let (base_url, handle) =
        sequenced_request_server(vec![json!({"data":{"status":"queued"}})], 150);
    set_env("EVEROS_API_KEY", "test-key");
    set_env("EVEROS_USER_ID", "u1");
    set_env("EVEROS_BASE_URL", &base_url);

    let result = everos_hermes_rust::mcp::call_tool(
        "everos_save_memory",
        json!({"content":"tool output","scope":"agent","role":"tool","flush":false}),
    );
    let requests = handle.join().unwrap();

    assert!(
        result.is_err(),
        "role=tool without tool_call_id must fail locally"
    );
    let err = result.unwrap_err().to_string();
    assert!(err.contains("tool_call_id"), "unexpected error: {err}");
    assert!(
        requests.is_empty(),
        "tool_call_id validation must happen before HTTP, got {requests:?}"
    );

    remove_env("EVEROS_API_KEY");
    remove_env("EVEROS_USER_ID");
    remove_env("EVEROS_BASE_URL");
}

#[test]
fn mcp_delete_batch_requires_explicit_user_and_confirmation_text() {
    let raw = everos_hermes_rust::mcp::call_tool(
        "everos_delete_memories",
        json!({"session_id":"sess-1","confirm":true,"confirm_scope_text":"delete user_id=u1 session_id=sess-1"}),
    )
    .unwrap();
    assert!(raw.contains("explicit user_id"));

    let raw = everos_hermes_rust::mcp::call_tool(
        "everos_delete_memories",
        json!({"user_id":"u1","session_id":"sess-1","confirm":true,"confirm_scope_text":"wrong"}),
    )
    .unwrap();
    assert!(raw.contains("confirm_scope_text"));
}
