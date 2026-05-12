use crate::env::get_env;
use crate::formatting::strip_vectors;
use reqwest::Method;
use reqwest::blocking::Client;
use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE};
use serde_json::{Map, Value, json};
use std::path::Path;
use std::time::Duration;
use thiserror::Error;
use url::Url;

pub const DEFAULT_BASE_URL: &str = "https://api.evermind.ai";
pub const DEFAULT_TIMEOUT: f64 = 10.0;
pub const DEFAULT_MEMORY_TYPES: [&str; 2] = ["episodic_memory", "profile"];

#[derive(Debug, Error)]
pub enum EverOSError {
    #[error("EVEROS_API_KEY is required. Create one at https://everos.evermind.ai/api-keys")]
    MissingApiKey,
    #[error("Invalid EVEROS_BASE_URL: {0:?}")]
    InvalidBaseUrl(String),
    #[error("EverOS request failed: {0}")]
    Request(String),
    #[error(
        "EverOS request timed out during {method} {path}. The server may still be processing the request; search existing memories or check a prior task/request id before retrying."
    )]
    Timeout { method: String, path: String },
    #[error("EverOS returned invalid JSON from {url}: {source}")]
    InvalidJson {
        url: String,
        source: serde_json::Error,
    },
    #[error("EverOS API error {0}")]
    Api(String),
}

pub type Result<T> = std::result::Result<T, EverOSError>;

#[derive(Debug, Clone)]
pub struct EverOSClient {
    pub api_key: String,
    pub base_url: String,
    pub timeout: f64,
}

impl EverOSClient {
    pub fn new(api_key: impl AsRef<str>, base_url: impl AsRef<str>, timeout: f64) -> Result<Self> {
        let api_key = api_key.as_ref().trim().to_string();
        if api_key.is_empty() {
            return Err(EverOSError::MissingApiKey);
        }
        let base_url = normalize_base_url(base_url.as_ref())?;
        let timeout = if timeout.is_finite() && timeout > 0.0 {
            timeout
        } else {
            DEFAULT_TIMEOUT
        };
        Ok(Self {
            api_key,
            base_url,
            timeout,
        })
    }

    pub fn from_env(hermes_home: Option<&Path>) -> Result<Self> {
        let api_key = get_env("EVEROS_API_KEY", "", hermes_home);
        let base_url = get_env("EVEROS_BASE_URL", DEFAULT_BASE_URL, hermes_home);
        let timeout = get_env("EVEROS_TIMEOUT", &DEFAULT_TIMEOUT.to_string(), hermes_home)
            .parse::<f64>()
            .unwrap_or(DEFAULT_TIMEOUT);
        Self::new(api_key, base_url, timeout)
    }

    pub fn request_json(
        &self,
        method: &str,
        path: &str,
        body: Option<Value>,
        timeout: Option<f64>,
    ) -> Result<Value> {
        let normalized_path = normalize_path(path);
        let url = format!("{}{}", self.base_url, normalized_path);
        let method_name = method.to_ascii_uppercase();
        let method = Method::from_bytes(method_name.as_bytes())
            .map_err(|err| EverOSError::Request(err.to_string()))?;
        let effective_timeout = timeout.unwrap_or(self.timeout);
        let client = Client::builder()
            .timeout(Duration::from_secs_f64(effective_timeout.max(0.001)))
            .build()
            .map_err(|err| EverOSError::Request(err.to_string()))?;
        let mut req = client
            .request(method, &url)
            .header(AUTHORIZATION, format!("Bearer {}", self.api_key))
            .header(CONTENT_TYPE, "application/json")
            .header(ACCEPT, "application/json");
        if let Some(body) = body {
            req = req.json(&drop_nulls(body));
        }
        let resp = req.send().map_err(|err| {
            if err.is_timeout() {
                EverOSError::Timeout {
                    method: method_name.clone(),
                    path: normalized_path.clone(),
                }
            } else {
                EverOSError::Request(err.to_string())
            }
        })?;
        let status = resp.status();
        let text = resp
            .text()
            .map_err(|err| EverOSError::Request(err.to_string()))?;
        if !status.is_success() {
            return Err(http_error_to_everos_error(
                status.as_u16(),
                &text,
                status.canonical_reason().unwrap_or("HTTP error"),
            ));
        }
        if text.trim().is_empty() {
            return Ok(json!({"ok": true, "status_code": status.as_u16()}));
        }
        let parsed: Value = serde_json::from_str(&text)
            .map_err(|source| EverOSError::InvalidJson { url, source })?;
        if parsed.is_object() {
            Ok(parsed)
        } else {
            Ok(json!({"data": parsed}))
        }
    }

