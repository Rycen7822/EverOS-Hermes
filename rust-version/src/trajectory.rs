use crate::formatting::compact_json;
use regex::Regex;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrajectoryBuildResult {
    pub messages: Vec<Value>,
    pub fingerprint: String,
    pub warnings: Vec<String>,
    pub source: String,
    pub input_count: usize,
    pub output_count: usize,
    pub dropped_count: usize,
    pub estimated_chars: usize,
}

#[allow(clippy::too_many_arguments)]
pub fn build_agent_trajectory_messages(
    messages: &[Value],
    session_id: &str,
    source: &str,
    now_ms: Option<u128>,
    max_messages: usize,
    max_message_chars: usize,
    max_tool_result_chars: usize,
    max_payload_chars: usize,
    include_system: bool,
) -> TrajectoryBuildResult {
    let base_now = now_ms.unwrap_or_else(current_ms);
    let mut warnings = Vec::new();
    let mut output = Vec::new();
    let mut dropped_count = 0usize;

    for (input_index, raw) in messages.iter().enumerate() {
        let role = raw
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase();
        if !matches!(role.as_str(), "user" | "assistant" | "tool" | "system") {
            dropped_count += 1;
            warnings.push(format!(
                "dropped unsupported role at index {input_index}: {}",
                if role.is_empty() {
                    "<empty>"
                } else {
                    role.as_str()
                }
            ));
            continue;
        }
        if role == "system" && !include_system {
            dropped_count += 1;
            continue;
        }
        let tool_call_id = raw
            .get("tool_call_id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string();
        if role == "tool" && tool_call_id.is_empty() {
            dropped_count += 1;
            warnings.push(format!(
                "dropped tool message at index {input_index}: missing tool_call_id"
            ));
            continue;
        }

        let tool_calls = if role == "assistant" {
            raw.get("tool_calls")
                .filter(|value| !value.is_null())
                .map(scrub_value)
        } else {
            None
        };
        let mut content = content_to_text(raw.get("content"));
        if content.trim().is_empty() && role == "assistant" && tool_calls.is_some() {
            content = "[Assistant requested tool calls]".to_string();
        }
        content = strip_context_blocks(&redact_text(&content))
            .trim()
            .to_string();
        let limit = if role == "tool" {
            max_tool_result_chars
        } else {
            max_message_chars
        };
        content = truncate(&content, limit);
        if content.trim().is_empty() {
            dropped_count += 1;
            warnings.push(format!(
                "dropped {role} message at index {input_index}: empty content"
            ));
            continue;
        }

        let timestamp = normalize_timestamp(raw.get("timestamp"), base_now + output.len() as u128);
        let mut map = Map::new();
        map.insert("role".to_string(), Value::String(role.clone()));
        map.insert("content".to_string(), Value::String(content.clone()));
        map.insert("timestamp".to_string(), json!(timestamp));
        map.insert(
            "message_id".to_string(),
            Value::String(message_id(
                session_id,
                input_index,
                &role,
                &tool_call_id,
                raw.get("timestamp"),
                &content,
                tool_calls.as_ref(),
            )),
        );
        map.insert("source".to_string(), Value::String(source.to_string()));
        if role == "tool" {
            map.insert("tool_call_id".to_string(), Value::String(tool_call_id));
        }
        if let Some(tool_calls) = tool_calls {
            map.insert("tool_calls".to_string(), tool_calls);
        }
        output.push(Value::Object(map));
    }

    if max_messages > 0 && output.len() > max_messages {
        let extra = output.len() - max_messages;
        output = output.split_off(extra);
        dropped_count += extra;
        warnings.push(format!(
            "dropped {extra} oldest messages due to max_messages"
        ));
    }

    let (output, budget_dropped) = enforce_payload_budget(output, max_payload_chars);
    if budget_dropped > 0 {
        dropped_count += budget_dropped;
        warnings.push(format!(
            "dropped {budget_dropped} oldest messages due to max_payload_chars"
        ));
    }
    let estimated_chars = estimate_chars(&output);
    let fingerprint = fingerprint(session_id, &output);
    let output_count = output.len();
    TrajectoryBuildResult {
        messages: output,
        fingerprint,
        warnings,
        source: source.to_string(),
        input_count: messages.len(),
        output_count,
        dropped_count,
        estimated_chars,
    }
}

fn current_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn content_to_text(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(text)) => text.clone(),
        Some(other) => compact_json(other),
    }
}

fn strip_context_blocks(text: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?is)<everos-context\b[^>]*>.*?</everos-context>|<memory-context\b[^>]*>.*?</memory-context>").unwrap()
    })
    .replace_all(text, "")
    .to_string()
}

