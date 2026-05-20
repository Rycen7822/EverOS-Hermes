use crate::agent_visibility::{
    audit_agent_visibility_with_options, build_agent_visibility_report,
    workflow_status_from_agent_visibility,
};
use crate::client::{DEFAULT_MEMORY_TYPES, EverOSClient, EverOSError};
use crate::redaction::{error_payload, sanitized_error_message};
use crate::response_normalization::count_hits;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn success_envelope(workflow: &str, status: &str) -> serde_json::Map<String, Value> {
    let mut payload = serde_json::Map::new();
    payload.insert("ok".to_string(), Value::Bool(true));
    payload.insert("workflow".to_string(), Value::String(workflow.to_string()));
    payload.insert("status".to_string(), Value::String(status.to_string()));
    payload.insert("retryable".to_string(), Value::Bool(false));
    payload.insert("suggested_next_actions".to_string(), Value::Array(vec![]));
    payload
}

pub fn error_envelope(workflow: &str, error_code: &str, message: &str) -> Value {
    json!({
        "ok": false,
        "workflow": workflow,
        "status": "error",
        "error_code": error_code,
        "message": sanitized_error_message(message),
        "retryable": false,
        "suggested_next_actions": ["inspect the validation error and retry with corrected arguments"]
    })
}

pub fn tool_flush_result_payload(response: &Value) -> Value {
    tool_flush_result_payload_with_attempt(response, None)
}

