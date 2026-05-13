use crate::client::{EverOSClient, EverOSError};
use serde_json::Value;

pub fn flush_memories_with_retry(
    client: &EverOSClient,
    user_id: &str,
    session_id: Option<&str>,
    scope: &str,
    timeout: Option<f64>,
    max_attempts: usize,
) -> Result<(Value, usize), EverOSError> {
    let max_attempts = max_attempts.max(1);
    let mut attempt_count = 0usize;
    loop {
        attempt_count += 1;
        match client.flush_memories_scoped(user_id, session_id, scope, timeout) {
            Ok(response) => return Ok((response, attempt_count)),
            Err(err) if attempt_count < max_attempts && is_retryable_flush_send_error(&err) => {}
            Err(err) => return Err(err),
        }
    }
}

fn is_retryable_flush_send_error(err: &EverOSError) -> bool {
    let EverOSError::Request(message) = err else {
        return false;
    };
    let text = message.to_ascii_lowercase();
    text.contains("error sending request")
        || text.contains("connection")
        || text.contains("closed")
        || text.contains("reset")
        || text.contains("unexpected eof")
        || text.contains("incomplete")
}