fn redact_text(text: &str) -> String {
    static AUTH_RE: OnceLock<Regex> = OnceLock::new();
    static SK_RE: OnceLock<Regex> = OnceLock::new();
    static KV_RE: OnceLock<Regex> = OnceLock::new();
    let text = AUTH_RE
        .get_or_init(|| Regex::new(r"(?i)Authorization:\s*Bearer\s+[A-Za-z0-9._\-]+").unwrap())
        .replace_all(text, "[REDACTED]")
        .to_string();
    let text = SK_RE
        .get_or_init(|| Regex::new(r"\bsk-[A-Za-z0-9]{12,}\b").unwrap())
        .replace_all(&text, "[REDACTED]")
        .to_string();
    KV_RE
        .get_or_init(|| {
            Regex::new(r"(?i)\b(api[_-]?key|token|password|secret)\s*[:=]\s*[^\s,;\]}]+").unwrap()
        })
        .replace_all(&text, "[REDACTED]")
        .to_string()
}

fn scrub_value(value: &Value) -> Value {
    match value {
        Value::String(text) => Value::String(strip_context_blocks(&redact_text(text))),
        Value::Array(items) => Value::Array(items.iter().map(scrub_value).collect()),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, value)| (key.clone(), scrub_value(value)))
                .collect(),
        ),
        other => other.clone(),
    }
}

fn truncate(text: &str, limit: usize) -> String {
    if limit == 0 || text.chars().count() <= limit {
        return text.to_string();
    }
    let marker = "[truncated]";
    let keep = limit.saturating_sub(marker.chars().count());
    format!("{}{}", text.chars().take(keep).collect::<String>(), marker)
}

fn normalize_timestamp(value: Option<&Value>, fallback_ms: u128) -> u128 {
    let Some(value) = value else {
        return fallback_ms;
    };
    if let Some(number) = value.as_f64()
        && number.is_finite()
    {
        return if number < 1_000_000_000_000.0 {
            (number * 1000.0) as u128
        } else {
            number as u128
        };
    }
    if let Some(text) = value.as_str()
        && let Ok(number) = text.trim().parse::<f64>()
    {
        return if number < 1_000_000_000_000.0 {
            (number * 1000.0) as u128
        } else {
            number as u128
        };
    }
    fallback_ms
}

fn canonical_json(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "null".to_string())
}

fn hash_text(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    let digest = hasher.finalize();
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn message_id(
    session_id: &str,
    input_index: usize,
    role: &str,
    tool_call_id: &str,
    original_timestamp: Option<&Value>,
    content: &str,
    tool_calls: Option<&Value>,
) -> String {
    let original_timestamp_part = original_timestamp.map(value_to_string).unwrap_or_default();
    let tool_calls_hash = tool_calls
        .map(|value| hash_text(&canonical_json(value)))
        .unwrap_or_default();
    let payload = [
        session_id.to_string(),
        input_index.to_string(),
        role.to_string(),
        tool_call_id.to_string(),
        original_timestamp_part,
        hash_text(content),
        tool_calls_hash,
    ]
    .join("|");
    format!("eh_{}", &hash_text(&payload)[..32])
}

fn value_to_string(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => compact_json(other),
    }
}

fn estimate_chars(messages: &[Value]) -> usize {
    messages
        .iter()
        .map(|message| canonical_json(message).len())
        .sum()
}

fn enforce_payload_budget(messages: Vec<Value>, max_payload_chars: usize) -> (Vec<Value>, usize) {
    if max_payload_chars == 0 || estimate_chars(&messages) <= max_payload_chars {
        return (messages, 0);
    }
    let last_user_index = messages
        .iter()
        .enumerate()
        .filter_map(|(index, message)| {
            (message.get("role").and_then(Value::as_str) == Some("user")).then_some(index)
        })
        .next_back();
    let protected_start = last_user_index.unwrap_or_else(|| messages.len().saturating_sub(1));
    let protected = messages[protected_start..].to_vec();
    let mut prefix = messages[..protected_start].to_vec();
    let mut dropped = 0usize;
    while !prefix.is_empty()
        && estimate_chars(&[prefix.clone(), protected.clone()].concat()) > max_payload_chars
    {
        prefix.remove(0);
        dropped += 1;
    }
    let combined = [prefix.clone(), protected.clone()].concat();
    if estimate_chars(&combined) <= max_payload_chars {
        return (combined, dropped);
    }
    (protected, dropped + prefix.len())
}

fn fingerprint(session_id: &str, messages: &[Value]) -> String {
    let normalized = Value::Array(
        messages
            .iter()
            .map(|message| {
                let mut map = message.as_object().cloned().unwrap_or_default();
                map.remove("message_id");
                map.remove("timestamp");
                map.remove("source");
                Value::Object(map)
            })
            .collect(),
    );
    hash_text(&canonical_json(
        &json!({"session_id": session_id, "messages": normalized}),
    ))
}
