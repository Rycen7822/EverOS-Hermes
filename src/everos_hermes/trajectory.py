from __future__ import annotations

import copy
import hashlib
import json
import re
import time
from dataclasses import dataclass
from datetime import datetime, timezone
from typing import Any, Literal


TrajectorySource = Literal["session_end", "pre_compress", "delegation", "sync_turn"]
_CONTEXT_BLOCK_RE = re.compile(r"<(?P<tag>everos-context|memory-context)\b[^>]*>.*?</(?P=tag)>", re.IGNORECASE | re.DOTALL)
_SECRET_PATTERNS = [
    re.compile(r"Authorization:\s*Bearer\s+[A-Za-z0-9._\-]+", re.IGNORECASE),
    re.compile(r"\bsk-[A-Za-z0-9]{12,}\b"),
    re.compile(r"\b(api[_-]?key|token|password|secret)\s*[:=]\s*[^\s,;\]}]+", re.IGNORECASE),
]


@dataclass(slots=True)
class TrajectoryBuildResult:
    messages: list[dict[str, Any]]
    fingerprint: str
    warnings: list[str]
    source: str
    input_count: int
    output_count: int
    dropped_count: int
    estimated_chars: int


def build_agent_trajectory_messages(
    messages: list[dict[str, Any]],
    *,
    session_id: str,
    source: TrajectorySource,
    now_ms: int | None = None,
    max_messages: int = 80,
    max_message_chars: int = 8000,
    max_tool_result_chars: int = 6000,
    max_payload_chars: int = 60000,
    include_system: bool = False,
) -> TrajectoryBuildResult:
    """Convert Hermes message history into bounded EverOS agent messages."""
    base_now = int(now_ms if now_ms is not None else time.time() * 1000)
    warnings: list[str] = []
    output: list[dict[str, Any]] = []
    dropped_count = 0

    for input_index, raw in enumerate(messages):
        role = str(raw.get("role") or "").strip().lower()
        if role not in {"user", "assistant", "tool", "system"}:
            dropped_count += 1
            warnings.append(f"dropped unsupported role at index {input_index}: {role or '<empty>'}")
            continue
        if role == "system" and not include_system:
            dropped_count += 1
            continue
        if role == "tool" and not str(raw.get("tool_call_id") or "").strip():
            dropped_count += 1
            warnings.append(f"dropped tool message at index {input_index}: missing tool_call_id")
            continue

        tool_calls = _scrub_value(raw.get("tool_calls")) if role == "assistant" and raw.get("tool_calls") else None
        content = _content_to_text(raw.get("content"))
        if not content and role == "assistant" and tool_calls:
            content = "[Assistant requested tool calls]"
        content = _strip_context_blocks(_redact_text(content)).strip()
        limit = max_tool_result_chars if role == "tool" else max_message_chars
        content = _truncate(content, limit)
        if not content:
            dropped_count += 1
            warnings.append(f"dropped {role} message at index {input_index}: empty content")
            continue

        timestamp = _normalize_timestamp(raw.get("timestamp"), base_now + len(output))
        message: dict[str, Any] = {
            "role": role,
            "content": content,
            "timestamp": timestamp,
            "message_id": _message_id(
                session_id=session_id,
                input_index=input_index,
                role=role,
                tool_call_id=str(raw.get("tool_call_id") or ""),
                original_timestamp=raw.get("timestamp"),
                content=content,
                tool_calls=tool_calls,
            ),
            "source": source,
        }
        if role == "tool":
            message["tool_call_id"] = str(raw.get("tool_call_id")).strip()
        if tool_calls:
            message["tool_calls"] = tool_calls
        output.append(message)

    if max_messages > 0 and len(output) > max_messages:
        extra = len(output) - max_messages
        output = output[-max_messages:]
        dropped_count += extra
        warnings.append(f"dropped {extra} oldest messages due to max_messages")

    output, budget_dropped = _enforce_payload_budget(output, max_payload_chars)
    if budget_dropped:
        dropped_count += budget_dropped
        warnings.append(f"dropped {budget_dropped} oldest messages due to max_payload_chars")

    estimated_chars = _estimate_chars(output)
    return TrajectoryBuildResult(
        messages=output,
        fingerprint=_fingerprint(session_id, output),
        warnings=warnings,
        source=source,
        input_count=len(messages),
        output_count=len(output),
        dropped_count=dropped_count,
        estimated_chars=estimated_chars,
    )


