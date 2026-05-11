use crate::client::{DEFAULT_BASE_URL, DEFAULT_MEMORY_TYPES, EverOSClient, EverOSError};
use crate::env::get_env;
use crate::formatting::{format_search_context, pretty_json};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

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
    pub timeout: f64,
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
            timeout: 10.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProviderInit {
    pub session_id: String,
    pub hermes_home: Option<PathBuf>,
    pub platform: String,
    pub user_id: String,
    pub user_name: String,
    pub agent_identity: String,
    pub agent_context: String,
}

impl ProviderInit {
    pub fn for_test(session_id: &str, hermes_home: &Path) -> Self {
        Self {
            session_id: session_id.to_string(),
            hermes_home: Some(hermes_home.to_path_buf()),
            platform: "cli".to_string(),
            user_id: String::new(),
            user_name: String::new(),
            agent_identity: "default".to_string(),
            agent_context: String::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct EverOSProvider {
    config: ProviderConfig,
    client: Option<EverOSClient>,
    active: bool,
    write_enabled: bool,
    user_id: String,
    session_id: String,
}

impl EverOSProvider {
    pub fn name(&self) -> &'static str {
        "everos"
    }

    pub fn is_available(hermes_home: Option<&Path>) -> bool {
        !get_env("EVEROS_API_KEY", "", hermes_home).is_empty()
    }

    pub fn initialize(init: ProviderInit) -> Result<Self, EverOSError> {
        let hermes_home = init
            .hermes_home
            .clone()
            .unwrap_or_else(|| crate::env::hermes_home(None));
        let mut config = load_config(&hermes_home);
        let api_key = get_env("EVEROS_API_KEY", "", Some(&hermes_home));
        let base_url = get_env("EVEROS_BASE_URL", &config.base_url, Some(&hermes_home));
        config.base_url = base_url.clone();
        let platform = if init.platform.trim().is_empty() {
            "cli".to_string()
        } else {
            init.platform.trim().to_string()
        };
        let user_id = resolve_user_id(&config, &init, &hermes_home, &platform);
        let write_enabled = !matches!(init.agent_context.as_str(), "cron" | "flush" | "subagent");
        let active = !api_key.is_empty();
        let client = if active {
            Some(EverOSClient::new(api_key, base_url, config.timeout)?)
        } else {
            None
        };
        Ok(Self {
            config,
            client,
            active,
            write_enabled,
            user_id,
            session_id: init.session_id,
        })
    }

    pub fn user_id(&self) -> &str {
        &self.user_id
    }

    pub fn system_prompt_block(&self) -> String {
        if !self.active {
            return String::new();
        }
        format!(
            "# EverOS Memory\nEverOS memory provider is active for user_id `{}`. Use EverOS memory context silently when relevant. Explicit tools available: everos_memory_search, everos_memory_save, everos_memory_get, everos_memory_flush, everos_memory_forget.",
            self.user_id
        )
    }

    pub fn prefetch(&self, query: &str, _session_id: Option<&str>) -> String {
        if !self.active
            || !self.config.auto_recall
            || self.client.is_none()
            || query.trim().is_empty()
        {
            return String::new();
        }
        let Some(client) = &self.client else {
            return String::new();
        };
        let query: String = query.chars().take(1000).collect();
        match client.search_memories(
            &query,
            Some(&self.user_id),
            None,
            None,
            None,
            &self.config.search_method,
            Some(self.config.memory_types.clone()),
            self.config.top_k,
            None,
            false,
            false,
            Some(self.config.timeout),
        ) {
            Ok(response) => {
                let formatted = format_search_context(&response, self.config.top_k as usize);
                if formatted.is_empty() {
                    String::new()
                } else {
                    format!("<everos-context>\n{formatted}\n</everos-context>")
                }
            }
            Err(_) => String::new(),
        }
    }

    pub fn tool_schemas(&self) -> Vec<Value> {
        provider_tool_schemas()
    }

    pub fn handle_tool_call(&self, tool_name: &str, args: Value) -> Result<String, EverOSError> {
        if !self.active || self.client.is_none() {
            return Ok(tool_error(
                "EverOS provider is not active. Set EVEROS_API_KEY and memory.provider: everos.",
            ));
        }
        let args = args.as_object().cloned().unwrap_or_default();
        let result = match tool_name {
            "everos_memory_save" => self.tool_save(&Value::Object(args))?,
            "everos_memory_search" => self.tool_search(&Value::Object(args))?,
            "everos_memory_get" => self.tool_get(&Value::Object(args))?,
            "everos_memory_flush" => self.tool_flush(&Value::Object(args))?,
            "everos_memory_forget" => self.tool_forget(&Value::Object(args))?,
            _ => tool_error(&format!("Unknown EverOS memory tool: {tool_name}")),
        };
        Ok(result)
    }

    pub fn sync_turn(
        &self,
        user_content: &str,
        assistant_content: &str,
        session_id: Option<&str>,
    ) -> Result<(), EverOSError> {
        if !self.active || !self.write_enabled || !self.config.auto_capture || self.client.is_none()
        {
            return Ok(());
        }
        let clean_user = clean_content(user_content);
        let clean_assistant = clean_content(assistant_content);
        if clean_user.is_empty() || clean_assistant.is_empty() || is_trivial(&clean_user) {
            return Ok(());
        }
        let sid = session_id
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(&self.session_id);
        let now = now_ms();
        let messages = vec![
            json!({"role":"user","timestamp":now,"content":clean_user}),
            json!({"role":"assistant","timestamp":now + 1,"content":clean_assistant}),
        ];
        let client = self.client.as_ref().expect("checked above");
        client.add_memories(&self.user_id, Some(sid), messages, true, false)?;
        if self.config.flush_after_turn {
            client.flush_memories(&self.user_id, Some(sid), false, None)?;
        }
        Ok(())
    }

    pub fn on_memory_write(
        &self,
        action: &str,
        target: &str,
        content: &str,
        _metadata: Option<Value>,
    ) -> Result<(), EverOSError> {
        if !matches!(action, "add" | "replace" | "update")
            || content.trim().is_empty()
            || !self.active
            || !self.write_enabled
            || self.client.is_none()
        {
            return Ok(());
        }
        let text = format!("Hermes {target} memory: {}", content.trim());
        let client = self.client.as_ref().expect("checked above");
        client.add_memories(
            &self.user_id,
            Some(&self.session_id),
            vec![json!({"role":"user","timestamp":now_ms(),"content":text})],
            true,
            false,
        )?;
        if self.config.flush_after_turn {
            client.flush_memories(&self.user_id, Some(&self.session_id), false, None)?;
        }
        Ok(())
    }

    pub fn on_session_end(&self) -> Result<(), EverOSError> {
        if !self.active
            || !self.write_enabled
            || self.client.is_none()
            || self.session_id.is_empty()
        {
            return Ok(());
        }
        self.client
            .as_ref()
            .expect("checked above")
            .flush_memories(&self.user_id, Some(&self.session_id), false, None)?;
        Ok(())
    }

    fn tool_save(&self, args: &Value) -> Result<String, EverOSError> {
        let content = value_string(args, "content").trim().to_string();
        if content.is_empty() {
            return Ok(tool_error("content is required"));
        }
        let session_id = value_string(args, "session_id");
        let session_id = if session_id.is_empty() {
            self.session_id.as_str()
        } else {
            session_id.as_str()
        };
        let session_id_opt = if session_id.is_empty() {
            None
        } else {
            Some(session_id)
        };
        let result = self.client.as_ref().expect("active").add_memories(
            &self.user_id,
            session_id_opt,
            vec![json!({"role":"user","timestamp":now_ms(),"content":content})],
            true,
            false,
        )?;
        let flush_requested = as_bool(args.get("flush"), true);
        let flush_payload = if flush_requested {
            match self.client.as_ref().expect("active").flush_memories(
                &self.user_id,
                session_id_opt,
                false,
                None,
            ) {
                Ok(response) => Some(flush_result_payload(&response)),
                Err(err @ EverOSError::Timeout { .. }) => Some(timeout_payload("flush", &err)),
                Err(err) => return Err(err),
            }
        } else {
            None
        };
        Ok(serde_json::to_string(&save_result_payload(
            &result,
            &self.user_id,
            session_id_opt,
            flush_requested,
            flush_payload,
        ))
        .unwrap())
    }

    fn tool_search(&self, args: &Value) -> Result<String, EverOSError> {
        let query = value_string(args, "query").trim().to_string();
        if query.is_empty() {
            return Ok(tool_error("query is required"));
        }
        let limit = int_between(args.get("limit"), 1, 20, self.config.top_k) as u64;
        let mut method = value_string(args, "method").to_ascii_lowercase();
        if method.is_empty() {
            method = self.config.search_method.clone();
        }
        if !matches!(method.as_str(), "keyword" | "vector" | "hybrid" | "agentic") {
            method = self.config.search_method.clone();
        }
        let session_id = value_string(args, "session_id");
        let memory_types = args
            .get("memory_types")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
            })
            .filter(|items| !items.is_empty())
            .unwrap_or_else(|| self.config.memory_types.clone());
        let response = self.client.as_ref().expect("active").search_memories(
            &query,
            Some(&self.user_id),
            None,
            if session_id.is_empty() {
                None
            } else {
                Some(session_id.as_str())
            },
            None,
            &method,
            Some(memory_types),
            limit,
            None,
            false,
            false,
            Some(if method == "agentic" {
                60.0
            } else {
                self.config.timeout
            }),
        )?;
        Ok(pretty_json(&response))
    }

