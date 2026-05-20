use super::*;

#[test]
fn provider_save_tool_adds_memory_and_flushes() {
    let _guard = ENV_LOCK.lock().unwrap();
    let home = temp_home("provider_save");
    let (base_url_add, handle_add) =
        one_request_server(json!({"data":{"status":"queued","task_id":"task-9"}}));
    fs::write(
        home.join(".env"),
        format!("EVEROS_API_KEY=test-key\nEVEROS_USER_ID=u1\nEVEROS_BASE_URL={base_url_add}\n"),
    )
    .unwrap();
    remove_env("EVEROS_API_KEY");
    remove_env("EVEROS_USER_ID");
    remove_env("EVEROS_BASE_URL");
    set_env("HERMES_HOME", home.to_str().unwrap());

    let provider = EverOSProvider::initialize(ProviderInit::for_test("sess-1", &home)).unwrap();
    let raw = provider
        .handle_tool_call(
            "everos_memory_save",
            json!({"content":"User prefers pytest.","flush":false}),
        )
        .unwrap();
    let response: Value = serde_json::from_str(&raw).unwrap();
    let request = handle_add.join().unwrap();

    assert_eq!(response["saved"], true);
    assert_eq!(response["message_queued"], true);
    assert_eq!(response["extraction_requested"], true);
    assert_eq!(response["searchable"], Value::Null);
    assert_eq!(response["flush"]["status"], "not_requested");
    assert_eq!(response["task_id"], "task-9");
    assert!(request.starts_with("POST /api/v1/memories HTTP/1.1"));
    assert_eq!(parse_http_body(&request)["user_id"], "u1");

    remove_env("HERMES_HOME");
}

#[test]
fn provider_save_tool_preserves_queue_payload_when_flush_has_non_timeout_error() {
    let _guard = ENV_LOCK.lock().unwrap();
    let home = temp_home("provider_save_flush_error");
    let (base_url, handle) = sequenced_status_request_server(
        vec![
            (
                200,
                json!({"data":{"status":"queued","task_id":"task-provider-flush"}}),
            ),
            (
                500,
                json!({"error":"flush failed token=provider-secret sk-provider-secret"}),
            ),
        ],
        500,
    );
    fs::write(
        home.join(".env"),
        format!("EVEROS_API_KEY=test-key\nEVEROS_USER_ID=u1\nEVEROS_BASE_URL={base_url}\n"),
    )
    .unwrap();
    remove_env("EVEROS_API_KEY");
    remove_env("EVEROS_USER_ID");
    remove_env("EVEROS_BASE_URL");
    set_env("HERMES_HOME", home.to_str().unwrap());

    let provider = EverOSProvider::initialize(ProviderInit::for_test("sess-1", &home)).unwrap();
    let raw = provider
        .handle_tool_call(
            "everos_memory_save",
            json!({"content":"User prefers pytest.","flush":true}),
        )
        .unwrap();
    let response: Value = serde_json::from_str(&raw).unwrap();
    let requests = handle.join().unwrap();

    assert_eq!(requests.len(), 2);
    assert_eq!(response["saved"], true);
    assert_eq!(response["message_queued"], true);
    assert_eq!(response["status"], "queued");
    assert_eq!(response["task_id"], "task-provider-flush");
    assert_eq!(response["flush"]["ok"], false);
    assert_eq!(response["flush"]["status"], "error");
    let rendered = response.to_string();
    assert!(rendered.contains("[REDACTED]"));
    assert!(!rendered.contains("provider-secret"));

    remove_env("HERMES_HOME");
}

#[test]
fn mcp_search_tool_strips_vectors_and_exposes_new_safety_parameters() {
    let _guard = ENV_LOCK.lock().unwrap();
    let (base_url, handle) = one_request_server(json!({
        "data": {
            "episodes": [{"id":"ep1","summary":"Coffee","vector":[0.1,0.2]}],
            "original_data": {"episodes": {"ep1": {"vector":[0.1,0.2],"summary":"Coffee"}}}
        }
    }));
    set_env("EVEROS_API_KEY", "test-key");
    set_env("EVEROS_BASE_URL", &base_url);
    set_env("EVEROS_USER_ID", "u1");

    let raw = everos_hermes_rust::mcp::call_tool(
        "everos_search_memories",
        json!({"query":"coffee","include_original_data":true}),
    )
    .unwrap();
    let request = handle.join().unwrap();
    let response: Value = serde_json::from_str(&raw).unwrap();

    assert_eq!(parse_http_body(&request)["include_original_data"], true);
    assert!(response.to_string().contains("Coffee"));
    assert!(!response.to_string().contains("vector"));

    let tools = everos_hermes_rust::mcp::tool_definitions();
    let search = tools
        .iter()
        .find(|tool| tool["name"] == "everos_search_memories")
        .unwrap();
    assert!(
        search["inputSchema"]["properties"]
            .get("include_vectors")
            .is_some()
    );
    let flush = tools
        .iter()
        .find(|tool| tool["name"] == "everos_flush_memories")
        .unwrap();
    assert!(flush["inputSchema"]["properties"].get("timeout").is_some());

    remove_env("EVEROS_API_KEY");
    remove_env("EVEROS_BASE_URL");
    remove_env("EVEROS_USER_ID");
}

