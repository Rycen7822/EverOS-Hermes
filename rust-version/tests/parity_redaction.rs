use everos_hermes_rust::trajectory::build_agent_trajectory_messages;
use serde_json::{Value, json};

#[test]
fn rust_trajectory_redacts_json_style_tool_call_arguments_and_secret_keyed_values() {
    let messages = vec![
        json!({
            "role":"assistant",
            "timestamp":1,
            "content":"tool call issued",
            "tool_calls":[{
                "id":"call-1",
                "type":"function",
                "function":{
                    "name":"save",
                    "arguments":"{\"api_key\":\"json-secret\",\"credentials\":{\"client_email\":\"json-credentials-secret\",\"client_id\":\"json-client-id-secret\"},\"nested\":{\"token\":\"nested-secret\"}}"
                },
                "metadata":{"authorization":"Bearer header-secret","credentials":"metadata-credentials-secret","safe":"visible"}
            }]
        }),
        json!({
            "role":"tool",
            "timestamp":2,
            "tool_call_id":"call-1",
            "content":{"credentials":{"client_email":"tool-email-secret","client_id":"tool-client-id-secret"},"ok":true}
        }),
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
    let rendered = Value::Array(result.messages.clone()).to_string();
    assert!(!rendered.contains("json-secret"));
    assert!(!rendered.contains("json-credentials-secret"));
    assert!(!rendered.contains("json-client-id-secret"));
    assert!(!rendered.contains("nested-secret"));
    assert!(!rendered.contains("header-secret"));
    assert!(!rendered.contains("metadata-credentials-secret"));
    assert!(!rendered.contains("tool-email-secret"));
    assert!(!rendered.contains("tool-client-id-secret"));
    assert!(rendered.contains("[REDACTED]"));
    assert!(rendered.contains("visible"));
}

#[test]
fn rust_redaction_handles_bearer_quoted_delimiters_and_truncates_errors() {
    let bearer_token = ["abc", "+def/", "ghi=~tail"].concat();
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
    let rendered = everos_hermes_rust::redaction::sanitized_error_message(format!(
        "backend failed token=\"{quoted_value}\" {credentials_key}={credentials_value} {credentials_key}=\"{credentials_blob}\" {credentials_key}={credentials_object} Authorization: Bearer {bearer_token} request_id=req-redaction {}",
        "x".repeat(1000)
    ));

    assert!(!rendered.contains(&bearer_token));
    assert!(!rendered.contains(&quoted_value));
    assert!(!rendered.contains(&credentials_value));
    assert!(!rendered.contains(&email_secret));
    assert!(!rendered.contains(&key_secret));
    assert!(!rendered.contains(&client_id_secret));
    assert!(rendered.contains("[REDACTED]"));
    assert!(rendered.contains("request_id=req-redaction"));
    assert!(rendered.len() < 650);
}
