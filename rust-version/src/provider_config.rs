use crate::client::{DEFAULT_BASE_URL, DEFAULT_MEMORY_TYPES};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::fs;
use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub base_url: String,
    pub user_id: String,
    pub auto_recall: bool,
    pub auto_capture: bool,
    pub flush_after_turn: bool,
    pub search_method: String,
    pub top_k: u64,
    pub memory_types: Vec<String>,
    pub capture_agent_memory: bool,
    pub agent_capture_mode: String,
    pub agent_recall: bool,
    pub agent_memory_types: Vec<String>,
    pub agent_flush_after_turn: bool,
    pub agent_visibility_verify_after_write: bool,
    pub agent_visibility_verify_after_flush: bool,
    pub agent_visibility_queries: Vec<String>,
    pub agent_visibility_top_k: usize,
    pub agent_visibility_timeout: f64,
    pub agent_visibility_get_page_size: usize,
    pub agent_visibility_retry_flush_attempts: usize,
    pub agentic_timeout: f64,
    pub max_context_items: u64,
    pub timeout: f64,
    pub max_context_chars: usize,
    pub include_recent_raw: bool,
    pub recent_raw_top_k: usize,
    pub profile_max_items: usize,
    pub agent_skills_max_items: usize,
    pub agent_cases_max_items: usize,
    pub episodic_max_items: usize,
    pub min_score: f64,
    pub min_recall_query_chars: usize,
    pub prefetch_cache_enabled: bool,
    pub prefetch_cache_ttl_seconds: u64,
    pub agent_trajectory_on_session_end: bool,
    pub agent_trajectory_on_pre_compress: bool,
    pub agent_trajectory_on_delegation: bool,
    pub agent_summary_after_turn: bool,
    pub agent_max_messages: usize,
    pub agent_max_message_chars: usize,
    pub agent_max_tool_result_chars: usize,
    pub agent_max_payload_chars: usize,
    pub agent_dedupe_entries: usize,
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.to_string(),
            user_id: String::new(),
            auto_recall: true,
            auto_capture: true,
            flush_after_turn: true,
            search_method: "hybrid".to_string(),
            top_k: 5,
            memory_types: DEFAULT_MEMORY_TYPES
                .iter()
                .map(|item| item.to_string())
                .collect(),
            capture_agent_memory: false,
            agent_capture_mode: "parallel".to_string(),
            agent_recall: false,
            agent_memory_types: vec!["agent_memory".to_string()],
            agent_flush_after_turn: true,
            agent_visibility_verify_after_write: false,
            agent_visibility_verify_after_flush: false,
            agent_visibility_queries: Vec::new(),
            agent_visibility_top_k: 5,
            agent_visibility_timeout: 30.0,
            agent_visibility_get_page_size: 20,
            agent_visibility_retry_flush_attempts: 1,
            agentic_timeout: 60.0,
            max_context_items: 8,
            timeout: 10.0,
            max_context_chars: 12_000,
            include_recent_raw: false,
            recent_raw_top_k: 4,
            profile_max_items: 3,
            agent_skills_max_items: 4,
            agent_cases_max_items: 4,
            episodic_max_items: 6,
            min_score: 0.0,
            min_recall_query_chars: 8,
            prefetch_cache_enabled: true,
            prefetch_cache_ttl_seconds: 90,
            agent_trajectory_on_session_end: true,
            agent_trajectory_on_pre_compress: true,
            agent_trajectory_on_delegation: true,
            agent_summary_after_turn: true,
            agent_max_messages: 80,
            agent_max_message_chars: 8_000,
            agent_max_tool_result_chars: 6_000,
            agent_max_payload_chars: 60_000,
            agent_dedupe_entries: 256,
        }
    }
}

pub fn load_config(hermes_home: &Path) -> ProviderConfig {
    let mut config = ProviderConfig::default();
    let path = hermes_home.join("everos.json");
    let Ok(raw) = fs::read_to_string(path) else {
        return config;
    };
    let Ok(value) = serde_json::from_str::<Value>(&raw) else {
        return config;
    };
    normalize_config_from_value(&mut config, &value);
    config
}

