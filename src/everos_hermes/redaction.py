from __future__ import annotations

import copy
import json
import re
from typing import Any

CONTEXT_BLOCK_RE = re.compile(r"<(?P<tag>everos-context|memory-context)\b[^>]*>.*?</(?P=tag)>", re.IGNORECASE | re.DOTALL)
SENSITIVE_KEY_PATTERN = r"api[_-]?key|token|access[_-]?token|refresh[_-]?token|password|passwd|secret|authorization|credentials?|private[_-]?key"
SENSITIVE_KEY_RE = re.compile(rf"(^|[_\-\s.])({SENSITIVE_KEY_PATTERN})([_\-\s.]|$)", re.IGNORECASE)
SENSITIVE_ASSIGN_RE = re.compile(rf"(?P<prefix>[\"']?(?:{SENSITIVE_KEY_PATTERN})[\"']?\s*[:=]\s*)", re.IGNORECASE)
MAX_SANITIZED_ERROR_CHARS = 500
DIAGNOSTIC_FIELD_RE = re.compile(
    r"\s+(?:request[_-]?id|trace[_-]?id|span[_-]?id|status|code)=[^\s,;\}\]]+",
    re.IGNORECASE,
)
EMBEDDED_JSON_STRING_PATTERNS = [
    re.compile(r"(?P<prefix>[\"']?[A-Za-z_][\w.\-]*[\"']?\s*[:=]\s*\")(?P<value>(?:\\.|[^\"])*)\""),
    re.compile(r"(?P<prefix>[\"']?[A-Za-z_][\w.\-]*[\"']?\s*[:=]\s*')(?P<value>(?:\\.|[^'])*)'"),
]
SECRET_TEXT_PATTERNS = [
    re.compile(r"Authorization\s*:\s*Bearer\s+[^\s,;\]}]+", re.IGNORECASE),
    re.compile(r"\bBearer\s+[A-Za-z0-9._~+/=\-]+", re.IGNORECASE),
    re.compile(r"\bsk-[A-Za-z0-9._\-]{4,}\b"),
    re.compile(
        rf"(?P<prefix>[\"']?(?:{SENSITIVE_KEY_PATTERN})[\"']?\s*[:=]\s*\")(?P<value>(?:\\.|[^\"])*)(?P<suffix>\")",
        re.IGNORECASE,
    ),
    re.compile(
        rf"(?P<prefix>[\"']?(?:{SENSITIVE_KEY_PATTERN})[\"']?\s*[:=]\s*')(?P<value>(?:\\.|[^'])*)(?P<suffix>')",
        re.IGNORECASE,
    ),
    re.compile(
        rf"(?P<prefix>[\"']?(?:{SENSITIVE_KEY_PATTERN})[\"']?\s*[:=]\s*)(?P<value>[^\r\n]+)",
        re.IGNORECASE,
    ),
]


def is_sensitive_key(key: Any) -> bool:
    return bool(SENSITIVE_KEY_RE.search(str(key)))


def strip_context_blocks(text: str) -> str:
    return CONTEXT_BLOCK_RE.sub("", text)


def _split_diagnostic_suffix(value: str) -> tuple[str, str]:
    return value, "".join(match.group(0) for match in DIAGNOSTIC_FIELD_RE.finditer(value))


def _truncate_sanitized(text: str) -> str:
    if len(text) <= MAX_SANITIZED_ERROR_CHARS:
        return text
    return text[:MAX_SANITIZED_ERROR_CHARS] + "...[truncated]"


def _decode_escaped_string(value: str) -> str | None:
    try:
        decoded = json.loads(f'"{value}"')
    except Exception:
        return None
    return decoded if isinstance(decoded, str) else None


def _escape_string_value(value: str) -> str:
    encoded = json.dumps(value, ensure_ascii=False)
    return encoded[1:-1]


def _scrub_json_text(text: str) -> str | None:
    stripped = text.strip()
    if stripped[:1] not in {"{", "["}:
        return None
    try:
        parsed = json.loads(stripped)
    except Exception:
        return None
    return json.dumps(scrub_value(parsed), ensure_ascii=False, sort_keys=True, separators=(",", ":"))


