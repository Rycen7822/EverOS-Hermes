from __future__ import annotations

import json
import time
from pathlib import Path
from typing import Any

from .client import DEFAULT_MEMORY_TYPES, EverOSClient, EverOSTimeoutError
from .schemas import normalize_scope, validate_messages

SEARCH_KEYS = (
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
)


def now_ms() -> int:
    return int(time.time() * 1000)


def success_envelope(*, workflow: str, status: str, **fields: Any) -> dict[str, Any]:
    payload: dict[str, Any] = {
        "ok": True,
        "workflow": workflow,
        "status": status,
        "retryable": False,
        "suggested_next_actions": [],
    }
    payload.update(fields)
    return payload


def error_envelope(*, workflow: str, error_code: str, message: str, retryable: bool = False, **fields: Any) -> dict[str, Any]:
    actions = list(fields.pop("suggested_next_actions", []))
    if not actions:
        actions = ["inspect the validation error and retry with corrected arguments"]
    payload: dict[str, Any] = {
        "ok": False,
        "workflow": workflow,
        "status": "error",
        "error_code": error_code,
        "message": message,
        "retryable": retryable,
        "suggested_next_actions": actions,
    }
    payload.update(fields)
    return payload


def save_result_payload(
    *,
    result: dict[str, Any],
    user_id: str,
    session_id: str | None,
    scope: str,
    flush_requested: bool,
    flush_result: dict[str, Any] | None = None,
    flush_error: EverOSTimeoutError | None = None,
) -> dict[str, Any]:
    data = result.get("data", {}) if isinstance(result, dict) else {}
    status = data.get("status", "") if isinstance(data, dict) else ""
    task_id = data.get("task_id", "") if isinstance(data, dict) else ""
    payload: dict[str, Any] = {
        "ok": True,
        "status": status or "queued",
        "saved": True,
        "message_queued": True,
        "extraction_requested": bool(task_id or status in {"queued", "processing", "success"} or flush_requested),
        "searchable": None,
        "scope": scope,
        "user_id": user_id,
        "session_id": session_id,
        "task_id": task_id,
    }
    if flush_result is not None:
        payload["flush"] = flush_result_payload(flush_result)
    elif flush_error is not None:
        payload["flush"] = timeout_payload("flush", flush_error)
    elif flush_requested:
        payload["flush"] = {"ok": False, "status": "missing", "error": "flush requested but no flush result was recorded"}
    else:
        payload["flush"] = {"ok": None, "status": "not_requested"}
    return payload


def flush_result_payload(response: dict[str, Any]) -> dict[str, Any]:
    data = response.get("data", {}) if isinstance(response, dict) else {}
    payload: dict[str, Any] = {"ok": True, "status": "success"}
    if isinstance(data, dict):
        for key in ("status", "request_id", "task_id", "message"):
            if data.get(key):
                payload[key] = data[key]
    if isinstance(response, dict) and response.get("status_code"):
        payload["status_code"] = response.get("status_code")
    return payload


def timeout_payload(operation: str, exc: EverOSTimeoutError) -> dict[str, Any]:
    return {
        "ok": False,
        "operation": operation,
        "status": "timeout",
        "error_code": "timeout",
        "message": str(exc),
        "retryable": bool(getattr(exc, "retryable", True)),
        "suggested_next_actions": list(getattr(exc, "suggested_next_actions", [])),
    }


def load_messages_from_file(file_path: str) -> list[dict[str, Any]]:
    path = Path(file_path).expanduser()
    text = path.read_text(encoding="utf-8")
    suffix = path.suffix.lower()
    if suffix == ".json":
        parsed = json.loads(text)
        if isinstance(parsed, dict):
            parsed = parsed.get("messages", parsed.get("data", []))
        if not isinstance(parsed, list):
            raise ValueError("JSON import file must be a list or an object with a messages list")
        return [_coerce_loaded_message(item) for item in parsed]
    if suffix in {".jsonl", ".ndjson"}:
        messages: list[dict[str, Any]] = []
        for line_no, line in enumerate(text.splitlines(), start=1):
            stripped = line.strip()
            if not stripped:
                continue
            parsed = json.loads(stripped)
            if not isinstance(parsed, dict):
                raise ValueError(f"JSONL line {line_no} is not an object")
            messages.append(_coerce_loaded_message(parsed))
        return messages
    chunks = [chunk.strip() for chunk in text.split("\n\n") if chunk.strip()]
    return [{"role": "user", "timestamp": now_ms(), "content": chunk} for chunk in chunks]