    fn tool_get(&self, args: &Value) -> Result<String, EverOSError> {
        let session_id = value_string(args, "session_id");
        let memory_type = non_empty_or(&value_string(args, "memory_type"), "episodic_memory");
        let response = self.client.as_ref().expect("active").get_memories(
            Some(&self.user_id),
            None,
            if session_id.is_empty() {
                None
            } else {
                Some(session_id.as_str())
            },
            None,
            &memory_type,
            int_between(args.get("page"), 1, 10000, 1) as u64,
            int_between(args.get("page_size"), 1, 100, 20) as u64,
            "timestamp",
            "desc",
        )?;
        Ok(pretty_json(&response))
    }

    fn tool_flush(&self, args: &Value) -> Result<String, EverOSError> {
        let session_id = value_string(args, "session_id");
        let sid = if session_id.is_empty() {
            self.session_id.as_str()
        } else {
            session_id.as_str()
        };
        let response = match self.client.as_ref().expect("active").flush_memories(
            &self.user_id,
            if sid.is_empty() { None } else { Some(sid) },
            false,
            float_or_none(args.get("timeout")),
        ) {
            Ok(response) => response,
            Err(err @ EverOSError::Timeout { .. }) => {
                return Ok(pretty_json(&timeout_payload("flush", &err)));
            }
            Err(err) => return Err(err),
        };
        Ok(pretty_json(&response))
    }

