use super::*;

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
    let source = include_str!("../../src/cli.rs");
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
