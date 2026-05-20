use crate::agent_visibility::{audit_agent_visibility, build_agent_visibility_report};
use crate::client::{EverOSClient, EverOSError};
use crate::context_assembler::{ContextAssemblyConfig, assemble_everos_context};
use crate::env::get_env;
use crate::flush_retry::flush_memories_with_retry;
use crate::formatting::{format_search_context, pretty_json};
use crate::policy::{should_skip_capture, should_skip_recall, stable_query_key};
pub use crate::provider_config::{ProviderConfig, load_config, save_config};
use crate::provider_config::{as_bool, parse_string_list};
pub use crate::provider_tools::provider_tool_schemas;
use crate::redaction::{error_payload, sanitized_error_message, strip_context_blocks};
use crate::response_normalization::{as_list as as_list_copy, response_data};
use crate::trajectory::{
    TrajectoryBuildOptions, TrajectoryBuildResult,
    build_agent_trajectory_messages_with_options as build_trajectory_messages_with_options,
};
use crate::workflows;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

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
                self.config.agent_visibility_retry_flush_attempts,
            )?;
            flush_payload = Some(workflows::tool_flush_result_payload_with_attempt(
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
                self.config.agent_visibility_retry_flush_attempts,
            ) {
                Ok((response, attempt_count)) => {
                    Some(workflows::tool_flush_result_payload_with_attempt(
                        &response,
                        Some(attempt_count),
                    ))
                }
                Err(err @ EverOSError::Timeout { .. }) => {
                    Some(workflows::tool_timeout_payload("flush", &err))
                }
                Err(err) => Some(error_payload("flush", &err)),
            }
        } else {
            None
        };
        let mut payload = workflows::tool_save_result_payload(
            &result,
            &self.user_id,
            session_id_opt,
            &scope,
            flush_requested,
            flush_payload,
        );
        if scope == "agent" {
            let should_audit = self.config.agent_visibility_verify_after_write
                || (flush_requested && self.config.agent_visibility_verify_after_flush);
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
                        payload.get("flush").cloned().unwrap_or(Value::Null),
                    );
                }
                if let Some(map) = payload.as_object_mut() {
                    map.insert("agent_visibility".to_string(), visibility);
                }
            } else {
                workflows::add_agent_visibility(
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
                self.config.agentic_timeout
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
            self.config.agent_visibility_retry_flush_attempts,
        ) {
            Ok(result) => result,
            Err(err @ EverOSError::Timeout { .. }) => {
                return Ok(pretty_json(&workflows::tool_timeout_payload("flush", &err)));
            }
            Err(err) => return Err(err),
        };
        if scope == "agent" {
            let flush_payload =
                workflows::tool_flush_result_payload_with_attempt(&response, Some(attempt_count));
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
        let response =
            self.client
                .as_ref()
                .expect("active")
                .delete_memories(Some(&memory_id), None, None)?;
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
            None,
            queries,
            None,
            int_between(args.get("top_k"), -1, 100, self.config.top_k),
            None,
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
            None,
            parse_string_list(args.get("verification_queries").unwrap_or(&Value::Null)),
            None,
            int_between(args.get("top_k"), -1, 100, self.config.top_k),
            None,
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
    strip_context_blocks(text).trim().to_string()
}

fn tool_error(message: &str) -> String {
    serde_json::to_string(&json!({"error": sanitized_error_message(message)})).unwrap()
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