pub fn tool_flush_result_payload_with_attempt(
    response: &Value,
    attempt_count: Option<usize>,
) -> Value {
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

pub fn flush_result_payload(response: &Value) -> Value {
    flush_result_payload_with_attempt(response, None)
}

pub fn flush_result_payload_with_attempt(response: &Value, attempt_count: Option<usize>) -> Value {
    let mut payload = tool_flush_result_payload_with_attempt(response, attempt_count);
    if let Some(map) = payload.as_object_mut() {
        map.entry("status".to_string())
            .or_insert_with(|| Value::String("success".to_string()));
        if let Some(status_code) = response.get("status_code") {
            map.insert("status_code".to_string(), status_code.clone());
        }
    }
    payload
}

pub fn tool_timeout_payload(operation: &str, err: &EverOSError) -> Value {
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

pub fn timeout_payload(operation: &str, err: &EverOSError) -> Value {
    let mut payload = tool_timeout_payload(operation, err);
    if let Some(map) = payload.as_object_mut() {
        let message = map
            .remove("error")
            .unwrap_or_else(|| Value::String(sanitized_error_message(err)));
        map.insert("status".to_string(), Value::String("timeout".to_string()));
        map.insert(
            "error_code".to_string(),
            Value::String("timeout".to_string()),
        );
        map.insert("message".to_string(), message);
    }
    payload
}

fn verification_error_payload(err: &EverOSError) -> Value {
    let mut payload = error_payload("verification", err);
    if let Some(map) = payload.as_object_mut() {
        map.insert("verified".to_string(), Value::Bool(false));
        map.insert("queries".to_string(), Value::Array(vec![]));
        map.insert(
            "suggested_next_actions".to_string(),
            json!([
                "the memory write was queued; inspect EverOS status/search before retrying verification",
                "rerun verify_session_ingest with the same user_id/session_id before duplicating writes"
            ]),
        );
    }
    payload
}

fn verification_failed(verification: &Value) -> bool {
    verification.get("operation").and_then(Value::as_str) == Some("verification")
        && verification.get("ok") == Some(&Value::Bool(false))
}

pub fn tool_save_result_payload(
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
        "user_id": user_id,
        "session_id": session_id,
        "scope": scope,
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

pub fn save_result_payload(
    result: &Value,
    user_id: &str,
    session_id: Option<&str>,
    scope: &str,
    flush_requested: bool,
    flush: Option<Value>,
) -> Value {
    let mut payload =
        tool_save_result_payload(result, user_id, session_id, scope, flush_requested, flush);
    if let Some(map) = payload.as_object_mut() {
        if map
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("")
            .is_empty()
        {
            map.insert("status".to_string(), Value::String("queued".to_string()));
        }
        map.insert("ok".to_string(), Value::Bool(true));
        if let (true, Some(flush)) = (
            flush_requested,
            map.get_mut("flush").and_then(Value::as_object_mut),
        ) {
            flush
                .entry("status".to_string())
                .or_insert_with(|| Value::String("missing".to_string()));
        }
    }
    payload
}

pub fn add_agent_visibility(
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

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn normalize_scope(scope: &str) -> Result<String, EverOSError> {
    match scope.trim() {
        "" | "personal" => Ok("personal".to_string()),
        "agent" => Ok("agent".to_string()),
        other => Err(EverOSError::Api(format!(
            "scope must be personal or agent, got {other}"
        ))),
    }
}

fn load_messages_from_file(file_path: &str) -> Result<Vec<Value>, EverOSError> {
    let text = fs::read_to_string(file_path).map_err(|err| {
        EverOSError::Api(format!("failed to read import file {file_path}: {err}"))
    })?;
    if file_path.ends_with(".json") {
        let parsed: Value = serde_json::from_str(&text)
            .map_err(|err| EverOSError::Api(format!("failed to parse JSON import file: {err}")))?;
        if let Some(items) = parsed.as_array() {
            return Ok(items.iter().map(coerce_message).collect());
        }
        if let Some(items) = parsed.get("messages").and_then(Value::as_array) {
            return Ok(items.iter().map(coerce_message).collect());
        }
        return Err(EverOSError::Api(
            "JSON import file must be a list or object with a messages list".to_string(),
        ));
    }
    if file_path.ends_with(".jsonl") || file_path.ends_with(".ndjson") {
        let mut messages = Vec::new();
        for (line_no, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let parsed: Value = serde_json::from_str(line).map_err(|err| {
                EverOSError::Api(format!("failed to parse JSONL line {}: {err}", line_no + 1))
            })?;
            messages.push(coerce_message(&parsed));
        }
        return Ok(messages);
    }
    Ok(text
        .split("\n\n")
        .map(str::trim)
        .filter(|chunk| !chunk.is_empty())
        .map(|chunk| json!({"role":"user","timestamp":now_ms(),"content":chunk}))
        .collect())
}

fn coerce_message(value: &Value) -> Value {
    if let Some(text) = value.as_str() {
        return json!({"role":"user","timestamp":now_ms(),"content":text});
    }
    let mut map = value.as_object().cloned().unwrap_or_default();
    map.entry("role".to_string())
        .or_insert_with(|| Value::String("user".to_string()));
    map.entry("timestamp".to_string())
        .or_insert_with(|| json!(now_ms()));
    let content = map
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    map.insert("content".to_string(), Value::String(content));
    Value::Object(map)
}

pub fn normalize_import_messages(
    messages: Vec<Value>,
    file_path: Option<&str>,
    default_role: &str,
) -> Result<(Vec<Value>, Vec<String>), EverOSError> {
    let mut loaded = Vec::new();
    if let Some(path) = file_path.filter(|path| !path.trim().is_empty()) {
        loaded.extend(load_messages_from_file(path)?);
    }
    loaded.extend(messages.into_iter().map(|message| coerce_message(&message)));
    let mut normalized = Vec::new();
    let mut warnings = Vec::new();
    let mut seen = HashSet::new();
    for (index, value) in loaded.into_iter().enumerate() {
        let mut map = value.as_object().cloned().unwrap_or_default();
        let role = map
            .get("role")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|role| !role.is_empty())
            .unwrap_or(default_role)
            .to_string();
        let content = map
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        if content.is_empty() {
            warnings.push(format!("messages[{index}].content is empty"));
        }
        let fingerprint = format!("{role}\0{content}");
        if !seen.insert(fingerprint) {
            warnings.push(format!(
                "messages[{index}] appears duplicate by role+content"
            ));
        }
        if role == "tool"
            && map
                .get("tool_call_id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .is_none()
        {
            warnings.push(format!(
                "messages[{index}].tool_call_id is required when role=tool"
            ));
        }
        if !map
            .get("timestamp")
            .is_some_and(|value| value.as_i64().is_some() || value.as_u64().is_some())
        {
            warnings.push(format!(
                "messages[{index}].timestamp must be an integer epoch millisecond value"
            ));
        }
        map.insert("role".to_string(), Value::String(role));
        map.insert("content".to_string(), Value::String(content));
        map.entry("timestamp".to_string())
            .or_insert_with(|| json!(now_ms()));
        normalized.push(Value::Object(map));
    }
    Ok((normalized, warnings))
}

fn json_bytes(value: &Value) -> usize {
    serde_json::to_vec(value)
        .map(|bytes| bytes.len())
        .unwrap_or_else(|_| value.to_string().len())
}

fn batch_payload_bytes(batch: &[Value]) -> usize {
    json_bytes(&json!({"messages": batch}))
}

fn message_metrics(messages: &[Value], batch_size: usize) -> Value {
    let size = batch_size.clamp(1, 100);
    let mut total_content_chars = 0usize;
    let mut max_content_chars = 0usize;
    for message in messages {
        let len = message
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or("")
            .chars()
            .count();
        total_content_chars += len;
        max_content_chars = max_content_chars.max(len);
    }
    let max_batch_payload_bytes = messages
        .chunks(size)
        .map(batch_payload_bytes)
        .max()
        .unwrap_or(0);
    json!({
        "total_messages": messages.len(),
        "batch_count": messages.chunks(size).len(),
        "requested_batch_size": batch_size,
        "effective_batch_size": size,
        "total_content_chars": total_content_chars,
        "max_content_chars": max_content_chars,
        "estimated_payload_bytes": batch_payload_bytes(messages),
        "max_batch_payload_bytes": max_batch_payload_bytes,
    })
}

fn is_cloud_403(err: &EverOSError) -> bool {
    let text = err.to_string().to_ascii_lowercase();
    text.contains("403") || text.contains("forbidden")
}

#[allow(clippy::too_many_arguments)]
pub fn verify_session_ingest(
    client: &EverOSClient,
    user_id: &str,
    session_id: Option<&str>,
    verification_queries: Vec<String>,
    memory_types: Option<Vec<String>>,
    scope: &str,
    top_k: i64,
    timeout: Option<f64>,
) -> Result<Value, EverOSError> {
    let scope = normalize_scope(scope)?;
    let memory_types = memory_types.unwrap_or_else(|| {
        DEFAULT_MEMORY_TYPES
            .iter()
            .map(|item| item.to_string())
            .collect()
    });
    let mut queries = Vec::new();
    let reuse_agent_memory_search =
        scope == "agent" && memory_types == ["agent_memory".to_string()];
    let mut agent_search_responses: HashMap<String, Value> = HashMap::new();
    let clean_queries: Vec<String> = verification_queries
        .into_iter()
        .map(|query| query.trim().to_string())
        .filter(|query| !query.is_empty())
        .collect();
    for query in &clean_queries {
        let response = client.search_memories(
            query,
            Some(user_id),
            session_id,
            None,
            "hybrid",
            Some(memory_types.clone()),
            top_k,
            None,
            false,
            false,
            timeout,
        )?;
        let hit_count = count_hits(&response);
        if reuse_agent_memory_search {
            agent_search_responses.insert(query.clone(), response.clone());
        }
        queries.push(json!({
            "query": query,
            "status": if hit_count > 0 { "hit" } else { "miss" },
            "hit_count": hit_count,
            "response": response,
        }));
    }
    let (mut status, mut verified) = if queries.is_empty() {
        ("verification_skipped".to_string(), Value::Null)
    } else if queries
        .iter()
        .all(|query| query["hit_count"].as_u64().unwrap_or(0) > 0)
    {
        ("verified".to_string(), Value::Bool(true))
    } else if queries
        .iter()
        .any(|query| query["hit_count"].as_u64().unwrap_or(0) > 0)
    {
        ("partially_verified".to_string(), Value::Bool(false))
    } else {
        ("not_yet_searchable".to_string(), Value::Bool(false))
    };
    let agent_visibility = if scope == "agent" {
        let visibility = audit_agent_visibility_with_options(
            client,
            user_id,
            session_id,
            &clean_queries,
            top_k,
            timeout,
            20,
            if reuse_agent_memory_search {
                Some(&agent_search_responses)
            } else {
                None
            },
            false,
        );
        status = workflow_status_from_agent_visibility(&visibility, &status).to_string();
        verified = if status == "verified" {
            Value::Bool(true)
        } else if matches!(
            status.as_str(),
            "agent_not_visible" | "partially_verified" | "agent_visibility_error"
        ) {
            Value::Bool(false)
        } else {
            verified
        };
        Some(visibility)
    } else {
        None
    };
    let mut payload = success_envelope("verify_session_ingest", &status);
    payload.insert("verified".to_string(), verified.clone());
    payload.insert("scope".to_string(), Value::String(scope));
    payload.insert("user_id".to_string(), Value::String(user_id.to_string()));
    payload.insert("session_id".to_string(), json!(session_id));
    payload.insert("memory_types".to_string(), json!(memory_types));
    payload.insert("queries".to_string(), Value::Array(queries));
    if let Some(visibility) = agent_visibility {
        payload.insert("agent_visibility".to_string(), visibility);
    }
    if verified != Value::Bool(true) {
        payload.insert(
            "suggested_next_actions".to_string(),
            json!([
                "wait for extraction and retry verification",
                "check user_id/session_id/scope and adjust verification queries"
            ]),
        );
    }
    Ok(Value::Object(payload))
}

#[allow(clippy::too_many_arguments)]
pub fn save_and_verify(
    client: &EverOSClient,
    content: &str,
    user_id: &str,
    session_id: Option<&str>,
    scope: &str,
    role: Option<&str>,
    tool_call_id: Option<&str>,
    flush: bool,
    flush_timeout: Option<f64>,
    mut verification_queries: Vec<String>,
    memory_types: Option<Vec<String>>,
    top_k: i64,
    timeout: Option<f64>,
) -> Result<Value, EverOSError> {
    let scope = normalize_scope(scope)?;
    let role = role
        .map(str::trim)
        .filter(|role| !role.is_empty())
        .unwrap_or(if scope == "agent" {
            "assistant"
        } else {
            "user"
        });
    let mut message = json!({"role":role,"timestamp":now_ms(),"content":content});
    if let (Some(id), Some(map)) = (
        tool_call_id.filter(|id| !id.trim().is_empty()),
        message.as_object_mut(),
    ) {
        map.insert(
            "tool_call_id".to_string(),
            Value::String(id.trim().to_string()),
        );
    }
    let result = client.add_memories_scoped(user_id, session_id, vec![message], true, &scope)?;
    let flush_payload = if flush {
        match client.flush_memories_scoped(user_id, session_id, &scope, flush_timeout) {
            Ok(response) => Some(flush_result_payload(&response)),
            Err(err @ EverOSError::Timeout { .. }) => Some(timeout_payload("flush", &err)),
            Err(err) => Some(error_payload("flush", &err)),
        }
    } else {
        None
    };
    if verification_queries.is_empty() {
        verification_queries.push(content.chars().take(200).collect());
    }
    let save = save_result_payload(&result, user_id, session_id, &scope, flush, flush_payload);
    let verification = match verify_session_ingest(
        client,
        user_id,
        session_id,
        verification_queries,
        memory_types,
        &scope,
        top_k,
        timeout,
    ) {
        Ok(value) => value,
        Err(err) => verification_error_payload(&err),
    };
    let mut status = if verification_failed(&verification) {
        "verification_error".to_string()
    } else {
        verification["status"]
            .as_str()
            .unwrap_or("queued")
            .to_string()
    };
    let agent_visibility = if scope == "agent" {
        let checks = verification
            .get("agent_visibility")
            .and_then(|value| value.get("agent_visibility_checks"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let visibility = build_agent_visibility_report(
            save.get("message_queued").and_then(Value::as_bool),
            save.get("flush").cloned(),
            checks,
            Some(user_id),
            session_id,
        );
        status = workflow_status_from_agent_visibility(&visibility, &status);
        Some(visibility)
    } else {
        None
    };
    let mut payload = success_envelope("save_and_verify", &status);
    payload.insert("save".to_string(), save);
    payload.insert("verification".to_string(), verification.clone());
    if let Some(visibility) = agent_visibility {
        payload.insert("agent_visibility".to_string(), visibility);
    }
    if let Some(actions) = verification.get("suggested_next_actions") {
        payload.insert("suggested_next_actions".to_string(), actions.clone());
    }
    Ok(Value::Object(payload))
}

#[allow(clippy::too_many_arguments)]
fn submit_batch_with_adaptive_split(
    client: &EverOSClient,
    user_id: &str,
    session_id: Option<&str>,
    scope: &str,
    batch: &[Value],
    batch_index: usize,
    split_from: Option<usize>,
    batch_reports: &mut Vec<Value>,
) -> (usize, usize, usize) {
    match client.add_memories_scoped(user_id, session_id, batch.to_vec(), true, scope) {
        Ok(response) => {
            let queued = batch.len();
            batch_reports.push(json!({
                "batch_index": batch_index,
                "split_from": split_from,
                "ok": true,
                "message_count": queued,
                "payload_bytes": batch_payload_bytes(batch),
                "status": response.pointer("/data/status").and_then(Value::as_str).unwrap_or("queued"),
                "task_id": response.pointer("/data/task_id").and_then(Value::as_str).unwrap_or(""),
                "response": response,
            }));
            (queued, 0, 0)
        }
        Err(err) if is_cloud_403(&err) && batch.len() > 1 => {
            let mid = (batch.len() / 2).max(1);
            batch_reports.push(json!({
                "batch_index": batch_index,
                "split_from": split_from,
                "ok": false,
                "message_count": batch.len(),
                "payload_bytes": batch_payload_bytes(batch),
                "error": sanitized_error_message(&err),
                "retryable": true,
                "split": true,
                "split_reason": "cloud_403",
                "split_into": [mid, batch.len() - mid],
            }));
            let (left_queued, left_failed, left_splits) = submit_batch_with_adaptive_split(
                client,
                user_id,
                session_id,
                scope,
                &batch[..mid],
                batch_index,
                Some(batch_index),
                batch_reports,
            );
            let (right_queued, right_failed, right_splits) = submit_batch_with_adaptive_split(
                client,
                user_id,
                session_id,
                scope,
                &batch[mid..],
                batch_index,
                Some(batch_index),
                batch_reports,
            );
            (
                left_queued + right_queued,
                left_failed + right_failed,
                1 + left_splits + right_splits,
            )
        }
        Err(err) => {
            let failed = batch.len();
            batch_reports.push(json!({
                "batch_index": batch_index,
                "split_from": split_from,
                "ok": false,
                "message_count": failed,
                "payload_bytes": batch_payload_bytes(batch),
                "error": sanitized_error_message(&err),
                "retryable": true,
            }));
            (0, failed, 0)
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn import_and_verify(
    client: &EverOSClient,
    user_id: &str,
    session_id: Option<&str>,
    messages: Vec<Value>,
    file_path: Option<&str>,
    scope: &str,
    dry_run: bool,
    batch_size: usize,
    flush: bool,
    flush_timeout: Option<f64>,
    verification_queries: Vec<String>,
    memory_types: Option<Vec<String>>,
    top_k: i64,
    timeout: Option<f64>,
) -> Result<Value, EverOSError> {
    let scope = normalize_scope(scope)?;
    let default_role = if scope == "agent" {
        "assistant"
    } else {
        "user"
    };
    let (messages, warnings) = normalize_import_messages(messages, file_path, default_role)?;
    let metrics = message_metrics(&messages, batch_size);
    if dry_run {
        let mut payload = success_envelope("import_and_verify", "dry_run");
        payload.insert("input_count".to_string(), json!(messages.len()));
        payload.insert("queued_count".to_string(), json!(0));
        payload.insert("failed_count".to_string(), json!(0));
        payload.insert("warnings".to_string(), json!(warnings));
        payload.insert("metrics".to_string(), metrics);
        payload.insert("batches".to_string(), json!([]));
        payload.insert(
            "verification".to_string(),
            json!({"status":"verification_skipped","verified":Value::Null,"queries":[]}),
        );
        payload.insert(
            "suggested_next_actions".to_string(),
            json!([
                "fix warnings before importing",
                "rerun with dry_run=false to import messages"
            ]),
        );
        return Ok(Value::Object(payload));
    }
    let blocking_warnings: Vec<String> = warnings
        .iter()
        .filter(|warning| {
            warning.contains("tool_call_id")
                || warning.contains("empty")
                || warning.contains("timestamp")
        })
        .cloned()
        .collect();
    if !blocking_warnings.is_empty() {
        let mut payload = error_envelope(
            "import_and_verify",
            "validation_failed",
            "import contains messages that cannot be safely submitted",
        );
        if let Some(map) = payload.as_object_mut() {
            map.insert("input_count".to_string(), json!(messages.len()));
            map.insert("queued_count".to_string(), json!(0));
            map.insert("failed_count".to_string(), json!(messages.len()));
            map.insert("warnings".to_string(), json!(warnings));
            map.insert("metrics".to_string(), metrics);
        }
        return Ok(payload);
    }
    let size = batch_size.clamp(1, 100);
    let mut batch_reports = Vec::new();
    let mut queued_count = 0usize;
    let mut failed_count = 0usize;
    let mut split_count = 0usize;
    for (index, batch) in messages.chunks(size).enumerate() {
        let (queued, failed, splits) = submit_batch_with_adaptive_split(
            client,
            user_id,
            session_id,
            &scope,
            batch,
            index,
            None,
            &mut batch_reports,
        );
        queued_count += queued;
        failed_count += failed;
        split_count += splits;
    }
    let flush_payload = if flush && queued_count > 0 {
        match client.flush_memories_scoped(user_id, session_id, &scope, flush_timeout) {
            Ok(response) => flush_result_payload(&response),
            Err(err @ EverOSError::Timeout { .. }) => timeout_payload("flush", &err),
            Err(err) => error_payload("flush", &err),
        }
    } else {
        json!({"ok":Value::Null,"status":"not_requested"})
    };
    let verification = if verification_queries.is_empty() {
        json!({"status":"verification_skipped","verified":Value::Null,"queries":[]})
    } else {
        match verify_session_ingest(
            client,
            user_id,
            session_id,
            verification_queries,
            memory_types,
            &scope,
            top_k,
            timeout,
        ) {
            Ok(value) => value,
            Err(err) => verification_error_payload(&err),
        }
    };
    let status = if verification_failed(&verification) && queued_count > 0 {
        "verification_error"
    } else if verification.get("verified") == Some(&Value::Bool(true)) {
        "verified"
    } else if verification.get("verified") == Some(&Value::Bool(false)) && queued_count > 0 {
        verification["status"]
            .as_str()
            .unwrap_or("not_yet_searchable")
    } else if failed_count > 0 && queued_count > 0 {
        "partially_queued"
    } else if failed_count > 0 {
        "failed"
    } else {
        "queued"
    };
    let mut actions = Vec::new();
    if split_count > 0 {
        actions.push(Value::String(
            "Cloud 403 triggered adaptive batch splitting; keep batch_size small for long messages"
                .to_string(),
        ));
    }
    if failed_count > 0 {
        actions.push(Value::String(
            "retry only failed batches using the batch report".to_string(),
        ));
    }
    if verification.get("verified") == Some(&Value::Bool(false)) {
        actions.push(Value::String(
            "wait for extraction and rerun verify_session_ingest".to_string(),
        ));
        actions.push(Value::String(
            "adjust verification queries if extraction consolidated memories".to_string(),
        ));
    }
    let mut payload = success_envelope("import_and_verify", status);
    payload.insert("input_count".to_string(), json!(messages.len()));
    payload.insert("queued_count".to_string(), json!(queued_count));
    payload.insert("failed_count".to_string(), json!(failed_count));
    payload.insert("split_count".to_string(), json!(split_count));
    payload.insert("scope".to_string(), Value::String(scope));
    payload.insert("user_id".to_string(), Value::String(user_id.to_string()));
    payload.insert("session_id".to_string(), json!(session_id));
    payload.insert("warnings".to_string(), json!(warnings));
    payload.insert("metrics".to_string(), metrics);
    payload.insert("batches".to_string(), Value::Array(batch_reports));
    payload.insert("flush".to_string(), flush_payload);
    payload.insert("verification".to_string(), verification);
    payload.insert("suggested_next_actions".to_string(), Value::Array(actions));
    Ok(Value::Object(payload))
}