pub fn save_config(values: &Value, hermes_home: &Path) -> std::io::Result<()> {
    let path = hermes_home.join("everos.json");
    let existing = fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .unwrap_or_else(|| json!({}));
    let mut merged = existing.as_object().cloned().unwrap_or_default();
    if let Some(values) = values.as_object() {
        for (key, value) in values {
            merged.insert(key.clone(), value.clone());
        }
    }
    let mut config = ProviderConfig::default();
    normalize_config_from_value(&mut config, &Value::Object(merged));
    fs::create_dir_all(hermes_home)?;
    let content = serde_json::to_string_pretty(&config).unwrap() + "\n";
    write_private_config(&path, content.as_bytes())
}

#[cfg(unix)]
fn write_private_config(path: &Path, content: &[u8]) -> std::io::Result<()> {
    let mut file = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(content)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn write_private_config(path: &Path, content: &[u8]) -> std::io::Result<()> {
    let mut file = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path)?;
    file.write_all(content)
}

fn normalize_config_from_value(config: &mut ProviderConfig, value: &Value) {
    let Some(map) = value.as_object() else {
        return;
    };
    if let Some(text) = map
        .get("base_url")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
    {
        config.base_url = text.trim().to_string();
    }
    if let Some(text) = map.get("user_id").and_then(Value::as_str) {
        config.user_id = text.trim().to_string();
    }
    for (key, slot) in [
        ("auto_recall", &mut config.auto_recall),
        ("auto_capture", &mut config.auto_capture),
        ("flush_after_turn", &mut config.flush_after_turn),
        ("capture_agent_memory", &mut config.capture_agent_memory),
        ("agent_recall", &mut config.agent_recall),
        ("agent_flush_after_turn", &mut config.agent_flush_after_turn),
        (
            "agent_visibility_verify_after_write",
            &mut config.agent_visibility_verify_after_write,
        ),
        (
            "agent_visibility_verify_after_flush",
            &mut config.agent_visibility_verify_after_flush,
        ),
        ("include_recent_raw", &mut config.include_recent_raw),
        ("prefetch_cache_enabled", &mut config.prefetch_cache_enabled),
        (
            "agent_trajectory_on_session_end",
            &mut config.agent_trajectory_on_session_end,
        ),
        (
            "agent_trajectory_on_pre_compress",
            &mut config.agent_trajectory_on_pre_compress,
        ),
        (
            "agent_trajectory_on_delegation",
            &mut config.agent_trajectory_on_delegation,
        ),
        (
            "agent_summary_after_turn",
            &mut config.agent_summary_after_turn,
        ),
    ] {
        if let Some(value) = map.get(key) {
            *slot = as_bool(Some(value), *slot);
        }
    }
    if let Some(value) = map.get("top_k").and_then(Value::as_u64) {
        config.top_k = value.clamp(1, 20);
    }
    if let Some(value) = map.get("max_context_items").and_then(Value::as_u64) {
        config.max_context_items = value.clamp(1, 50);
    }
    if let Some(value) = map.get("timeout").and_then(Value::as_f64) {
        config.timeout = value.clamp(1.0, 60.0);
    }
    if let Some(value) = map.get("agentic_timeout").and_then(Value::as_f64) {
        config.agentic_timeout = value.clamp(1.0, 120.0);
    }
    if let Some(value) = map.get("agent_visibility_timeout").and_then(Value::as_f64) {
        config.agent_visibility_timeout = value.clamp(1.0, 120.0);
    }
    if let Some(value) = map.get("agent_visibility_top_k").and_then(Value::as_u64) {
        config.agent_visibility_top_k = (value as usize).clamp(1, 20);
    }
    if let Some(value) = map
        .get("agent_visibility_get_page_size")
        .and_then(Value::as_u64)
    {
        config.agent_visibility_get_page_size = (value as usize).clamp(1, 100);
    }
    if let Some(value) = map
        .get("agent_visibility_retry_flush_attempts")
        .and_then(Value::as_u64)
    {
        config.agent_visibility_retry_flush_attempts = (value as usize).clamp(1, 5);
    }
    if let Some(value) = map.get("max_context_chars").and_then(Value::as_u64) {
        config.max_context_chars = (value as usize).clamp(1_000, 50_000);
    }
    if let Some(value) = map.get("recent_raw_top_k").and_then(Value::as_u64) {
        config.recent_raw_top_k = (value as usize).clamp(0, 20);
    }
    if let Some(value) = map.get("profile_max_items").and_then(Value::as_u64) {
        config.profile_max_items = (value as usize).clamp(0, 20);
    }
    if let Some(value) = map.get("agent_skills_max_items").and_then(Value::as_u64) {
        config.agent_skills_max_items = (value as usize).clamp(0, 20);
    }
    if let Some(value) = map.get("agent_cases_max_items").and_then(Value::as_u64) {
        config.agent_cases_max_items = (value as usize).clamp(0, 20);
    }
    if let Some(value) = map.get("episodic_max_items").and_then(Value::as_u64) {
        config.episodic_max_items = (value as usize).clamp(0, 20);
    }
    if let Some(value) = map.get("min_score").and_then(Value::as_f64) {
        config.min_score = value.clamp(0.0, 1.0);
    }
    if let Some(value) = map.get("min_recall_query_chars").and_then(Value::as_u64) {
        config.min_recall_query_chars = (value as usize).clamp(0, 200);
    }
    if let Some(value) = map
        .get("prefetch_cache_ttl_seconds")
        .and_then(Value::as_u64)
    {
        config.prefetch_cache_ttl_seconds = value.clamp(1, 600);
    }
    if let Some(value) = map.get("agent_max_messages").and_then(Value::as_u64) {
        config.agent_max_messages = (value as usize).clamp(1, 200);
    }
    if let Some(value) = map.get("agent_max_message_chars").and_then(Value::as_u64) {
        config.agent_max_message_chars = (value as usize).clamp(100, 20_000);
    }
    if let Some(value) = map
        .get("agent_max_tool_result_chars")
        .and_then(Value::as_u64)
    {
        config.agent_max_tool_result_chars = (value as usize).clamp(100, 20_000);
    }
    if let Some(value) = map.get("agent_max_payload_chars").and_then(Value::as_u64) {
        config.agent_max_payload_chars = (value as usize).clamp(1_000, 200_000);
    }
    if let Some(value) = map.get("agent_dedupe_entries").and_then(Value::as_u64) {
        config.agent_dedupe_entries = (value as usize).clamp(16, 4_096);
    }
    if let Some(mode) = map.get("agent_capture_mode").and_then(Value::as_str) {
        let mode = mode.trim().to_ascii_lowercase();
        if matches!(mode.as_str(), "parallel" | "agent_only" | "off") {
            config.agent_capture_mode = mode;
        }
    }
    if let Some(method) = map.get("search_method").and_then(Value::as_str) {
        let method = method.trim().to_ascii_lowercase();
        if matches!(method.as_str(), "keyword" | "vector" | "hybrid" | "agentic") {
            config.search_method = method;
        }
    }
    if let Some(types) = map.get("memory_types") {
        let parsed = if let Some(text) = types.as_str() {
            text.split(',')
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        } else if let Some(items) = types.as_array() {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        if !parsed.is_empty() {
            config.memory_types = parsed;
        }
    }
    if let Some(types) = map.get("agent_memory_types") {
        let parsed = parse_string_list(types);
        if !parsed.is_empty() {
            config.agent_memory_types = parsed;
        }
    }
    if let Some(queries) = map.get("agent_visibility_queries") {
        config.agent_visibility_queries = parse_string_list(queries);
    }
}

pub(crate) fn as_bool(value: Option<&Value>, default: bool) -> bool {
    match value {
        Some(Value::Bool(flag)) => *flag,
        Some(Value::String(text)) => match text.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "y" | "on" => true,
            "0" | "false" | "no" | "n" | "off" => false,
            _ => default,
        },
        Some(Value::Number(number)) => number.as_i64().map(|value| value != 0).unwrap_or(default),
        _ => default,
    }
}

pub(crate) fn parse_string_list(value: &Value) -> Vec<String> {
    if let Some(text) = value.as_str() {
        text.split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(ToString::to_string)
            .collect()
    } else if let Some(items) = value.as_array() {
        items
            .iter()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(ToString::to_string)
            .collect()
    } else {
        Vec::new()
    }
}
