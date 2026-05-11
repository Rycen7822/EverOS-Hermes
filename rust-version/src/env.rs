use std::collections::HashMap;
use std::env as std_env;
use std::fs;
use std::path::{Path, PathBuf};

fn is_valid_key(key: &str) -> bool {
    let mut chars = key.chars();
    match chars.next() {
        Some(c) if c == '_' || c.is_ascii_alphabetic() => {}
        _ => return false,
    }
    chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
}

pub fn hermes_home(explicit: Option<&Path>) -> PathBuf {
    if let Some(path) = explicit {
        return path.to_path_buf();
    }
    if let Some(value) = std_env::var_os("HERMES_HOME").filter(|value| !value.is_empty()) {
        return PathBuf::from(value);
    }
    default_hermes_home()
}

pub fn default_hermes_home() -> PathBuf {
    if let Some(home) = std_env::var_os("HOME") {
        return PathBuf::from(home).join(".hermes");
    }
    if let Some(profile) = std_env::var_os("USERPROFILE") {
        return PathBuf::from(profile).join(".hermes");
    }
    PathBuf::from(".hermes")
}

pub fn hermes_dotenv_path(explicit_home: Option<&Path>) -> PathBuf {
    hermes_home(explicit_home).join(".env")
}

pub fn read_dotenv(path: &Path) -> HashMap<String, String> {
    let Ok(text) = fs::read_to_string(path) else {
        return HashMap::new();
    };
    let mut values = HashMap::new();
    for raw_line in text.lines() {
        let mut line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix("export ") {
            line = rest.trim_start();
        }
        let Some((key, raw_value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if !is_valid_key(key) {
            continue;
        }
        values.insert(key.to_string(), parse_dotenv_value(raw_value));
    }
    values
}

pub fn dotenv_values(explicit_home: Option<&Path>) -> HashMap<String, String> {
    let mut merged = HashMap::new();
    for path in dotenv_lookup_paths(explicit_home) {
        for (key, value) in read_dotenv(&path) {
            merged.entry(key).or_insert(value);
        }
    }
    merged
}

pub fn get_env(name: &str, default: &str, explicit_home: Option<&Path>) -> String {
    if let Ok(value) = std_env::var(name) {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    for path in dotenv_lookup_paths(explicit_home) {
        let values = read_dotenv(&path);
        if let Some(value) = values.get(name) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return trimmed.to_string();
            }
        }
    }
    default.trim().to_string()
}

fn dotenv_lookup_paths(explicit_home: Option<&Path>) -> Vec<PathBuf> {
    let mut homes = Vec::new();
    if let Some(path) = explicit_home {
        homes.push(path.to_path_buf());
    } else if let Some(value) = std_env::var_os("HERMES_HOME").filter(|value| !value.is_empty()) {
        homes.push(PathBuf::from(value));
    }
    homes.push(default_hermes_home());

    let mut paths = Vec::new();
    for home in homes {
        let path = home.join(".env");
        if !paths.iter().any(|existing| existing == &path) {
            paths.push(path);
        }
    }
    paths
}

fn parse_dotenv_value(raw: &str) -> String {
    let mut value = raw.trim().to_string();
    if value.len() >= 2 {
        let first = value.as_bytes()[0] as char;
        let last = value.as_bytes()[value.len() - 1] as char;
        if (first == '\'' || first == '"') && first == last {
            value = value[1..value.len() - 1].to_string();
            if first == '"' {
                value = unescape_double_quoted(&value);
            }
            return value;
        }
    }
    for marker in [" #", "\t#"] {
        if let Some(index) = value.find(marker) {
            value.truncate(index);
            value = value.trim_end().to_string();
            break;
        }
    }
    value
}

fn unescape_double_quoted(value: &str) -> String {
    let mut out = String::new();
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some('"') => out.push('"'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}