def _coerce_loaded_message(item: Any) -> dict[str, Any]:
    if isinstance(item, str):
        return {"role": "user", "timestamp": now_ms(), "content": item}
    if not isinstance(item, dict):
        raise ValueError("imported messages must be objects or strings")
    message = dict(item)
    message.setdefault("role", "user")
    message.setdefault("timestamp", now_ms())
    message["content"] = str(message.get("content") or "")
    return message


def normalize_import_messages(messages: list[dict[str, Any]] | None, file_path: str | None, *, default_role: str = "user") -> tuple[list[dict[str, Any]], list[str]]:
    loaded: list[dict[str, Any]] = []
    warnings: list[str] = []
    if file_path:
        loaded.extend(load_messages_from_file(file_path))
    if messages:
        loaded.extend(_coerce_loaded_message(message) for message in messages)
    normalized: list[dict[str, Any]] = []
    seen: set[str] = set()
    for index, message in enumerate(loaded):
        item = dict(message)
        item["role"] = str(item.get("role") or default_role)
        item.setdefault("timestamp", now_ms())
        item["content"] = str(item.get("content") or "").strip()
        timestamp = item.get("timestamp")
        if not isinstance(timestamp, int) or isinstance(timestamp, bool):
            warnings.append(f"messages[{index}].timestamp must be an integer epoch millisecond value")
        if not item["content"]:
            warnings.append(f"messages[{index}].content is empty")
        fingerprint = f"{item.get('role')}\0{item.get('content')}"
        if fingerprint in seen:
            warnings.append(f"messages[{index}] appears duplicate by role+content")
        seen.add(fingerprint)
        if item.get("role") == "tool" and not str(item.get("tool_call_id") or "").strip():
            warnings.append(f"messages[{index}].tool_call_id is required when role=tool")
        normalized.append(item)
    return normalized, warnings


def batch_items(items: list[dict[str, Any]], batch_size: int) -> list[list[dict[str, Any]]]:
    size = max(1, min(100, int(batch_size or 50)))
    return [items[index : index + size] for index in range(0, len(items), size)]


def message_metrics(messages: list[dict[str, Any]], batch_size: int) -> dict[str, Any]:
    batches = batch_items(messages, batch_size)
    content_lengths = [len(str(message.get("content") or "")) for message in messages]
    batch_payload_bytes = [_json_bytes({"messages": batch}) for batch in batches]
    return {
        "total_messages": len(messages),
        "batch_count": len(batches),
        "requested_batch_size": batch_size,
        "effective_batch_size": max(1, min(100, int(batch_size or 50))),
        "total_content_chars": sum(content_lengths),
        "max_content_chars": max(content_lengths, default=0),
        "estimated_payload_bytes": _json_bytes({"messages": messages}),
        "max_batch_payload_bytes": max(batch_payload_bytes, default=0),
    }


def _json_bytes(value: Any) -> int:
    return len(json.dumps(value, ensure_ascii=False).encode("utf-8"))


def _is_cloud_403(exc: Exception) -> bool:
    text = str(exc).lower()
    return "403" in text or "forbidden" in text


def count_hits(response: dict[str, Any]) -> int:
    data = response.get("data", response) if isinstance(response, dict) else {}
    return _count_hits_value(data)


def _count_hits_value(value: Any) -> int:
    if isinstance(value, list):
        return len(value)
    if not isinstance(value, dict):
        return 0
    total = 0
    for key, child in value.items():
        if key in SEARCH_KEYS and isinstance(child, list):
            total += len(child)
        elif key in SEARCH_KEYS and isinstance(child, dict):
            total += _count_hits_value(child)
    return total