def _content_to_text(value: Any) -> str:
    if value is None:
        return ""
    if isinstance(value, str):
        return value
    return json.dumps(value, ensure_ascii=False, sort_keys=True)


def _strip_context_blocks(text: str) -> str:
    return _CONTEXT_BLOCK_RE.sub("", text)


def _redact_text(text: str) -> str:
    redacted = text
    for pattern in _SECRET_PATTERNS:
        redacted = pattern.sub("[REDACTED]", redacted)
    return redacted


def _scrub_value(value: Any) -> Any:
    if isinstance(value, str):
        return _strip_context_blocks(_redact_text(value))
    if isinstance(value, list):
        return [_scrub_value(item) for item in value]
    if isinstance(value, dict):
        return {str(key): _scrub_value(val) for key, val in value.items()}
    return copy.deepcopy(value)


def _truncate(text: str, limit: int) -> str:
    if limit <= 0 or len(text) <= limit:
        return text
    marker = "[truncated]"
    return text[: max(0, limit - len(marker))] + marker


def _normalize_timestamp(value: Any, fallback_ms: int) -> int:
    if value is None or value == "":
        return fallback_ms
    if isinstance(value, (int, float)):
        number = float(value)
        return int(number * 1000) if number < 1_000_000_000_000 else int(number)
    if isinstance(value, str):
        stripped = value.strip()
        try:
            number = float(stripped)
        except ValueError:
            try:
                normalized = stripped.replace("Z", "+00:00")
                dt = datetime.fromisoformat(normalized)
                if dt.tzinfo is None:
                    dt = dt.replace(tzinfo=timezone.utc)
                return int(dt.timestamp() * 1000)
            except ValueError:
                return fallback_ms
        return int(number * 1000) if number < 1_000_000_000_000 else int(number)
    return fallback_ms


def _canonical_json(value: Any) -> str:
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":"), default=str)


def _hash_text(text: str) -> str:
    return hashlib.sha256(text.encode("utf-8")).hexdigest()


def _message_id(
    *,
    session_id: str,
    input_index: int,
    role: str,
    tool_call_id: str,
    original_timestamp: Any,
    content: str,
    tool_calls: Any,
) -> str:
    original_timestamp_part = "" if original_timestamp is None else str(original_timestamp)
    tool_calls_hash = _hash_text(_canonical_json(tool_calls)) if tool_calls else ""
    payload = "|".join(
        [
            session_id,
            str(input_index),
            role,
            tool_call_id,
            original_timestamp_part,
            _hash_text(content),
            tool_calls_hash,
        ]
    )
    return "eh_" + _hash_text(payload)[:32]


def _estimate_chars(messages: list[dict[str, Any]]) -> int:
    return sum(len(_canonical_json(message)) for message in messages)


def _enforce_payload_budget(messages: list[dict[str, Any]], max_payload_chars: int) -> tuple[list[dict[str, Any]], int]:
    if max_payload_chars <= 0 or _estimate_chars(messages) <= max_payload_chars:
        return messages, 0
    last_user_index = None
    for index, message in enumerate(messages):
        if message.get("role") == "user":
            last_user_index = index
    protected_start = last_user_index if last_user_index is not None else max(0, len(messages) - 1)
    protected = messages[protected_start:]
    prefix = messages[:protected_start]
    dropped = 0
    while prefix and _estimate_chars(prefix + protected) > max_payload_chars:
        prefix.pop(0)
        dropped += 1
    if _estimate_chars(prefix + protected) <= max_payload_chars:
        return prefix + protected, dropped
    return protected, dropped + len(prefix)


def _fingerprint(session_id: str, messages: list[dict[str, Any]]) -> str:
    normalized: list[dict[str, Any]] = []
    for message in messages:
        item = {key: value for key, value in message.items() if key not in {"message_id", "timestamp", "source"}}
        normalized.append(item)
    return _hash_text(_canonical_json({"session_id": session_id, "messages": normalized}))