    fn tool_forget(&self, args: &Value) -> Result<String, EverOSError> {
        if !as_bool(args.get("confirm"), false) {
            return Ok(tool_error(
                "confirm=true is required before deleting an EverOS memory",
            ));
        }
        let memory_id = value_string(args, "memory_id");
        if memory_id.trim().is_empty() {
            return Ok(tool_error("memory_id is required"));
        }
        let response = self.client.as_ref().expect("active").delete_memories(
            Some(&memory_id),
            None,
            None,
            None,
        )?;
        Ok(pretty_json(&response))
    }
}

pub fn provider_tool_schemas() -> Vec<Value> {
    vec![
        json!({"name":"everos_memory_save","description":"Queue an explicit long-term memory message in EverOS and optionally request extraction; saved=true does not guarantee a structured memory is immediately searchable.","parameters":{"type":"object","properties":{"content":{"type":"string","description":"Memory content to store."},"session_id":{"type":"string","description":"Optional EverOS/Hermes session id."},"flush":{"type":"boolean","description":"Trigger EverOS extraction immediately. Default true."}},"required":["content"]}}),
        json!({"name":"everos_memory_search","description":"Search EverOS long-term memory using keyword, vector, hybrid, or agentic retrieval.","parameters":{"type":"object","properties":{"query":{"type":"string","description":"Search query."},"limit":{"type":"integer","description":"Maximum results, usually 5-10."},"method":{"type":"string","enum":["keyword","vector","hybrid","agentic"],"description":"Retrieval method. Default hybrid."},"session_id":{"type":"string","description":"Optional session filter."},"memory_types":{"type":"array","items":{"type":"string"},"description":"Optional EverOS memory types."}},"required":["query"]}}),
        json!({"name":"everos_memory_get","description":"Get structured EverOS memories by type for the configured user.","parameters":{"type":"object","properties":{"memory_type":{"type":"string","enum":["episodic_memory","profile","agent_case","agent_skill"],"description":"Memory type to retrieve."},"page":{"type":"integer","description":"Page number starting at 1."},"page_size":{"type":"integer","description":"Items per page, 1-100."},"session_id":{"type":"string","description":"Optional session filter."}}}}),
        json!({"name":"everos_memory_flush","description":"Force EverOS memory extraction for the configured user/session. Timeout errors are retryable; search/status checks should happen before retrying.","parameters":{"type":"object","properties":{"session_id":{"type":"string","description":"Optional session id."},"timeout":{"type":"number","description":"Optional per-call timeout in seconds."}}}}),
        json!({"name":"everos_memory_forget","description":"Delete an EverOS memory by id. Requires confirm=true because this is destructive.","parameters":{"type":"object","properties":{"memory_id":{"type":"string","description":"Exact EverOS memory id to delete."},"confirm":{"type":"boolean","description":"Must be true to delete."}},"required":["memory_id","confirm"]}}),
    ]
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
    fs::write(path, serde_json::to_string_pretty(&config).unwrap() + "\n")
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
    ] {
        if let Some(value) = map.get(key) {
            *slot = as_bool(Some(value), *slot);
        }
    }
    if let Some(value) = map.get("top_k").and_then(Value::as_u64) {
        config.top_k = value.clamp(1, 20);
    }
    if let Some(value) = map.get("timeout").and_then(Value::as_f64) {
        config.timeout = value.clamp(1.0, 60.0);
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
}

