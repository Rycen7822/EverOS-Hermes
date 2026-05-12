use crate::client::{DEFAULT_MEMORY_TYPES, EverOSClient, EverOSError};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

const SEARCH_KEYS: [&str; 10] = [
    "episodes",
    "profiles",
    "raw_messages",
    "agent_memory",
    "agent_cases",
    "agent_skills",
    "cases",
    "skills",
    "items",
    "results",
];

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
        "message": message,
        "retryable": false,
        "suggested_next_actions": ["inspect the validation error and retry with corrected arguments"]
    })
}

pub fn flush_result_payload(response: &Value) -> Value {
    let data = response.get("data").unwrap_or(response);
    let mut payload = serde_json::Map::new();
    payload.insert("ok".to_string(), Value::Bool(true));
    payload.insert("status".to_string(), Value::String("success".to_string()));
    for key in ["status", "request_id", "task_id", "message"] {
        if let Some(value) = data.get(key).filter(|value| !value.is_null()) {
            payload.insert(key.to_string(), value.clone());
        }
    }
    if let Some(status_code) = response.get("status_code") {
        payload.insert("status_code".to_string(), status_code.clone());
    }
    Value::Object(payload)
}

pub fn timeout_payload(operation: &str, err: &EverOSError) -> Value {
    json!({
        "ok": false,
        "operation": operation,
        "status": "timeout",
        "error_code": "timeout",
        "message": err.to_string(),
        "retryable": true,
        "suggested_next_actions": [
            "search existing memories before retrying, because the server may have completed the request after the client timed out",
            "if the operation returned a task_id or request_id earlier, check that status before issuing another write/flush",
            "retry with a longer timeout only if search/status checks do not show the expected result"
        ]
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
    let status = result
        .pointer("/data/status")
        .and_then(Value::as_str)
        .unwrap_or("queued");
    let task_id = result
        .pointer("/data/task_id")
        .and_then(Value::as_str)
        .unwrap_or("");
    let extraction_requested = flush_requested
        || !task_id.is_empty()
        || matches!(status, "queued" | "processing" | "success");
    json!({
        "ok": true,
        "status": status,
        "saved": true,
        "message_queued": true,
        "extraction_requested": extraction_requested,
        "searchable": Value::Null,
        "user_id": user_id,
        "session_id": session_id,
        "scope": scope,
        "task_id": task_id,
        "flush": flush.unwrap_or_else(|| {
            if flush_requested {
                json!({"ok": false, "status":"missing", "error": "flush requested but no flush result was recorded"})
            } else {
                json!({"ok": Value::Null, "status": "not_requested"})
            }
        }),
    })
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
        map.insert("role".to_string(), Value::String(role));
        map.insert("content".to_string(), Value::String(content));
        map.entry("timestamp".to_string())
            .or_insert_with(|| json!(now_ms()));
        normalized.push(Value::Object(map));
    }
    Ok((normalized, warnings))
}

fn batched(items: &[Value], batch_size: usize) -> Vec<Vec<Value>> {
    let size = batch_size.clamp(1, 100);
    items.chunks(size).map(|chunk| chunk.to_vec()).collect()
}

pub fn count_hits(response: &Value) -> usize {
    let value = response.get("data").unwrap_or(response);
    count_hits_value(value)
}

fn count_hits_value(value: &Value) -> usize {
    if let Some(items) = value.as_array() {
        return items.len();
    }
    let Some(map) = value.as_object() else {
        return 0;
    };
    map.iter()
        .filter(|(key, _child)| SEARCH_KEYS.contains(&key.as_str()))
        .map(|(_key, child)| count_hits_value(child))
        .sum()
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
    for query in verification_queries
        .into_iter()
        .map(|query| query.trim().to_string())
        .filter(|query| !query.is_empty())
    {
        let response = client.search_memories(
            &query,
            Some(user_id),
            None,
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
        queries.push(json!({
            "query": query,
            "status": if hit_count > 0 { "hit" } else { "miss" },
            "hit_count": hit_count,
            "response": response,
        }));
    }
    let (status, verified) = if queries.is_empty() {
        ("verification_skipped", Value::Null)
    } else if queries
        .iter()
        .all(|query| query["hit_count"].as_u64().unwrap_or(0) > 0)
    {
        ("verified", Value::Bool(true))
    } else if queries
        .iter()
        .any(|query| query["hit_count"].as_u64().unwrap_or(0) > 0)
    {
        ("partially_verified", Value::Bool(false))
    } else {
        ("not_yet_searchable", Value::Bool(false))
    };
    let mut payload = success_envelope("verify_session_ingest", status);
    payload.insert("verified".to_string(), verified.clone());
    payload.insert("scope".to_string(), Value::String(scope));
    payload.insert("user_id".to_string(), Value::String(user_id.to_string()));
    payload.insert("session_id".to_string(), json!(session_id));
    payload.insert("memory_types".to_string(), json!(memory_types));
    payload.insert("queries".to_string(), Value::Array(queries));
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
            Err(err) => return Err(err),
        }
    } else {
        None
    };
    if verification_queries.is_empty() {
        verification_queries.push(content.chars().take(200).collect());
    }
    let save = save_result_payload(&result, user_id, session_id, &scope, flush, flush_payload);
    let verification = verify_session_ingest(
        client,
        user_id,
        session_id,
        verification_queries,
        memory_types,
        &scope,
        top_k,
        timeout,
    )?;
    let status = verification["status"]
        .as_str()
        .unwrap_or("queued")
        .to_string();
    let mut payload = success_envelope("save_and_verify", &status);
    payload.insert("save".to_string(), save);
    payload.insert("verification".to_string(), verification.clone());
    if let Some(actions) = verification.get("suggested_next_actions") {
        payload.insert("suggested_next_actions".to_string(), actions.clone());
    }
    Ok(Value::Object(payload))
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
    workflow: &str,
) -> Result<Value, EverOSError> {
    let scope = normalize_scope(scope)?;
    let default_role = if scope == "agent" {
        "assistant"
    } else {
        "user"
    };
    let (messages, warnings) = normalize_import_messages(messages, file_path, default_role)?;
    if dry_run {
        let mut payload = success_envelope(workflow, "dry_run");
        payload.insert("input_count".to_string(), json!(messages.len()));
        payload.insert("queued_count".to_string(), json!(0));
        payload.insert("failed_count".to_string(), json!(0));
        payload.insert("warnings".to_string(), json!(warnings));
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
        .filter(|warning| warning.contains("tool_call_id") || warning.contains("empty"))
        .cloned()
        .collect();
    if !blocking_warnings.is_empty() {
        let mut payload = error_envelope(
            workflow,
            "validation_failed",
            "import contains messages that cannot be safely submitted",
        );
        if let Some(map) = payload.as_object_mut() {
            map.insert("input_count".to_string(), json!(messages.len()));
            map.insert("queued_count".to_string(), json!(0));
            map.insert("failed_count".to_string(), json!(messages.len()));
            map.insert("warnings".to_string(), json!(warnings));
        }
        return Ok(payload);
    }
    let batches = batched(&messages, batch_size);
    let mut batch_reports = Vec::new();
    let mut queued_count = 0usize;
    let mut failed_count = 0usize;
    for (index, batch) in batches.into_iter().enumerate() {
        match client.add_memories_scoped(user_id, session_id, batch.clone(), true, &scope) {
            Ok(response) => {
                queued_count += batch.len();
                batch_reports.push(json!({
                    "batch_index": index,
                    "ok": true,
                    "message_count": batch.len(),
                    "status": response.pointer("/data/status").and_then(Value::as_str).unwrap_or("queued"),
                    "task_id": response.pointer("/data/task_id").and_then(Value::as_str).unwrap_or(""),
                    "response": response,
                }));
            }
            Err(err) => {
                failed_count += batch.len();
                batch_reports.push(json!({
                    "batch_index": index,
                    "ok": false,
                    "message_count": batch.len(),
                    "error": err.to_string(),
                    "retryable": true,
                }));
            }
        }
    }
    let flush_payload = if flush && queued_count > 0 {
        match client.flush_memories_scoped(user_id, session_id, &scope, flush_timeout) {
            Ok(response) => flush_result_payload(&response),
            Err(err @ EverOSError::Timeout { .. }) => timeout_payload("flush", &err),
            Err(err) => return Err(err),
        }
    } else {
        json!({"ok":Value::Null,"status":"not_requested"})
    };
    let verification = if verification_queries.is_empty() {
        json!({"status":"verification_skipped","verified":Value::Null,"queries":[]})
    } else {
        verify_session_ingest(
            client,
            user_id,
            session_id,
            verification_queries,
            memory_types,
            &scope,
            top_k,
            timeout,
        )?
    };
    let status = if verification.get("verified") == Some(&Value::Bool(true)) {
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
    let mut payload = success_envelope(workflow, status);
    payload.insert("input_count".to_string(), json!(messages.len()));
    payload.insert("queued_count".to_string(), json!(queued_count));
    payload.insert("failed_count".to_string(), json!(failed_count));
    payload.insert("scope".to_string(), Value::String(scope));
    payload.insert("user_id".to_string(), Value::String(user_id.to_string()));
    payload.insert("session_id".to_string(), json!(session_id));
    payload.insert("warnings".to_string(), json!(warnings));
    payload.insert("batches".to_string(), Value::Array(batch_reports));
    payload.insert("flush".to_string(), flush_payload);
    payload.insert("verification".to_string(), verification);
    payload.insert("suggested_next_actions".to_string(), Value::Array(actions));
    Ok(Value::Object(payload))
}