    pub fn add_memories(
        &self,
        user_id: &str,
        session_id: Option<&str>,
        messages: Vec<Value>,
        async_mode: bool,
        agent: bool,
    ) -> Result<Value> {
        self.add_memories_scoped(
            user_id,
            session_id,
            messages,
            async_mode,
            if agent { "agent" } else { "personal" },
        )
    }

    pub fn add_memories_scoped(
        &self,
        user_id: &str,
        session_id: Option<&str>,
        messages: Vec<Value>,
        async_mode: bool,
        scope: &str,
    ) -> Result<Value> {
        validate_messages(&messages, scope)?;
        let path = if normalize_scope(scope)? == "agent" {
            "/api/v1/memories/agent"
        } else {
            "/api/v1/memories"
        };
        self.request_json(
            "POST",
            path,
            Some(json!({
                "user_id": user_id,
                "session_id": session_id,
                "messages": messages,
                "async_mode": async_mode,
            })),
            None,
        )
    }

    pub fn add_group_memories(
        &self,
        group_id: &str,
        messages: Vec<Value>,
        group_meta: Option<Value>,
        async_mode: bool,
    ) -> Result<Value> {
        self.request_json(
            "POST",
            "/api/v1/memories/group",
            Some(json!({"group_id": group_id, "group_meta": group_meta, "messages": messages, "async_mode": async_mode})),
            None,
        )
    }

    pub fn flush_memories(
        &self,
        user_id: &str,
        session_id: Option<&str>,
        agent: bool,
        timeout: Option<f64>,
    ) -> Result<Value> {
        self.flush_memories_scoped(
            user_id,
            session_id,
            if agent { "agent" } else { "personal" },
            timeout,
        )
    }

    pub fn flush_memories_scoped(
        &self,
        user_id: &str,
        session_id: Option<&str>,
        scope: &str,
        timeout: Option<f64>,
    ) -> Result<Value> {
        let path = if normalize_scope(scope)? == "agent" {
            "/api/v1/memories/agent/flush"
        } else {
            "/api/v1/memories/flush"
        };
        self.request_json(
            "POST",
            path,
            Some(json!({"user_id": user_id, "session_id": session_id})),
            timeout,
        )
    }

