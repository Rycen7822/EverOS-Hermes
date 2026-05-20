use crate::agent_visibility::build_agent_visibility_report;
use crate::client::EverOSError;
use crate::flush_retry::flush_memories_with_retry;
use crate::formatting::{format_search_context, pretty_json};
use crate::mcp::{default_user_id, make_client};
use crate::redaction::{error_payload, sanitized_error_message};
use crate::workflows;
use serde_json::{Value, json};
use std::time::{SystemTime, UNIX_EPOCH};

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
            let flush_timeout = timeout_arg(&value, "flush_timeout")?;
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
                match flush_memories_with_retry(
                    &client,
                    &uid,
                    session_id.as_deref(),
                    &scope,
                    flush_timeout,
                    2,
                ) {
                    Ok((response, _attempt_count)) => {
                        Some(workflows::tool_flush_result_payload(&response))
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
                &uid,
                session_id.as_deref(),
                &scope,
                flush,
                flush_payload,
            );
            if scope == "agent" {
                workflows::add_agent_visibility(
                    &mut payload,
                    Some(true),
                    Some(&uid),
                    session_id.as_deref(),
                );
            }
            Ok(pretty_json(&payload))
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
            let flush_timeout = timeout_arg(&value, "flush_timeout")?;
            let client = make_client()?;
            let result = client.add_memories_scoped(
                &uid,
                session_id.as_deref(),
                messages,
                async_mode,
                &scope,
            )?;
            if flush {
                match flush_memories_with_retry(
                    &client,
                    &uid,
                    session_id.as_deref(),
                    &scope,
                    flush_timeout,
                    2,
                ) {
                    Ok((response, attempt_count)) => {
                        let mut payload = json!({
                            "ok": true,
                            "add": result,
                            "flush": workflows::tool_flush_result_payload_with_attempt(&response, Some(attempt_count)),
                        });
                        if scope == "agent" {
                            workflows::add_agent_visibility(
                                &mut payload,
                                Some(true),
                                Some(&uid),
                                session_id.as_deref(),
                            );
                        }
                        Ok(pretty_json(&payload))
                    }
                    Err(err @ EverOSError::Timeout { .. }) => {
                        let mut payload = json!({"ok": true, "add": result, "flush": workflows::tool_timeout_payload("flush", &err)});
                        if scope == "agent" {
                            workflows::add_agent_visibility(
                                &mut payload,
                                Some(true),
                                Some(&uid),
                                session_id.as_deref(),
                            );
                        }
                        Ok(pretty_json(&payload))
                    }
                    Err(err) => {
                        let mut payload = json!({"ok": true, "add": result, "flush": error_payload("flush", &err)});
                        if scope == "agent" {
                            workflows::add_agent_visibility(
                                &mut payload,
                                Some(true),
                                Some(&uid),
                                session_id.as_deref(),
                            );
                        }
                        Ok(pretty_json(&payload))
                    }
                }
            } else if scope == "agent" {
                let mut payload = json!({"ok": true, "add": result});
                workflows::add_agent_visibility(
                    &mut payload,
                    Some(true),
                    Some(&uid),
                    session_id.as_deref(),
                );
                Ok(pretty_json(&payload))
            } else {
                Ok(pretty_json(&result))
            }
        }
        "everos_flush_memories" => {
            let uid = optional_string(&value, "user_id").unwrap_or_else(default_user_id);
            let session_id = optional_string(&value, "session_id");
            let scope = scope_from_args(&value)?;
            let timeout = timeout_arg(&value, "timeout")?;
            let client = make_client()?;
            match flush_memories_with_retry(
                &client,
                &uid,
                session_id.as_deref(),
                &scope,
                timeout,
                2,
            ) {
                Ok((response, attempt_count)) => {
                    if scope == "agent" {
                        let flush_payload = workflows::tool_flush_result_payload_with_attempt(
                            &response,
                            Some(attempt_count),
                        );
                        Ok(pretty_json(&json!({
                            "flush": flush_payload.clone(),
                            "agent_visibility": build_agent_visibility_report(
                                None,
                                Some(flush_payload),
                                vec![],
                                Some(&uid),
                                session_id.as_deref(),
                            ),
                        })))
                    } else {
                        Ok(pretty_json(&response))
                    }
                }
                Err(err @ EverOSError::Timeout { .. }) => {
                    Ok(pretty_json(&workflows::tool_timeout_payload("flush", &err)))
                }
                Err(err) => Err(err.into()),
            }
        }
        "everos_search_memories" => {
            let query = required_string(&value, "query")?;
            let uid = optional_string(&value, "user_id").unwrap_or_else(default_user_id);
            let session_id = optional_string(&value, "session_id");
            let method = optional_string(&value, "method")
                .unwrap_or_else(|| "hybrid".to_string())
                .to_ascii_lowercase();
            let top_k = int_arg(&value, "top_k", 5)?;
            let filters = value.get("filters").cloned();
            let radius = float_arg(&value, "radius");
            let timeout = timeout_arg(&value, "timeout")?.or(if method == "agentic" {
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
                        map.insert(
                            "fallback_reason".into(),
                            Value::String(sanitized_error_message(&err)),
                        );
                    }
                    response
                }
                Err(err @ EverOSError::Timeout { .. }) => {
                    return Ok(pretty_json(&workflows::tool_timeout_payload(
                        "search", &err,
                    )));
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
        "everos_verify_session_ingest" => {
            let uid = optional_string(&value, "user_id").unwrap_or_else(default_user_id);
            let session_id = optional_string(&value, "session_id");
            let scope = scope_from_args(&value)?;
            let memory_types = string_array_arg(&value, "memory_types");
            let memory_types = (!memory_types.is_empty()).then_some(memory_types);
            let top_k = int_arg(&value, "top_k", 5)?;
            let timeout = timeout_arg(&value, "timeout")?;
            let queries = string_array_arg(&value, "verification_queries");
            if queries.is_empty() {
                anyhow::bail!("verification_queries is required");
            }
            Ok(pretty_json(&workflows::verify_session_ingest(
                &make_client()?,
                &uid,
                session_id.as_deref(),
                queries,
                memory_types,
                &scope,
                top_k,
                timeout,
            )?))
        }
        "everos_save_and_verify" => {
            let content = required_string(&value, "content")?;
            let uid = optional_string(&value, "user_id").unwrap_or_else(default_user_id);
            let session_id = optional_string(&value, "session_id");
            let scope = scope_from_args(&value)?;
            let role = optional_string(&value, "role");
            let tool_call_id = optional_string(&value, "tool_call_id");
            let flush = bool_arg(&value, "flush", true);
            let flush_timeout = timeout_arg(&value, "flush_timeout")?;
            let mut queries = string_array_arg(&value, "verification_queries");
            if let Some(query) = optional_string(&value, "verification_query") {
                queries.insert(0, query);
            }
            let memory_types = string_array_arg(&value, "memory_types");
            let memory_types = (!memory_types.is_empty()).then_some(memory_types);
            let top_k = int_arg(&value, "top_k", 5)?;
            let timeout = timeout_arg(&value, "timeout")?;
            Ok(pretty_json(&workflows::save_and_verify(
                &make_client()?,
                &content,
                &uid,
                session_id.as_deref(),
                &scope,
                role.as_deref(),
                tool_call_id.as_deref(),
                flush,
                flush_timeout,
                queries,
                memory_types,
                top_k,
                timeout,
            )?))
        }
        "everos_import_and_verify" => {
            let uid = optional_string(&value, "user_id").unwrap_or_else(default_user_id);
            let session_id = optional_string(&value, "session_id");
            let scope = scope_from_args(&value)?;
            let messages = object_array_arg(&value, "messages");
            let file_path = optional_string(&value, "file_path");
            let dry_run = bool_arg(&value, "dry_run", false);
            let batch_size = uint_arg(&value, "batch_size", 50)? as usize;
            let flush = bool_arg(&value, "flush", true);
            let flush_timeout = timeout_arg(&value, "flush_timeout")?;
            let queries = string_array_arg(&value, "verification_queries");
            let memory_types = string_array_arg(&value, "memory_types");
            let memory_types = (!memory_types.is_empty()).then_some(memory_types);
            let top_k = int_arg(&value, "top_k", 5)?;
            let timeout = timeout_arg(&value, "timeout")?;
            Ok(pretty_json(&workflows::import_and_verify(
                &make_client()?,
                &uid,
                session_id.as_deref(),
                messages,
                file_path.as_deref(),
                &scope,
                dry_run,
                batch_size,
                flush,
                flush_timeout,
                queries,
                memory_types,
                top_k,
                timeout,
            )?))
        }
        _ => anyhow::bail!("Unknown EverOS MCP tool: {name}"),
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

fn string_array_arg(value: &Value, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn object_array_arg(value: &Value, key: &str) -> Vec<Value> {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter(|item| item.is_object())
                .cloned()
                .collect()
        })
        .unwrap_or_default()
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

fn timeout_arg(value: &Value, key: &str) -> anyhow::Result<Option<f64>> {
    let Some(raw) = value.get(key) else {
        return Ok(None);
    };
    if raw.is_null() || raw.as_str().is_some_and(|text| text.trim().is_empty()) {
        return Ok(None);
    }
    let parsed = raw
        .as_f64()
        .or_else(|| {
            raw.as_str()
                .and_then(|text| text.trim().parse::<f64>().ok())
        })
        .filter(|value| value.is_finite())
        .ok_or_else(|| anyhow::anyhow!("{key} must be a positive number"))?;
    if parsed <= 0.0 {
        anyhow::bail!("{key} must be a positive number");
    }
    Ok(Some(parsed))
}

fn scope_from_args(value: &Value) -> anyhow::Result<String> {
    let agent = value.get("agent").map(|_| bool_arg(value, "agent", false));
    let scope = optional_string(value, "scope")
        .map(|scope| scope.to_ascii_lowercase())
        .unwrap_or_else(|| {
            if agent.unwrap_or(false) {
                "agent".to_string()
            } else {
                "personal".to_string()
            }
        });
    match scope.as_str() {
        "personal" | "agent" => {
            if matches!(agent, Some(agent) if agent != (scope == "agent")) {
                anyhow::bail!("scope conflicts with backward-compatible agent alias");
            }
            Ok(scope)
        }
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
