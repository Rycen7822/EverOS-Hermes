use everos_hermes_rust::client::{DEFAULT_BASE_URL, DEFAULT_MEMORY_TYPES, EverOSClient};
use everos_hermes_rust::context_assembler::{ContextAssemblyConfig, assemble_everos_context};
use everos_hermes_rust::env::{get_env, read_dotenv};
use everos_hermes_rust::formatting::{format_search_context, strip_vectors};
use everos_hermes_rust::mcp::{TOOL_NAMES, tool_definitions};
use everos_hermes_rust::policy::{should_skip_capture, should_skip_recall, stable_query_key};
use everos_hermes_rust::provider::{
    EverOSProvider, ProviderConfig, ProviderInit, load_config, provider_tool_schemas,
};
use everos_hermes_rust::trajectory::{
    TrajectoryBuildOptions, build_agent_trajectory_messages,
    build_agent_trajectory_messages_with_options,
};
use serde_json::{Value, json};
use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
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

fn one_status_empty_request_server(status: u16) -> (String, thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let raw = read_http_request(&mut stream);
        let reason = if status >= 400 {
            "Internal Server Error"
        } else {
            "OK"
        };
        let reply =
            format!("HTTP/1.1 {status} {reason}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        stream.write_all(reply.as_bytes()).unwrap();
        raw
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

fn snapshot_json(name: &str) -> Value {
    let raw = fs::read_to_string(format!("../tests/contracts/{name}")).unwrap();
    serde_json::from_str(&raw).unwrap()
}

fn simplify_provider_property(schema: &Value) -> Value {
    let mut out = serde_json::Map::new();
    for key in ["type", "enum", "default", "description"] {
        if let Some(value) = schema.get(key) {
            out.insert(key.to_string(), value.clone());
        }
    }
    Value::Object(out)
}

fn sorted_string_values(value: Option<&Value>) -> Vec<String> {
    let mut values = value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    values.sort();
    values
}

fn provider_schema_snapshot() -> Value {
    Value::Array(
        provider_tool_schemas()
            .into_iter()
            .map(|schema| {
                let params = &schema["parameters"];
                let properties = params["properties"].as_object().unwrap();
                let mut simplified = serde_json::Map::<String, Value>::new();
                let mut keys = properties.keys().cloned().collect::<Vec<_>>();
                keys.sort();
                for key in keys {
                    simplified.insert(key.clone(), simplify_provider_property(properties.get(&key).unwrap()));
                }
                json!({
                    "name": schema["name"],
                    "description": schema.get("description").cloned().unwrap_or(Value::String(String::new())),
                    "required": sorted_string_values(params.get("required")),
                    "properties": simplified,
                })
            })
            .collect(),
    )
}

fn description_summary(value: Option<&Value>) -> String {
    let text = value.and_then(Value::as_str).unwrap_or_default();
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if let Some((first, _)) = collapsed.split_once(". ") {
        format!("{first}.")
    } else {
        collapsed
    }
}

fn mcp_schema_snapshot() -> Value {
    Value::Array(
        tool_definitions()
            .into_iter()
            .map(|schema| {
                let input = &schema["inputSchema"];
                let properties = input["properties"].as_object().unwrap();
                let mut property_names = properties.keys().cloned().collect::<Vec<_>>();
                property_names.sort();
                let output = schema.get("outputSchema").unwrap_or(&Value::Null);
                let output_properties = output
                    .get("properties")
                    .and_then(Value::as_object)
                    .map(|properties| {
                        let mut names = properties.keys().cloned().collect::<Vec<_>>();
                        names.sort();
                        names
                    })
                    .unwrap_or_default();
                let output_required = sorted_string_values(output.get("required"));
                let output_shape = if output_required == ["result"] && output_properties == ["result"] {
                    json!("result")
                } else if output_required.is_empty()
                    && output_properties == ["ok", "retryable", "status", "suggested_next_actions", "workflow"]
                {
                    json!("workflow")
                } else {
                    json!({"required": output_required, "properties": output_properties})
                };
                let annotations = schema.get("annotations").unwrap_or(&Value::Null);
                json!({
                    "name": schema["name"],
                    "title": schema["title"],
                    "description_summary": description_summary(schema.get("description")),
                    "required": sorted_string_values(input.get("required")),
                    "properties": property_names,
                    "output_shape": output_shape,
                    "annotation_profile": format!(
                        "{}:{}:{}:{}",
                        if annotations["readOnlyHint"].as_bool().unwrap_or(false) { "read" } else { "write" },
                        if annotations["destructiveHint"].as_bool().unwrap_or(false) { "destructive" } else { "safe" },
                        if annotations["idempotentHint"].as_bool().unwrap_or(false) { "idem" } else { "nonidem" },
                        if annotations["openWorldHint"].as_bool().unwrap_or(false) { "open" } else { "closed" },
                    ),
                })
            })
            .collect(),
    )
}

fn provider_config_contract() -> Value {
    let raw = fs::read_to_string("../tests/contracts/provider_config_contract.json").unwrap();
    serde_json::from_str(&raw).unwrap()
}

fn settings_validation_contract() -> Value {
    let raw = fs::read_to_string("../tests/contracts/settings_validation_cases.json").unwrap();
    serde_json::from_str(&raw).unwrap()
}

fn response_normalization_contract() -> Value {
    let raw = fs::read_to_string("../tests/contracts/response_normalization_cases.json").unwrap();
    serde_json::from_str(&raw).unwrap()
}

fn provider_config_usize_field(config: &ProviderConfig, key: &str) -> usize {
    match key {
        "max_context_chars" => config.max_context_chars,
        "recent_raw_top_k" => config.recent_raw_top_k,
        "profile_max_items" => config.profile_max_items,
        "agent_skills_max_items" => config.agent_skills_max_items,
        "agent_cases_max_items" => config.agent_cases_max_items,
        "episodic_max_items" => config.episodic_max_items,
        "min_recall_query_chars" => config.min_recall_query_chars,
        "prefetch_cache_ttl_seconds" => config.prefetch_cache_ttl_seconds as usize,
        "agent_max_messages" => config.agent_max_messages,
        "agent_max_message_chars" => config.agent_max_message_chars,
        "agent_max_tool_result_chars" => config.agent_max_tool_result_chars,
        "agent_max_payload_chars" => config.agent_max_payload_chars,
        "agent_dedupe_entries" => config.agent_dedupe_entries,
        other => panic!("unsupported provider config contract field: {other}"),
    }
}

fn keep_alive_two_request_server() -> (String, thread::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        drop(listener);
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut requests = Vec::new();
        for index in 0..2 {
            requests.push(read_http_request(&mut stream));
            let body = json!({"data": {"request_index": index + 1}}).to_string();
            let connection = if index == 0 { "keep-alive" } else { "close" };
            let reply = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: {}\r\n\r\n{}",
                body.len(),
                connection,
                body
            );
            stream.write_all(reply.as_bytes()).unwrap();
            stream.flush().unwrap();
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

fn sequenced_status_request_server(
    responses: Vec<(u16, Value)>,
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
                    let (status, response) = &responses[requests.len()];
                    let body = response.to_string();
                    let reason = if *status >= 400 {
                        "Internal Server Error"
                    } else {
                        "OK"
                    };
                    let reply = format!(
                        "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        status,
                        reason,
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

fn dropped_then_response_server(response: Value) -> (String, thread::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = thread::spawn(move || {
        let mut requests = Vec::new();
        let (mut first, _) = listener.accept().unwrap();
        requests.push(read_http_request(&mut first));
        drop(first);

        let (mut second, _) = listener.accept().unwrap();
        requests.push(read_http_request(&mut second));
        let body = response.to_string();
        let reply = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        second.write_all(reply.as_bytes()).unwrap();
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
fn client_reuses_http_connection_for_consecutive_requests() {
    let (base_url, handle) = keep_alive_two_request_server();
    let client = EverOSClient::new("key", &base_url, 1.0).unwrap();

    let first = client
        .request_json("GET", "/api/v1/settings", None, None)
        .unwrap();
    let second = client
        .request_json("GET", "/api/v1/settings", None, None)
        .unwrap();

    assert_eq!(first["data"]["request_index"], 1);
    assert_eq!(second["data"]["request_index"], 2);
    let requests = handle.join().unwrap();
    assert_eq!(requests.len(), 2);
    assert!(requests[0].starts_with("GET /api/v1/settings HTTP/1.1"));
    assert!(requests[1].starts_with("GET /api/v1/settings HTTP/1.1"));
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
fn agent_visibility_workflow_status_mapping_is_stable() {
    use everos_hermes_rust::agent_visibility::workflow_status_from_agent_visibility;

    assert_eq!(
        workflow_status_from_agent_visibility(
            &json!({"agent_visibility_status":"visible"}),
            "fallback"
        ),
        "verified"
    );
    assert_eq!(
        workflow_status_from_agent_visibility(
            &json!({"agent_visibility_status":"partial"}),
            "fallback"
        ),
        "partially_verified"
    );
    assert_eq!(
        workflow_status_from_agent_visibility(
            &json!({"agent_visibility_status":"not_visible"}),
            "fallback"
        ),
        "agent_not_visible"
    );
    assert_eq!(
        workflow_status_from_agent_visibility(
            &json!({"agent_visibility_status":"error"}),
            "fallback"
        ),
        "agent_visibility_error"
    );
    assert_eq!(
        workflow_status_from_agent_visibility(
            &json!({"agent_visibility_status":"unchecked"}),
            "fallback"
        ),
        "fallback"
    );
}

#[test]
fn provider_tool_schemas_match_snapshot() {
    assert_eq!(
        provider_schema_snapshot(),
        snapshot_json("provider_tools.snapshot.json")
    );
}

#[test]
fn mcp_tool_schemas_match_snapshot() {
    assert_eq!(
        mcp_schema_snapshot(),
        snapshot_json("mcp_tools.snapshot.json")
    );
}

#[test]
fn provider_config_contract_clamps_drift_prone_fields() {
    let contract = provider_config_contract();
    let fields = contract["fields"].as_object().unwrap();
    let defaults = ProviderConfig::default();
    for (key, spec) in fields {
        assert_eq!(
            provider_config_usize_field(&defaults, key),
            spec["default"].as_u64().unwrap() as usize,
            "default for {key}"
        );
    }

    let home = temp_home("provider_config_contract_min");
    let below_min = Value::Object(
        fields
            .iter()
            .filter(|(_, spec)| spec["min"].as_u64().unwrap() > 0)
            .map(|(key, _)| (key.clone(), json!(0)))
            .collect(),
    );
    fs::write(home.join("everos.json"), below_min.to_string()).unwrap();
    let loaded = load_config(&home);
    for (key, spec) in fields {
        if spec["min"].as_u64().unwrap() > 0 {
            assert_eq!(
                provider_config_usize_field(&loaded, key),
                spec["min"].as_u64().unwrap() as usize,
                "min clamp for {key}"
            );
        }
    }

    let home = temp_home("provider_config_contract_max");
    let above_max = Value::Object(
        fields
            .iter()
            .map(|(key, spec)| (key.clone(), json!(spec["max"].as_u64().unwrap() + 1)))
            .collect(),
    );
    fs::write(home.join("everos.json"), above_max.to_string()).unwrap();
    let loaded = load_config(&home);
    for (key, spec) in fields {
        assert_eq!(
            provider_config_usize_field(&loaded, key),
            spec["max"].as_u64().unwrap() as usize,
            "max clamp for {key}"
        );
    }
}

#[test]
fn provider_agent_visibility_config_defaults_and_load_overrides() {
    let defaults = ProviderConfig::default();
    assert!(!defaults.agent_visibility_verify_after_write);
    assert!(!defaults.agent_visibility_verify_after_flush);
    assert!(defaults.agent_visibility_queries.is_empty());
    assert_eq!(defaults.agent_visibility_top_k, 5);
    assert_eq!(defaults.agent_visibility_timeout, 30.0);
    assert_eq!(defaults.agent_visibility_get_page_size, 20);
    assert_eq!(defaults.agent_visibility_retry_flush_attempts, 1);

    let home = temp_home("provider_visibility_config");
    fs::write(
        home.join("everos.json"),
        json!({
            "agent_visibility_verify_after_write": true,
            "agent_visibility_verify_after_flush": true,
            "agent_visibility_queries": "alpha, beta",
            "agent_visibility_top_k": 99,
            "agent_visibility_timeout": 0.1,
            "agent_visibility_get_page_size": 200,
            "agent_visibility_retry_flush_attempts": 9
        })
        .to_string(),
    )
    .unwrap();
    let loaded = load_config(&home);
    assert!(loaded.agent_visibility_verify_after_write);
    assert!(loaded.agent_visibility_verify_after_flush);
    assert_eq!(loaded.agent_visibility_queries, vec!["alpha", "beta"]);
    assert_eq!(loaded.agent_visibility_top_k, 20);
    assert_eq!(loaded.agent_visibility_timeout, 1.0);
    assert_eq!(loaded.agent_visibility_get_page_size, 100);
    assert_eq!(loaded.agent_visibility_retry_flush_attempts, 5);
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
            "everos_memory_save_and_verify",
            "everos_memory_import_and_verify",
            "everos_memory_verify_session",
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
fn mcp_tool_name_constant_matches_expected_thirteen_tools() {
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

#[test]
fn provider_workflow_tools_run_save_and_verify() {
    let _guard = ENV_LOCK.lock().unwrap();
    let home = temp_home("provider_save_verify");
    let (base_url, handle) = sequenced_request_server(
        vec![
            json!({"data":{"status":"queued","task_id":"task-save"}}),
            json!({"data":{"status":"success"}}),
            json!({"data":{"episodes":[{"summary":"pytest preference"}]}}),
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
            "everos_memory_save_and_verify",
            json!({"content":"User prefers pytest.","verification_query":"pytest preference","session_id":"sess-verify","flush":true}),
        )
        .unwrap();
    let response: Value = serde_json::from_str(&raw).unwrap();
    let requests = handle.join().unwrap();

    assert_eq!(response["ok"], true);
    assert_eq!(response["status"], "verified");
    assert_eq!(requests.len(), 3);

    remove_env("HERMES_HOME");
}

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
    assert_eq!(
        response["agent_visibility"]["agent_visibility_status"],
        "unchecked"
    );
    assert_eq!(response["agent_visibility"]["agent_raw_queued"], true);

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
fn rust_trajectory_options_builder_matches_legacy_signature() {
    let messages = vec![
        json!({"role":"system","content":"hidden"}),
        json!({"role":"user","content":"hello","timestamp":1_700_000_000u64}),
        json!({"role":"assistant","content":"world","timestamp":1_700_000_001u64}),
    ];
    let options = TrajectoryBuildOptions {
        session_id: "sess-options".to_string(),
        source: "pre_compress".to_string(),
        now_ms: Some(1_800_000_000_000),
        max_messages: 10,
        max_message_chars: 100,
        max_tool_result_chars: 50,
        max_payload_chars: 10_000,
        include_system: true,
    };

    let via_options = build_agent_trajectory_messages_with_options(&messages, &options);
    let legacy = build_agent_trajectory_messages(
        &messages,
        "sess-options",
        "pre_compress",
        Some(1_800_000_000_000),
        10,
        100,
        50,
        10_000,
        true,
    );

    assert_eq!(via_options, legacy);
    let roles: Vec<_> = via_options
        .messages
        .iter()
        .map(|message| message["role"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(roles, vec!["system", "user", "assistant"]);
}

#[test]
fn rust_context_engine_policy_trajectory_match_python_contract() {
    let config = json!({
        "max_context_chars": 12_000,
        "agent_recall": true,
        "include_recent_raw": true,
        "recent_raw_top_k": 4,
        "min_recall_query_chars": 8
    });
    assert_eq!(
        should_skip_recall("ok", "sess-1", &config),
        (true, "trivial_query".to_string())
    );
    assert_eq!(
        should_skip_recall("继续下一步实验", "sess-1", &config),
        (false, String::new())
    );
    assert_eq!(
        should_skip_capture("thanks", "done", "sess-1", &config),
        (true, "trivial_turn".to_string())
    );
    let cache_key = stable_query_key(" Debug   Cache ", "sess-1", &config);
    assert_eq!(cache_key.len(), 64);
    assert_eq!(
        cache_key,
        stable_query_key("debug cache", "sess-1", &config)
    );

    let messages = vec![
        json!({"role":"system","content":"do not export"}),
        json!({"role":"user","timestamp":1,"content":"run diagnostics <everos-context>old</everos-context> token=very-secret"}),
        json!({"role":"assistant","timestamp":2,"content":"","tool_calls":[{"id":"call-1","function":{"name":"diagnose","arguments":"{\"api_key\":\"hidden\"}"}}]}),
        json!({"role":"tool","timestamp":3,"tool_call_id":"call-1","content":"diagnostics ok"}),
        json!({"role":"tool","timestamp":4,"content":"missing id"}),
    ];
    let built = build_agent_trajectory_messages(
        &messages,
        "sess-traj",
        "pre_compress",
        Some(10_000),
        80,
        2_000,
        2_000,
        6_000,
        false,
    );
    let rebuilt = build_agent_trajectory_messages(
        &messages,
        "sess-traj",
        "pre_compress",
        Some(10_000),
        80,
        2_000,
        2_000,
        6_000,
        false,
    );
    assert_eq!(built.input_count, 5);
    assert_eq!(built.output_count, 3);
    assert_eq!(built.dropped_count, 2);
    assert_eq!(built.fingerprint, rebuilt.fingerprint);
    assert_eq!(built.messages[0]["source"], "pre_compress");
    assert!(
        built.messages[0]["message_id"]
            .as_str()
            .unwrap()
            .starts_with("eh_")
    );
    assert!(
        !built.messages[0]["content"]
            .as_str()
            .unwrap()
            .contains("everos-context")
    );
    assert!(
        !built.messages[0]["content"]
            .as_str()
            .unwrap()
            .contains("very-secret")
    );
    assert_eq!(
        built.messages[1]["content"],
        "[Assistant requested tool calls]"
    );
    assert_eq!(built.messages[1]["tool_calls"][0]["id"], "call-1");
    assert_eq!(built.messages[2]["tool_call_id"], "call-1");
}

#[test]
fn rust_context_assembler_renders_python_v2_sections_and_generic_agent_memory() {
    let cfg = ContextAssemblyConfig {
        max_context_chars: 12_000,
        profile_max_items: 3,
        agent_skills_max_items: 4,
        agent_cases_max_items: 4,
        episodic_max_items: 6,
        recent_raw_top_k: 4,
        min_score: 0.0,
    };
    let assembled = assemble_everos_context(
        Some(&json!({"data": {
            "profiles": [{"id":"profile-1","profile_data":{"explicit_info":["User verifies every phase"]},"score":1.0}],
            "agent_skills": [{"id":"skill-1","name":"timeout recovery","description":"poll status before retry","score":0.9}],
            "agent_memory": [{"id":"generic-1","task_intent":"debug cache","approach":"reuse cached result","score":0.8}],
            "episodes": [{"id":"episode-1","subject":"cache","summary":"Cache should avoid duplicate search","score":0.7}]
        }})),
        Some(
            &json!({"data": {"raw_messages": [{"id":"raw-1","role":"user","content":"recent raw clue","score":0.6}]}}),
        ),
        "debug cache",
        &cfg,
        "prefetch",
    );
    let text = assembled.text;
    assert!(text.starts_with("<everos-context version=\"2\" source=\"prefetch\">"));
    let profile_idx = text.find("<profile>").unwrap();
    let skill_idx = text.find("<agent_skills>").unwrap();
    let case_idx = text.find("<agent_cases>").unwrap();
    let episodic_idx = text.find("<episodic>").unwrap();
    let raw_idx = text.find("<recent_context>").unwrap();
    assert!(
        profile_idx < skill_idx
            && skill_idx < case_idx
            && case_idx < episodic_idx
            && episodic_idx < raw_idx
    );
    assert!(text.contains("User verifies every phase"));
    assert!(text.contains("timeout recovery: poll status before retry"));
    assert!(text.contains("[agent_memory] debug cache: reuse cached result"));
    assert!(text.contains("cache: Cache should avoid duplicate search [score=0.70]"));
    assert!(text.contains("user: recent raw clue"));
    assert_eq!(assembled.included_counts["recent_context"], 1);
}

#[test]
fn provider_prefetch_uses_v2_assembler_cache_agent_and_session_scoped_raw() {
    let _guard = ENV_LOCK.lock().unwrap();
    let home = temp_home("provider_context_engine");
    let (base_url, handle) = sequenced_request_server(
        vec![
            json!({"data":{"profiles":[{"id":"profile-1","profile_data":{"explicit_info":["User verifies every phase"]},"score":1.0}],"episodes":[{"id":"episode-1","subject":"cache","summary":"Cache should avoid duplicate search","score":0.7}]}}),
            json!({"data":{"agent_cases":[{"id":"case-1","task_intent":"debug cache","approach":"reuse cached result","score":0.9}]}}),
            json!({"data":{"raw_messages":[{"id":"raw-1","role":"user","content":"recent raw clue","score":0.8}]}}),
        ],
        500,
    );
    fs::write(
        home.join(".env"),
        format!("EVEROS_API_KEY=test-key\nEVEROS_USER_ID=u1\nEVEROS_BASE_URL={base_url}\n"),
    )
    .unwrap();
    fs::write(
        home.join("everos.json"),
        json!({"agent_recall":true,"include_recent_raw":true,"prefetch_cache_ttl_seconds":90})
            .to_string(),
    )
    .unwrap();
    remove_env("EVEROS_API_KEY");
    remove_env("EVEROS_USER_ID");
    remove_env("EVEROS_BASE_URL");
    set_env("HERMES_HOME", home.to_str().unwrap());

    let provider = EverOSProvider::initialize(ProviderInit::for_test("sess-1", &home)).unwrap();
    let context = provider.prefetch("debug cache", Some("sess-2"));
    let cached = provider.prefetch("debug cache", Some("sess-2"));
    let requests = handle.join().unwrap();
    let bodies: Vec<Value> = requests.iter().map(|raw| parse_http_body(raw)).collect();

    assert_eq!(context, cached);
    assert!(context.starts_with("<everos-context version=\"2\" source=\"prefetch\">"));
    assert!(context.contains("User verifies every phase"));
    assert!(context.contains("reuse cached result"));
    assert!(context.contains("recent raw clue"));
    assert_eq!(requests.len(), 3);
    assert_eq!(
        bodies[0]["memory_types"],
        json!(["episodic_memory", "profile"])
    );
    assert_eq!(bodies[1]["memory_types"], json!(["agent_memory"]));
    assert_eq!(bodies[2]["memory_types"], json!(["raw_message"]));
    assert_eq!(bodies[0].get("session_id"), None);
    assert_eq!(bodies[1].get("session_id"), None);
    assert_eq!(bodies[2]["filters"]["AND"][0]["session_id"], "sess-2");

    remove_env("HERMES_HOME");
}

#[test]
fn provider_sync_turn_adds_personal_message_ids_and_respects_agent_summary_flag() {
    let _guard = ENV_LOCK.lock().unwrap();
    let home = temp_home("provider_sync_ids");
    let (base_url, handle) =
        n_request_server(json!({"data":{"status":"queued","task_id":"task-sync"}}), 1);
    fs::write(
        home.join(".env"),
        format!("EVEROS_API_KEY=test-key\nEVEROS_USER_ID=u1\nEVEROS_BASE_URL={base_url}\n"),
    )
    .unwrap();
    fs::write(
        home.join("everos.json"),
        json!({"capture_agent_memory":true,"agent_summary_after_turn":false,"flush_after_turn":false}).to_string(),
    )
    .unwrap();
    remove_env("EVEROS_API_KEY");
    remove_env("EVEROS_USER_ID");
    remove_env("EVEROS_BASE_URL");
    set_env("HERMES_HOME", home.to_str().unwrap());

    let provider = EverOSProvider::initialize(ProviderInit::for_test("sess-1", &home)).unwrap();
    provider
        .sync_turn("remember deterministic ids", "Noted.", Some("sess-2"))
        .unwrap();
    let requests = handle.join().unwrap();
    let body = parse_http_body(&requests[0]);
    let messages = body["messages"].as_array().unwrap();

    assert_eq!(requests.len(), 1);
    assert!(requests[0].starts_with("POST /api/v1/memories "));
    assert_eq!(body["session_id"], "sess-2");
    assert_eq!(messages[0]["role"], "user");
    assert_eq!(messages[1]["role"], "assistant");
    assert!(
        messages[0]["message_id"]
            .as_str()
            .unwrap()
            .starts_with("eh_")
    );
    assert!(
        messages[1]["message_id"]
            .as_str()
            .unwrap()
            .starts_with("eh_")
    );

    remove_env("HERMES_HOME");
}

#[test]
fn provider_pre_compress_and_session_end_capture_structured_trajectory_with_dedupe() {
    let _guard = ENV_LOCK.lock().unwrap();
    let home = temp_home("provider_precompress");
    let (base_url, handle) = n_request_server(
        json!({"data":{"status":"queued","task_id":"task-agent"}}),
        2,
    );
    fs::write(
        home.join(".env"),
        format!("EVEROS_API_KEY=test-key\nEVEROS_USER_ID=u1\nEVEROS_BASE_URL={base_url}\n"),
    )
    .unwrap();
    fs::write(
        home.join("everos.json"),
        json!({"capture_agent_memory":true}).to_string(),
    )
    .unwrap();
    remove_env("EVEROS_API_KEY");
    remove_env("EVEROS_USER_ID");
    remove_env("EVEROS_BASE_URL");
    set_env("HERMES_HOME", home.to_str().unwrap());

    let provider = EverOSProvider::initialize(ProviderInit::for_test("sess-1", &home)).unwrap();
    let messages = vec![
        json!({"role":"user","timestamp":1,"content":"run diagnostics"}),
        json!({"role":"assistant","timestamp":2,"content":"","tool_calls":[{"id":"call-1","function":{"name":"diagnose"}}]}),
        json!({"role":"tool","timestamp":3,"tool_call_id":"call-1","content":"diagnostics ok"}),
        json!({"role":"assistant","timestamp":4,"content":"verified"}),
    ];

    let summary = provider.on_pre_compress(&messages).unwrap();
    provider.on_session_end(&messages).unwrap();
    let requests = handle.join().unwrap();
    let first_body = parse_http_body(&requests[0]);

    assert!(summary.contains("EverOS captured 4 agent trajectory messages for session sess-1"));
    assert_eq!(requests.len(), 2);
    assert!(requests[0].starts_with("POST /api/v1/memories/agent "));
    assert!(requests[1].starts_with("POST /api/v1/memories/flush "));
    assert_eq!(first_body["messages"][1]["tool_calls"][0]["id"], "call-1");
    assert_eq!(first_body["messages"][2]["tool_call_id"], "call-1");
    assert!(
        first_body["messages"]
            .as_array()
            .unwrap()
            .iter()
            .all(|message| message["source"] == "pre_compress")
    );

    remove_env("HERMES_HOME");
}

#[test]
fn provider_delegation_writes_child_session_id_prefix_and_agent_flush() {
    let _guard = ENV_LOCK.lock().unwrap();
    let home = temp_home("provider_delegation");
    let (base_url, handle) = n_request_server(
        json!({"data":{"status":"queued","task_id":"task-delegation"}}),
        2,
    );
    fs::write(
        home.join(".env"),
        format!("EVEROS_API_KEY=test-key\nEVEROS_USER_ID=u1\nEVEROS_BASE_URL={base_url}\n"),
    )
    .unwrap();
    fs::write(
        home.join("everos.json"),
        json!({"capture_agent_memory":true}).to_string(),
    )
    .unwrap();
    remove_env("EVEROS_API_KEY");
    remove_env("EVEROS_USER_ID");
    remove_env("EVEROS_BASE_URL");
    set_env("HERMES_HOME", home.to_str().unwrap());

    let provider = EverOSProvider::initialize(ProviderInit::for_test("sess-1", &home)).unwrap();
    provider
        .on_delegation(
            "investigate failing test",
            "fixed with a regression test",
            Some("child-42"),
        )
        .unwrap();
    let requests = handle.join().unwrap();
    let body = parse_http_body(&requests[0]);
    let assistant = &body["messages"][1];

    assert!(requests[0].starts_with("POST /api/v1/memories/agent "));
    assert!(requests[1].starts_with("POST /api/v1/memories/agent/flush "));
    assert!(
        assistant["content"]
            .as_str()
            .unwrap()
            .starts_with("[delegation child_session_id=child-42]")
    );
    assert_eq!(assistant["child_session_id"], "child-42");

    remove_env("HERMES_HOME");
}

#[test]
fn provider_session_end_still_flushes_personal_after_agent_write_error() {
    let _guard = ENV_LOCK.lock().unwrap();
    let home = temp_home("provider_session_end_error");
    let (base_url, handle) = sequenced_status_request_server(
        vec![
            (500, json!({"error":"agent write failed"})),
            (200, json!({"ok":true})),
        ],
        800,
    );
    fs::write(
        home.join(".env"),
        format!("EVEROS_API_KEY=test-key\nEVEROS_USER_ID=u1\nEVEROS_BASE_URL={base_url}\n"),
    )
    .unwrap();
    fs::write(
        home.join("everos.json"),
        json!({"capture_agent_memory":true}).to_string(),
    )
    .unwrap();
    remove_env("EVEROS_API_KEY");
    remove_env("EVEROS_USER_ID");
    remove_env("EVEROS_BASE_URL");
    set_env("HERMES_HOME", home.to_str().unwrap());

    let provider = EverOSProvider::initialize(ProviderInit::for_test("sess-err", &home)).unwrap();
    provider
        .on_session_end(&[json!({"role":"assistant","content":"final summary"})])
        .unwrap();
    let requests = handle.join().unwrap();

    assert_eq!(requests.len(), 2);
    assert!(requests[0].starts_with("POST /api/v1/memories/agent "));
    assert!(requests[1].starts_with("POST /api/v1/memories/flush "));

    remove_env("HERMES_HOME");
}

#[test]
fn provider_cli_routes_messages_precompress_session_end_and_delegation() {
    let _guard = ENV_LOCK.lock().unwrap();
    let home = temp_home("provider_cli_hooks");
    let queued = json!({"data":{"status":"queued","task_id":"task-cli"}});
    let (base_url, handle) = sequenced_request_server(vec![queued; 6], 1200);
    fs::write(
        home.join(".env"),
        format!("EVEROS_API_KEY=test-key\nEVEROS_USER_ID=u1\nEVEROS_BASE_URL={base_url}\n"),
    )
    .unwrap();
    fs::write(
        home.join("everos.json"),
        json!({"capture_agent_memory":true}).to_string(),
    )
    .unwrap();
    remove_env("EVEROS_API_KEY");
    remove_env("EVEROS_USER_ID");
    remove_env("EVEROS_BASE_URL");
    set_env("HERMES_HOME", home.to_str().unwrap());

    let bin = env!("CARGO_BIN_EXE_everos-hermes-rust");
    let state = json!({
        "session_id":"sess-cli",
        "hermes_home": home.to_string_lossy(),
        "platform":"cli",
        "agent_identity":"default"
    });
    let messages = json!([
        {"role":"user","timestamp":1,"content":"run cli hook test"},
        {"role":"assistant","timestamp":2,"content":"done"}
    ]);

    let run_provider = |args: Vec<String>, payload: Value| {
        let mut child = Command::new(bin)
            .args(args)
            .env_remove("EVEROS_API_KEY")
            .env_remove("EVEROS_USER_ID")
            .env_remove("EVEROS_BASE_URL")
            .env("HERMES_HOME", &home)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(payload.to_string().as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    };

    let precompress = run_provider(
        vec![
            "provider".into(),
            "on-pre-compress".into(),
            "--payload-stdin".into(),
        ],
        json!({"state": state.clone(), "messages": messages.clone()}),
    );
    assert!(
        precompress.status.success(),
        "{}",
        String::from_utf8_lossy(&precompress.stderr)
    );
    assert!(
        String::from_utf8_lossy(&precompress.stdout)
            .contains("EverOS captured 2 agent trajectory messages")
    );

    let session_end = run_provider(
        vec![
            "provider".into(),
            "on-session-end".into(),
            "--payload-stdin".into(),
        ],
        json!({"state": state.clone(), "messages": messages}),
    );
    assert!(
        session_end.status.success(),
        "{}",
        String::from_utf8_lossy(&session_end.stderr)
    );

    let delegation = run_provider(
        vec![
            "provider".into(),
            "on-delegation".into(),
            "--payload-stdin".into(),
        ],
        json!({
            "state": state,
            "task": "investigate child task",
            "result": "fixed in subagent",
            "child_session_id": "child-cli"
        }),
    );
    assert!(
        delegation.status.success(),
        "{}",
        String::from_utf8_lossy(&delegation.stderr)
    );

    let requests = handle.join().unwrap();
    assert_eq!(requests.len(), 6);
    assert!(requests[0].starts_with("POST /api/v1/memories/agent "));
    assert!(requests[1].starts_with("POST /api/v1/memories/agent "));
    assert!(requests[2].starts_with("POST /api/v1/memories/agent/flush "));
    assert!(requests[3].starts_with("POST /api/v1/memories/flush "));
    assert!(requests[4].starts_with("POST /api/v1/memories/agent "));
    assert!(requests[5].starts_with("POST /api/v1/memories/agent/flush "));
    assert_eq!(
        parse_http_body(&requests[1])["messages"][0]["source"],
        "session_end"
    );
    assert!(
        parse_http_body(&requests[4])["messages"][1]["content"]
            .as_str()
            .unwrap()
            .starts_with("[delegation child_session_id=child-cli]")
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

#[test]
fn rust_cli_provider_helper_names_make_short_lived_boundary_explicit() {
    let source = include_str!("../src/cli.rs");
    assert!(source.contains("short-lived compatibility shim"));
    assert!(source.contains("fn short_lived_provider_payload"));
    assert!(source.contains("fn short_lived_provider_from_payload"));
    assert!(source.contains("fn apply_short_lived_provider_state"));
    assert!(source.contains("fn normalize_short_lived_provider_init"));
    assert!(!source.contains("fn provider_payload("));
    assert!(!source.contains("fn provider_from_payload("));
    assert!(!source.contains("fn apply_state_value("));
    assert!(!source.contains("fn normalize_provider_init("));
}

#[test]
fn client_response_envelope_contract_cases() {
    let cases = snapshot_json("http_response_envelope_cases.json");
    for case in cases["cases"].as_array().unwrap() {
        match case["operation"].as_str().unwrap() {
            "request_json" => {
                let response = &case["server_response"];
                let request = &case["request"];
                let (base_url, handle) = if response["status"].as_u64().unwrap() == 204 {
                    one_status_empty_request_server(204)
                } else {
                    one_request_server(response["body"].clone())
                };
                let client = EverOSClient::new("test-key", &base_url, 10.0).unwrap();
                let actual = client
                    .request_json(
                        request["method"].as_str().unwrap(),
                        request["path"].as_str().unwrap(),
                        None,
                        None,
                    )
                    .unwrap();
                handle.join().unwrap();
                assert_eq!(actual, case["expected_response"]);
            }
            "delete_memories" => {
                let (base_url, handle) = one_status_empty_request_server(204);
                let client = EverOSClient::new("test-key", &base_url, 10.0).unwrap();
                let args = &case["args"];
                let actual = client
                    .delete_memories(args["memory_id"].as_str(), None, None)
                    .unwrap();
                let request = handle.join().unwrap();
                assert_eq!(parse_http_body(&request), case["expected_request"]["body"]);
                assert_eq!(actual, case["expected_response"]);
            }
            other => panic!("unsupported http response contract case: {other}"),
        }
    }
}

#[test]
fn client_param_normalization_contract_cases() {
    let cases = snapshot_json("client_param_normalization_cases.json");
    for case in cases["cases"].as_array().unwrap() {
        match case["surface"].as_str().unwrap() {
            "client.search" => {
                let (base_url, handle) = one_request_server(json!({"data":{"episodes":[]}}));
                let client = EverOSClient::new("test-key", &base_url, 10.0).unwrap();
                let args = &case["args"];
                client
                    .search_memories(
                        args["query"].as_str().unwrap(),
                        args["user_id"].as_str(),
                        None,
                        None,
                        args["method"].as_str().unwrap(),
                        None,
                        5,
                        None,
                        false,
                        false,
                        None,
                    )
                    .unwrap();
                let body = parse_http_body(&handle.join().unwrap());
                assert_eq!(
                    body["method"],
                    case["expected_request"]["body_subset"]["method"]
                );
            }
            "client.get" => {
                let (base_url, handle) = one_request_server(json!({"data":{"items":[]}}));
                let client = EverOSClient::new("test-key", &base_url, 10.0).unwrap();
                let args = &case["args"];
                client
                    .get_memories(
                        args["user_id"].as_str(),
                        None,
                        None,
                        args["memory_type"].as_str().unwrap(),
                        1,
                        20,
                        "timestamp",
                        args["rank_order"].as_str().unwrap(),
                    )
                    .unwrap();
                let body = parse_http_body(&handle.join().unwrap());
                assert_eq!(
                    body["rank_order"],
                    case["expected_request"]["body_subset"]["rank_order"]
                );
            }
            "mcp.add_memories" => {
                let err =
                    everos_hermes_rust::mcp::call_tool("everos_add_memories", case["args"].clone())
                        .unwrap_err()
                        .to_string();
                assert!(err.contains(case["error_contains"].as_str().unwrap()));
            }
            "mcp.flush" => {
                let _guard = ENV_LOCK.lock().unwrap();
                set_env("EVEROS_API_KEY", "test-key");
                set_env("EVEROS_USER_ID", "u1");
                set_env("EVEROS_BASE_URL", "http://127.0.0.1:9");
                let err = everos_hermes_rust::mcp::call_tool(
                    "everos_flush_memories",
                    case["args"].clone(),
                )
                .unwrap_err()
                .to_string();
                assert!(err.contains(case["error_contains"].as_str().unwrap()));
                remove_env("EVEROS_API_KEY");
                remove_env("EVEROS_USER_ID");
                remove_env("EVEROS_BASE_URL");
            }
            other => panic!("unsupported param normalization contract case: {other}"),
        }
    }
}

#[test]
fn client_session_filter_requires_exact_non_empty_operator_and_group_helpers_fail_closed() {
    let client = EverOSClient::new("test-key", "http://127.0.0.1:9", 0.05).unwrap();
    for filters in [
        json!({"session_id": {}}),
        json!({"session_id": {"eq": ""}}),
        json!({"session_id": {"eq": 123}}),
        json!({"session_id": ""}),
        json!({"AND": [{"session_id": {"eq": 123}}]}),
    ] {
        assert!(
            client
                .search_memories(
                    "coffee",
                    Some("u1"),
                    Some("sess"),
                    Some(filters),
                    "hybrid",
                    None,
                    5,
                    None,
                    false,
                    false,
                    None,
                )
                .is_err()
        );
    }
}
