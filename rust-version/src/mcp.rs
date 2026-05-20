use crate::client::EverOSClient;
use crate::env::get_env;
use crate::redaction::sanitized_error_message;
use serde_json::{Value, json};
use std::io::{self, BufRead, BufReader, Read, Write};

const MAX_FRAME_HEADER_LINE_BYTES: usize = 8 * 1024;
const MAX_FRAME_HEADER_BYTES: usize = 64 * 1024;
const MAX_FRAME_BODY_BYTES: usize = 16 * 1024 * 1024;

pub use crate::mcp_dispatch::call_tool;
pub use crate::mcp_tools::{TOOL_NAMES, tool_definitions};

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
                    json!({"content":[{"type":"text","text":format!("Error: {}", sanitized_error_message(&err))}],"isError":true})
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

pub fn read_frame<R: BufRead + Read>(reader: &mut R) -> io::Result<Option<Value>> {
    let first = loop {
        let Some(line) = read_bounded_line(reader, true)? else {
            return Ok(None);
        };
        if !line.trim().is_empty() {
            break line;
        }
    };
    if first.trim_start().starts_with('{') {
        let value = serde_json::from_str(first.trim())
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
        return Ok(Some(value));
    }
    validate_header_line(&first)?;
    let mut header_bytes = first.len();
    let mut content_length = parse_content_length(&first);
    loop {
        let Some(line) = read_bounded_line(reader, false)? else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unexpected EOF while reading MCP headers",
            ));
        };
        validate_header_line(&line)?;
        header_bytes += line.len();
        if header_bytes > MAX_FRAME_HEADER_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "MCP headers exceed maximum size",
            ));
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
    if length > MAX_FRAME_BODY_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "MCP frame body exceeds maximum size",
        ));
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    let value = serde_json::from_slice(&body)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    Ok(Some(value))
}

fn read_bounded_line<R: BufRead>(
    reader: &mut R,
    first_frame_line: bool,
) -> io::Result<Option<String>> {
    let mut out = Vec::new();
    let mut limit = if first_frame_line {
        None
    } else {
        Some(MAX_FRAME_HEADER_LINE_BYTES)
    };
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            if out.is_empty() {
                return Ok(None);
            }
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unexpected EOF while reading MCP line",
            ));
        }
        if first_frame_line && limit.is_none() {
            match available.iter().find(|byte| !byte.is_ascii_whitespace()) {
                Some(byte) if *byte == b'{' => limit = Some(MAX_FRAME_BODY_BYTES),
                Some(_) => limit = Some(MAX_FRAME_HEADER_LINE_BYTES),
                None => {}
            }
        }
        let max_len = limit.unwrap_or(MAX_FRAME_BODY_BYTES);
        let newline_pos = available.iter().position(|byte| *byte == b'\n');
        let take = newline_pos.map_or(available.len(), |pos| pos + 1);
        if out.len() + take > max_len {
            return Err(line_limit_error(max_len));
        }
        out.extend_from_slice(&available[..take]);
        reader.consume(take);
        if newline_pos.is_some() {
            let line = String::from_utf8(out)
                .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
            return Ok(Some(line));
        }
    }
}

fn line_limit_error(limit: usize) -> io::Error {
    let message = if limit == MAX_FRAME_BODY_BYTES {
        "JSON-RPC line frame exceeds maximum body size"
    } else {
        "MCP header line exceeds maximum size"
    };
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn validate_header_line(line: &str) -> io::Result<()> {
    if line.len() > MAX_FRAME_HEADER_LINE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "MCP header line exceeds maximum size",
        ));
    }
    Ok(())
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
