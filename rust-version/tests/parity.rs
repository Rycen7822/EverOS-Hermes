use everos_hermes_rust::client::{DEFAULT_BASE_URL, DEFAULT_MEMORY_TYPES, EverOSClient};
use everos_hermes_rust::env::{get_env, read_dotenv};
use everos_hermes_rust::formatting::format_search_context;
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
use std::time::{SystemTime, UNIX_EPOCH};

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
    assert_eq!(response["task_id"], "task-9");
    assert!(request.starts_with("POST /api/v1/memories HTTP/1.1"));
    assert_eq!(parse_http_body(&request)["user_id"], "u1");

    remove_env("HERMES_HOME");
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
