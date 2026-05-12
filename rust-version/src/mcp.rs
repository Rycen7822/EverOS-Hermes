use crate::client::{DEFAULT_MEMORY_TYPES, EverOSClient, EverOSError};
use crate::env::get_env;
use crate::formatting::{format_search_context, pretty_json};
use serde_json::{Value, json};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::time::{SystemTime, UNIX_EPOCH};

pub const TOOL_NAMES: [&str; 9] = [
    "everos_save_memory",
    "everos_add_memories",
    "everos_flush_memories",
    "everos_search_memories",
    "everos_get_memories",
    "everos_delete_memories",
    "everos_get_task_status",
    "everos_get_settings",
    "everos_update_settings",
];

pub fn make_client() -> crate::client::Result<EverOSClient> {
    EverOSClient::from_env(None)
}

pub fn default_user_id() -> String {
    let value = get_env("EVEROS_USER_ID", "", None);
    if value.is_empty() {
        "hermes_default".to_string()
    } else {
        value
    }
}

pub fn run_stdio() -> anyhow::Result<()> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut reader = BufReader::new(stdin.lock());
    let mut writer = stdout.lock();
    while let Some(request) = read_frame(&mut reader)? {
        if let Some(response) = handle_jsonrpc_message(&request) {
            write_frame(&mut writer, &response)?;
        }
    }
    Ok(())
}

pub fn tool_definitions() -> Vec<Value> {
    vec![
        json!({"name":"everos_save_memory","title":"Save EverOS Memory","description":"Queue one explicit text memory message for EverOS extraction. saved=true means accepted, not immediately searchable.","inputSchema":{"type":"object","properties":{"content":{"type":"string"},"user_id":{"type":"string"},"session_id":{"type":"string"},"scope":{"type":"string","enum":["personal","agent"],"default":"personal"},"role":{"type":"string","enum":["user","assistant","tool","system"]},"tool_call_id":{"type":"string","description":"Required when role=tool for agent memory."},"flush":{"type":"boolean","default":true},"async_mode":{"type":"boolean","default":true},"flush_timeout":{"type":"number"}},"required":["content"]},"annotations":{"readOnlyHint":false,"destructiveHint":false,"idempotentHint":false,"openWorldHint":true}}),
        json!({"name":"everos_add_memories","title":"Add EverOS Memory Messages","description":"Add one or more personal or agent-trajectory messages to EverOS. Prefer scope over deprecated agent alias.","inputSchema":{"type":"object","properties":{"messages":{"type":"array","items":{"type":"object"}},"user_id":{"type":"string"},"session_id":{"type":"string"},"scope":{"type":"string","enum":["personal","agent"],"default":"personal"},"async_mode":{"type":"boolean","default":true},"agent":{"type":"boolean"},"flush":{"type":"boolean","default":false},"flush_timeout":{"type":"number"}},"required":["messages"]},"annotations":{"readOnlyHint":false,"destructiveHint":false,"idempotentHint":false,"openWorldHint":true}}),
        json!({"name":"everos_flush_memories","title":"Flush EverOS Memories","description":"Trigger EverOS boundary detection and memory extraction immediately. Timeout errors are retryable; search/status checks should happen before retrying.","inputSchema":{"type":"object","properties":{"user_id":{"type":"string"},"session_id":{"type":"string"},"scope":{"type":"string","enum":["personal","agent"],"default":"personal"},"agent":{"type":"boolean"},"timeout":{"type":"number"}}},"annotations":{"readOnlyHint":false,"destructiveHint":false,"idempotentHint":true,"openWorldHint":true}}),
        json!({"name":"everos_search_memories","title":"Search EverOS Memories","description":"Search EverOS memory using keyword, vector, hybrid, or agentic retrieval. Vector fields are stripped by default even when include_original_data=true; set include_vectors=true only for debugging.","inputSchema":{"type":"object","properties":{"query":{"type":"string"},"user_id":{"type":"string"},"session_id":{"type":"string"},"filters":{"type":"object"},"method":{"type":"string","enum":["keyword","vector","hybrid","agentic"],"default":"hybrid"},"top_k":{"type":"integer","default":5,"minimum":-1,"maximum":100},"memory_types":{"type":"array","items":{"type":"string","enum":["episodic_memory","profile","raw_message","agent_memory"]}},"radius":{"type":"number","minimum":0,"maximum":1},"include_original_data":{"type":"boolean","default":false},"include_vectors":{"type":"boolean","default":false},"response_format":{"type":"string","enum":["json","markdown"],"default":"json"},"timeout":{"type":"number"},"fallback_to_hybrid":{"type":"boolean","default":true}},"required":["query"]},"annotations":{"readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":true}}),
        json!({"name":"everos_get_memories","title":"Get EverOS Memories","description":"Retrieve structured EverOS memories by memory_type with pagination. get supports agent_case/agent_skill; search uses agent_memory.","inputSchema":{"type":"object","properties":{"user_id":{"type":"string"},"session_id":{"type":"string"},"filters":{"type":"object"},"memory_type":{"type":"string","enum":["episodic_memory","profile","agent_case","agent_skill"],"default":"episodic_memory"},"page":{"type":"integer","default":1},"page_size":{"type":"integer","default":20},"rank_by":{"type":"string","default":"timestamp"},"rank_order":{"type":"string","enum":["asc","desc"],"default":"desc"},"response_format":{"type":"string","enum":["json","markdown"],"default":"json"}}},"annotations":{"readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":true}}),
        json!({"name":"everos_delete_memories","title":"Delete EverOS Memories","description":"Delete EverOS memory by exact memory_id, or batch-delete by explicit user/session when confirmed.","inputSchema":{"type":"object","properties":{"memory_id":{"type":"string"},"user_id":{"type":"string"},"session_id":{"type":"string"},"confirm":{"type":"boolean","default":false},"confirm_scope_text":{"type":"string"}}},"annotations":{"readOnlyHint":false,"destructiveHint":true,"idempotentHint":true,"openWorldHint":true}}),
        json!({"name":"everos_get_task_status","title":"Get EverOS Task Status","description":"Check an asynchronous EverOS extraction task status.","inputSchema":{"type":"object","properties":{"task_id":{"type":"string"}},"required":["task_id"]},"annotations":{"readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":true}}),
        json!({"name":"everos_get_settings","title":"Get EverOS Settings","description":"Get current EverOS memory-space settings.","inputSchema":{"type":"object","properties":{}},"annotations":{"readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":true}}),
        json!({"name":"everos_update_settings","title":"Update EverOS Settings","description":"Update EverOS memory-space settings with strict schema validation by default.","inputSchema":{"type":"object","properties":{"settings":{"type":"object"},"strict":{"type":"boolean","default":true},"return_diff":{"type":"boolean","default":true}},"required":["settings"]},"annotations":{"readOnlyHint":false,"destructiveHint":false,"idempotentHint":true,"openWorldHint":true}}),
    ]
}