    pub fn flush_group_memories(&self, group_id: &str, timeout: Option<f64>) -> Result<Value> {
        self.request_json(
            "POST",
            "/api/v1/memories/group/flush",
            Some(json!({"group_id": group_id})),
            timeout,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn get_memories(
        &self,
        user_id: Option<&str>,
        group_id: Option<&str>,
        session_id: Option<&str>,
        filters: Option<Value>,
        memory_type: &str,
        page: u64,
        page_size: u64,
        rank_by: &str,
        rank_order: &str,
    ) -> Result<Value> {
        validate_get_params(memory_type, page, page_size, rank_by, rank_order)?;
        let resolved_filters = build_filters(user_id, group_id, session_id, filters)?;
        self.request_json(
            "POST",
            "/api/v1/memories/get",
            Some(json!({
                "memory_type": memory_type,
                "filters": resolved_filters,
                "page": page,
                "page_size": page_size,
                "rank_by": rank_by,
                "rank_order": rank_order,
            })),
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn search_memories(
        &self,
        query: &str,
        user_id: Option<&str>,
        group_id: Option<&str>,
        session_id: Option<&str>,
        filters: Option<Value>,
        method: &str,
        memory_types: Option<Vec<String>>,
        top_k: i64,
        radius: Option<f64>,
        include_original_data: bool,
        include_vectors: bool,
        timeout: Option<f64>,
    ) -> Result<Value> {
        validate_search_params(method, memory_types.as_deref(), top_k, radius)?;
        let resolved_filters = build_filters(user_id, group_id, session_id, filters)?;
        let memory_types = memory_types.unwrap_or_else(|| {
            DEFAULT_MEMORY_TYPES
                .iter()
                .map(|item| item.to_string())
                .collect()
        });
        let response = self.request_json(
            "POST",
            "/api/v1/memories/search",
            Some(json!({
                "query": query,
                "filters": resolved_filters,
                "method": method,
                "memory_types": memory_types,
                "top_k": top_k,
                "radius": radius,
                "include_original_data": include_original_data,
            })),
            timeout,
        )?;
        Ok(if include_vectors {
            response
        } else {
            strip_vectors(&response)
        })
    }

    pub fn delete_memories(
        &self,
        memory_id: Option<&str>,
        user_id: Option<&str>,
        group_id: Option<&str>,
        session_id: Option<&str>,
    ) -> Result<Value> {
        validate_delete_request(memory_id, user_id, group_id, session_id)?;
        let body = if let Some(memory_id) = memory_id.filter(|value| !value.trim().is_empty()) {
            json!({"memory_id": memory_id})
        } else {
            json!({"user_id": user_id, "group_id": group_id, "session_id": session_id})
        };
        self.request_json("POST", "/api/v1/memories/delete", Some(body), None)
    }

    pub fn get_task_status(&self, task_id: &str) -> Result<Value> {
        let quoted: String = url::form_urlencoded::byte_serialize(task_id.as_bytes()).collect();
        self.request_json("GET", &format!("/api/v1/tasks/{quoted}"), None, None)
    }

    pub fn get_settings(&self) -> Result<Value> {
        self.request_json("GET", "/api/v1/settings", None, None)
    }

    pub fn update_settings(
        &self,
        settings: Value,
        strict: bool,
        return_diff: bool,
    ) -> Result<Value> {
        let validated = validate_settings_update(settings, strict)?;
        let before = if return_diff {
            Some(self.get_settings()?)
        } else {
            None
        };
        let mut response =
            self.request_json("PUT", "/api/v1/settings", Some(validated.clone()), None)?;
        if let Some(before) = before {
            let after = self.get_settings()?;
            let diff = settings_diff(&before, &after, &validated);
            let updated = settings_updated_payload(&after);
            if let Some(map) = response.as_object_mut() {
                map.insert("diff".to_string(), diff);
                map.insert("updated".to_string(), updated);
            } else {
                response = json!({"data": response, "diff": diff, "updated": updated});
            }
        }
        Ok(response)
    }
}

pub fn normalize_base_url(url: &str) -> Result<String> {
    let url = if url.trim().is_empty() {
        DEFAULT_BASE_URL
    } else {
        url.trim()
    }
    .trim_end_matches('/')
    .to_string();
    let parsed = Url::parse(&url).map_err(|_| EverOSError::InvalidBaseUrl(url.clone()))?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
        return Err(EverOSError::InvalidBaseUrl(url));
    }
    Ok(parsed.as_str().trim_end_matches('/').to_string())
}

pub fn normalize_path(path: &str) -> String {
    if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    }
}

pub fn drop_nulls(value: Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.into_iter()
                .filter_map(|(key, value)| {
                    if value.is_null() {
                        None
                    } else {
                        Some((key, drop_nulls(value)))
                    }
                })
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.into_iter().map(drop_nulls).collect()),
        other => other,
    }
}

