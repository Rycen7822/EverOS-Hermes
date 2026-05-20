use crate::agent_visibility::{audit_agent_visibility, build_agent_visibility_report};
use crate::client::{DEFAULT_BASE_URL, DEFAULT_MEMORY_TYPES, EverOSClient, EverOSError};
use crate::context_assembler::{ContextAssemblyConfig, assemble_everos_context};
use crate::env::get_env;
use crate::flush_retry::flush_memories_with_retry;
use crate::formatting::{format_search_context, pretty_json};
use crate::policy::{should_skip_capture, should_skip_recall, stable_query_key};
pub use crate::provider_tools::provider_tool_schemas;
use crate::redaction::{error_payload, sanitized_error_message};
use crate::response_normalization::{as_list as as_list_copy, response_data};
use crate::trajectory::{
    TrajectoryBuildOptions, TrajectoryBuildResult,
    build_agent_trajectory_messages_with_options as build_trajectory_messages_with_options,
};
use crate::workflows;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

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
    pub agent_visibility_retry_flush_backoff_ms: u64,
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
            agent_visibility_retry_flush_backoff_ms: 250,
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

#[derive(Debug)]
pub struct EverOSProvider {
    config: ProviderConfig,
    client: Option<EverOSClient>,
    active: bool,
    write_enabled: bool,
    user_id: String,
    session_id: String,
    prefetch_cache: Mutex<HashMap<String, PrefetchCacheEntry>>,
    agent_saved_fingerprints: Mutex<VecDeque<String>>,
    last_agent_visibility_status: Mutex<Value>,
}

#[derive(Debug, Clone)]
struct PrefetchCacheEntry {
    created_at: Instant,
    text: String,
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
            prefetch_cache: Mutex::new(HashMap::new()),
            agent_saved_fingerprints: Mutex::new(VecDeque::new()),
            last_agent_visibility_status: Mutex::new(Value::Object(Map::new())),
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

