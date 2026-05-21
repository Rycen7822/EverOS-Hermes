use super::*;

#[test]
fn mcp_stdio_binary_initializes() {
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