def _redact_embedded_json_strings(text: str) -> str:
    redacted = text
    for pattern in EMBEDDED_JSON_STRING_PATTERNS:
        def replace(match: re.Match[str]) -> str:
            decoded = _decode_escaped_string(match.group("value"))
            if decoded is None:
                return match.group(0)
            scrubbed = _scrub_json_text(decoded)
            if scrubbed is None:
                return match.group(0)
            suffix = '"' if match.group(0).endswith('"') else "'"
            return f"{match.group('prefix')}{_escape_string_value(scrubbed)}{suffix}"

        redacted = pattern.sub(replace, redacted)
    return redacted


def _find_balanced_end(text: str, start: int) -> int | None:
    pairs = {"{": "}", "[": "]"}
    opener = text[start : start + 1]
    closer = pairs.get(opener)
    if closer is None:
        return None
    depth = 0
    quote: str | None = None
    escape = False
    for pos in range(start, len(text)):
        char = text[pos]
        if quote is not None:
            if escape:
                escape = False
            elif char == "\\":
                escape = True
            elif char == quote:
                quote = None
            continue
        if char in {"'", '"'}:
            quote = char
        elif char == opener:
            depth += 1
        elif char == closer:
            depth -= 1
            if depth == 0:
                return pos + 1
    return None


def _redact_sensitive_jsonish_assignments(text: str) -> str:
    out: list[str] = []
    cursor = 0
    for match in SENSITIVE_ASSIGN_RE.finditer(text):
        if match.start() < cursor:
            continue
        value_start = match.end()
        while value_start < len(text) and text[value_start].isspace():
            value_start += 1
        if value_start >= len(text) or text[value_start] not in {"{", "["}:
            continue
        value_end = _find_balanced_end(text, value_start)
        if value_end is None:
            continue
        out.append(text[cursor : match.start()])
        out.append(match.group("prefix"))
        out.append("[REDACTED]")
        cursor = value_end
    if not out:
        return text
    out.append(text[cursor:])
    return "".join(out)


def redact_text(text: str) -> str:
    raw = str(text or "")
    scrubbed = _scrub_json_text(raw)
    if scrubbed is not None:
        return scrubbed
    redacted = _redact_embedded_json_strings(raw)
    redacted = _redact_sensitive_jsonish_assignments(redacted)
    for pattern in SECRET_TEXT_PATTERNS:
        if "prefix" in pattern.groupindex:
            def replace_secret(match: re.Match[str]) -> str:
                suffix = match.groupdict().get("suffix")
                if suffix is not None:
                    return f"{match.group('prefix')}[REDACTED]{suffix}"
                _, diagnostic_suffix = _split_diagnostic_suffix(match.group("value"))
                return f"{match.group('prefix')}[REDACTED]{diagnostic_suffix}"

            redacted = pattern.sub(replace_secret, redacted)
        else:
            redacted = pattern.sub("[REDACTED]", redacted)
    return redacted


def scrub_value(value: Any) -> Any:
    if isinstance(value, str):
        return strip_context_blocks(redact_text(value))
    if isinstance(value, list):
        return [scrub_value(item) for item in value]
    if isinstance(value, tuple):
        return [scrub_value(item) for item in value]
    if isinstance(value, dict):
        out: dict[str, Any] = {}
        for key, val in value.items():
            key_text = str(key)
            out[key_text] = "[REDACTED]" if is_sensitive_key(key_text) else scrub_value(val)
        return out
    return copy.deepcopy(value)


def sanitized_error_message(exc: BaseException | str) -> str:
    return _truncate_sanitized(redact_text(str(exc)))


def error_payload(operation: str, exc: BaseException | str, *, retryable: bool = True) -> dict[str, Any]:
    return {
        "ok": False,
        "operation": operation,
        "status": "error",
        "error_code": exc.__class__.__name__ if isinstance(exc, BaseException) else "error",
        "message": sanitized_error_message(exc),
        "retryable": retryable,
        "suggested_next_actions": ["inspect EverOS status/search before retrying to avoid duplicate writes"],
    }
