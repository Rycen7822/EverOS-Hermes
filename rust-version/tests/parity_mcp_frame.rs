use std::io::Cursor;

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