def verify_session_ingest(
    *,
    client: EverOSClient,
    user_id: str,
    session_id: str | None,
    verification_queries: list[str],
    memory_types: list[str] | None = None,
    scope: str = "personal",
    top_k: int = 5,
    timeout: float | None = None,
) -> dict[str, Any]:
    resolved_scope = normalize_scope(scope)
    resolved_types = list(memory_types or DEFAULT_MEMORY_TYPES)
    queries: list[dict[str, Any]] = []
    for query in [q for q in verification_queries if str(q).strip()]:
        response = client.search_memories(
            query=str(query),
            user_id=user_id,
            session_id=session_id,
            method="hybrid",
            memory_types=resolved_types,
            top_k=top_k,
            include_original_data=False,
            include_vectors=False,
            timeout=timeout,
        )
        hit_count = count_hits(response)
        queries.append({
            "query": str(query),
            "status": "hit" if hit_count else "miss",
            "hit_count": hit_count,
            "response": response,
        })
    if not queries:
        status = "verification_skipped"
        verified = None
    elif all(item["hit_count"] > 0 for item in queries):
        status = "verified"
        verified = True
    elif any(item["hit_count"] > 0 for item in queries):
        status = "partially_verified"
        verified = False
    else:
        status = "not_yet_searchable"
        verified = False
    actions = [] if verified else ["wait for extraction and retry verification", "check user_id/session_id/scope and adjust verification queries"]
    return success_envelope(
        workflow="verify_session_ingest",
        status=status,
        verified=verified,
        scope=resolved_scope,
        user_id=user_id,
        session_id=session_id,
        memory_types=resolved_types,
        queries=queries,
        suggested_next_actions=actions,
    )


def save_and_verify(
    *,
    client: EverOSClient,
    content: str,
    user_id: str,
    session_id: str | None,
    scope: str = "personal",
    role: str | None = None,
    tool_call_id: str | None = None,
    flush: bool = True,
    flush_timeout: float | None = None,
    verification_query: str | None = None,
    verification_queries: list[str] | None = None,
    memory_types: list[str] | None = None,
    top_k: int = 5,
    timeout: float | None = None,
) -> dict[str, Any]:
    resolved_scope = normalize_scope(scope)
    resolved_role = role or ("assistant" if resolved_scope == "agent" else "user")
    message: dict[str, Any] = {"role": resolved_role, "timestamp": now_ms(), "content": content}
    if tool_call_id:
        message["tool_call_id"] = tool_call_id
    result = client.add_memories(user_id=user_id, session_id=session_id, messages=[message], async_mode=True, scope=resolved_scope)
    flush_result = None
    flush_error = None
    if flush:
        try:
            flush_result = client.flush_memories(user_id=user_id, session_id=session_id, scope=resolved_scope, timeout=flush_timeout)
        except EverOSTimeoutError as exc:
            flush_error = exc
    save_payload = save_result_payload(
        result=result,
        user_id=user_id,
        session_id=session_id,
        scope=resolved_scope,
        flush_requested=flush,
        flush_result=flush_result,
        flush_error=flush_error,
    )
    queries = list(verification_queries or [])
    if verification_query:
        queries.insert(0, verification_query)
    if not queries and content:
        queries = [content[:200]]
    verification = verify_session_ingest(
        client=client,
        user_id=user_id,
        session_id=session_id,
        verification_queries=queries,
        memory_types=memory_types,
        scope=resolved_scope,
        top_k=top_k,
        timeout=timeout,
    )
    status = verification["status"] if verification.get("verified") is not None else "queued"
    return success_envelope(
        workflow="save_and_verify",
        status=status,
        save=save_payload,
        verification=verification,
        suggested_next_actions=verification.get("suggested_next_actions", []),
    )