pub fn handle_jsonrpc_message(request: &Value) -> Option<Value> {
    let id = request.get("id").cloned()?;
    let method = request
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let result = match method {
        "initialize" => {
            let protocol = request
                .pointer("/params/protocolVersion")
                .and_then(Value::as_str)
                .unwrap_or("2024-11-05");
            json!({"protocolVersion":protocol,"capabilities":{"tools":{"listChanged":false}},"serverInfo":{"name":"everos_mcp","version":env!("CARGO_PKG_VERSION")}})
        }
        "ping" => json!({}),
        "tools/list" => json!({"tools": tool_definitions()}),
        "tools/call" => {
            let name = request
                .pointer("/params/name")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let args = request
                .pointer("/params/arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            match call_tool(name, args) {
                Ok(text) => json!({"content":[{"type":"text","text":text}],"isError":false}),
                Err(err) => {
                    json!({"content":[{"type":"text","text":format!("Error: {err}")}],"isError":true})
                }
            }
        }
        _ => {
            return Some(
                json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":format!("Method not found: {method}")}}),
            );
        }
    };
    Some(json!({"jsonrpc":"2.0","id":id,"result":result}))
}

pub fn call_tool(name: &str, args: Value) -> anyhow::Result<String> {
    let args = args.as_object().cloned().unwrap_or_default();
    let value = Value::Object(args.clone());
    match name {
        "everos_save_memory" => {
            let content = required_string(&value, "content")?;
            let uid = optional_string(&value, "user_id").unwrap_or_else(default_user_id);
            let session_id = optional_string(&value, "session_id");
            let flush = bool_arg(&value, "flush", true);
            let async_mode = bool_arg(&value, "async_mode", true);
            let flush_timeout = float_arg(&value, "flush_timeout");
            let scope = scope_from_args(&value)?;
            let role = optional_string(&value, "role").unwrap_or_else(|| {
                if scope == "agent" {
                    "assistant".to_string()
                } else {
                    "user".to_string()
                }
            });
            let mut message = json!({"role":role,"timestamp":now_ms(),"content":content});
            if let (Some(tool_call_id), Some(map)) = (
                optional_string(&value, "tool_call_id"),
                message.as_object_mut(),
            ) {
                map.insert("tool_call_id".to_string(), Value::String(tool_call_id));
            }
            let client = make_client()?;
            let result = client.add_memories_scoped(
                &uid,
                session_id.as_deref(),
                vec![message],
                async_mode,
                &scope,
            )?;
            let flush_payload = if flush {
                match client.flush_memories_scoped(
                    &uid,
                    session_id.as_deref(),
                    &scope,
                    flush_timeout,
                ) {
                    Ok(response) => Some(flush_result_payload(&response)),
                    Err(err @ EverOSError::Timeout { .. }) => Some(timeout_payload("flush", &err)),
                    Err(err) => return Err(err.into()),
                }
            } else {
                None
            };
            Ok(pretty_json(&save_result_payload(
                &result,
                &uid,
                session_id.as_deref(),
                &scope,
                flush,
                flush_payload,
            )))
        }
        "everos_add_memories" => {
            let messages = value
                .get("messages")
                .and_then(Value::as_array)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("messages is required"))?;
            let uid = optional_string(&value, "user_id").unwrap_or_else(default_user_id);
            let session_id = optional_string(&value, "session_id");
            let async_mode = bool_arg(&value, "async_mode", true);
            let scope = scope_from_args(&value)?;
            let flush = bool_arg(&value, "flush", false);
            let flush_timeout = float_arg(&value, "flush_timeout");
            let client = make_client()?;
            let result = client.add_memories_scoped(
                &uid,
                session_id.as_deref(),
                messages,
                async_mode,
                &scope,
            )?;
            if flush {
                match client.flush_memories_scoped(
                    &uid,
                    session_id.as_deref(),
                    &scope,
                    flush_timeout,
                ) {
                    Ok(response) => Ok(pretty_json(
                        &json!({"ok": true, "add": result, "flush": flush_result_payload(&response)}),
                    )),
                    Err(err @ EverOSError::Timeout { .. }) => Ok(pretty_json(
                        &json!({"ok": true, "add": result, "flush": timeout_payload("flush", &err)}),
                    )),
                    Err(err) => Err(err.into()),
                }
            } else {
                Ok(pretty_json(&result))
            }
        }
        "everos_flush_memories" => {
            let uid = optional_string(&value, "user_id").unwrap_or_else(default_user_id);
            let session_id = optional_string(&value, "session_id");
            let scope = scope_from_args(&value)?;
            let timeout = float_arg(&value, "timeout");
            match make_client()?.flush_memories_scoped(&uid, session_id.as_deref(), &scope, timeout)
            {
                Ok(response) => Ok(pretty_json(&response)),
                Err(err @ EverOSError::Timeout { .. }) => {
                    Ok(pretty_json(&timeout_payload("flush", &err)))
                }
                Err(err) => Err(err.into()),
            }
        }
        "everos_search_memories" => {
            let query = required_string(&value, "query")?;
            let uid = optional_string(&value, "user_id").unwrap_or_else(default_user_id);
            let session_id = optional_string(&value, "session_id");
            let method = optional_string(&value, "method").unwrap_or_else(|| "hybrid".to_string());
            let top_k = int_arg(&value, "top_k", 5)?;
            let filters = value.get("filters").cloned();
            let radius = float_arg(&value, "radius");
            let timeout = float_arg(&value, "timeout").or(if method == "agentic" {
                Some(60.0)
            } else {
                None
            });
            let fallback_to_hybrid = bool_arg(&value, "fallback_to_hybrid", true);
            let memory_types = value
                .get("memory_types")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(Value::as_str)
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                })
                .filter(|items| !items.is_empty());
            let include_original_data = bool_arg(&value, "include_original_data", false);
            let include_vectors = bool_arg(&value, "include_vectors", false);
            let client = make_client()?;
            let response = match client.search_memories(
                &query,
                Some(&uid),
                None,
                session_id.as_deref(),
                filters.clone(),
                &method,
                memory_types.clone(),
                top_k,
                radius,
                include_original_data,
                include_vectors,
                timeout,
            ) {
                Ok(response) => response,
                Err(err @ EverOSError::Timeout { .. })
                    if method == "agentic" && fallback_to_hybrid =>
                {
                    let mut response = client.search_memories(
                        &query,
                        Some(&uid),
                        None,
                        session_id.as_deref(),
                        filters,
                        "hybrid",
                        memory_types,
                        top_k,
                        radius,
                        include_original_data,
                        include_vectors,
                        timeout,
                    )?;
                    if let Some(map) = response.as_object_mut() {
                        map.insert("fallback_used".into(), Value::Bool(true));
                        map.insert("fallback_reason".into(), Value::String(err.to_string()));
                    }
                    response
                }
                Err(err @ EverOSError::Timeout { .. }) => {
                    return Ok(pretty_json(&timeout_payload("search", &err)));
                }
                Err(err) => return Err(err.into()),
            };
            Ok(render(
                &response,
                optional_string(&value, "response_format")
                    .as_deref()
                    .unwrap_or("json"),
            ))
        }
        "everos_get_memories" => {
            let uid = optional_string(&value, "user_id").unwrap_or_else(default_user_id);
            let session_id = optional_string(&value, "session_id");
            let memory_type = optional_string(&value, "memory_type")
                .unwrap_or_else(|| "episodic_memory".to_string());
            let page = uint_arg(&value, "page", 1)?;
            let page_size = uint_arg(&value, "page_size", 20)?;
            let filters = value.get("filters").cloned();
            let rank_by =
                optional_string(&value, "rank_by").unwrap_or_else(|| "timestamp".to_string());
            let rank_order = optional_string(&value, "rank_order")
                .unwrap_or_else(|| "desc".to_string())
                .to_ascii_lowercase();
            let response = make_client()?.get_memories(
                Some(&uid),
                None,
                session_id.as_deref(),
                filters,
                &memory_type,
                page,
                page_size,
                &rank_by,
                &rank_order,
            )?;
            Ok(render(
                &response,
                optional_string(&value, "response_format")
                    .as_deref()
                    .unwrap_or("json"),
            ))
        }
        "everos_delete_memories" => {
            if !bool_arg(&value, "confirm", false) {
                return Ok(pretty_json(
                    &json!({"error":"confirm=true is required before deleting EverOS memories"}),
                ));
            }
            let memory_id = optional_string(&value, "memory_id");
            let uid = optional_string(&value, "user_id");
            let session_id = optional_string(&value, "session_id");
            if memory_id.is_some() && (uid.is_some() || session_id.is_some()) {
                return Ok(pretty_json(
                    &json!({"error":"single delete by memory_id cannot include user_id or session_id"}),
                ));
            }
            if memory_id.is_none() {
                let Some(uid_text) = uid.as_deref() else {
                    return Ok(pretty_json(
                        &json!({"error":"batch delete requires explicit user_id"}),
                    ));
                };
                let expected = delete_confirm_text(uid_text, session_id.as_deref());
                if optional_string(&value, "confirm_scope_text").as_deref()
                    != Some(expected.as_str())
                {
                    return Ok(pretty_json(
                        &json!({"error":format!("confirm_scope_text must exactly match {expected:?}")}),
                    ));
                }
            }
            Ok(pretty_json(&make_client()?.delete_memories(
                memory_id.as_deref(),
                uid.as_deref(),
                None,
                session_id.as_deref(),
            )?))
        }
        "everos_get_task_status" => Ok(pretty_json(
            &make_client()?.get_task_status(&required_string(&value, "task_id")?)?,
        )),
        "everos_get_settings" => Ok(pretty_json(&make_client()?.get_settings()?)),
        "everos_update_settings" => {
            let strict = bool_arg(&value, "strict", true);
            let return_diff = bool_arg(&value, "return_diff", true);
            Ok(pretty_json(&make_client()?.update_settings(
                value.get("settings").cloned().unwrap_or_else(|| json!({})),
                strict,
                return_diff,
            )?))
        }
        _ => anyhow::bail!("Unknown EverOS MCP tool: {name}"),
    }
}

