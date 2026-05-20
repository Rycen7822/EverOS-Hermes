use regex::{Captures, Regex};
use serde_json::{Value, json};
use std::fmt::Display;
use std::sync::OnceLock;

const REDACTED: &str = "[REDACTED]";
const MAX_SANITIZED_ERROR_CHARS: usize = 500;
const SENSITIVE_KEY_PATTERN: &str = "api[_-]?key|token|access[_-]?token|refresh[_-]?token|password|passwd|secret|authorization|credentials?|private[_-]?key";

pub fn strip_context_blocks(text: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?is)<everos-context\b[^>]*>.*?</everos-context>|<memory-context\b[^>]*>.*?</memory-context>").unwrap()
    })
    .replace_all(text, "")
    .to_string()
}

pub fn redact_text(text: &str) -> String {
    static AUTH_HEADER_RE: OnceLock<Regex> = OnceLock::new();
    static BEARER_RE: OnceLock<Regex> = OnceLock::new();
    static SK_RE: OnceLock<Regex> = OnceLock::new();
    static KV_DOUBLE_RE: OnceLock<Regex> = OnceLock::new();
    static KV_SINGLE_RE: OnceLock<Regex> = OnceLock::new();
    static KV_UNQUOTED_RE: OnceLock<Regex> = OnceLock::new();

    let text = AUTH_HEADER_RE
        .get_or_init(|| Regex::new(r"(?i)Authorization\s*:\s*Bearer\s+[^\s,;\]}]+").unwrap())
        .replace_all(text, REDACTED)
        .to_string();
    let text = BEARER_RE
        .get_or_init(|| Regex::new(r"(?i)\bBearer\s+[A-Za-z0-9._~+/=\-]+").unwrap())
        .replace_all(&text, REDACTED)
        .to_string();
    let text = SK_RE
        .get_or_init(|| Regex::new(r"\bsk-[A-Za-z0-9._\-]{4,}\b").unwrap())
        .replace_all(&text, REDACTED)
        .to_string();
    let text = redact_embedded_json_strings(&text);
    let text = redact_sensitive_jsonish_assignments(&text);
    let text = KV_DOUBLE_RE
        .get_or_init(|| {
            Regex::new(r#"(?i)(?P<prefix>[\"']?(?:api[_-]?key|token|access[_-]?token|refresh[_-]?token|password|passwd|secret|authorization|credentials?|private[_-]?key)[\"']?\s*[:=]\s*\")(?P<value>(?:\\.|[^"])*)(?P<suffix>\")"#).unwrap()
        })
        .replace_all(&text, |caps: &Captures<'_>| redact_kv_match(caps))
        .to_string();
    let text = KV_SINGLE_RE
        .get_or_init(|| {
            Regex::new(r#"(?i)(?P<prefix>[\"']?(?:api[_-]?key|token|access[_-]?token|refresh[_-]?token|password|passwd|secret|authorization|credentials?|private[_-]?key)[\"']?\s*[:=]\s*')(?P<value>(?:\\.|[^'])*)(?P<suffix>')"#).unwrap()
        })
        .replace_all(&text, |caps: &Captures<'_>| redact_kv_match(caps))
        .to_string();
    KV_UNQUOTED_RE
        .get_or_init(|| {
            Regex::new(r#"(?i)(?P<prefix>[\"']?(?:api[_-]?key|token|access[_-]?token|refresh[_-]?token|password|passwd|secret|authorization|credentials?|private[_-]?key)[\"']?\s*[:=]\s*)(?P<value>[^\r\n]+)"#).unwrap()
        })
        .replace_all(&text, |caps: &Captures<'_>| redact_kv_match(caps))
        .to_string()
}

fn redact_embedded_json_strings(text: &str) -> String {
    static DOUBLE_RE: OnceLock<Regex> = OnceLock::new();
    static SINGLE_RE: OnceLock<Regex> = OnceLock::new();
    let text = DOUBLE_RE
        .get_or_init(|| {
            Regex::new("(?P<prefix>[\\\"']?[A-Za-z_][A-Za-z0-9_.-]*[\\\"']?\\s*[:=]\\s*\\\")(?P<value>(?:\\\\.|[^\\\"])*)\\\"").unwrap()
        })
        .replace_all(text, |caps: &Captures<'_>| redact_embedded_json_match(caps, "\""))
        .to_string();
    SINGLE_RE
        .get_or_init(|| {
            Regex::new("(?P<prefix>[\\\"']?[A-Za-z_][A-Za-z0-9_.-]*[\\\"']?\\s*[:=]\\s*')(?P<value>(?:\\\\.|[^'])*)'").unwrap()
        })
        .replace_all(&text, |caps: &Captures<'_>| redact_embedded_json_match(caps, "'"))
        .to_string()
}

fn redact_embedded_json_match(caps: &Captures<'_>, suffix: &str) -> String {
    let Some(full) = caps.get(0).map(|m| m.as_str()) else {
        return String::new();
    };
    let prefix = caps.name("prefix").map(|m| m.as_str()).unwrap_or_default();
    let value = caps.name("value").map(|m| m.as_str()).unwrap_or_default();
    let quoted = format!("\"{value}\"");
    let Ok(decoded) = serde_json::from_str::<String>(&quoted) else {
        return full.to_string();
    };
    let Some(scrubbed) = scrub_json_text(&decoded) else {
        return full.to_string();
    };
    let Ok(encoded) = serde_json::to_string(&scrubbed) else {
        return full.to_string();
    };
    let escaped = encoded
        .strip_prefix('"')
        .and_then(|inner| inner.strip_suffix('"'))
        .unwrap_or(encoded.as_str());
    format!("{prefix}{escaped}{suffix}")
}

fn redact_sensitive_jsonish_assignments(text: &str) -> String {
    static ASSIGN_RE: OnceLock<Regex> = OnceLock::new();
    let assign_re = ASSIGN_RE.get_or_init(|| {
        Regex::new(r#"(?i)(?P<prefix>[\"']?(?:api[_-]?key|token|access[_-]?token|refresh[_-]?token|password|passwd|secret|authorization|credentials?|private[_-]?key)[\"']?\s*[:=]\s*)"#).unwrap()
    });
    let mut out = String::new();
    let mut cursor = 0usize;
    let mut changed = false;
    for caps in assign_re.captures_iter(text) {
        let Some(full) = caps.get(0) else {
            continue;
        };
        if full.start() < cursor {
            continue;
        }
        let value_start = skip_whitespace(text, full.end());
        let Some(opener) = text[value_start..].chars().next() else {
            continue;
        };
        if opener != '{' && opener != '[' {
            continue;
        }
        let Some(value_end) = find_balanced_end(text, value_start) else {
            continue;
        };
        out.push_str(&text[cursor..full.start()]);
        out.push_str(
            caps.name("prefix")
                .map(|m| m.as_str())
                .unwrap_or(full.as_str()),
        );
        out.push_str(REDACTED);
        cursor = value_end;
        changed = true;
    }
    if !changed {
        return text.to_string();
    }
    out.push_str(&text[cursor..]);
    out
}

fn skip_whitespace(text: &str, mut index: usize) -> usize {
    while index < text.len() {
        let Some(ch) = text[index..].chars().next() else {
            return index;
        };
        if !ch.is_whitespace() {
            return index;
        }
        index += ch.len_utf8();
    }
    index
}

fn find_balanced_end(text: &str, start: usize) -> Option<usize> {
    let opener = text[start..].chars().next()?;
    let closer = match opener {
        '{' => '}',
        '[' => ']',
        _ => return None,
    };
    let mut depth = 0usize;
    let mut quote: Option<char> = None;
    let mut escape = false;
    for (offset, ch) in text[start..].char_indices() {
        if let Some(active_quote) = quote {
            if escape {
                escape = false;
            } else if ch == '\\' {
                escape = true;
            } else if ch == active_quote {
                quote = None;
            }
            continue;
        }
        if ch == '"' || ch == '\'' {
            quote = Some(ch);
        } else if ch == opener {
            depth += 1;
        } else if ch == closer {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return Some(start + offset + ch.len_utf8());
            }
        }
    }
    None
}

fn redact_kv_match(caps: &Captures<'_>) -> String {
    let prefix = caps.name("prefix").map(|m| m.as_str()).unwrap_or_default();
    let suffix = caps.name("suffix").map(|m| m.as_str()).unwrap_or_default();
    if !suffix.is_empty() {
        return format!("{prefix}{REDACTED}{suffix}");
    }
    let value = caps.name("value").map(|m| m.as_str()).unwrap_or_default();
    let diagnostic_suffix = diagnostic_suffix(value);
    format!("{prefix}{REDACTED}{diagnostic_suffix}")
}

fn diagnostic_suffix(value: &str) -> String {
    static DIAGNOSTIC_RE: OnceLock<Regex> = OnceLock::new();
    DIAGNOSTIC_RE
        .get_or_init(|| {
            Regex::new(
                r"(?i)\s+(?:request[_-]?id|trace[_-]?id|span[_-]?id|status|code)=[^\s,;\}\]]+",
            )
            .unwrap()
        })
        .find_iter(value)
        .map(|found| found.as_str())
        .collect()
}

pub fn sanitized_error_message(err: impl Display) -> String {
    truncate_sanitized(redact_text(&err.to_string()))
}

fn truncate_sanitized(text: String) -> String {
    if text.chars().count() <= MAX_SANITIZED_ERROR_CHARS {
        return text;
    }
    let mut out: String = text.chars().take(MAX_SANITIZED_ERROR_CHARS).collect();
    out.push_str("...[truncated]");
    out
}

pub fn error_payload(operation: &str, err: impl Display) -> Value {
    json!({
        "ok": false,
        "operation": operation,
        "status": "error",
        "error_code": "error",
        "message": sanitized_error_message(err),
        "retryable": true,
        "suggested_next_actions": ["inspect EverOS status/search before retrying to avoid duplicate writes"],
    })
}

pub fn scrub_value(value: &Value) -> Value {
    match value {
        Value::String(text) => Value::String(scrub_string(text)),
        Value::Array(items) => Value::Array(items.iter().map(scrub_value).collect()),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, value)| {
                    let value = if is_sensitive_key(key) {
                        Value::String(REDACTED.to_string())
                    } else {
                        scrub_value(value)
                    };
                    (key.clone(), value)
                })
                .collect(),
        ),
        other => other.clone(),
    }
}

fn scrub_string(text: &str) -> String {
    let stripped = strip_context_blocks(text);
    if let Some(scrubbed) = scrub_json_text(&stripped) {
        return scrubbed;
    }
    redact_text(&stripped)
}

fn scrub_json_text(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if !(trimmed.starts_with('{') || trimmed.starts_with('[')) {
        return None;
    }
    let parsed = serde_json::from_str::<Value>(trimmed).ok()?;
    serde_json::to_string(&scrub_value(&parsed)).ok()
}

fn is_sensitive_key(key: &str) -> bool {
    static KEY_RE: OnceLock<Regex> = OnceLock::new();
    KEY_RE
        .get_or_init(|| {
            Regex::new(&format!(
                r"(?i)(^|[_\-\s.])({SENSITIVE_KEY_PATTERN})([_\-\s.]|$)"
            ))
            .unwrap()
        })
        .is_match(key)
}