def import_and_verify(
    *,
    client: EverOSClient,
    user_id: str,
    session_id: str | None,
    messages: list[dict[str, Any]] | None = None,
    file_path: str | None = None,
    scope: str = "personal",
    dry_run: bool = False,
    batch_size: int = 50,
    flush: bool = True,
    flush_timeout: float | None = None,
    verification_queries: list[str] | None = None,
    memory_types: list[str] | None = None,
    top_k: int = 5,
    timeout: float | None = None,
    workflow: str = "import_and_verify",
) -> dict[str, Any]:
    resolved_scope = normalize_scope(scope)
    normalized, warnings = normalize_import_messages(messages, file_path, default_role=("assistant" if resolved_scope == "agent" else "user"))
    metrics = message_metrics(normalized, batch_size)
    try:
        validate_messages(normalized, resolved_scope)
    except ValueError as exc:
        message = str(exc)
        if message not in warnings:
            warnings.append(message)
    fatal_tokens = ("tool_call_id", "empty", "timestamp", "role", "message_id", "1..500")
    validation_warnings = [warning for warning in warnings if any(token in warning for token in fatal_tokens)]
    if dry_run:
        actions = ["fix warnings before importing"] if warnings else ["rerun with dry_run=false to import messages"]
        return success_envelope(
            workflow=workflow,
            status="dry_run",
            input_count=len(normalized),
            queued_count=0,
            failed_count=0,
            warnings=warnings,
            metrics=metrics,
            batches=[],
            verification={"status": "verification_skipped", "verified": None, "queries": []},
            suggested_next_actions=actions,
        )
    if validation_warnings:
        return error_envelope(
            workflow=workflow,
            error_code="validation_failed",
            message="import contains messages that cannot be safely submitted",
            input_count=len(normalized),
            queued_count=0,
            failed_count=len(normalized),
            warnings=warnings,
            metrics=metrics,
        )
    batches = batch_items(normalized, batch_size)
    batch_reports: list[dict[str, Any]] = []
    queued_count = 0
    failed_count = 0
    split_count = 0

    def submit_batch(batch: list[dict[str, Any]], *, batch_index: int, split_from: int | None = None) -> tuple[int, int, int]:
        try:
            result = client.add_memories(
                user_id=user_id,
                session_id=session_id,
                messages=batch,
                async_mode=True,
                scope=resolved_scope,
            )
            data = result.get("data", {}) if isinstance(result, dict) else {}
            batch_reports.append({
                "batch_index": batch_index,
                "split_from": split_from,
                "ok": True,
                "message_count": len(batch),
                "payload_bytes": _json_bytes({"messages": batch}),
                "status": data.get("status", "queued") if isinstance(data, dict) else "queued",
                "task_id": data.get("task_id", "") if isinstance(data, dict) else "",
                "response": result,
            })
            return len(batch), 0, 0
        except Exception as exc:  # keep importing independent batches if possible
            if _is_cloud_403(exc) and len(batch) > 1:
                mid = max(1, len(batch) // 2)
                batch_reports.append({
                    "batch_index": batch_index,
                    "split_from": split_from,
                    "ok": False,
                    "message_count": len(batch),
                    "payload_bytes": _json_bytes({"messages": batch}),
                    "error": str(exc),
                    "retryable": True,
                    "split": True,
                    "split_reason": "cloud_403",
                    "split_into": [mid, len(batch) - mid],
                })
                left_queued, left_failed, left_splits = submit_batch(batch[:mid], batch_index=batch_index, split_from=batch_index)
                right_queued, right_failed, right_splits = submit_batch(batch[mid:], batch_index=batch_index, split_from=batch_index)
                return left_queued + right_queued, left_failed + right_failed, 1 + left_splits + right_splits
            batch_reports.append({
                "batch_index": batch_index,
                "split_from": split_from,
                "ok": False,
                "message_count": len(batch),
                "payload_bytes": _json_bytes({"messages": batch}),
                "error": str(exc),
                "retryable": True,
            })
            return 0, len(batch), 0

    for index, batch in enumerate(batches):
        queued, failed, splits = submit_batch(batch, batch_index=index)
        queued_count += queued
        failed_count += failed
        split_count += splits
    flush_payload = {"ok": None, "status": "not_requested"}
    if flush and queued_count:
        try:
            flush_payload = flush_result_payload(client.flush_memories(user_id=user_id, session_id=session_id, scope=resolved_scope, timeout=flush_timeout))
        except EverOSTimeoutError as exc:
            flush_payload = timeout_payload("flush", exc)
    verification = verify_session_ingest(
        client=client,
        user_id=user_id,
        session_id=session_id,
        verification_queries=list(verification_queries or []),
        memory_types=memory_types,
        scope=resolved_scope,
        top_k=top_k,
        timeout=timeout,
    ) if verification_queries else {"status": "verification_skipped", "verified": None, "queries": []}
    if verification.get("verified") is True:
        status = "verified"
    elif verification.get("verified") is False and queued_count:
        status = verification.get("status", "not_yet_searchable")
    elif failed_count and queued_count:
        status = "partially_queued"
    elif failed_count:
        status = "failed"
    else:
        status = "queued"
    actions: list[str] = []
    if split_count:
        actions.append("Cloud 403 triggered adaptive batch splitting; keep batch_size small for long messages")
    if failed_count:
        actions.append("retry only failed batches using the batch report")
    if verification.get("verified") is False:
        actions.extend(["wait for extraction and rerun verify_session_ingest", "adjust verification queries if extraction consolidated memories"])
    return success_envelope(
        workflow=workflow,
        status=status,
        input_count=len(normalized),
        queued_count=queued_count,
        failed_count=failed_count,
        split_count=split_count,
        scope=resolved_scope,
        user_id=user_id,
        session_id=session_id,
        warnings=warnings,
        metrics=metrics,
        batches=batch_reports,
        flush=flush_payload,
        verification=verification,
        suggested_next_actions=actions,
    )
