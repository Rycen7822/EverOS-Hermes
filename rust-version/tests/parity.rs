use everos_hermes_rust::client::{DEFAULT_BASE_URL, DEFAULT_MEMORY_TYPES, EverOSClient};
use everos_hermes_rust::env::{get_env, read_dotenv};
use everos_hermes_rust::formatting::{format_search_context, strip_vectors};
use everos_hermes_rust::mcp::TOOL_NAMES;
use everos_hermes_rust::provider::{EverOSProvider, ProviderInit};
use serde_json::{Value, json};
use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn temp_home(name: &str) -> PathBuf {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis();
    let path = std::env::temp_dir().join(format!("everos_hermes_rust_{name}_{millis}"));
    fs::create_dir_all(&path).unwrap();
    path
}

fn set_env(key: &str, value: &str) {
    unsafe { std::env::set_var(key, value) }
}

fn remove_env(key: &str) {
    unsafe { std::env::remove_var(key) }
}

fn one_request_server(response: Value) -> (String, thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = Vec::new();
        let mut tmp = [0u8; 4096];
        loop {
            let n = stream.read(&mut tmp).unwrap();
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&tmp[..n]);
            if let Some(header_end) = find_bytes(&buf, b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&buf[..header_end]).to_string();
                let content_length = headers
                    .lines()
                    .find_map(|line| {
                        line.strip_prefix("content-length: ")
                            .or_else(|| line.strip_prefix("Content-Length: "))
                    })
                    .and_then(|value| value.trim().parse::<usize>().ok())
                    .unwrap_or(0);
                if buf.len() >= header_end + 4 + content_length {
                    break;
                }
            }
        }
        let body = response.to_string();
        let reply = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(reply.as_bytes()).unwrap();
        String::from_utf8_lossy(&buf).to_string()
    });
    (format!("http://{addr}"), handle)
}

fn n_request_server(response: Value, count: usize) -> (String, thread::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = thread::spawn(move || {
        let mut requests = Vec::new();
        for _ in 0..count {
            let (mut stream, _) = listener.accept().unwrap();
            let raw = read_http_request(&mut stream);
            let body = response.to_string();
            let reply = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(reply.as_bytes()).unwrap();
            requests.push(raw);
        }
        requests
    });
    (format!("http://{addr}"), handle)
}

fn sequenced_request_server(
    responses: Vec<Value>,
    idle_timeout_ms: u64,
) -> (String, thread::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = thread::spawn(move || {
        let mut requests = Vec::new();
        let idle_timeout = Duration::from_millis(idle_timeout_ms);
        let mut deadline = Instant::now() + idle_timeout;
        while requests.len() < responses.len() && Instant::now() < deadline {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let raw = read_http_request(&mut stream);
                    let body = responses[requests.len()].to_string();
                    let reply = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    stream.write_all(reply.as_bytes()).unwrap();
                    requests.push(raw);
                    deadline = Instant::now() + idle_timeout;
                }
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(err) => panic!("test server accept failed: {err}"),
            }
        }
        requests
    });
    (format!("http://{addr}"), handle)
}