pub fn build_filters(
    user_id: Option<&str>,
    group_id: Option<&str>,
    session_id: Option<&str>,
    filters: Option<Value>,
) -> Result<Value> {
    if group_id.filter(|value| !value.trim().is_empty()).is_some() {
        return Err(EverOSError::Api(
            "group memory is out of scope for this EverOS-Hermes release".to_string(),
        ));
    }
    let mut map = match filters {
        Some(Value::Object(map)) => map,
        Some(_) => return Err(EverOSError::Api("filters must be an object".to_string())),
        None => Map::new(),
    };
    validate_filter_map(&map)?;
    if let Some(user_id) = user_id.filter(|value| !value.trim().is_empty()) {
        if let Some(existing) = map.get("user_id").and_then(Value::as_str) {
            if existing != user_id {
                return Err(EverOSError::Api(
                    "conflicting user_id between top-level parameter and filters".to_string(),
                ));
            }
        } else {
            map.insert("user_id".into(), Value::String(user_id.to_string()));
        }
    }
    if !map.contains_key("user_id") {
        return Err(EverOSError::Api("filters must include user_id".to_string()));
    }
    if let Some(session_id) = session_id.filter(|value| !value.trim().is_empty()) {
        if filter_has_session_conflict(&Value::Object(map.clone()), session_id) {
            return Err(EverOSError::Api(
                "conflicting session_id between top-level parameter and filters".to_string(),
            ));
        }
        if !filter_has_session(&Value::Object(map.clone())) {
            let mut clauses = match map.remove("AND") {
                Some(Value::Array(items)) => items,
                _ => Vec::new(),
            };
            clauses.push(json!({"session_id": session_id}));
            map.insert("AND".into(), Value::Array(clauses));
        }
    }
    Ok(Value::Object(map))
}

fn normalize_scope(scope: &str) -> Result<&'static str> {
    match scope.trim().to_ascii_lowercase().as_str() {
        "" | "personal" => Ok("personal"),
        "agent" => Ok("agent"),
        other => Err(EverOSError::Api(format!(
            "scope must be personal or agent, got {other}"
        ))),
    }
}

fn validate_messages(messages: &[Value], scope: &str) -> Result<()> {
    let scope = normalize_scope(scope)?;
    if messages.is_empty() || messages.len() > 500 {
        return Err(EverOSError::Api(
            "messages length must be 1..=500".to_string(),
        ));
    }
    for message in messages {
        let Some(map) = message.as_object() else {
            return Err(EverOSError::Api(
                "each message must be an object".to_string(),
            ));
        };
        let role = map.get("role").and_then(Value::as_str).unwrap_or("");
        let allowed = if scope == "agent" {
            matches!(role, "user" | "assistant" | "tool" | "system")
        } else {
            matches!(role, "user" | "assistant" | "system")
        };
        if !allowed {
            return Err(EverOSError::Api(format!(
                "invalid role {role:?} for {scope} memory"
            )));
        }
        if role == "tool"
            && map
                .get("tool_call_id")
                .and_then(Value::as_str)
                .is_none_or(|value| value.trim().is_empty())
        {
            return Err(EverOSError::Api(
                "tool_call_id is required when role='tool'".to_string(),
            ));
        }
        if !map
            .get("timestamp")
            .is_some_and(|value| value.as_i64().is_some() || value.as_u64().is_some())
        {
            return Err(EverOSError::Api(
                "each message timestamp must be epoch milliseconds".to_string(),
            ));
        }
        if map
            .get("content")
            .and_then(Value::as_str)
            .is_none_or(|value| value.trim().is_empty())
        {
            return Err(EverOSError::Api(
                "each message content is required".to_string(),
            ));
        }
    }
    Ok(())
}

fn validate_search_params(
    method: &str,
    memory_types: Option<&[String]>,
    top_k: i64,
    radius: Option<f64>,
) -> Result<()> {
    if !matches!(method, "keyword" | "vector" | "hybrid" | "agentic") {
        return Err(EverOSError::Api(
            "method must be keyword, vector, hybrid, or agentic".to_string(),
        ));
    }
    if !(-1..=100).contains(&top_k) {
        return Err(EverOSError::Api(
            "top_k must be between -1 and 100".to_string(),
        ));
    }
    if let Some(radius) = radius {
        if !(0.0..=1.0).contains(&radius) {
            return Err(EverOSError::Api(
                "radius must be between 0 and 1".to_string(),
            ));
        }
        if method == "keyword" {
            return Err(EverOSError::Api(
                "radius is only valid for vector, hybrid, or agentic search".to_string(),
            ));
        }
    }
    if let Some(types) = memory_types {
        for ty in types {
            if !matches!(
                ty.as_str(),
                "episodic_memory" | "profile" | "raw_message" | "agent_memory"
            ) {
                return Err(EverOSError::Api(format!("invalid search memory_type {ty}")));
            }
        }
    }
    Ok(())
}

