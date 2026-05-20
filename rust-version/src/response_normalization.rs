use serde_json::{Map, Value, json};

pub const SEARCH_KEYS: [&str; 11] = [
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
    "memories",
];

pub fn response_payload(response: &Value) -> &Value {
    response.get("data").unwrap_or(response)
}

pub fn response_data(response: Option<&Value>) -> Map<String, Value> {
    let Some(response) = response else {
        return Map::new();
    };
    response_payload(response)
        .as_object()
        .cloned()
        .unwrap_or_default()
}

pub fn as_list(value: Option<&Value>) -> Vec<Value> {
    match value {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(items)) => items.clone(),
        Some(other) => vec![other.clone()],
    }
}

pub fn count_hits(response: &Value) -> usize {
    count_hits_value(response_payload(response))
}

pub fn response_summary(response: &Value) -> Value {
    let data = response_payload(response);
    let hit_count = count_hits(response);
    if let Some(map) = data.as_object() {
        let mut keys: Vec<String> = map.keys().cloned().collect();
        keys.sort();
        return json!({"keys": keys, "hit_count": hit_count});
    }
    if let Some(items) = data.as_array() {
        return json!({"items": items.len(), "hit_count": hit_count});
    }
    json!({"type": value_type_name(data), "hit_count": hit_count})
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

fn value_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "NoneType",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "str",
        Value::Array(_) => "list",
        Value::Object(_) => "dict",
    }
}
