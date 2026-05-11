use crate::client::{DEFAULT_MEMORY_TYPES, EverOSClient};
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
        json!({"name":"everos_save_memory","title":"Save EverOS Memory","description":"Save one explicit text memory to EverOS.","inputSchema":{"type":"object","properties":{"content":{"type":"string"},"user_id":{"type":"string"},"session_id":{"type":"string"},"flush":{"type":"boolean","default":true},"async_mode":{"type":"boolean","default":true}},"required":["content"]},"annotations":{"readOnlyHint":false,"destructiveHint":false,"idempotentHint":false,"openWorldHint":true}}),
        json!({"name":"everos_add_memories","title":"Add EverOS Memory Messages","description":"Add one or more personal or agent-trajectory messages to EverOS.","inputSchema":{"type":"object","properties":{"messages":{"type":"array","items":{"type":"object"}},"user_id":{"type":"string"},"session_id":{"type":"string"},"async_mode":{"type":"boolean","default":true},"agent":{"type":"boolean","default":false},"flush":{"type":"boolean","default":false}},"required":["messages"]},"annotations":{"readOnlyHint":false,"destructiveHint":false,"idempotentHint":false,"openWorldHint":true}}),
        json!({"name":"everos_flush_memories","title":"Flush EverOS Memories","description":"Trigger EverOS boundary detection and memory extraction immediately.","inputSchema":{"type":"object","properties":{"user_id":{"type":"string"},"session_id":{"type":"string"},"agent":{"type":"boolean","default":false}}},"annotations":{"readOnlyHint":false,"destructiveHint":false,"idempotentHint":true,"openWorldHint":true}}),
        json!({"name":"everos_search_memories","title":"Search EverOS Memories","description":"Search EverOS memory using keyword, vector, hybrid, or agentic retrieval.","inputSchema":{"type":"object","properties":{"query":{"type":"string"},"user_id":{"type":"string"},"session_id":{"type":"string"},"method":{"type":"string","enum":["keyword","vector","hybrid","agentic"],"default":"hybrid"},"top_k":{"type":"integer","default":5},"memory_types":{"type":"array","items":{"type":"string"}},"include_original_data":{"type":"boolean","default":false},"response_format":{"type":"string","enum":["json","markdown"],"default":"json"}},"required":["query"]},"annotations":{"readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":true}}),
        json!({"name":"everos_get_memories","title":"Get EverOS Memories","description":"Retrieve structured EverOS memories by memory_type with pagination.","inputSchema":{"type":"object","properties":{"user_id":{"type":"string"},"session_id":{"type":"string"},"memory_type":{"type":"string","enum":["episodic_memory","profile","agent_case","agent_skill"],"default":"episodic_memory"},"page":{"type":"integer","default":1},"page_size":{"type":"integer","default":20},"response_format":{"type":"string","enum":["json","markdown"],"default":"json"}}},"annotations":{"readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":true}}),
        json!({"name":"everos_delete_memories","title":"Delete EverOS Memories","description":"Delete EverOS memory by exact memory_id, or batch-delete by user/session when explicitly confirmed.","inputSchema":{"type":"object","properties":{"memory_id":{"type":"string"},"user_id":{"type":"string"},"session_id":{"type":"string"},"confirm":{"type":"boolean","default":false}}},"annotations":{"readOnlyHint":false,"destructiveHint":true,"idempotentHint":true,"openWorldHint":true}}),
        json!({"name":"everos_get_task_status","title":"Get EverOS Task Status","description":"Check an asynchronous EverOS extraction task status.","inputSchema":{"type":"object","properties":{"task_id":{"type":"string"}},"required":["task_id"]},"annotations":{"readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":true}}),
        json!({"name":"everos_get_settings","title":"Get EverOS Settings","description":"Get current EverOS memory-space settings.","inputSchema":{"type":"object","properties":{}},"annotations":{"readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":true}}),
        json!({"name":"everos_update_settings","title":"Update EverOS Settings","description":"Update EverOS memory-space settings. Only supplied fields are changed.","inputSchema":{"type":"object","properties":{"settings":{"type":"object"}},"required":["settings"]},"annotations":{"readOnlyHint":false,"destructiveHint":false,"idempotentHint":true,"openWorldHint":true}}),
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
            let client = make_client()?;
            let result = client.add_memories(
                &uid,
                session_id.as_deref(),
                vec![json!({"role":"user","timestamp":now_ms(),"content":content})],
                async_mode,
                false,
            )?;
            if flush {
                client.flush_memories(&uid, session_id.as_deref(), false)?;
            }
            Ok(pretty_json(
                &json!({"saved":true,"user_id":uid,"session_id":session_id,"status":result.pointer("/data/status").and_then(Value::as_str).unwrap_or(""),"task_id":result.pointer("/data/task_id").and_then(Value::as_str).unwrap_or("")}),
            ))
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
            let agent = bool_arg(&value, "agent", false);
            let flush = bool_arg(&value, "flush", false);
            let client = make_client()?;
            let result =
                client.add_memories(&uid, session_id.as_deref(), messages, async_mode, agent)?;
            if flush {
                client.flush_memories(&uid, session_id.as_deref(), agent)?;
            }
            Ok(pretty_json(&result))
        }
        "everos_flush_memories" => {
            let uid = optional_string(&value, "user_id").unwrap_or_else(default_user_id);
            let session_id = optional_string(&value, "session_id");
            let agent = bool_arg(&value, "agent", false);
            Ok(pretty_json(&make_client()?.flush_memories(
                &uid,
                session_id.as_deref(),
                agent,
            )?))
        }
        "everos_search_memories" => {
            let query = required_string(&value, "query")?;
            let uid = optional_string(&value, "user_id").unwrap_or_else(default_user_id);
            let session_id = optional_string(&value, "session_id");
            let method = optional_string(&value, "method").unwrap_or_else(|| "hybrid".to_string());
            let top_k = int_arg(&value, "top_k", 5, 1, 20) as u64;
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
            let response = make_client()?.search_memories(
                &query,
                Some(&uid),
                None,
                session_id.as_deref(),
                None,
                &method,
                memory_types,
                top_k,
                None,
                include_original_data,
                None,
            )?;
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
            let page = int_arg(&value, "page", 1, 1, 10000) as u64;
            let page_size = int_arg(&value, "page_size", 20, 1, 100) as u64;
            let response = make_client()?.get_memories(
                Some(&uid),
                None,
                session_id.as_deref(),
                None,
                &memory_type,
                page,
                page_size,
                "timestamp",
                "desc",
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
            let uid = optional_string(&value, "user_id").or_else(|| {
                if memory_id.is_none() {
                    Some(default_user_id())
                } else {
                    None
                }
            });
            let session_id = optional_string(&value, "session_id");
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
        "everos_update_settings" => Ok(pretty_json(
            &make_client()?
                .update_settings(value.get("settings").cloned().unwrap_or_else(|| json!({})))?,
        )),
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

fn int_arg(value: &Value, key: &str, default: i64, low: i64, high: i64) -> i64 {
    value
        .get(key)
        .and_then(|value| {
            value
                .as_i64()
                .or_else(|| value.as_str().and_then(|text| text.parse::<i64>().ok()))
        })
        .unwrap_or(default)
        .clamp(low, high)
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