    pub fn prefetch(&self, query: &str, session_id: Option<&str>) -> String {
        if !self.active
            || !self.config.auto_recall
            || self.client.is_none()
            || query.trim().is_empty()
        {
            return String::new();
        }
        let sid = session_id
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(&self.session_id);
        let config_value = self.config_value();
        if should_skip_recall(query, sid, &config_value).0 {
            return String::new();
        }
        let query: String = query.chars().take(1000).collect();
        let cache_key = stable_query_key(&query, sid, &config_value);
        if let Some(cached) = self.cached_prefetch(&cache_key) {
            return cached;
        }
        let Some(client) = &self.client else {
            return String::new();
        };

        let main_response = client
            .search_memories(
                &query,
                Some(&self.user_id),
                None,
                None,
                None,
                &self.config.search_method,
                Some(self.config.memory_types.clone()),
                self.config.top_k as i64,
                None,
                false,
                false,
                Some(self.config.timeout),
            )
            .ok();
        let agent_response = if self.config.agent_recall {
            client
                .search_memories(
                    &query,
                    Some(&self.user_id),
                    None,
                    None,
                    None,
                    &self.config.search_method,
                    Some(self.config.agent_memory_types.clone()),
                    self.config.top_k as i64,
                    None,
                    false,
                    false,
                    Some(self.config.agentic_timeout),
                )
                .ok()
        } else {
            None
        };
        let raw_response = if self.config.include_recent_raw && !sid.trim().is_empty() {
            client
                .search_memories(
                    &query,
                    Some(&self.user_id),
                    None,
                    Some(sid),
                    None,
                    &self.config.search_method,
                    Some(vec!["raw_message".to_string()]),
                    self.config.recent_raw_top_k as i64,
                    None,
                    false,
                    false,
                    Some(self.config.timeout),
                )
                .ok()
        } else {
            None
        };
        let merged = merge_agent_response(main_response.as_ref(), agent_response.as_ref());
        let assembled = assemble_everos_context(
            Some(&merged),
            raw_response.as_ref(),
            &query,
            &self.context_assembly_config(),
            "prefetch",
        );
        self.store_prefetch(cache_key, assembled.text.clone());
        assembled.text
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
            "everos_memory_save_and_verify" => self.tool_save_and_verify(&Value::Object(args))?,
            "everos_memory_import_and_verify" => {
                self.tool_import_and_verify(&Value::Object(args))?
            }
            "everos_memory_verify_session" => self.tool_verify_session(&Value::Object(args))?,
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
        let sid = session_id
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(&self.session_id);
        if should_skip_capture(&clean_user, &clean_assistant, sid, &self.config_value()).0 {
            return Ok(());
        }
        let now = now_ms();
        let personal_messages =
            build_personal_turn_messages(&clean_user, &clean_assistant, sid, now);
        let client = self.client.as_ref().expect("checked above");
        let write_personal = self.config.agent_capture_mode != "agent_only";
        let write_agent = self.config.capture_agent_memory
            && self.config.agent_summary_after_turn
            && self.config.agent_capture_mode != "off";
        if write_personal {
            client.add_memories_scoped(
                &self.user_id,
                Some(sid),
                personal_messages,
                true,
                "personal",
            )?;
            if self.config.flush_after_turn {
                client.flush_memories_scoped(&self.user_id, Some(sid), "personal", None)?;
            }
        }
        if write_agent {
            let agent_result =
                self.build_agent_summary_result(&clean_user, &clean_assistant, sid, now + 2);
            self.write_agent_trajectory(&agent_result, sid, true, "sync_turn.agent")?;
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

    pub fn on_session_end(&self, messages: &[Value]) -> Result<(), EverOSError> {
        if !self.active
            || !self.write_enabled
            || self.client.is_none()
            || self.session_id.is_empty()
        {
            return Ok(());
        }
        if self.config.capture_agent_memory && self.config.agent_trajectory_on_session_end {
            let result = self.build_agent_trajectory_result(messages, "session_end");
            let _ =
                self.write_agent_trajectory(&result, &self.session_id, true, "session_end.agent");
        }
        self.client
            .as_ref()
            .expect("checked above")
            .flush_memories_scoped(&self.user_id, Some(&self.session_id), "personal", None)?;
        Ok(())
    }

    pub fn on_pre_compress(&self, messages: &[Value]) -> Result<String, EverOSError> {
        if !self.active
            || !self.write_enabled
            || self.client.is_none()
            || self.session_id.is_empty()
            || !self.config.capture_agent_memory
            || !self.config.agent_trajectory_on_pre_compress
        {
            return Ok(String::new());
        }
        let result = self.build_agent_trajectory_result(messages, "pre_compress");
        let wrote =
            self.write_agent_trajectory(&result, &self.session_id, false, "pre_compress.agent")?;
        if !wrote {
            return Ok(String::new());
        }
        Ok(format!(
            "EverOS captured {} agent trajectory messages for session {}; preserve task outcome and reusable tool lessons.",
            result.output_count, self.session_id
        ))
    }

    pub fn on_delegation(
        &self,
        task: &str,
        result: &str,
        child_session_id: Option<&str>,
    ) -> Result<(), EverOSError> {
        if !self.active
            || !self.write_enabled
            || self.client.is_none()
            || self.session_id.is_empty()
            || !self.config.capture_agent_memory
            || !self.config.agent_trajectory_on_delegation
        {
            return Ok(());
        }
        let child = child_session_id.unwrap_or_default().trim();
        let prefix = if child.is_empty() {
            "[delegation] ".to_string()
        } else {
            format!("[delegation child_session_id={child}] ")
        };
        let now = now_ms();
        let messages = vec![
            json!({"role":"user","timestamp":now,"content":task.trim()}),
            json!({"role":"assistant","timestamp":now + 1,"content":format!("{}{}", prefix, result.trim())}),
        ];
        let mut built = build_trajectory_messages_with_options(
            &messages,
            &self.trajectory_options("delegation", &self.session_id, now, None),
        );
        if !child.is_empty() {
            for message in built.messages.iter_mut() {
                if message.get("role").and_then(Value::as_str) == Some("assistant")
                    && let Some(map) = message.as_object_mut()
                {
                    map.insert(
                        "child_session_id".to_string(),
                        Value::String(child.to_string()),
                    );
                }
            }
        }
        self.write_agent_trajectory(&built, &self.session_id, true, "delegation.agent")?;
        Ok(())
    }

    fn cached_prefetch(&self, cache_key: &str) -> Option<String> {
        if !self.config.prefetch_cache_enabled || self.config.prefetch_cache_ttl_seconds == 0 {
            return None;
        }
        let ttl = Duration::from_secs(self.config.prefetch_cache_ttl_seconds);
        let mut cache = self.prefetch_cache.lock().ok()?;
        if let Some(entry) = cache.get(cache_key)
            && entry.created_at.elapsed() <= ttl
        {
            return Some(entry.text.clone());
        }
        cache.remove(cache_key);
        None
    }

    fn store_prefetch(&self, cache_key: String, text: String) {
        if !self.config.prefetch_cache_enabled || self.config.prefetch_cache_ttl_seconds == 0 {
            return;
        }
        if let Ok(mut cache) = self.prefetch_cache.lock() {
            cache.insert(
                cache_key,
                PrefetchCacheEntry {
                    created_at: Instant::now(),
                    text,
                },
            );
            while cache.len() > 128 {
                if let Some(oldest) = cache
                    .iter()
                    .min_by_key(|(_, entry)| entry.created_at)
                    .map(|(key, _)| key.clone())
                {
                    cache.remove(&oldest);
                } else {
                    break;
                }
            }
        }
    }

    fn context_assembly_config(&self) -> ContextAssemblyConfig {
        ContextAssemblyConfig {
            max_context_chars: self.config.max_context_chars,
            profile_max_items: self.config.profile_max_items,
            agent_skills_max_items: self.config.agent_skills_max_items,
            agent_cases_max_items: self.config.agent_cases_max_items,
            episodic_max_items: self.config.episodic_max_items,
            recent_raw_top_k: self.config.recent_raw_top_k,
            min_score: self.config.min_score,
        }
    }

    fn config_value(&self) -> Value {
        serde_json::to_value(&self.config).unwrap_or_else(|_| Value::Object(Map::new()))
    }

    fn build_agent_summary_result(
        &self,
        user_content: &str,
        assistant_content: &str,
        session_id: &str,
        now_ms: u128,
    ) -> TrajectoryBuildResult {
        let messages = vec![
            json!({"role":"user","timestamp":now_ms,"content":format!("Task request: {}", truncate_for_memory(user_content, 4000))}),
            json!({"role":"assistant","timestamp":now_ms + 1,"content":format!("Agent response summary: {}\nOutcome: completed_or_partial\nReusable lesson hint: capture approach, correction, and verification if useful.", truncate_for_memory(assistant_content, 4000))}),
        ];
        build_trajectory_messages_with_options(
            &messages,
            &self.trajectory_options("sync_turn", session_id, now_ms, Some(2)),
        )
    }

    fn build_agent_trajectory_result(
        &self,
        messages: &[Value],
        source: &str,
    ) -> TrajectoryBuildResult {
        build_trajectory_messages_with_options(
            messages,
            &self.trajectory_options(source, &self.session_id, now_ms(), None),
        )
    }

    fn trajectory_options(
        &self,
        source: &str,
        session_id: &str,
        now_ms: u128,
        max_messages: Option<usize>,
    ) -> TrajectoryBuildOptions {
        TrajectoryBuildOptions {
            session_id: session_id.to_string(),
            source: source.to_string(),
            now_ms: Some(now_ms),
            max_messages: max_messages.unwrap_or(self.config.agent_max_messages),
            max_message_chars: self.config.agent_max_message_chars,
            max_tool_result_chars: self.config.agent_max_tool_result_chars,
            max_payload_chars: self.config.agent_max_payload_chars,
            include_system: false,
        }
    }

    fn agent_visibility_queries(
        &self,
        texts: Vec<String>,
        session_id: &str,
        markers: &[&str],
    ) -> Vec<String> {
        let configured = self
            .config
            .agent_visibility_queries
            .iter()
            .map(|query| query.trim().to_string())
            .filter(|query| !query.is_empty())
            .collect::<Vec<_>>();
        if !configured.is_empty() {
            return configured;
        }
        let mut queries = texts
            .into_iter()
            .map(|text| text.trim().chars().take(200).collect::<String>())
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>();
        for marker in markers {
            let marker = marker.trim();
            if !marker.is_empty() {
                queries.push(marker.to_string());
            }
        }
        if !session_id.trim().is_empty() {
            queries.push(format!("session:{session_id}"));
        }
        if queries.is_empty() {
            queries.push("agent memory".to_string());
        }
        queries.truncate(2);
        queries
    }

    fn write_agent_trajectory(
        &self,
        result: &TrajectoryBuildResult,
        session_id: &str,
        flush_allowed: bool,
        _operation: &str,
    ) -> Result<bool, EverOSError> {
        if result.messages.is_empty() || self.agent_fingerprint_seen(&result.fingerprint) {
            return Ok(false);
        }
        let Some(client) = &self.client else {
            return Ok(false);
        };
        client.add_memories_scoped(
            &self.user_id,
            Some(session_id),
            result.messages.clone(),
            true,
            "agent",
        )?;
        self.remember_agent_fingerprint(result.fingerprint.clone());
        let mut flush_payload = None;
        if flush_allowed && self.config.agent_flush_after_turn {
            let (flush, attempt_count) = flush_memories_with_retry(
                client,
                &self.user_id,
                Some(session_id),
                "agent",
                None,
                2,
            )?;
            flush_payload = Some(flush_result_payload_with_attempt(
                &flush,
                Some(attempt_count),
            ));
        }
        let should_audit = self.config.agent_visibility_verify_after_write
            || (flush_payload.is_some() && self.config.agent_visibility_verify_after_flush);
        if should_audit {
            let texts = result
                .messages
                .iter()
                .filter_map(|message| message.get("content").and_then(Value::as_str))
                .map(ToString::to_string)
                .collect::<Vec<_>>();
            let queries = self.agent_visibility_queries(texts, session_id, &[_operation]);
            let mut visibility = audit_agent_visibility(
                client,
                &self.user_id,
                Some(session_id),
                &queries,
                self.config.agent_visibility_top_k as i64,
                Some(self.config.agent_visibility_timeout),
                self.config.agent_visibility_get_page_size as u64,
            );
            if let Some(map) = visibility.as_object_mut() {
                map.insert("agent_raw_queued".to_string(), Value::Bool(true));
                map.insert(
                    "agent_flush".to_string(),
                    flush_payload.clone().unwrap_or(Value::Null),
                );
            }
            if let Ok(mut slot) = self.last_agent_visibility_status.lock() {
                *slot = visibility;
            }
        } else if let Some(flush_payload) = flush_payload
            && let Ok(mut slot) = self.last_agent_visibility_status.lock()
        {
            *slot = build_agent_visibility_report(
                Some(true),
                Some(flush_payload),
                vec![],
                Some(&self.user_id),
                Some(session_id),
            );
        }
        Ok(true)
    }

    fn agent_fingerprint_seen(&self, fingerprint: &str) -> bool {
        self.agent_saved_fingerprints
            .lock()
            .map(|items| items.iter().any(|item| item == fingerprint))
            .unwrap_or(false)
    }

    fn remember_agent_fingerprint(&self, fingerprint: String) {
        if fingerprint.is_empty() {
            return;
        }
        if let Ok(mut items) = self.agent_saved_fingerprints.lock() {
            if let Some(pos) = items.iter().position(|item| item == &fingerprint) {
                items.remove(pos);
            }
            items.push_back(fingerprint);
            let max_entries = self.config.agent_dedupe_entries.max(1);
            while items.len() > max_entries {
                items.pop_front();
            }
        }
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
        let scope = normalize_scope_arg(&value_string(args, "scope"));
        let role = non_empty_or(
            &value_string(args, "role"),
            if scope == "agent" {
                "assistant"
            } else {
                "user"
            },
        );
        let mut message = json!({"role":role,"timestamp":now_ms(),"content":content});
        if let (Some(tool_call_id), Some(map)) = (
            args.get("tool_call_id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty()),
            message.as_object_mut(),
        ) {
            map.insert(
                "tool_call_id".to_string(),
                Value::String(tool_call_id.to_string()),
            );
        }
        let result = self.client.as_ref().expect("active").add_memories_scoped(
            &self.user_id,
            session_id_opt,
            vec![message],
            true,
            &scope,
        )?;
        let flush_requested = as_bool(args.get("flush"), true);
        let flush_payload = if flush_requested {
            match flush_memories_with_retry(
                self.client.as_ref().expect("active"),
                &self.user_id,
                session_id_opt,
                &scope,
                None,
                2,
            ) {
                Ok((response, attempt_count)) => Some(flush_result_payload_with_attempt(
                    &response,
                    Some(attempt_count),
                )),
                Err(err @ EverOSError::Timeout { .. }) => Some(timeout_payload("flush", &err)),
                Err(err) => Some(error_payload("flush", &err)),
            }
        } else {
            None
        };
        let mut payload = save_result_payload(
            &result,
            &self.user_id,
            session_id_opt,
            &scope,
            flush_requested,
            flush_payload,
        );
        if scope == "agent" {
            let flush_for_visibility = payload.get("flush").cloned();
            let should_audit = self.config.agent_visibility_verify_after_write
                || (flush_for_visibility.is_some()
                    && self.config.agent_visibility_verify_after_flush);
            if should_audit {
                let queries = self.agent_visibility_queries(
                    vec![content.clone()],
                    session_id,
                    &["tool_save"],
                );
                let mut visibility = audit_agent_visibility(
                    self.client.as_ref().expect("active"),
                    &self.user_id,
                    session_id_opt,
                    &queries,
                    self.config.agent_visibility_top_k as i64,
                    Some(self.config.agent_visibility_timeout),
                    self.config.agent_visibility_get_page_size as u64,
                );
                if let Some(map) = visibility.as_object_mut() {
                    map.insert("agent_raw_queued".to_string(), Value::Bool(true));
                    map.insert(
                        "agent_flush".to_string(),
                        flush_for_visibility.unwrap_or(Value::Null),
                    );
                }
                if let Some(map) = payload.as_object_mut() {
                    map.insert("agent_visibility".to_string(), visibility);
                }
            } else {
                add_agent_visibility(
                    &mut payload,
                    Some(true),
                    Some(&self.user_id),
                    session_id_opt,
                );
            }
        }
        Ok(serde_json::to_string(&payload).unwrap())
    }

    fn tool_search(&self, args: &Value) -> Result<String, EverOSError> {
        let query = value_string(args, "query").trim().to_string();
        if query.is_empty() {
            return Ok(tool_error("query is required"));
        }
        let limit = if args.get("top_k").is_some() {
            int_between(args.get("top_k"), -1, 100, self.config.top_k)
        } else {
            int_between(args.get("limit"), -1, 100, self.config.top_k)
        };
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
            args.get("filters").cloned(),
            &method,
            Some(memory_types),
            limit,
            float_or_none(args.get("radius")),
            as_bool(args.get("include_original_data"), false),
            as_bool(args.get("include_vectors"), false),
            Some(if method == "agentic" {
                60.0
            } else {
                self.config.timeout
            }),
        )?;
        if value_string(args, "response_format") == "markdown" {
            let formatted =
                format_search_context(&response, self.config.max_context_items as usize);
            if !formatted.is_empty() {
                return Ok(formatted);
            }
        }
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
            args.get("filters").cloned(),
            &memory_type,
            int_between(args.get("page"), 1, 10000, 1) as u64,
            int_between(args.get("page_size"), 1, 100, 20) as u64,
            &non_empty_or(&value_string(args, "rank_by"), "timestamp"),
            &non_empty_or(&value_string(args, "rank_order"), "desc"),
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
        let scope = normalize_scope_arg(&value_string(args, "scope"));
        let (response, attempt_count) = match flush_memories_with_retry(
            self.client.as_ref().expect("active"),
            &self.user_id,
            if sid.is_empty() { None } else { Some(sid) },
            &scope,
            float_or_none(args.get("timeout")),
            2,
        ) {
            Ok(result) => result,
            Err(err @ EverOSError::Timeout { .. }) => {
                return Ok(pretty_json(&timeout_payload("flush", &err)));
            }
            Err(err) => return Err(err),
        };
        if scope == "agent" {
            let flush_payload = flush_result_payload_with_attempt(&response, Some(attempt_count));
            return Ok(pretty_json(&json!({
                "flush": flush_payload.clone(),
                "agent_visibility": build_agent_visibility_report(
                    None,
                    Some(flush_payload),
                    vec![],
                    Some(&self.user_id),
                    if sid.is_empty() { None } else { Some(sid) },
                ),
            })));
        }
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

    fn tool_save_and_verify(&self, args: &Value) -> Result<String, EverOSError> {
        let content = value_string(args, "content").trim().to_string();
        if content.is_empty() {
            return Ok(tool_error("content is required"));
        }
        let session_id = value_string(args, "session_id");
        let sid = if session_id.is_empty() {
            self.session_id.as_str()
        } else {
            session_id.as_str()
        };
        let mut queries =
            parse_string_list(args.get("verification_queries").unwrap_or(&Value::Null));
        let verification_query = value_string(args, "verification_query");
        if !verification_query.trim().is_empty() {
            queries.insert(0, verification_query.trim().to_string());
        }
        let memory_types = args
            .get("memory_types")
            .map(parse_string_list)
            .filter(|items| !items.is_empty());
        let scope = normalize_scope_arg(&value_string(args, "scope"));
        let role = optional_value_string(args, "role");
        let tool_call_id = optional_value_string(args, "tool_call_id");
        let result = workflows::save_and_verify(
            self.client.as_ref().expect("active"),
            &content,
            &self.user_id,
            if sid.is_empty() { None } else { Some(sid) },
            &scope,
            role.as_deref(),
            tool_call_id.as_deref(),
            as_bool(args.get("flush"), true),
            float_or_none(args.get("flush_timeout")),
            queries,
            memory_types,
            int_between(args.get("top_k"), -1, 100, self.config.top_k),
            float_or_none(args.get("timeout")),
        )?;
        Ok(pretty_json(&result))
    }

    fn tool_import_and_verify(&self, args: &Value) -> Result<String, EverOSError> {
        let session_id = value_string(args, "session_id");
        let sid = if session_id.is_empty() {
            self.session_id.as_str()
        } else {
            session_id.as_str()
        };
        let messages = args
            .get("messages")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let memory_types = args
            .get("memory_types")
            .map(parse_string_list)
            .filter(|items| !items.is_empty());
        let file_path = optional_value_string(args, "file_path");
        let scope = normalize_scope_arg(&value_string(args, "scope"));
        let result = workflows::import_and_verify(
            self.client.as_ref().expect("active"),
            &self.user_id,
            if sid.is_empty() { None } else { Some(sid) },
            messages,
            file_path.as_deref(),
            &scope,
            as_bool(args.get("dry_run"), false),
            int_between(args.get("batch_size"), 1, 100, 50) as usize,
            as_bool(args.get("flush"), true),
            float_or_none(args.get("flush_timeout")),
            parse_string_list(args.get("verification_queries").unwrap_or(&Value::Null)),
            memory_types,
            int_between(args.get("top_k"), -1, 100, self.config.top_k),
            float_or_none(args.get("timeout")),
            "import_and_verify",
        )?;
        Ok(pretty_json(&result))
    }

    fn tool_verify_session(&self, args: &Value) -> Result<String, EverOSError> {
        let queries = parse_string_list(args.get("verification_queries").unwrap_or(&Value::Null));
        if queries.is_empty() {
            return Ok(tool_error("verification_queries is required"));
        }
        let session_id = value_string(args, "session_id");
        let sid = if session_id.is_empty() {
            self.session_id.as_str()
        } else {
            session_id.as_str()
        };
        let memory_types = args
            .get("memory_types")
            .map(parse_string_list)
            .filter(|items| !items.is_empty());
        let result = workflows::verify_session_ingest(
            self.client.as_ref().expect("active"),
            &self.user_id,
            if sid.is_empty() { None } else { Some(sid) },
            queries,
            memory_types,
            &normalize_scope_arg(&value_string(args, "scope")),
            int_between(args.get("top_k"), -1, 100, self.config.top_k),
            float_or_none(args.get("timeout")),
        )?;
        Ok(pretty_json(&result))
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
    if let Some(value) = map
        .get("agent_visibility_retry_flush_backoff_ms")
        .and_then(Value::as_u64)
    {
        config.agent_visibility_retry_flush_backoff_ms = value.clamp(0, 2_000);
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

fn tool_error(message: &str) -> String {
    serde_json::to_string(&json!({"error": sanitized_error_message(message)})).unwrap()
}

fn timeout_payload(operation: &str, err: &EverOSError) -> Value {
    json!({
        "ok": false,
        "operation": operation,
        "error": sanitized_error_message(err),
        "retryable": true,
        "suggested_next_actions": [
            "search existing memories before retrying, because the server may have completed the request after the client timed out",
            "if the operation returned a task_id or request_id earlier, check that status before issuing another write/flush",
            "retry with a longer timeout only if search/status checks do not show the expected result"
        ]
    })
}

fn flush_result_payload_with_attempt(response: &Value, attempt_count: Option<usize>) -> Value {
    let data = response.get("data").unwrap_or(response);
    let mut payload = serde_json::Map::new();
    payload.insert("ok".to_string(), Value::Bool(true));
    if let Some(attempt_count) = attempt_count.filter(|value| *value > 1) {
        payload.insert("attempt_count".to_string(), json!(attempt_count));
    }
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
    scope: &str,
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
        "scope": scope,
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

fn add_agent_visibility(
    payload: &mut Value,
    agent_raw_queued: Option<bool>,
    user_id: Option<&str>,
    session_id: Option<&str>,
) {
    let flush = payload.get("flush").cloned();
    if let Some(map) = payload.as_object_mut() {
        map.insert(
            "agent_visibility".to_string(),
            build_agent_visibility_report(agent_raw_queued, flush, vec![], user_id, session_id),
        );
    }
}

fn value_string(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn optional_value_string(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(ToString::to_string)
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

fn normalize_scope_arg(scope: &str) -> String {
    match scope.trim().to_ascii_lowercase().as_str() {
        "agent" => "agent".to_string(),
        _ => "personal".to_string(),
    }
}

fn parse_string_list(value: &Value) -> Vec<String> {
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

fn build_personal_turn_messages(
    user_content: &str,
    assistant_content: &str,
    session_id: &str,
    now_ms: u128,
) -> Vec<Value> {
    let mut messages = vec![
        json!({"role":"user","timestamp":now_ms,"content":user_content}),
        json!({"role":"assistant","timestamp":now_ms + 1,"content":assistant_content}),
    ];
    for (index, message) in messages.iter_mut().enumerate() {
        let role = message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let timestamp = message
            .get("timestamp")
            .and_then(Value::as_u64)
            .unwrap_or(now_ms as u64);
        let content = message
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let message_id = personal_message_id(session_id, role, index, timestamp, content);
        if let Some(map) = message.as_object_mut() {
            map.insert("message_id".to_string(), Value::String(message_id));
        }
    }
    messages
}

fn personal_message_id(
    session_id: &str,
    role: &str,
    index: usize,
    timestamp: u64,
    content: &str,
) -> String {
    let payload = json!({
        "session_id": session_id,
        "role": role,
        "index": index,
        "timestamp": timestamp,
        "content": content,
    });
    format!(
        "eh_{}",
        &hash_text(&serde_json::to_string(&payload).unwrap_or_default())[..32]
    )
}

fn merge_agent_response(main_response: Option<&Value>, agent_response: Option<&Value>) -> Value {
    let mut data = response_data(main_response);
    let agent_data = response_data(agent_response);
    for key in ["agent_skills", "agent_cases"] {
        if let Some(value) = agent_data.get(key) {
            let mut items = as_list_copy(data.get(key));
            items.extend(as_list_copy(Some(value)));
            if !items.is_empty() {
                data.insert(key.to_string(), Value::Array(items));
            }
        }
    }
    if let Some(value) = agent_data.get("agent_memory") {
        let mut items = as_list_copy(data.get("agent_memory"));
        items.extend(as_list_copy(Some(value)));
        if !items.is_empty() {
            data.insert("agent_memory".to_string(), Value::Array(items));
        }
    } else if let Some(value) = agent_data
        .get("results")
        .or_else(|| agent_data.get("memories"))
        .or_else(|| agent_data.get("episodes"))
    {
        let mut items = as_list_copy(data.get("agent_memory"));
        items.extend(as_list_copy(Some(value)));
        if !items.is_empty() {
            data.insert("agent_memory".to_string(), Value::Array(items));
        }
    }
    Value::Object(Map::from_iter([("data".to_string(), Value::Object(data))]))
}

fn hash_text(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    let digest = hasher.finalize();
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn truncate_for_memory(text: &str, limit: usize) -> String {
    let text = text.trim();
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let mut truncated = text.chars().take(limit).collect::<String>();
    truncated.push('…');
    truncated
}