fn resolve_user_id(
    config: &ProviderConfig,
    init: &ProviderInit,
    hermes_home: &Path,
    platform: &str,
) -> String {
    let template = get_env("EVEROS_USER_ID", "", Some(hermes_home));
    let template = if template.is_empty() {
        config.user_id.trim().to_string()
    } else {
        template
    };
    let gateway_user_id = init.user_id.trim();
    let identity = if init.agent_identity.trim().is_empty() {
        "default"
    } else {
        init.agent_identity.trim()
    };
    let user_name = if init.user_name.trim().is_empty() {
        if gateway_user_id.is_empty() {
            identity
        } else {
            gateway_user_id
        }
    } else {
        init.user_name.trim()
    };
    if template.is_empty() {
        return if gateway_user_id.is_empty() {
            format!("hermes_{identity}")
        } else {
            gateway_user_id.to_string()
        };
    }
    template
        .replace(
            "{user_id}",
            if gateway_user_id.is_empty() {
                identity
            } else {
                gateway_user_id
            },
        )
        .replace("{user_name}", user_name)
        .replace("{identity}", identity)
        .replace("{platform}", platform)
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn clean_content(text: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?is)<memory-context>[\s\S]*?</memory-context>|<everos-context>[\s\S]*?</everos-context>").unwrap())
        .replace_all(text, "")
        .trim()
        .to_string()
}

fn is_trivial(text: &str) -> bool {
    matches!(
        text.trim()
            .trim_end_matches('.')
            .to_ascii_lowercase()
            .as_str(),
        "ok" | "okay"
            | "thanks"
            | "thank you"
            | "got it"
            | "sure"
            | "yes"
            | "no"
            | "yep"
            | "nope"
            | "k"
            | "ty"
            | "thx"
            | "np"
    )
}

fn tool_error(message: &str) -> String {
    serde_json::to_string(&json!({"error": message})).unwrap()
}

fn timeout_payload(operation: &str, err: &EverOSError) -> Value {
    json!({
        "ok": false,
        "operation": operation,
        "error": err.to_string(),
        "retryable": true,
        "suggested_next_actions": [
            "search existing memories before retrying, because the server may have completed the request after the client timed out",
            "if the operation returned a task_id or request_id earlier, check that status before issuing another write/flush",
            "retry with a longer timeout only if search/status checks do not show the expected result"
        ]
    })
}

fn flush_result_payload(response: &Value) -> Value {
    let data = response.get("data").unwrap_or(response);
    let mut payload = serde_json::Map::new();
    payload.insert("ok".to_string(), Value::Bool(true));
    for key in ["status", "request_id", "task_id", "message"] {
        if let Some(value) = data.get(key).filter(|value| !value.is_null()) {
            payload.insert(key.to_string(), value.clone());
        }
    }
    Value::Object(payload)
}

fn save_result_payload(
    result: &Value,
    user_id: &str,
    session_id: Option<&str>,
    flush_requested: bool,
    flush: Option<Value>,
) -> Value {
    let status = result
        .pointer("/data/status")
        .and_then(Value::as_str)
        .unwrap_or("");
    let task_id = result
        .pointer("/data/task_id")
        .and_then(Value::as_str)
        .unwrap_or("");
    let extraction_requested = flush_requested
        || !task_id.is_empty()
        || matches!(status, "queued" | "processing" | "success");
    json!({
        "saved": true,
        "message_queued": true,
        "extraction_requested": extraction_requested,
        "searchable": Value::Null,
        "user_id": user_id,
        "session_id": session_id,
        "status": status,
        "task_id": task_id,
        "flush": flush.unwrap_or_else(|| {
            if flush_requested {
                json!({"ok": false, "error": "flush requested but no flush result was recorded"})
            } else {
                json!({"ok": Value::Null, "status": "not_requested"})
            }
        }),
    })
}

fn value_string(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn non_empty_or(value: &str, default: &str) -> String {
    if value.trim().is_empty() {
        default.to_string()
    } else {
        value.trim().to_string()
    }
}

fn as_bool(value: Option<&Value>, default: bool) -> bool {
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

fn int_between(value: Option<&Value>, low: i64, high: i64, default: u64) -> i64 {
    let parsed = value
        .and_then(|value| {
            value
                .as_i64()
                .or_else(|| value.as_str().and_then(|text| text.parse::<i64>().ok()))
        })
        .unwrap_or(default as i64);
    parsed.clamp(low, high)
}

fn float_or_none(value: Option<&Value>) -> Option<f64> {
    value
        .and_then(|value| {
            value
                .as_f64()
                .or_else(|| value.as_str().and_then(|text| text.parse::<f64>().ok()))
        })
        .filter(|value| value.is_finite() && *value > 0.0)
}