#[test]
fn mcp_save_tool_returns_queue_semantics_when_flush_disabled() {
    let _guard = ENV_LOCK.lock().unwrap();
    let (base_url, handle) =
        one_request_server(json!({"data":{"status":"queued","task_id":"task-mcp"}}));
    set_env("EVEROS_API_KEY", "test-key");
    set_env("EVEROS_BASE_URL", &base_url);
    set_env("EVEROS_USER_ID", "u1");

    let raw = everos_hermes_rust::mcp::call_tool(
        "everos_save_memory",
        json!({"content":"User prefers pytest.","session_id":"sess-1","flush":false}),
    )
    .unwrap();
    let request = handle.join().unwrap();
    let response: Value = serde_json::from_str(&raw).unwrap();

    assert_eq!(parse_http_body(&request)["session_id"], "sess-1");
    assert_eq!(response["saved"], true);
    assert_eq!(response["message_queued"], true);
    assert_eq!(response["extraction_requested"], true);
    assert_eq!(response["searchable"], Value::Null);
    assert_eq!(response["flush"]["status"], "not_requested");
    assert_eq!(response["task_id"], "task-mcp");

    remove_env("EVEROS_API_KEY");
    remove_env("EVEROS_BASE_URL");
    remove_env("EVEROS_USER_ID");
}

#[test]
fn mcp_agent_save_add_flush_return_unchecked_visibility_and_retry_transient_flush() {
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
    let paths: Vec<&str> = requests
        .iter()
        .map(|raw| raw.lines().next().unwrap_or(""))
        .collect();

    assert_eq!(
        save["agent_visibility"]["agent_visibility_status"],
        "unchecked"
    );
    assert_eq!(save["agent_visibility"]["agent_raw_queued"], true);
    assert_eq!(
        add["agent_visibility"]["agent_visibility_status"],
        "unchecked"
    );
    assert_eq!(add["agent_visibility"]["agent_flush"]["status"], "success");
    assert_eq!(
        paths,
        vec![
            "POST /api/v1/memories/agent HTTP/1.1",
            "POST /api/v1/memories/agent HTTP/1.1",
            "POST /api/v1/memories/agent/flush HTTP/1.1",
        ]
    );

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
    let paths: Vec<&str> = requests
        .iter()
        .map(|raw| raw.lines().next().unwrap_or(""))
        .collect();

    assert_eq!(response["status"], "agent_not_visible");
    assert_eq!(
        response["agent_visibility"]["agent_visibility_status"],
        "not_visible"
    );
    assert_eq!(response["agent_visibility"]["agent_raw_queued"], true);
    assert_eq!(response["agent_visibility"]["verification_user_id"], "u1");
    assert_eq!(
        response["agent_visibility"]["verification_session_id"],
        "sess-agent"
    );
    let visibility_checks = response["agent_visibility"]["agent_visibility_checks"]
        .as_array()
        .unwrap();
    assert!(
        visibility_checks
            .iter()
            .all(|check| check["user_id"] == "u1")
    );
    assert!(
        visibility_checks
            .iter()
            .all(|check| check["session_id"] == "sess-agent")
    );
    assert_eq!(visibility_checks.len(), 3);
    assert_eq!(
        paths,
        vec![
            "POST /api/v1/memories/agent HTTP/1.1",
            "POST /api/v1/memories/agent/flush HTTP/1.1",
            "POST /api/v1/memories/search HTTP/1.1",
            "POST /api/v1/memories/search HTTP/1.1",
            "POST /api/v1/memories/get HTTP/1.1",
            "POST /api/v1/memories/get HTTP/1.1",
        ]
    );
    assert_eq!(
        parse_http_body(&requests[3])["memory_types"],
        json!(["agent_memory"])
    );
    assert_eq!(parse_http_body(&requests[4])["memory_type"], "agent_case");
    assert_eq!(parse_http_body(&requests[5])["memory_type"], "agent_skill");

    remove_env("EVEROS_API_KEY");
    remove_env("EVEROS_USER_ID");
    remove_env("EVEROS_BASE_URL");
}

