use crate::env::get_env;
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
    #[error("EverOS request timed out")]
    Timeout,
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
        let url = format!("{}{}", self.base_url, normalize_path(path));
        let method = Method::from_bytes(method.to_ascii_uppercase().as_bytes())
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
                EverOSError::Timeout
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
            return Ok(json!({}));
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
        let path = if agent {
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
    ) -> Result<Value> {
        let path = if agent {
            "/api/v1/memories/agent/flush"
        } else {
            "/api/v1/memories/flush"
        };
        self.request_json(
            "POST",
            path,
            Some(json!({"user_id": user_id, "session_id": session_id})),
            None,
        )
    }

    pub fn flush_group_memories(&self, group_id: &str) -> Result<Value> {
        self.request_json(
            "POST",
            "/api/v1/memories/group/flush",
            Some(json!({"group_id": group_id})),
            None,
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
        let resolved_filters = build_filters(user_id, group_id, session_id, filters);
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
        top_k: u64,
        radius: Option<f64>,
        include_original_data: bool,
        timeout: Option<f64>,
    ) -> Result<Value> {
        let resolved_filters = build_filters(user_id, group_id, session_id, filters);
        let memory_types = memory_types.unwrap_or_else(|| {
            DEFAULT_MEMORY_TYPES
                .iter()
                .map(|item| item.to_string())
                .collect()
        });
        self.request_json(
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
        )
    }

    pub fn delete_memories(
        &self,
        memory_id: Option<&str>,
        user_id: Option<&str>,
        group_id: Option<&str>,
        session_id: Option<&str>,
    ) -> Result<Value> {
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

    pub fn update_settings(&self, settings: Value) -> Result<Value> {
        self.request_json("PUT", "/api/v1/settings", Some(settings), None)
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
) -> Value {
    let mut map = match filters {
        Some(Value::Object(map)) => map,
        _ => Map::new(),
    };
    if let Some(user_id) = user_id {
        map.insert("user_id".into(), Value::String(user_id.to_string()));
    }
    if let Some(group_id) = group_id {
        map.insert("group_id".into(), Value::String(group_id.to_string()));
    }
    if let Some(session_id) = session_id.filter(|value| !value.trim().is_empty()) {
        let mut clauses = match map.remove("AND") {
            Some(Value::Array(items)) => items,
            _ => Vec::new(),
        };
        clauses.push(json!({"session_id": session_id}));
        map.insert("AND".into(), Value::Array(clauses));
    }
    Value::Object(map)
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