pub fn read_frame<R: BufRead + Read>(reader: &mut R) -> io::Result<Option<Value>> {
    let mut first = String::new();
    loop {
        first.clear();
        let n = reader.read_line(&mut first)?;
        if n == 0 {
            return Ok(None);
        }
        if !first.trim().is_empty() {
            break;
        }
    }
    if first.trim_start().starts_with('{') {
        let value = serde_json::from_str(first.trim())
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
        return Ok(Some(value));
    }
    let mut content_length = parse_content_length(&first);
    let mut line = String::new();
    loop {
        line.clear();
        let n = reader.read_line(&mut line)?;
        if n == 0 {
            return Ok(None);
        }
        if line == "\r\n" || line == "\n" || line.trim().is_empty() {
            break;
        }
        if content_length.is_none() {
            content_length = parse_content_length(&line);
        }
    }
    let Some(length) = content_length else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "missing Content-Length",
        ));
    };
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    let value = serde_json::from_slice(&body)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    Ok(Some(value))
}

pub fn write_frame<W: Write>(writer: &mut W, value: &Value) -> io::Result<()> {
    let body = serde_json::to_vec(value).map_err(io::Error::other)?;
    writer.write_all(&body)?;
    writer.write_all(b"\n")?;
    writer.flush()
}

fn parse_content_length(line: &str) -> Option<usize> {
    let (key, value) = line.split_once(':')?;
    if key.trim().eq_ignore_ascii_case("Content-Length") {
        value.trim().parse().ok()
    } else {
        None
    }
}

