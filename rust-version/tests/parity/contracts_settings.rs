use super::*;

#[test]
fn response_normalization_contract_cases_match_python() {
    use everos_hermes_rust::response_normalization::{
        as_list, count_hits, response_data, response_summary,
    };

    let contract = response_normalization_contract();
    for case in contract["cases"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let response = &case["response"];
        let mut keys: Vec<String> = response_data(Some(response)).keys().cloned().collect();
        keys.sort();
        let expected_keys: Vec<String> = case["expected_data_keys"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap().to_string())
            .collect();
        assert_eq!(keys, expected_keys, "{name}");
        assert_eq!(
            count_hits(response),
            case["expected_hit_count"].as_u64().unwrap() as usize,
            "{name}"
        );
        assert_eq!(
            response_summary(response),
            case["expected_summary"],
            "{name}"
        );
    }

    for case in contract["as_list_cases"].as_array().unwrap() {
        let input = case.get("input").filter(|value| !value.is_null());
        assert_eq!(as_list(input), case["expected"].as_array().unwrap().clone());
    }
}

#[test]
fn settings_validation_contract_cases_match_python() {
    let contract = settings_validation_contract();
    for case in contract["cases"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
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
            assert!(result.is_ok(), "{name} should be valid: {result:?}");
            assert_eq!(requests.len(), 1, "{name} should send one PUT request");
            assert!(requests[0].starts_with("PUT /api/v1/settings HTTP/1.1"));
            assert_eq!(parse_http_body(&requests[0]), case["normalized"]);
        } else {
            assert!(result.is_err(), "{name} should be rejected locally");
            let err = result.unwrap_err().to_string();
            assert!(
                err.contains(case["error_contains"].as_str().unwrap()),
                "{name} unexpected error: {err}"
            );
            assert!(
                requests.is_empty(),
                "{name} should fail before HTTP, got {requests:?}"
            );
        }
    }
}

#[test]
fn mcp_update_settings_rejects_unknown_strict_fields_before_http() {
    let _guard = ENV_LOCK.lock().unwrap();
    let (base_url, handle) =
        sequenced_request_server(vec![json!({"data":{"timezone":"UTC"}})], 150);
    set_env("EVEROS_API_KEY", "test-key");
    set_env("EVEROS_BASE_URL", &base_url);

    let result = everos_hermes_rust::mcp::call_tool(
        "everos_update_settings",
        json!({"settings":{"unknown_field":"should_not_send"},"strict":true}),
    );
    let requests = handle.join().unwrap();

    assert!(
        result.is_err(),
        "strict unknown setting should fail locally"
    );
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("Unknown settings fields"),
        "unexpected error: {err}"
    );
    assert!(
        requests.is_empty(),
        "strict validation must reject unknown fields before HTTP, got {requests:?}"
    );

    remove_env("EVEROS_API_KEY");
    remove_env("EVEROS_BASE_URL");
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
fn mcp_save_memory_agent_default_role_and_explicit_tool_call_id_body() {
    let _guard = ENV_LOCK.lock().unwrap();
    let (base_url, handle) = n_request_server(
        json!({"data":{"status":"queued","task_id":"task-agent"}}),
        2,
    );
    set_env("EVEROS_API_KEY", "test-key");
    set_env("EVEROS_USER_ID", "u1");
    set_env("EVEROS_BASE_URL", &base_url);

    everos_hermes_rust::mcp::call_tool(
        "everos_save_memory",
        json!({"content":"agent summary","scope":"agent","session_id":"sess-1","flush":false}),
    )
    .unwrap();
    everos_hermes_rust::mcp::call_tool(
        "everos_save_memory",
        json!({"content":"tool output","scope":"agent","role":"tool","tool_call_id":"tool-call-1","session_id":"sess-1","flush":false}),
    )
    .unwrap();
    let requests = handle.join().unwrap();
    let default_body = parse_http_body(&requests[0]);
    let tool_body = parse_http_body(&requests[1]);

    assert!(requests[0].starts_with("POST /api/v1/memories/agent HTTP/1.1"));
    assert_eq!(default_body["messages"][0]["role"], "assistant");
    assert_eq!(tool_body["messages"][0]["role"], "tool");
    assert_eq!(tool_body["messages"][0]["tool_call_id"], "tool-call-1");

    remove_env("EVEROS_API_KEY");
    remove_env("EVEROS_USER_ID");
    remove_env("EVEROS_BASE_URL");
}

#[test]
fn mcp_numeric_boundaries_reject_invalid_args_before_http() {
    let _guard = ENV_LOCK.lock().unwrap();
    let (base_url, handle) = sequenced_request_server(
        vec![
            json!({"data":{"episodes":[]}}),
            json!({"data":{"items":[]}}),
            json!({"data":{"episodes":[]}}),
        ],
        150,
    );
    set_env("EVEROS_API_KEY", "test-key");
    set_env("EVEROS_USER_ID", "u1");
    set_env("EVEROS_BASE_URL", &base_url);

    let bad_top_k = everos_hermes_rust::mcp::call_tool(
        "everos_search_memories",
        json!({"query":"q","top_k":101}),
    );
    let bad_page = everos_hermes_rust::mcp::call_tool(
        "everos_get_memories",
        json!({"page":0,"page_size":101}),
    );
    let bad_radius = everos_hermes_rust::mcp::call_tool(
        "everos_search_memories",
        json!({"query":"q","radius":1.1}),
    );
    let requests = handle.join().unwrap();

    for (label, result) in [
        ("top_k", bad_top_k),
        ("page/page_size", bad_page),
        ("radius", bad_radius),
    ] {
        assert!(result.is_err(), "{label} should be rejected locally");
    }
    assert!(
        requests.is_empty(),
        "invalid numeric arguments must not send HTTP, got {requests:?}"
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