#[test]
fn provider_save_config_drops_api_key_and_uses_private_permissions() {
    let home = temp_home("provider_save_config_private_permissions");
    everos_hermes_rust::provider::save_config(
        &json!({"api_key":"secret-config-key","user_id":"u1","base_url":"https://example.test"}),
        &home,
    )
    .unwrap();

    let config_path = home.join("everos.json");
    let text = fs::read_to_string(&config_path).unwrap();
    assert!(!text.contains("secret-config-key"));
    assert!(!text.contains("api_key"));
    assert_eq!(load_config(&home).user_id, "u1");
    #[cfg(unix)]
    assert_eq!(
        fs::metadata(&config_path).unwrap().permissions().mode() & 0o777,
        0o600
    );
}

#[test]
fn mcp_save_memory_preserves_queue_payload_when_flush_has_non_timeout_error() {
    let _guard = ENV_LOCK.lock().unwrap();
    let (base_url, handle) = sequenced_status_request_server(
        vec![
            (
                200,
                json!({"data":{"status":"queued","task_id":"task-save"}}),
            ),
            (
                500,
                json!({"message":"flush failed api_key=flush-secret","request_id":"req-1"}),
            ),
        ],
        500,
    );
    set_env("EVEROS_API_KEY", "test-key");
    set_env("EVEROS_BASE_URL", &base_url);
    set_env("EVEROS_USER_ID", "u1");

    let raw = everos_hermes_rust::mcp::call_tool(
        "everos_save_memory",
        json!({"content":"User prefers pytest.","session_id":"sess-1","flush":true}),
    )
    .unwrap();
    let response: Value = serde_json::from_str(&raw).unwrap();
    let rendered = response.to_string();
    let requests = handle.join().unwrap();

    assert_eq!(requests.len(), 2);
    assert_eq!(response["saved"], true);
    assert_eq!(response["message_queued"], true);
    assert_eq!(response["task_id"], "task-save");
    assert_eq!(response["flush"]["ok"], false);
    assert_eq!(response["flush"]["status"], "error");
    assert!(!rendered.contains("flush-secret"));
    assert!(rendered.contains("[REDACTED]"));

    remove_env("EVEROS_API_KEY");
    remove_env("EVEROS_BASE_URL");
    remove_env("EVEROS_USER_ID");
}

#[test]
fn mcp_save_and_verify_preserves_save_payload_when_flush_has_non_timeout_error() {
    let _guard = ENV_LOCK.lock().unwrap();
    let (base_url, handle) = sequenced_status_request_server(
        vec![
            (
                200,
                json!({"data":{"status":"queued","task_id":"task-save-verify"}}),
            ),
            (
                500,
                json!({"message":"flush failed token=workflow-secret","request_id":"req-2"}),
            ),
            (200, json!({"data":{"episodes":[]}})),
        ],
        500,
    );
    set_env("EVEROS_API_KEY", "test-key");
    set_env("EVEROS_BASE_URL", &base_url);
    set_env("EVEROS_USER_ID", "u1");

    let raw = everos_hermes_rust::mcp::call_tool(
        "everos_save_and_verify",
        json!({"content":"User prefers pytest.","session_id":"sess-1","flush":true,"verification_query":"pytest"}),
    )
    .unwrap();
    let response: Value = serde_json::from_str(&raw).unwrap();
    let rendered = response.to_string();
    let requests = handle.join().unwrap();

    assert_eq!(requests.len(), 3);
    assert_eq!(response["save"]["saved"], true);
    assert_eq!(response["save"]["message_queued"], true);
    assert_eq!(response["save"]["task_id"], "task-save-verify");
    assert_eq!(response["save"]["flush"]["ok"], false);
    assert_eq!(response["save"]["flush"]["status"], "error");
    assert_eq!(response["verification"]["status"], "not_yet_searchable");
    assert!(!rendered.contains("workflow-secret"));
    assert!(rendered.contains("[REDACTED]"));

    remove_env("EVEROS_API_KEY");
    remove_env("EVEROS_BASE_URL");
    remove_env("EVEROS_USER_ID");
}