fn validate_get_params(
    memory_type: &str,
    page: u64,
    page_size: u64,
    rank_by: &str,
    rank_order: &str,
) -> Result<()> {
    if !matches!(
        memory_type,
        "episodic_memory" | "profile" | "agent_case" | "agent_skill"
    ) {
        return Err(EverOSError::Api(format!(
            "invalid get memory_type {memory_type}"
        )));
    }
    if page == 0 || page_size == 0 || page_size > 100 {
        return Err(EverOSError::Api(
            "page must be >= 1 and page_size must be 1..=100".to_string(),
        ));
    }
    if !matches!(rank_by, "timestamp" | "created_at" | "updated_at") {
        return Err(EverOSError::Api(
            "rank_by must be timestamp, created_at, or updated_at".to_string(),
        ));
    }
    if !matches!(rank_order, "asc" | "desc") {
        return Err(EverOSError::Api(
            "rank_order must be asc or desc".to_string(),
        ));
    }
    Ok(())
}

fn validate_settings_update(settings: Value, strict: bool) -> Result<Value> {
    let Value::Object(map) = settings else {
        return Err(EverOSError::Api(
            "settings must be a non-empty object".to_string(),
        ));
    };
    if map.is_empty() {
        return Err(EverOSError::Api(
            "settings must be a non-empty object".to_string(),
        ));
    }
    if strict {
        let mut unknown: Vec<String> = map
            .keys()
            .filter(|key| !matches!(key.as_str(), "timezone" | "llm_custom_setting"))
            .cloned()
            .collect();
        unknown.sort();
        if !unknown.is_empty() {
            return Err(EverOSError::Api(format!(
                "Unknown settings fields {unknown:?}; allowed fields: [\"llm_custom_setting\", \"timezone\"]"
            )));
        }
    }
    let mut out = Map::new();
    for (key, value) in map {
        match key.as_str() {
            "timezone" => {
                if value
                    .as_str()
                    .is_none_or(|timezone| timezone.trim().is_empty())
                {
                    return Err(EverOSError::Api(
                        "timezone must be an IANA timezone string".to_string(),
                    ));
                }
                out.insert(key, value);
            }
            "llm_custom_setting" => {
                if !value.is_object() {
                    return Err(EverOSError::Api(
                        "llm_custom_setting must be an object".to_string(),
                    ));
                }
                out.insert(key, value);
            }
            _ if !strict => {
                out.insert(key, value);
            }
            _ => {}
        }
    }
    Ok(Value::Object(out))
}

fn settings_diff(before: &Value, after: &Value, requested: &Value) -> Value {
    let mut diff = Map::new();
    let before_data = settings_data_object(before);
    let after_data = settings_data_object(after);
    if let Some(requested) = requested.as_object() {
        for key in requested.keys() {
            let old = before_data
                .and_then(|map| map.get(key))
                .cloned()
                .unwrap_or(Value::Null);
            let new = after_data
                .and_then(|map| map.get(key))
                .cloned()
                .unwrap_or(Value::Null);
            if old != new {
                diff.insert(key.clone(), json!({"before": old, "after": new}));
            }
        }
    }
    Value::Object(diff)
}

fn settings_updated_payload(after: &Value) -> Value {
    after.get("data").cloned().unwrap_or_else(|| after.clone())
}

fn settings_data_object(value: &Value) -> Option<&Map<String, Value>> {
    value
        .get("data")
        .and_then(Value::as_object)
        .or_else(|| value.as_object())
}

fn validate_delete_request(
    memory_id: Option<&str>,
    user_id: Option<&str>,
    group_id: Option<&str>,
    session_id: Option<&str>,
) -> Result<()> {
    if group_id.filter(|value| !value.trim().is_empty()).is_some() {
        return Err(EverOSError::Api(
            "group delete is out of scope for this EverOS-Hermes release".to_string(),
        ));
    }
    let has_memory_id = memory_id.is_some_and(|value| !value.trim().is_empty());
    if has_memory_id && (user_id.is_some() || session_id.is_some()) {
        return Err(EverOSError::Api(
            "single delete by memory_id cannot include user_id or session_id".to_string(),
        ));
    }
    if !has_memory_id && user_id.filter(|value| !value.trim().is_empty()).is_none() {
        return Err(EverOSError::Api(
            "batch delete requires explicit user_id".to_string(),
        ));
    }
    Ok(())
}

