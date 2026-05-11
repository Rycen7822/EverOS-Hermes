use serde_json::Value;

pub fn compact_json(data: &Value) -> String {
    serde_json::to_string(data).unwrap_or_else(|_| "null".to_string())
}

pub fn pretty_json(data: &Value) -> String {
    serde_json::to_string_pretty(data).unwrap_or_else(|_| "null".to_string())
}

pub fn format_search_context(response: &Value, max_items: usize) -> String {
    let data = response.get("data").unwrap_or(response);
    let Some(data) = data.as_object() else {
        return String::new();
    };

    let mut lines = Vec::new();
    let episodes = as_list(
        data.get("episodes")
            .or_else(|| data.get("results"))
            .or_else(|| data.get("memories")),
    );
    let profiles = as_list(data.get("profiles").or_else(|| data.get("profile")));
    let agent_cases = as_list(data.get("agent_cases"));
    let agent_skills = as_list(data.get("agent_skills"));

    let episode_lines = format_episodes(&episodes, max_items);
    let profile_lines = format_profiles(&profiles, max_items);
    let agent_case_lines = format_agent_cases(&agent_cases, max_items);
    let agent_skill_lines = format_agent_skills(&agent_skills, max_items);

    if !episode_lines.is_empty() {
        lines.push("## Episodes".to_string());
        lines.extend(episode_lines);
    }
    if !profile_lines.is_empty() {
        lines.push("## Profile".to_string());
        lines.extend(profile_lines);
    }
    if !agent_case_lines.is_empty() {
        lines.push("## Agent Cases".to_string());
        lines.extend(agent_case_lines);
    }
    if !agent_skill_lines.is_empty() {
        lines.push("## Agent Skills".to_string());
        lines.extend(agent_skill_lines);
    }

    if lines.is_empty() {
        String::new()
    } else {
        format!("# EverOS Memory\n{}", lines.join("\n"))
    }
}

fn format_episodes(items: &[Value], max_items: usize) -> Vec<String> {
    let mut lines = Vec::new();
    for item in items.iter().take(max_items) {
        if let Some(map) = item.as_object() {
            let subject = first_text(item, &["subject", "title", "topic", "type"]);
            let summary = first_text(
                item,
                &[
                    "summary",
                    "episode",
                    "content",
                    "memory",
                    "text",
                    "narrative",
                ],
            );
            let score = map
                .get("score")
                .or_else(|| map.get("relevance_score"))
                .or_else(|| map.get("similarity"));
            let prefix = if subject.is_empty() {
                "- ".to_string()
            } else {
                format!("- {subject}: ")
            };
            let body = truncate(
                &(if summary.is_empty() {
                    compact_json(item)
                } else {
                    summary
                }),
                700,
            );
            lines.push(format!("{prefix}{body}{}", format_score(score)));
        } else if let Some(text) = scalar_text(item).filter(|text| !text.trim().is_empty()) {
            lines.push(format!("- {}", truncate(&text, 500)));
        }
    }
    lines
}

fn format_profiles(items: &[Value], max_items: usize) -> Vec<String> {
    let mut lines = Vec::new();
    for item in items.iter().take(max_items) {
        if let Some(map) = item.as_object() {
            let profile_data = map
                .get("profile_data")
                .filter(|value| value.is_object())
                .unwrap_or(item);
            for key in [
                "explicit_info",
                "implicit_traits",
                "preferences",
                "facts",
                "traits",
            ] {
                for fact in as_list(profile_data.get(key)) {
                    let text = stringify_fact(&fact);
                    if !text.is_empty() {
                        lines.push(format!(
                            "- {}: {}",
                            key.replace('_', " "),
                            truncate(&text, 500)
                        ));
                        if lines.len() >= max_items {
                            return lines;
                        }
                    }
                }
            }
            if lines.is_empty() {
                lines.push(format!("- {}", truncate(&compact_json(item), 700)));
            }
        } else if let Some(text) = scalar_text(item).filter(|text| !text.trim().is_empty()) {
            lines.push(format!("- {}", truncate(&text, 500)));
        }
    }
    lines
}

fn format_agent_cases(items: &[Value], max_items: usize) -> Vec<String> {
    items
        .iter()
        .take(max_items)
        .filter_map(|item| {
            item.as_object()?;
            let intent = first_text(item, &["task_intent", "intent", "name"]);
            let approach = first_text(item, &["approach", "summary", "content"]);
            Some(if intent.is_empty() {
                format!(
                    "- {}",
                    if approach.is_empty() {
                        compact_json(item)
                    } else {
                        approach
                    }
                )
            } else {
                format!("- {intent}: {approach}")
            })
        })
        .collect()
}

fn format_agent_skills(items: &[Value], max_items: usize) -> Vec<String> {
    items
        .iter()
        .take(max_items)
        .filter_map(|item| {
            item.as_object()?;
            let name = first_text(item, &["name", "title"]);
            let desc = first_text(item, &["description", "content", "summary"]);
            Some(if name.is_empty() {
                format!(
                    "- {}",
                    if desc.is_empty() {
                        compact_json(item)
                    } else {
                        desc
                    }
                )
            } else {
                format!("- {name}: {desc}")
            })
        })
        .collect()
}

fn as_list(value: Option<&Value>) -> Vec<Value> {
    match value {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(items)) => items.clone(),
        Some(other) => vec![other.clone()],
    }
}

fn first_text(mapping: &Value, keys: &[&str]) -> String {
    let Some(map) = mapping.as_object() else {
        return String::new();
    };
    for key in keys {
        if let Some(text) = map.get(*key).and_then(Value::as_str) {
            let text = text.trim();
            if !text.is_empty() {
                return text.to_string();
            }
        }
    }
    String::new()
}

fn stringify_fact(value: &Value) -> String {
    if let Some(text) = value.as_str() {
        return text.trim().to_string();
    }
    if value.is_object() {
        let text = first_text(value, &["text", "content", "fact", "value", "summary"]);
        return if text.is_empty() {
            compact_json(value)
        } else {
            text
        };
    }
    scalar_text(value).unwrap_or_default().trim().to_string()
}

fn scalar_text(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(flag) => Some(flag.to_string()),
        _ => None,
    }
}

fn format_score(score: Option<&Value>) -> String {
    let Some(score) = score else {
        return String::new();
    };
    if let Some(value) = score.as_f64() {
        if (0.0..=1.0).contains(&value) {
            return format!(" [score={value:.2}]");
        }
        return format!(" [score={value}]");
    }
    if let Some(text) = scalar_text(score) {
        return format!(" [score={text}]");
    }
    format!(" [score={}]", compact_json(score))
}

fn truncate(text: &str, limit: usize) -> String {
    text.chars().take(limit).collect()
}