fn read_http_request(stream: &mut impl Read) -> String {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    loop {
        let n = stream.read(&mut tmp).unwrap();
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(header_end) = find_bytes(&buf, b"\r\n\r\n") {
            let headers = String::from_utf8_lossy(&buf[..header_end]).to_string();
            let content_length = headers
                .lines()
                .find_map(|line| {
                    line.strip_prefix("content-length: ")
                        .or_else(|| line.strip_prefix("Content-Length: "))
                })
                .and_then(|value| value.trim().parse::<usize>().ok())
                .unwrap_or(0);
            if buf.len() >= header_end + 4 + content_length {
                break;
            }
        }
    }
    String::from_utf8_lossy(&buf).to_string()
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn parse_http_body(raw: &str) -> Value {
    let body = raw.split("\r\n\r\n").nth(1).unwrap_or("");
    serde_json::from_str(body).unwrap()
}

#[test]
fn dotenv_lookup_prefers_process_env_then_hermes_home_file() {
    let _guard = ENV_LOCK.lock().unwrap();
    let home = temp_home("dotenv");
    fs::write(
        home.join(".env"),
        "# comment\nexport EVEROS_API_KEY='dotenv-key'\nEVEROS_BASE_URL=https://everos.example.test/ # comment\nEVEROS_TIMEOUT=3\n",
    )
    .unwrap();
    remove_env("EVEROS_API_KEY");
    remove_env("EVEROS_BASE_URL");
    set_env("HERMES_HOME", home.to_str().unwrap());

    let parsed = read_dotenv(&home.join(".env"));
    assert_eq!(parsed.get("EVEROS_API_KEY").unwrap(), "dotenv-key");
    assert_eq!(get_env("EVEROS_API_KEY", "", Some(&home)), "dotenv-key");
    set_env("EVEROS_API_KEY", "process-key");
    assert_eq!(get_env("EVEROS_API_KEY", "", Some(&home)), "process-key");

    remove_env("EVEROS_API_KEY");
    remove_env("EVEROS_BASE_URL");
    remove_env("HERMES_HOME");
}

#[test]
fn client_posts_bearer_json_to_add_memories() {
    let (base_url, handle) =
        one_request_server(json!({"data":{"status":"queued","task_id":"task-1"}}));
    let client = EverOSClient::new("test-key", &base_url, 7.0).unwrap();
    let response = client
        .add_memories(
            "user_001",
            Some("session_001"),
            vec![json!({"role":"user","timestamp":1711900000000_i64,"content":"I like black coffee."})],
            true,
            false,
        )
        .unwrap();
    let raw = handle.join().unwrap();
    let body = parse_http_body(&raw);

    assert_eq!(response["data"]["task_id"], "task-1");
    assert!(raw.starts_with("POST /api/v1/memories HTTP/1.1"));
    assert!(
        raw.contains("authorization: Bearer test-key")
            || raw.contains("Authorization: Bearer test-key")
    );
    assert_eq!(body["user_id"], "user_001");
    assert_eq!(body["session_id"], "session_001");
    assert_eq!(body["async_mode"], true);
    assert_eq!(body["messages"][0]["content"], "I like black coffee.");
}

#[test]
fn client_search_uses_hybrid_defaults_and_session_filter() {
    let (base_url, handle) = one_request_server(json!({"data":{"episodes":[]}}));
    let client = EverOSClient::new("test-key", &base_url, 10.0).unwrap();
    client
        .search_memories(
            "coffee preference",
            Some("user_001"),
            None,
            Some("session_001"),
            None,
            "hybrid",
            None,
            5,
            None,
            false,
            false,
            None,
        )
        .unwrap();
    let raw = handle.join().unwrap();
    let body = parse_http_body(&raw);

    assert!(raw.starts_with("POST /api/v1/memories/search HTTP/1.1"));
    assert_eq!(body["query"], "coffee preference");
    assert_eq!(body["filters"]["user_id"], "user_001");
    assert_eq!(body["filters"]["AND"][0]["session_id"], "session_001");
    assert_eq!(body["method"], "hybrid");
    assert_eq!(body["memory_types"], json!(DEFAULT_MEMORY_TYPES));
    assert_eq!(body["top_k"], 5);
    assert_eq!(body.get("radius"), None);
}

#[test]
fn client_search_strips_vectors_by_default_but_can_keep_them() {
    let payload = json!({
        "data": {
            "episodes": [{"id":"ep1","summary":"Coffee","vector":[0.1,0.2]}],
            "original_data": {"episodes": {"ep1": {"vector":[0.1,0.2],"embedding":[0.3]}}}
        }
    });

    let stripped = strip_vectors(&payload);
    assert!(stripped.to_string().contains("Coffee"));
    assert!(!stripped.to_string().contains("vector"));
    assert!(!stripped.to_string().contains("embedding"));

    let (base_url, handle) = one_request_server(payload.clone());
    let client = EverOSClient::new("test-key", &base_url, 10.0).unwrap();
    let response = client
        .search_memories(
            "coffee",
            Some("user_001"),
            None,
            None,
            None,
            "hybrid",
            None,
            5,
            None,
            true,
            false,
            None,
        )
        .unwrap();
    handle.join().unwrap();
    assert!(response.to_string().contains("Coffee"));
    assert!(!response.to_string().contains("vector"));

    let (base_url, handle) = one_request_server(payload);
    let client = EverOSClient::new("test-key", &base_url, 10.0).unwrap();
    let response = client
        .search_memories(
            "coffee",
            Some("user_001"),
            None,
            None,
            None,
            "hybrid",
            None,
            5,
            None,
            true,
            true,
            None,
        )
        .unwrap();
    handle.join().unwrap();
    assert_eq!(response["data"]["episodes"][0]["vector"], json!([0.1, 0.2]));
}

#[test]
fn formatting_renders_episode_and_profile_context() {
    let context = format_search_context(
        &json!({
            "data": {
                "episodes": [{"subject":"coffee preference","summary":"User prefers strong black Americano.","score":0.91}],
                "profiles": [{"profile_data":{"explicit_info":["User likes black coffee"],"implicit_traits":["Prefers concise recommendations"]}}]
            }
        }),
        5,
    );
    assert!(context.contains("# EverOS Memory"));
    assert!(context.contains("coffee preference"));
    assert!(context.contains("strong black Americano"));
    assert!(context.contains("User likes black coffee"));
}

#[test]
fn provider_availability_user_resolution_and_tool_schemas_match_python_surface() {
    let _guard = ENV_LOCK.lock().unwrap();
    let home = temp_home("provider");
    fs::write(
        home.join(".env"),
        "EVEROS_API_KEY=test-key\nEVEROS_USER_ID=hermes_{identity}_{platform}\n",
    )
    .unwrap();
    remove_env("EVEROS_API_KEY");
    remove_env("EVEROS_USER_ID");
    set_env("HERMES_HOME", home.to_str().unwrap());

    assert!(EverOSProvider::is_available(Some(&home)));
    let provider = EverOSProvider::initialize(ProviderInit {
        session_id: "sess-1".into(),
        hermes_home: Some(home.clone()),
        platform: "telegram".into(),
        user_id: "tg-42".into(),
        user_name: "Xu".into(),
        agent_identity: "default".into(),
        agent_context: "".into(),
    })
    .unwrap();

    assert_eq!(provider.name(), "everos");
    assert_eq!(provider.user_id(), "hermes_default_telegram");
    assert!(
        provider
            .system_prompt_block()
            .contains("everos_memory_search")
    );
    let tool_names: Vec<String> = provider
        .tool_schemas()
        .iter()
        .map(|schema| schema["name"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        tool_names,
        vec![
            "everos_memory_save",
            "everos_memory_search",
            "everos_memory_get",
            "everos_memory_flush",
            "everos_memory_forget",
        ]
    );

    remove_env("HERMES_HOME");
}

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
fn mcp_tool_name_constant_matches_expected_nine_tools() {
    assert_eq!(
        TOOL_NAMES,
        [
            "everos_save_memory",
            "everos_add_memories",
            "everos_flush_memories",
            "everos_search_memories",
            "everos_get_memories",
            "everos_delete_memories",
            "everos_get_task_status",
            "everos_get_settings",
            "everos_update_settings",
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
            None,
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
            .delete_memories(Some("mem-1"), Some("u"), None, None)
            .is_err()
    );
    assert!(
        client
            .delete_memories(None, None, None, Some("sess"))
            .is_err()
    );
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
fn mcp_search_preserves_radius_zero() {
    let _guard = ENV_LOCK.lock().unwrap();
    let (base_url, handle) = one_request_server(json!({"data":{"episodes":[]}}));
    set_env("EVEROS_API_KEY", "test-key");
    set_env("EVEROS_USER_ID", "u1");
    set_env("EVEROS_BASE_URL", &base_url);

    everos_hermes_rust::mcp::call_tool(
        "everos_search_memories",
        json!({"query":"q","method":"hybrid","radius":0,"top_k":1}),
    )
    .unwrap();
    let raw = handle.join().unwrap();
    let body = parse_http_body(&raw);

    assert_eq!(body["radius"], json!(0.0));
    assert_eq!(body["top_k"], 1);

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

#[test]
fn provider_save_tool_scope_agent_posts_agent_endpoint() {
    let _guard = ENV_LOCK.lock().unwrap();
    let home = temp_home("provider_agent_save");
    let (base_url, handle) =
        one_request_server(json!({"data":{"status":"queued","task_id":"task-agent"}}));
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
            json!({"content":"retry with timeout","scope":"agent","flush":false}),
        )
        .unwrap();
    let request = handle.join().unwrap();
    let response: Value = serde_json::from_str(&raw).unwrap();
    assert!(request.starts_with("POST /api/v1/memories/agent HTTP/1.1"));
    assert_eq!(response["scope"], "agent");

    remove_env("HERMES_HOME");
}

#[test]
fn provider_sync_turn_capture_agent_memory_posts_personal_and_agent_endpoints() {
    let _guard = ENV_LOCK.lock().unwrap();
    let home = temp_home("provider_agent_sync");
    let (base_url, handle) =
        n_request_server(json!({"data":{"status":"queued","task_id":"task-sync"}}), 2);
    fs::write(
        home.join(".env"),
        format!("EVEROS_API_KEY=test-key\nEVEROS_USER_ID=u1\nEVEROS_BASE_URL={base_url}\n"),
    )
    .unwrap();
    fs::write(
        home.join("everos.json"),
        json!({
            "auto_capture": true,
            "flush_after_turn": false,
            "capture_agent_memory": true,
            "agent_capture_mode": "parallel",
            "agent_flush_after_turn": false
        })
        .to_string(),
    )
    .unwrap();
    remove_env("EVEROS_API_KEY");
    remove_env("EVEROS_USER_ID");
    remove_env("EVEROS_BASE_URL");
    set_env("HERMES_HOME", home.to_str().unwrap());

    let provider = EverOSProvider::initialize(ProviderInit::for_test("sess-1", &home)).unwrap();
    provider
        .sync_turn(
            "please debug the timeout regression",
            "checked task status before retrying and fixed it",
            Some("sess-2"),
        )
        .unwrap();
    let requests = handle.join().unwrap();
    let paths: Vec<&str> = requests
        .iter()
        .map(|raw| raw.lines().next().unwrap_or(""))
        .collect();
    assert!(
        paths
            .iter()
            .any(|line| line.starts_with("POST /api/v1/memories "))
    );
    assert!(
        paths
            .iter()
            .any(|line| line.starts_with("POST /api/v1/memories/agent "))
    );
    let agent_body = requests
        .iter()
        .find(|raw| raw.starts_with("POST /api/v1/memories/agent "))
        .map(|raw| parse_http_body(raw))
        .unwrap();
    assert_eq!(agent_body["user_id"], "u1");
    assert_eq!(agent_body["session_id"], "sess-2");
    assert!(
        agent_body["messages"][0]["content"]
            .as_str()
            .unwrap()
            .contains("Task request:")
    );

    remove_env("HERMES_HOME");
}

#[test]
fn mcp_stdio_binary_initializes_and_lists_tools() {
    let bin = env!("CARGO_BIN_EXE_everos-hermes-rust");
    let mut child = Command::new(bin)
        .arg("mcp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let initialize = json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"parity-test","version":"0"}}});
    write_frame(child.stdin.as_mut().unwrap(), &initialize);
    let response = read_frame(child.stdout.as_mut().unwrap());
    assert_eq!(response["id"], 1);
    assert_eq!(response["result"]["serverInfo"]["name"], "everos_mcp");

    let list = json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}});
    write_frame(child.stdin.as_mut().unwrap(), &list);
    let response = read_frame(child.stdout.as_mut().unwrap());
    let names: Vec<String> = response["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(names, TOOL_NAMES);

    drop(child.stdin.take());
    child.kill().ok();
    child.wait().ok();
}

fn write_frame<W: Write>(writer: &mut W, value: &Value) {
    let body = value.to_string();
    write!(writer, "Content-Length: {}\r\n\r\n{}", body.len(), body).unwrap();
    writer.flush().unwrap();
}

fn read_frame<R: Read>(reader: &mut R) -> Value {
    let mut raw = Vec::new();
    let mut one = [0u8; 1];
    reader.read_exact(&mut one).unwrap();
    raw.push(one[0]);
    if one[0] == b'{' {
        while !raw.ends_with(b"\n") {
            reader.read_exact(&mut one).unwrap();
            raw.push(one[0]);
        }
        return serde_json::from_slice(raw.strip_suffix(b"\n").unwrap_or(&raw)).unwrap();
    }
    while !raw.ends_with(b"\r\n\r\n") {
        reader.read_exact(&mut one).unwrap();
        raw.push(one[0]);
    }
    let header = String::from_utf8(raw).unwrap();
    let len = header
        .lines()
        .find_map(|line| {
            line.strip_prefix("Content-Length: ")
                .or_else(|| line.strip_prefix("content-length: "))
        })
        .unwrap()
        .trim()
        .parse::<usize>()
        .unwrap();
    let mut body = vec![0u8; len];
    reader.read_exact(&mut body).unwrap();
    serde_json::from_slice(&body).unwrap()
}
