use everos_hermes_rust::client::{DEFAULT_MEMORY_TYPES, EverOSClient};
use everos_hermes_rust::context_assembler::{ContextAssemblyConfig, assemble_everos_context};
use everos_hermes_rust::env::{get_env, read_dotenv};
use everos_hermes_rust::formatting::format_search_context;
use everos_hermes_rust::mcp::tool_definitions;
use everos_hermes_rust::policy::{should_skip_capture, should_skip_recall, stable_query_key};
use everos_hermes_rust::provider::{
    EverOSProvider, ProviderConfig, ProviderInit, load_config, provider_tool_schemas,
};

use serde_json::{Value, json};
use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
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

fn set_client_env(base_url: &str) {
    set_env("EVEROS_API_KEY", "test-key");
    set_env("EVEROS_USER_ID", "u1");
    set_env("EVEROS_BASE_URL", base_url);
}

fn clear_client_env() {
    remove_env("EVEROS_API_KEY");
    remove_env("EVEROS_USER_ID");
    remove_env("EVEROS_BASE_URL");
}

fn use_home_dotenv(home: &Path, base_url: &str) {
    fs::write(
        home.join(".env"),
        format!("EVEROS_API_KEY=test-key\nEVEROS_USER_ID=u1\nEVEROS_BASE_URL={base_url}\n"),
    )
    .unwrap();
    clear_client_env();
    set_env("HERMES_HOME", home.to_str().unwrap());
}

fn one_request_server(response: Value) -> (String, thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let raw = read_http_request(&mut stream);
        let body = response.to_string();
        let reply = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(reply.as_bytes()).unwrap();
        raw
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

fn provider_property_signature(name: &str, schema: &Value) -> Value {
    let kind = schema.get("type").and_then(Value::as_str).unwrap_or("?");
    if let (true, Some(item)) = (kind == "array", schema.get("items")) {
        let item_kind = item.get("type").and_then(Value::as_str).unwrap_or("?");
        let enum_values = sorted_string_values(item.get("enum"));
        let suffix = if enum_values.is_empty() {
            String::new()
        } else {
            format!(":{}", enum_values.join("|"))
        };
        return json!(format!("{name}:array<{item_kind}{suffix}>"));
    }
    let enum_values = sorted_string_values(schema.get("enum"));
    let suffix = if enum_values.is_empty() {
        String::new()
    } else {
        format!(":{}", enum_values.join("|"))
    };
    json!(format!("{name}:{kind}{suffix}"))
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
                let mut keys = properties.keys().cloned().collect::<Vec<_>>();
                keys.sort();
                let mut item = json!({
                    "name": schema["name"],
                    "required": sorted_string_values(params.get("required")),
                    "properties": keys.iter().map(|key| provider_property_signature(key, properties.get(key).unwrap())).collect::<Vec<_>>(),
                });
                if item["required"].as_array().is_some_and(Vec::is_empty) {
                    item.as_object_mut().unwrap().remove("required");
                }
                item
            })
            .collect(),
    )
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
                let output_shape =
                    if output_required == ["result"] && output_properties == ["result"] {
                        json!("result")
                    } else if output_required.is_empty()
                        && output_properties
                            == [
                                "ok",
                                "retryable",
                                "status",
                                "suggested_next_actions",
                                "workflow",
                            ]
                    {
                        json!("workflow")
                    } else {
                        json!({"required": output_required, "properties": output_properties})
                    };
                let annotations = schema.get("annotations").unwrap_or(&Value::Null);
                let annotation_profile = [
                    if annotations["readOnlyHint"].as_bool().unwrap_or(false) {
                        "read"
                    } else {
                        "write"
                    },
                    if annotations["destructiveHint"].as_bool().unwrap_or(false) {
                        "destructive"
                    } else {
                        "safe"
                    },
                    if annotations["idempotentHint"].as_bool().unwrap_or(false) {
                        "idem"
                    } else {
                        "nonidem"
                    },
                    if annotations["openWorldHint"].as_bool().unwrap_or(false) {
                        "open"
                    } else {
                        "closed"
                    },
                ]
                .join(":");
                let mut item = json!({
                    "name": schema["name"],
                    "required": sorted_string_values(input.get("required")),
                    "properties": property_names,
                    "output_shape": output_shape,
                    "annotation_profile": annotation_profile,
                });
                let obj = item.as_object_mut().unwrap();
                if obj["required"].as_array().is_some_and(Vec::is_empty) {
                    obj.remove("required");
                }
                if obj["properties"].as_array().is_some_and(Vec::is_empty) {
                    obj.remove("properties");
                }
                if obj["output_shape"].as_str() == Some("result") {
                    obj.remove("output_shape");
                }
                item
            })
            .collect(),
    )
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

fn sequenced_request_server(
    responses: Vec<Value>,
    idle_timeout_ms: u64,
) -> (String, thread::JoinHandle<Vec<String>>) {
    sequenced_status_request_server(
        responses
            .into_iter()
            .map(|response| (200, response))
            .collect(),
        idle_timeout_ms,
    )
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

#[path = "parity/client_core.rs"]
mod client_core;
#[path = "parity/contracts_settings.rs"]
mod contracts_settings;
#[path = "parity/edge_contracts.rs"]
mod edge_contracts;
#[path = "parity/import_verify.rs"]
mod import_verify;
#[path = "parity/provider_lifecycle.rs"]
mod provider_lifecycle;
#[path = "parity/provider_mcp_tools.rs"]
mod provider_mcp_tools;
#[path = "parity/schemas_config.rs"]
mod schemas_config;
#[path = "parity/stdio_cli_contracts.rs"]
mod stdio_cli_contracts;