#[test]
fn mcp_save_and_verify_preserves_save_payload_when_verification_has_error() {
    let _guard = ENV_LOCK.lock().unwrap();
    let (base_url, handle) = sequenced_status_request_server(
        vec![
            (
                200,
                json!({"data":{"status":"queued","task_id":"task-save-verify-error"}}),
            ),
            (
                200,
                json!({"data":{"status":"success","task_id":"task-flush"}}),
            ),
            (
                500,
                json!({"message":"search failed token=verify-secret","request_id":"req-verify"}),
            ),
        ],
        500,
    );
    set_env("EVEROS_API_KEY", "test-key");
    set_env("EVEROS_BASE_URL", &base_url);
    set_env("EVEROS_USER_ID", "u1");

    let raw = everos_hermes_rust::mcp::call_tool(
        "everos_save_and_verify",
        json!({"content":"User prefers pytest.","session_id":"sess-1","flush":true,"verification_query":"pytest"}),
    )
    .unwrap();
    let response: Value = serde_json::from_str(&raw).unwrap();
    let rendered = response.to_string();
    let requests = handle.join().unwrap();

    assert_eq!(requests.len(), 3);
    assert_eq!(response["ok"], true);
    assert_eq!(response["status"], "verification_error");
    assert_eq!(response["save"]["saved"], true);
    assert_eq!(response["save"]["message_queued"], true);
    assert_eq!(response["save"]["task_id"], "task-save-verify-error");
    assert_eq!(response["verification"]["ok"], false);
    assert_eq!(response["verification"]["status"], "error");
    assert_eq!(response["verification"]["verified"], false);
    assert!(!rendered.contains("verify-secret"));
    assert!(rendered.contains("[REDACTED]"));

    remove_env("EVEROS_API_KEY");
    remove_env("EVEROS_BASE_URL");
    remove_env("EVEROS_USER_ID");
}

#[test]
fn mcp_jsonrpc_tool_error_redacts_backend_error_body() {
    let _guard = ENV_LOCK.lock().unwrap();
    let (base_url, handle) = sequenced_status_request_server(
        vec![(
            500,
            json!({"message":"backend failed Authorization: Bearer backend-secret","request_id":"req-3"}),
        )],
        500,
    );
    set_env("EVEROS_API_KEY", "test-key");
    set_env("EVEROS_BASE_URL", &base_url);
    set_env("EVEROS_USER_ID", "u1");

    let response = everos_hermes_rust::mcp::handle_jsonrpc_message(&json!({
        "jsonrpc":"2.0",
        "id":1,
        "method":"tools/call",
        "params":{"name":"everos_search_memories","arguments":{"query":"coffee"}}
    }))
    .unwrap();
    let rendered = response.to_string();
    let requests = handle.join().unwrap();

    assert_eq!(requests.len(), 1);
    assert_eq!(response["result"]["isError"], true);
    assert!(!rendered.contains("backend-secret"));
    assert!(rendered.contains("[REDACTED]"));

    remove_env("EVEROS_API_KEY");
    remove_env("EVEROS_BASE_URL");
    remove_env("EVEROS_USER_ID");
}

#[test]
fn mcp_tool_name_constant_matches_expected_twelve_tools() {
    assert_eq!(
        TOOL_NAMES.as_slice(),
        &[
            "everos_save_memory",
            "everos_add_memories",
            "everos_flush_memories",
            "everos_search_memories",
            "everos_get_memories",
            "everos_delete_memories",
            "everos_get_task_status",
            "everos_get_settings",
            "everos_update_settings",
            "everos_verify_session_ingest",
            "everos_save_and_verify",
            "everos_import_and_verify",
        ]
    );
    assert_eq!(DEFAULT_BASE_URL, "https://api.evermind.ai");
}

#[test]
fn client_accepts_top_k_minus_one_and_radius_filters() {
    let (base_url, handle) = one_request_server(json!({"data":{"episodes":[]}}));
    let client = EverOSClient::new("test-key", &base_url, 10.0).unwrap();
    client
        .search_memories(
            "debug timeout",
            Some("user_001"),
            Some("session_001"),
            Some(json!({"AND":[{"timestamp":{"gte":1}}]})),
            "hybrid",
            Some(vec!["agent_memory".to_string()]),
            -1,
            Some(0.5),
            false,
            false,
            None,
        )
        .unwrap();
    let body = parse_http_body(&handle.join().unwrap());
    assert_eq!(body["top_k"], -1);
    assert_eq!(body["radius"], 0.5);
    assert_eq!(body["memory_types"], json!(["agent_memory"]));
    assert_eq!(body["filters"]["user_id"], "user_001");
    assert_eq!(body["filters"]["AND"][0]["timestamp"]["gte"], 1);
    assert_eq!(body["filters"]["AND"][1]["session_id"], "session_001");
}

