use std::io::Cursor;

#[test]
fn mcp_read_frame_allows_large_raw_json_lines_within_body_limit() {
    let large_id = "x".repeat(9000);
    let mut raw = format!(r#"{{"jsonrpc":"2.0","id":"{large_id}","method":"ping"}}"#);
    raw.push('\n');
    let mut frame = Cursor::new(raw.into_bytes());

    let value = everos_hermes_rust::mcp::read_frame(&mut frame)
        .unwrap()
        .unwrap();

    assert_eq!(value["method"], "ping");
    assert_eq!(value["id"].as_str().unwrap().len(), 9000);
}

#[test]
fn mcp_read_frame_allows_large_raw_json_lines_with_leading_whitespace() {
    let mut raw = " ".repeat(9000);
    raw.push_str(r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#);
    raw.push('\n');
    let mut frame = Cursor::new(raw.into_bytes());

    let value = everos_hermes_rust::mcp::read_frame(&mut frame)
        .unwrap()
        .unwrap();

    assert_eq!(value["method"], "ping");
}

#[test]
fn mcp_read_frame_rejects_oversized_headers_and_bodies() {
    let mut huge_header = Cursor::new(format!("{}\n", "X".repeat(9000)).into_bytes());
    let err = everos_hermes_rust::mcp::read_frame(&mut huge_header).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);

    let mut no_newline_header = Cursor::new("X".repeat(9000).into_bytes());
    let err = everos_hermes_rust::mcp::read_frame(&mut no_newline_header).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);

    let too_large = 16 * 1024 * 1024 + 1;
    let mut huge_body = Cursor::new(
        format!(
            "Content-Length: {too_large}\r\n\r\n{}
",
            "{}"
        )
        .into_bytes(),
    );
    let err = everos_hermes_rust::mcp::read_frame(&mut huge_body).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
}
