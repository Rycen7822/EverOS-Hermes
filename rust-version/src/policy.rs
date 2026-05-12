use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

const TRIVIAL_RECALL: [&str; 10] = [
    "ok",
    "okay",
    "k",
    "yes",
    "no",
    "done",
    "thanks",
    "thank you",
    "hi",
    "hello",
];
const TRIVIAL_CAPTURE_EXTRA: [&str; 2] = ["ack", "roger"];
const REAL_SHORT_TASKS: [&str; 4] = ["继续", "下一步", "继续下一步", "继续下一步实验"];
const RELEVANT_CONFIG_KEYS: [&str; 10] = [
    "max_context_chars",
    "include_recent_raw",
    "recent_raw_top_k",
    "profile_max_items",
    "agent_skills_max_items",
    "agent_cases_max_items",
    "episodic_max_items",
    "min_score",
    "agent_recall",
    "agent_memory_types",
];

pub fn should_skip_recall(query: &str, session_id: &str, config: &Value) -> (bool, String) {
    let normalized = normalize_query(query);
    if normalized.is_empty() {
        return (true, "empty_query".to_string());
    }
    if let Some(reason) = session_skip_reason(session_id) {
        return (true, reason.to_string());
    }
    if REAL_SHORT_TASKS.contains(&normalized.as_str()) {
        return (false, String::new());
    }
    let min_chars = int_config(config, "min_recall_query_chars", 8).max(0) as usize;
    if normalized.chars().count() < min_chars && TRIVIAL_RECALL.contains(&normalized.as_str()) {
        return (true, "trivial_query".to_string());
    }
    (false, String::new())
}

pub fn should_skip_capture(
    user_content: &str,
    assistant_content: &str,
    session_id: &str,
    _config: &Value,
) -> (bool, String) {
    let user = normalize_query(user_content);
    let assistant = normalize_query(assistant_content);
    if user.is_empty() || assistant.is_empty() {
        return (true, "empty_turn".to_string());
    }
    if let Some(reason) = session_skip_reason(session_id) {
        return (true, reason.to_string());
    }
    if REAL_SHORT_TASKS.contains(&user.as_str()) {
        return (false, String::new());
    }
    if TRIVIAL_RECALL.contains(&user.as_str()) || TRIVIAL_CAPTURE_EXTRA.contains(&user.as_str()) {
        return (true, "trivial_turn".to_string());
    }
    (false, String::new())
}

pub fn stable_query_key(query: &str, session_id: &str, config: &Value) -> String {
    let mut relevant = Map::new();
    if let Some(map) = config.as_object() {
        for key in RELEVANT_CONFIG_KEYS {
            if let Some(value) = map.get(key) {
                relevant.insert(key.to_string(), value.clone());
            }
        }
    }
    let payload = json!({
        "query": normalize_query(query),
        "session_id": session_id,
        "config": Value::Object(relevant),
    });
    hash_text(&serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string()))
}

fn normalize_query(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_lowercase()
}

fn session_skip_reason(session_id: &str) -> Option<&'static str> {
    let session = session_id.trim().to_ascii_lowercase();
    if session.starts_with("temp:") {
        Some("temporary_session")
    } else if session.starts_with("internal:") {
        Some("internal_session")
    } else {
        None
    }
}

fn int_config(config: &Value, key: &str, default: i64) -> i64 {
    config
        .get(key)
        .and_then(|value| {
            value
                .as_i64()
                .or_else(|| value.as_str().and_then(|text| text.parse::<i64>().ok()))
        })
        .unwrap_or(default)
}

fn hash_text(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    let digest = hasher.finalize();
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}