fn render(response: &Value, response_format: &str) -> String {
    if response_format == "markdown" {
        let formatted = format_search_context(response, 20);
        if !formatted.is_empty() {
            return formatted;
        }
    }
    pretty_json(response)
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

fn required_string(value: &Value, key: &str) -> anyhow::Result<String> {
    let text = optional_string(value, key).unwrap_or_default();
    if text.trim().is_empty() {
        anyhow::bail!("{key} is required")
    } else {
        Ok(text)
    }
}

fn optional_string(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(ToString::to_string)
}

fn bool_arg(value: &Value, key: &str, default: bool) -> bool {
    match value.get(key) {
        Some(Value::Bool(flag)) => *flag,
        Some(Value::String(text)) => match text.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "y" | "on" => true,
            "0" | "false" | "no" | "n" | "off" => false,
            _ => default,
        },
        _ => default,
    }
}

fn int_arg(value: &Value, key: &str, default: i64) -> anyhow::Result<i64> {
    match value.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(Value::Number(number)) => number
            .as_i64()
            .ok_or_else(|| anyhow::anyhow!("{key} must be an integer")),
        Some(Value::String(text)) if text.trim().is_empty() => Ok(default),
        Some(Value::String(text)) => text
            .trim()
            .parse::<i64>()
            .map_err(|_| anyhow::anyhow!("{key} must be an integer")),
        Some(_) => anyhow::bail!("{key} must be an integer"),
    }
}