#[test]
fn client_rejects_invalid_search_get_delete_contracts_before_request() {
    let client = EverOSClient::new("test-key", "http://127.0.0.1:9", 0.05).unwrap();
    assert!(
        client
            .search_memories(
                "q",
                Some("u"),
                None,
                None,
                "hybrid",
                Some(vec!["agent_case".to_string()]),
                5,
                None,
                false,
                false,
                None
            )
            .is_err()
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
            .search_memories(
                "q",
                Some("u"),
                None,
                None,
                "hybrid",
                None,
                5,
                Some(1.1),
                false,
                false,
                None
            )
            .is_err()
    );
    assert!(
        client
            .get_memories(
                Some("u"),
                None,
                None,
                "agent_memory",
                1,
                20,
                "timestamp",
                "desc"
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

#[test]
fn formatter_renders_nested_agent_memory_and_raw_messages() {
    let context = format_search_context(
        &json!({"data":{"raw_messages":[{"role":"user","content":"raw request"}],"agent_memory":{"cases":[{"task_intent":"debug timeout","approach":"check task status before retry"}],"skills":[{"name":"timeout recovery","description":"poll task status"}]}}}),
        5,
    );
    assert!(context.contains("## Raw Messages"));
    assert!(context.contains("raw request"));
    assert!(context.contains("## Agent Cases"));
    assert!(context.contains("check task status before retry"));
    assert!(context.contains("## Agent Skills"));
    assert!(context.contains("timeout recovery"));
}

#[test]
fn mcp_and_provider_schemas_expose_cloud_v1_parameters() {
    let tools = everos_hermes_rust::mcp::tool_definitions();
    let search = tools
        .iter()
        .find(|tool| tool["name"] == "everos_search_memories")
        .unwrap();
    for key in ["filters", "radius", "timeout", "fallback_to_hybrid"] {
        assert!(
            search["inputSchema"]["properties"].get(key).is_some(),
            "missing mcp search {key}"
        );
    }
    let get = tools
        .iter()
        .find(|tool| tool["name"] == "everos_get_memories")
        .unwrap();
    for key in ["filters", "rank_by", "rank_order"] {
        assert!(
            get["inputSchema"]["properties"].get(key).is_some(),
            "missing mcp get {key}"
        );
    }
    let save = tools
        .iter()
        .find(|tool| tool["name"] == "everos_save_memory")
        .unwrap();
    assert_eq!(
        save["inputSchema"]["properties"]["scope"]["enum"],
        json!(["personal", "agent"])
    );
    assert!(
        save["inputSchema"]["properties"]
            .get("tool_call_id")
            .is_some(),
        "missing mcp save tool_call_id"
    );
    let delete = tools
        .iter()
        .find(|tool| tool["name"] == "everos_delete_memories")
        .unwrap();
    assert!(
        delete["inputSchema"]["properties"]
            .get("confirm_scope_text")
            .is_some()
    );

    let schemas = everos_hermes_rust::provider::provider_tool_schemas();
    let provider_save = schemas
        .iter()
        .find(|tool| tool["name"] == "everos_memory_save")
        .unwrap();
    assert_eq!(
        provider_save["parameters"]["properties"]["scope"]["enum"],
        json!(["personal", "agent"])
    );
    assert!(
        provider_save["parameters"]["properties"]
            .get("tool_call_id")
            .is_some(),
        "missing provider save tool_call_id"
    );
    let provider_search = schemas
        .iter()
        .find(|tool| tool["name"] == "everos_memory_search")
        .unwrap();
    for key in ["filters", "radius", "top_k", "response_format"] {
        assert!(
            provider_search["parameters"]["properties"]
                .get(key)
                .is_some(),
            "missing provider search {key}"
        );
    }
}

#[test]
fn mcp_and_provider_workflow_tools_are_registered() {
    let tools = everos_hermes_rust::mcp::tool_definitions();
    for name in [
        "everos_verify_session_ingest",
        "everos_save_and_verify",
        "everos_import_and_verify",
    ] {
        let tool = tools
            .iter()
            .find(|tool| tool["name"] == name)
            .unwrap_or_else(|| panic!("missing MCP workflow tool {name}"));
        assert_eq!(tool["outputSchema"]["properties"]["ok"]["type"], "boolean");
        assert_eq!(
            tool["outputSchema"]["properties"]["status"]["type"],
            "string"
        );
    }
    let schemas = everos_hermes_rust::provider::provider_tool_schemas();
    for name in [
        "everos_memory_save_and_verify",
        "everos_memory_import_and_verify",
        "everos_memory_verify_session",
    ] {
        assert!(
            schemas.iter().any(|tool| tool["name"] == name),
            "missing provider workflow tool {name}"
        );
    }
}
