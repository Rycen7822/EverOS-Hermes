from __future__ import annotations

import hashlib
import json
import time
from dataclasses import dataclass
from datetime import datetime, timezone
from typing import Any, Literal

from .redaction import redact_text, scrub_value, strip_context_blocks


TrajectorySource = Literal["session_end", "pre_compress", "delegation", "sync_turn"]


@dataclass(slots=True)
class TrajectoryBuildResult:
    messages: list[dict[str, Any]]
    fingerprint: str


@dataclass(slots=True)
class TrajectoryBuildOptions:
    session_id: str
    source: TrajectorySource
    now_ms: int | None = None
    max_messages: int = 80
    max_message_chars: int = 8000
    max_tool_result_chars: int = 6000
    max_payload_chars: int = 60000
    include_system: bool = False


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
    return build_agent_trajectory_messages_with_options(
        messages,
        TrajectoryBuildOptions(
            session_id=session_id,
            source=source,
            now_ms=now_ms,
            max_messages=max_messages,
            max_message_chars=max_message_chars,
            max_tool_result_chars=max_tool_result_chars,
            max_payload_chars=max_payload_chars,
            include_system=include_system,
        ),
    )


def build_agent_trajectory_messages_with_options(
    messages: list[dict[str, Any]],
    options: TrajectoryBuildOptions,
) -> TrajectoryBuildResult:
    """Convert Hermes message history into bounded EverOS agent messages."""
    base_now = int(options.now_ms if options.now_ms is not None else time.time() * 1000)
    output: list[dict[str, Any]] = []

    for input_index, raw in enumerate(messages):
        role = str(raw.get("role") or "").strip().lower()
        if role not in {"user", "assistant", "tool", "system"}:
            continue
        if role == "system" and not options.include_system:
            continue
        if role == "tool" and not str(raw.get("tool_call_id") or "").strip():
            continue

        tool_calls = scrub_value(raw.get("tool_calls")) if role == "assistant" and raw.get("tool_calls") else None
        content = _content_to_text(raw.get("content"))
        if not content and role == "assistant" and tool_calls:
            content = "[Assistant requested tool calls]"
        content = strip_context_blocks(redact_text(content)).strip()
        limit = options.max_tool_result_chars if role == "tool" else options.max_message_chars
        content = _truncate(content, limit)
        if not content:
            continue

        timestamp = _normalize_timestamp(raw.get("timestamp"), base_now + len(output))
        message: dict[str, Any] = {
            "role": role,
            "content": content,
            "timestamp": timestamp,
            "message_id": _message_id(
                session_id=options.session_id,
                input_index=input_index,
                role=role,
                tool_call_id=str(raw.get("tool_call_id") or ""),
                original_timestamp=raw.get("timestamp"),
                content=content,
                tool_calls=tool_calls,
            ),
            "source": options.source,
        }
        if role == "tool":
            message["tool_call_id"] = str(raw.get("tool_call_id")).strip()
        if tool_calls:
            message["tool_calls"] = tool_calls
        output.append(message)

    if options.max_messages > 0 and len(output) > options.max_messages:
        output = output[-options.max_messages:]

    output = _enforce_payload_budget(output, options.max_payload_chars)
    return TrajectoryBuildResult(messages=output, fingerprint=_fingerprint(options.session_id, output))


def _content_to_text(value: Any) -> str:
    if value is None:
        return ""
    if isinstance(value, str):
        return value
    return json.dumps(value, ensure_ascii=False, sort_keys=True)


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


def _enforce_payload_budget(messages: list[dict[str, Any]], max_payload_chars: int) -> list[dict[str, Any]]:
    if max_payload_chars <= 0 or _estimate_chars(messages) <= max_payload_chars:
        return messages
    last_user_index = None
    for index, message in enumerate(messages):
        if message.get("role") == "user":
            last_user_index = index
    protected_start = last_user_index if last_user_index is not None else max(0, len(messages) - 1)
    protected = messages[protected_start:]
    prefix = messages[:protected_start]
    while prefix and _estimate_chars(prefix + protected) > max_payload_chars:
        prefix.pop(0)
    if _estimate_chars(prefix + protected) <= max_payload_chars:
        return prefix + protected
    return protected


def _fingerprint(session_id: str, messages: list[dict[str, Any]]) -> str:
    normalized: list[dict[str, Any]] = []
    for message in messages:
        item = {key: value for key, value in message.items() if key not in {"message_id", "timestamp", "source"}}
        normalized.append(item)
    return _hash_text(_canonical_json({"session_id": session_id, "messages": normalized}))