fn uint_arg(value: &Value, key: &str, default: u64) -> anyhow::Result<u64> {
    let parsed = int_arg(value, key, default as i64)?;
    if parsed < 0 {
        anyhow::bail!("{key} must be a non-negative integer");
    }
    Ok(parsed as u64)
}

fn float_arg(value: &Value, key: &str) -> Option<f64> {
    value
        .get(key)
        .and_then(|value| {
            value
                .as_f64()
                .or_else(|| value.as_str().and_then(|text| text.parse::<f64>().ok()))
        })
        .filter(|value| value.is_finite())
}

fn scope_from_args(value: &Value) -> anyhow::Result<String> {
    let scope = optional_string(value, "scope").unwrap_or_else(|| {
        if bool_arg(value, "agent", false) {
            "agent".to_string()
        } else {
            "personal".to_string()
        }
    });
    match scope.as_str() {
        "personal" | "agent" => Ok(scope),
        other => anyhow::bail!("scope must be personal or agent, got {other}"),
    }
}

fn delete_confirm_text(user_id: &str, session_id: Option<&str>) -> String {
    match session_id.filter(|value| !value.trim().is_empty()) {
        Some(session_id) => format!("delete user_id={user_id} session_id={session_id}"),
        None => format!("delete user_id={user_id}"),
    }
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

#[allow(dead_code)]
fn _default_memory_types() -> Vec<String> {
    DEFAULT_MEMORY_TYPES
        .iter()
        .map(|item| item.to_string())
        .collect()
}
