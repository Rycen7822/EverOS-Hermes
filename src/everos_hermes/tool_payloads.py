from __future__ import annotations

from typing import Any

from .client import EverOSTimeoutError
from .redaction import error_payload, sanitized_error_message


def timeout_payload(operation: str, exc: EverOSTimeoutError) -> dict[str, Any]:
    return {
        "ok": False,
        "operation": operation,
        "error": sanitized_error_message(exc),
        "retryable": bool(getattr(exc, "retryable", True)),
        "suggested_next_actions": list(getattr(exc, "suggested_next_actions", [])),
    }


def flush_result_payload(response: dict[str, Any], *, attempt_count: int | None = None) -> dict[str, Any]:
    data = response.get("data", {}) if isinstance(response, dict) else {}
    payload: dict[str, Any] = {"ok": True}
    if attempt_count is not None:
        payload["attempt_count"] = attempt_count
    if isinstance(data, dict):
        for key in ("status", "request_id", "task_id", "message"):
            if data.get(key):
                payload[key] = data[key]
    return payload


def save_result_payload(
    *,
    result: dict[str, Any],
    user_id: str,
    session_id: str | None,
    scope: str = "personal",
    flush_requested: bool,
    flush_result: dict[str, Any] | None = None,
    flush_error: Exception | None = None,
) -> dict[str, Any]:
    data = result.get("data", {}) if isinstance(result, dict) else {}
    status = data.get("status", "") if isinstance(data, dict) else ""
    task_id = data.get("task_id", "") if isinstance(data, dict) else ""
    payload: dict[str, Any] = {
        "saved": True,
        "message_queued": True,
        "extraction_requested": bool(task_id or status in {"queued", "processing", "success"} or flush_requested),
        "searchable": None,
        "scope": scope,
        "user_id": user_id,
        "session_id": session_id,
        "status": status,
        "task_id": task_id,
    }
    if flush_result is not None:
        payload["flush"] = flush_result_payload(flush_result)
    elif isinstance(flush_error, EverOSTimeoutError):
        payload["flush"] = timeout_payload("flush", flush_error)
    elif flush_error is not None:
        payload["flush"] = error_payload("flush", flush_error)
    elif flush_requested:
        payload["flush"] = {"ok": False, "error": "flush requested but no flush result was recorded"}
    else:
        payload["flush"] = {"ok": None, "status": "not_requested"}
    return payload
