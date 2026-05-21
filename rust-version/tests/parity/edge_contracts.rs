use everos_hermes_rust::{
    mcp::read_frame, redaction::sanitized_error_message,
    trajectory::build_agent_trajectory_messages,
};
use serde_json::{Value, json};
use std::io::Cursor;

#[test]
fn mcp_read_frame_rejects_oversized_headers_and_bodies() {
    for raw in [format!("{}\n", "X".repeat(9000)), "X".repeat(9000)] {
        let mut cursor = Cursor::new(raw.into_bytes());
        assert_eq!(
            read_frame(&mut cursor).unwrap_err().kind(),
            std::io::ErrorKind::InvalidData
        );
    }

    let too_large = 16 * 1024 * 1024 + 1;
    let mut huge_body =
        Cursor::new(format!("Content-Length: {too_large}\r\n\r\n{}\n", "{}").into_bytes());
    assert_eq!(
        read_frame(&mut huge_body).unwrap_err().kind(),
        std::io::ErrorKind::InvalidData
    );
}

#[test]
fn rust_trajectory_redacts_json_style_tool_call_arguments_and_secret_keyed_values() {
    let messages = vec![
        json!({"role":"assistant","timestamp":1,"content":"tool call issued","tool_calls":[{"id":"call-1","type":"function","function":{"name":"save","arguments":"{\"api_key\":\"json-secret\",\"credentials\":{\"client_email\":\"json-credentials-secret\",\"client_id\":\"json-client-id-secret\"},\"nested\":{\"token\":\"nested-secret\"}}"},"metadata":{"authorization":"Bearer header-secret","credentials":"metadata-credentials-secret","safe":"visible"}}]}),
        json!({"role":"tool","timestamp":2,"tool_call_id":"call-1","content":{"credentials":{"client_email":"tool-email-secret","client_id":"tool-client-id-secret"},"ok":true}}),
    ];
    let result = build_agent_trajectory_messages(
        &messages,
        "sess",
        "review",
        Some(1),
        10,
        2000,
        2000,
        5000,
        false,
    );
    let rendered = Value::Array(result.messages).to_string();
    for secret in [
        "json-secret",
        "json-credentials-secret",
        "json-client-id-secret",
        "nested-secret",
        "header-secret",
        "metadata-credentials-secret",
        "tool-email-secret",
        "tool-client-id-secret",
    ] {
        assert!(!rendered.contains(secret));
    }
    assert!(rendered.contains("[REDACTED]"));
    assert!(rendered.contains("visible"));
}

#[test]
fn rust_redaction_handles_bearer_quoted_delimiters_and_truncates_errors() {
    let quoted_value = ["quoted,", "semi;", "with]delimiters"].concat();
    let credentials_value = ["cred", " plural tail"].concat();
    let credentials_key = ["creden", "tials"].concat();
    let email_secret = ["email", "-secret"].concat();
    let key_secret = ["key", "-secret"].concat();
    let client_id_secret = ["client", "-id-secret"].concat();
    let credentials_blob =
        format!(r#"{{\"client_email\":\"{email_secret}\",\"private_key\":\"{key_secret}\"}}"#);
    let credentials_object =
        format!(r#"{{"client_email":"{email_secret}","client_id":"{client_id_secret}"}}"#);
    let rendered = sanitized_error_message(format!(
        "backend failed token=\"{quoted_value}\" {credentials_key}={credentials_value} {credentials_key}=\"{credentials_blob}\" {credentials_key}={credentials_object} Authorization: Bearer *** request_id=req-redaction {}",
        "x".repeat(1000)
    ));

    for secret in [
        &quoted_value,
        &credentials_value,
        &email_secret,
        &key_secret,
        &client_id_secret,
    ] {
        assert!(!rendered.contains(secret));
    }
    assert!(rendered.contains("[REDACTED]"));
    assert!(rendered.contains("request_id=req-redaction"));
    assert!(rendered.len() < 650);
}
