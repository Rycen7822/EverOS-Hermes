use crate::client::{EverOSClient, EverOSError};
use crate::response_normalization::{count_hits, response_summary};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::time::Instant;

pub fn build_agent_visibility_report(
    agent_raw_queued: Option<bool>,
    agent_flush: Option<Value>,
    checks: Vec<Value>,
    user_id: Option<&str>,
    session_id: Option<&str>,
) -> Value {
    let hit_count = checks
        .iter()
        .filter(|check| check_hit_count(check) > 0)
        .count();
    let error_count = checks
        .iter()
        .filter(|check| check.get("status").and_then(Value::as_str) == Some("error"))
        .count();
    let (structured_visible, status) = if checks.is_empty() {
        (Value::Null, "unchecked")
    } else if hit_count == checks.len() {
        (Value::Bool(true), "visible")
    } else if hit_count > 0 {
        (Value::Bool(true), "partial")
    } else if error_count > 0 {
        (Value::Bool(false), "error")
    } else {
        (Value::Bool(false), "not_visible")
    };

    let mut report = json!({
        "agent_raw_queued": agent_raw_queued,
        "agent_flush": agent_flush,
        "agent_structured_visible": structured_visible,
        "agent_visibility_status": status,
        "agent_visibility_checks": checks,
    });
    if let Some(map) = report.as_object_mut() {
        if let Some(user_id) = user_id {
            map.insert(
                "verification_user_id".to_string(),
                Value::String(user_id.to_string()),
            );
        }
        if let Some(session_id) = session_id {
            map.insert(
                "verification_session_id".to_string(),
                Value::String(session_id.to_string()),
            );
        }
    }
    report
}

pub fn workflow_status_from_agent_visibility(agent_visibility: &Value, fallback: &str) -> String {
    match agent_visibility
        .get("agent_visibility_status")
        .and_then(Value::as_str)
    {
        Some("visible") => "verified".to_string(),
        Some("partial") => "partially_verified".to_string(),
        Some("not_visible") => "agent_not_visible".to_string(),
        Some("error") => "agent_visibility_error".to_string(),
        _ => fallback.to_string(),
    }
}

pub fn audit_agent_visibility(
    client: &EverOSClient,
    user_id: &str,
    session_id: Option<&str>,
    queries: &[String],
    top_k: i64,
    timeout: Option<f64>,
    get_page_size: u64,
) -> Value {
    audit_agent_visibility_with_options(
        client,
        user_id,
        session_id,
        queries,
        top_k,
        timeout,
        get_page_size,
        None,
        false,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn audit_agent_visibility_with_options(
    client: &EverOSClient,
    user_id: &str,
    session_id: Option<&str>,
    queries: &[String],
    top_k: i64,
    timeout: Option<f64>,
    get_page_size: u64,
    precomputed_searches: Option<&HashMap<String, Value>>,
    include_responses: bool,
) -> Value {
    let mut checks = Vec::new();
    for query in queries
        .iter()
        .map(|query| query.trim())
        .filter(|query| !query.is_empty())
    {
        let started = Instant::now();
        let mut check = json!({
            "kind": "search",
            "user_id": user_id,
            "session_id": session_id,
            "memory_types": ["agent_memory"],
            "query": query,
        });
        let search_result = precomputed_searches
            .and_then(|items| items.get(query).cloned())
            .map(Ok)
            .unwrap_or_else(|| {
                client.search_memories(
                    query,
                    Some(user_id),
                    None,
                    session_id,
                    None,
                    "hybrid",
                    Some(vec!["agent_memory".to_string()]),
                    top_k,
                    None,
                    false,
                    false,
                    timeout,
                )
            });
        match search_result {
            Ok(response) => {
                let hit_count = count_hits(&response);
                set_check_fields(
                    &mut check,
                    if hit_count > 0 { "hit" } else { "miss" },
                    hit_count,
                    Some(response),
                    None,
                    include_responses,
                );
            }
            Err(err) => {
                set_check_fields(&mut check, "error", 0, None, Some(err), include_responses);
            }
        }
        set_latency(&mut check, started);
        checks.push(check);
    }

    for memory_type in ["agent_case", "agent_skill"] {
        let started = Instant::now();
        let mut check = json!({"kind": "get", "user_id": user_id, "session_id": session_id, "memory_type": memory_type});
        match client.get_memories(
            Some(user_id),
            None,
            session_id,
            None,
            memory_type,
            1,
            get_page_size,
            "timestamp",
            "desc",
        ) {
            Ok(response) => {
                let hit_count = count_hits(&response);
                set_check_fields(
                    &mut check,
                    if hit_count > 0 { "hit" } else { "miss" },
                    hit_count,
                    Some(response),
                    None,
                    include_responses,
                );
            }
            Err(err) => {
                set_check_fields(&mut check, "error", 0, None, Some(err), include_responses);
            }
        }
        set_latency(&mut check, started);
        checks.push(check);
    }

    build_agent_visibility_report(None, None, checks, Some(user_id), session_id)
}

fn set_latency(check: &mut Value, started: Instant) {
    if let Some(map) = check.as_object_mut() {
        map.insert(
            "latency_ms".to_string(),
            json!(started.elapsed().as_secs_f64() * 1000.0),
        );
    }
}

fn check_hit_count(check: &Value) -> u64 {
    check
        .get("hit_count")
        .and_then(|value| {
            value
                .as_u64()
                .or_else(|| value.as_i64().map(|num| num.max(0) as u64))
        })
        .unwrap_or(0)
}

fn set_check_fields(
    check: &mut Value,
    status: &str,
    hit_count: usize,
    response: Option<Value>,
    error: Option<EverOSError>,
    include_response: bool,
) {
    if let Some(map) = check.as_object_mut() {
        map.insert("status".to_string(), Value::String(status.to_string()));
        map.insert("hit_count".to_string(), json!(hit_count));
        if let Some(response) = response {
            map.insert("response_summary".to_string(), response_summary(&response));
            if include_response {
                map.insert("response".to_string(), response);
            }
        }
        if let Some(error) = error {
            map.insert("error".to_string(), Value::String(error.to_string()));
        }
    }
}