fn validate_filter_map(map: &Map<String, Value>) -> Result<()> {
    for (key, value) in map {
        match key.as_str() {
            "user_id" => {
                if value.as_str().is_none_or(|text| text.trim().is_empty()) {
                    return Err(EverOSError::Api(
                        "filters.user_id must be a non-empty string".to_string(),
                    ));
                }
            }
            "session_id" => validate_string_or_operator("session_id", value)?,
            "timestamp" => validate_timestamp_filter(value)?,
            "AND" | "OR" => {
                let Some(items) = value.as_array() else {
                    return Err(EverOSError::Api(format!("filters.{key} must be an array")));
                };
                for item in items {
                    let Some(child) = item.as_object() else {
                        return Err(EverOSError::Api(format!(
                            "filters.{key} entries must be objects"
                        )));
                    };
                    validate_filter_map(child)?;
                }
            }
            other => {
                return Err(EverOSError::Api(format!(
                    "unsupported filter field {other}"
                )));
            }
        }
    }
    Ok(())
}

fn validate_string_or_operator(name: &str, value: &Value) -> Result<()> {
    if value.as_str().is_some() {
        return Ok(());
    }
    let Some(map) = value.as_object() else {
        return Err(EverOSError::Api(format!(
            "filters.{name} must be a string or operator object"
        )));
    };
    for op in map.keys() {
        if !matches!(op.as_str(), "eq") {
            return Err(EverOSError::Api(format!(
                "unsupported {name} operator {op}"
            )));
        }
    }
    Ok(())
}

fn validate_timestamp_filter(value: &Value) -> Result<()> {
    if value.is_number() {
        return Ok(());
    }
    let Some(map) = value.as_object() else {
        return Err(EverOSError::Api(
            "filters.timestamp must be a number or operator object".to_string(),
        ));
    };
    for op in map.keys() {
        if !matches!(op.as_str(), "eq" | "gt" | "gte" | "lt" | "lte") {
            return Err(EverOSError::Api(format!(
                "unsupported timestamp operator {op}"
            )));
        }
    }
    Ok(())
}

fn filter_has_session(value: &Value) -> bool {
    match value {
        Value::Object(map) => map
            .iter()
            .any(|(key, value)| key == "session_id" || filter_has_session(value)),
        Value::Array(items) => items.iter().any(filter_has_session),
        _ => false,
    }
}

fn filter_has_session_conflict(value: &Value, expected: &str) -> bool {
    match value {
        Value::Object(map) => map.iter().any(|(key, value)| {
            if key == "session_id" {
                if let Some(text) = value.as_str() {
                    return text != expected;
                }
                if let Some(eq) = value.get("eq").and_then(Value::as_str) {
                    return eq != expected;
                }
            }
            filter_has_session_conflict(value, expected)
        }),
        Value::Array(items) => items
            .iter()
            .any(|item| filter_has_session_conflict(item, expected)),
        _ => false,
    }
}

fn http_error_to_everos_error(status: u16, raw: &str, reason: &str) -> EverOSError {
    let mut detail = raw.to_string();
    if let Ok(Value::Object(map)) = serde_json::from_str::<Value>(raw) {
        let mut bits = vec![status.to_string()];
        if let Some(code) = map.get("code").and_then(Value::as_str) {
            bits.push(code.to_string());
        }
        if let Some(message) = map.get("message").and_then(Value::as_str) {
            bits.push(message.to_string());
        }
        if let Some(request_id) = map.get("request_id").and_then(Value::as_str) {
            bits.push(format!("request_id={request_id}"));
        }
        detail = bits.join(": ");
    }
    if detail.trim().is_empty() {
        detail = reason.to_string();
    }
    EverOSError::Api(detail)
}
