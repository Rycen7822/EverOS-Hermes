from __future__ import annotations

import json
import time
from typing import Any, Literal, TypedDict

from mcp.server.fastmcp import FastMCP

from .agent_visibility import build_agent_visibility_report
from .client import DEFAULT_BASE_URL, DEFAULT_MEMORY_TYPES, EverOSClient, EverOSError, EverOSTimeoutError
from .env import get_env
from .flush_retry import flush_memories_with_retry
from .formatting import format_search_context, pretty_json, strip_vectors
from .schemas import GetMemoryType, MemoryScope, SearchMemoryType, delete_confirm_text, normalize_scope
from .workflows import import_and_verify, save_and_verify, verify_session_ingest

mcp = FastMCP("everos_mcp")

TOOL_NAMES = [
    "everos_save_memory",
    "everos_add_memories",
    "everos_flush_memories",
    "everos_search_memories",
    "everos_get_memories",
    "everos_delete_memories",
    "everos_get_task_status",
    "everos_get_settings",
    "everos_update_settings",
    "everos_batch_ingest",
    "everos_verify_session_ingest",
    "everos_save_and_verify",
    "everos_import_and_verify",
]

RetrievalMethod = Literal["keyword", "vector", "hybrid", "agentic"]
ResponseFormat = Literal["json", "markdown"]


class WorkflowOutput(TypedDict, total=False):
    ok: bool
    workflow: str
    status: str
    retryable: bool
    suggested_next_actions: list[str]


def make_client() -> EverOSClient:
    return EverOSClient(
        api_key=get_env("EVEROS_API_KEY", ""),
        base_url=get_env("EVEROS_BASE_URL", DEFAULT_BASE_URL),
        timeout=float(get_env("EVEROS_TIMEOUT", "10")),
    )

def default_user_id() -> str:
    return get_env("EVEROS_USER_ID", "") or "hermes_default"


def now_ms() -> int:
    return int(time.time() * 1000)


def _render(response: dict[str, Any], response_format: str = "json") -> str:
    if response_format == "markdown":
        formatted = format_search_context(response, max_items=20)
        return formatted or pretty_json(response)
    return pretty_json(response)


def _flush_result_payload(response: dict[str, Any], *, attempt_count: int | None = None) -> WorkflowOutput:
    data = response.get("data", {}) if isinstance(response, dict) else {}
    payload: dict[str, Any] = {"ok": True}
    if attempt_count is not None:
        payload["attempt_count"] = attempt_count
    if isinstance(data, dict):
        if data.get("status"):
            payload["status"] = data.get("status")
        if data.get("request_id"):
            payload["request_id"] = data.get("request_id")
        if data.get("task_id"):
            payload["task_id"] = data.get("task_id")
        if data.get("message"):
            payload["message"] = data.get("message")
    return payload


def _timeout_payload(operation: str, exc: EverOSTimeoutError) -> WorkflowOutput:
    return {
        "ok": False,
        "operation": operation,
        "error": str(exc),
        "retryable": bool(getattr(exc, "retryable", True)),
        "suggested_next_actions": list(getattr(exc, "suggested_next_actions", [])),
    }


def _save_result_payload(
    *,
    result: dict[str, Any],
    user_id: str,
    session_id: str | None,
    scope: str = "personal",
    flush_requested: bool,
    flush_result: dict[str, Any] | None = None,
    flush_error: EverOSTimeoutError | None = None,
) -> WorkflowOutput:
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
        payload["flush"] = _flush_result_payload(flush_result)
    elif flush_error is not None:
        payload["flush"] = _timeout_payload("flush", flush_error)
    elif flush_requested:
        payload["flush"] = {"ok": False, "error": "flush requested but no flush result was recorded"}
    else:
        payload["flush"] = {"ok": None, "status": "not_requested"}
    return payload


@mcp.tool(
    name="everos_save_memory",
    title="Save EverOS Memory",
    annotations={"readOnlyHint": False, "destructiveHint": False, "idempotentHint": False, "openWorldHint": True},
)
async def everos_save_memory(
    content: str,
    user_id: str | None = None,
    session_id: str | None = None,
    scope: MemoryScope = "personal",
    role: Literal["user", "assistant", "tool", "system"] | None = None,
    tool_call_id: str | None = None,
    flush: bool = True,
    async_mode: bool = True,
    flush_timeout: float | None = None,
) -> str:
    """Queue one explicit text memory message for EverOS extraction.

    This is a convenience wrapper around POST /api/v1/memories. It stores the
    provided content as a user message for the target user_id, then optionally
    calls /api/v1/memories/flush. `saved=true` means the message was accepted;
    inspect `flush`, `task_id`, and follow-up search results before assuming a
    structured profile/fact is already searchable.
    """
    resolved_scope = normalize_scope(scope)
    resolved_role = role or ("assistant" if resolved_scope == "agent" else "user")
    message: dict[str, Any] = {"role": resolved_role, "timestamp": now_ms(), "content": content}
    if tool_call_id:
        message["tool_call_id"] = tool_call_id
    uid = user_id or default_user_id()
    client = make_client()
    result = client.add_memories(
        user_id=uid,
        session_id=session_id,
        messages=[message],
        async_mode=async_mode,
        scope=resolved_scope,
    )
    flush_result = None
    flush_error = None
    if flush:
        try:
            flush_result, _attempt_count = flush_memories_with_retry(
                client,
                user_id=uid,
                session_id=session_id,
                scope=resolved_scope,
                timeout=flush_timeout,
            )
        except EverOSTimeoutError as exc:
            flush_error = exc
    payload = _save_result_payload(
        result=result,
        user_id=uid,
        session_id=session_id,
        scope=resolved_scope,
        flush_requested=flush,
        flush_result=flush_result,
        flush_error=flush_error,
    )
    if resolved_scope == "agent":
        payload["agent_visibility"] = build_agent_visibility_report(
            agent_raw_queued=True,
            agent_flush=payload.get("flush") if isinstance(payload.get("flush"), dict) else None,
            checks=[],
        )
    return pretty_json(payload)


@mcp.tool(
    name="everos_add_memories",
    title="Add EverOS Memory Messages",
    annotations={"readOnlyHint": False, "destructiveHint": False, "idempotentHint": False, "openWorldHint": True},
)
async def everos_add_memories(
    messages: list[dict[str, Any]],
    user_id: str | None = None,
    session_id: str | None = None,
    scope: MemoryScope = "personal",
    async_mode: bool = True,
    agent: bool | None = None,
    flush: bool = False,
    flush_timeout: float | None = None,
) -> str:
    """Add one or more personal or agent-trajectory messages to EverOS.

    Messages must follow EverOS v1 schema: role, timestamp (Unix ms), and content.
    Optional message_id is preserved when provided for idempotent retries.
    For agent=True, EverOS /api/v1/memories/agent is used and role can also be tool.
    """
    resolved_scope = normalize_scope(scope, agent)
    uid = user_id or default_user_id()
    client = make_client()
    result = client.add_memories(user_id=uid, session_id=session_id, messages=messages, async_mode=async_mode, scope=resolved_scope)
    if flush:
        try:
            flush_result, attempt_count = flush_memories_with_retry(
                client,
                user_id=uid,
                session_id=session_id,
                scope=resolved_scope,
                timeout=flush_timeout,
            )
            payload: dict[str, Any] = {"ok": True, "add": result, "flush": _flush_result_payload(flush_result, attempt_count=attempt_count)}
        except EverOSTimeoutError as exc:
            payload = {"ok": True, "add": result, "flush": _timeout_payload("flush", exc)}
    elif resolved_scope == "agent":
        payload = {"ok": True, "add": result}
    else:
        return pretty_json(result)
    if resolved_scope == "agent":
        payload["agent_visibility"] = build_agent_visibility_report(
            agent_raw_queued=True,
            agent_flush=payload.get("flush") if isinstance(payload.get("flush"), dict) else None,
            checks=[],
        )
    return pretty_json(payload)


@mcp.tool(
    name="everos_flush_memories",
    title="Flush EverOS Memories",
    annotations={"readOnlyHint": False, "destructiveHint": False, "idempotentHint": True, "openWorldHint": True},
)
async def everos_flush_memories(
    user_id: str | None = None,
    session_id: str | None = None,
    scope: MemoryScope = "personal",
    agent: bool | None = None,
    timeout: float | None = None,
) -> str:
    """Trigger EverOS boundary detection and memory extraction immediately.

    If the client times out, the response is a retryable structured error;
    search/status checks should be attempted before issuing another flush.
    """
    resolved_scope = normalize_scope(scope, agent)
    uid = user_id or default_user_id()
    client = make_client()
    try:
        response, attempt_count = flush_memories_with_retry(
            client,
            user_id=uid,
            session_id=session_id,
            scope=resolved_scope,
            timeout=timeout,
        )
    except EverOSTimeoutError as exc:
        return pretty_json(_timeout_payload("flush", exc))
    if resolved_scope == "agent":
        flush_payload = _flush_result_payload(response, attempt_count=attempt_count)
        return pretty_json(
            {
                "flush": flush_payload,
                "agent_visibility": build_agent_visibility_report(agent_raw_queued=None, agent_flush=flush_payload, checks=[]),
            }
        )
    return pretty_json(response)


@mcp.tool(
    name="everos_search_memories",
    title="Search EverOS Memories",
    annotations={"readOnlyHint": True, "destructiveHint": False, "idempotentHint": True, "openWorldHint": True},
)
async def everos_search_memories(
    query: str,
    user_id: str | None = None,
    session_id: str | None = None,
    filters: dict[str, Any] | None = None,
    method: RetrievalMethod = "hybrid",
    top_k: int = 5,
    memory_types: list[SearchMemoryType] | None = None,
    radius: float | None = None,
    include_original_data: bool = False,
    include_vectors: bool = False,
    response_format: ResponseFormat = "json",
    timeout: float | None = None,
    fallback_to_hybrid: bool = True,
) -> str:
    """Search EverOS memory using keyword, vector, hybrid, or agentic retrieval.

    Defaults follow EverOS guidance: method=hybrid, memory_types=[episodic_memory, profile], top_k=5.
    Use method=agentic only for complex multi-part queries because it is slower and more expensive.
    """
    uid = user_id or default_user_id()
    resolved_types = list(memory_types or DEFAULT_MEMORY_TYPES)
    client = make_client()
    fallback_used = False
    try:
        response = client.search_memories(
            query=query,
            user_id=uid,
            session_id=session_id,
            filters=filters,
            method=method,
            memory_types=resolved_types,
            top_k=top_k,
            radius=radius,
            include_original_data=include_original_data,
            include_vectors=include_vectors,
            timeout=timeout,
        )
    except EverOSTimeoutError as exc:
        if method == "agentic" and fallback_to_hybrid:
            response = client.search_memories(
                query=query,
                user_id=uid,
                session_id=session_id,
                filters=filters,
                method="hybrid",
                memory_types=resolved_types,
                top_k=top_k,
                radius=radius,
                include_original_data=include_original_data,
                include_vectors=include_vectors,
                timeout=timeout,
            )
            response["fallback_used"] = True
            response["fallback_reason"] = str(exc)
            fallback_used = True
        else:
            return pretty_json(_timeout_payload("search", exc))
    if not include_vectors:
        response = strip_vectors(response)
    if fallback_used and isinstance(response, dict):
        response["fallback_used"] = True
    return _render(response, response_format)


@mcp.tool(
    name="everos_get_memories",
    title="Get EverOS Memories",
    annotations={"readOnlyHint": True, "destructiveHint": False, "idempotentHint": True, "openWorldHint": True},
)
async def everos_get_memories(
    user_id: str | None = None,
    session_id: str | None = None,
    filters: dict[str, Any] | None = None,
    memory_type: GetMemoryType = "episodic_memory",
    page: int = 1,
    page_size: int = 20,
    rank_by: str = "timestamp",
    rank_order: Literal["asc", "desc"] = "desc",
    response_format: ResponseFormat = "json",
) -> str:
    """Retrieve structured EverOS memories by memory_type with pagination."""
    uid = user_id or default_user_id()
    response = make_client().get_memories(
        user_id=uid,
        session_id=session_id,
        filters=filters,
        memory_type=memory_type,
        page=page,
        page_size=page_size,
        rank_by=rank_by,
        rank_order=rank_order,
    )
    return _render(response, response_format)


@mcp.tool(
    name="everos_delete_memories",
    title="Delete EverOS Memories",
    annotations={"readOnlyHint": False, "destructiveHint": True, "idempotentHint": True, "openWorldHint": True},
)
async def everos_delete_memories(
    memory_id: str | None = None,
    user_id: str | None = None,
    session_id: str | None = None,
    confirm: bool = False,
    confirm_scope_text: str | None = None,
) -> str:
    """Delete EverOS memory by exact memory_id, or batch-delete by user/session when explicitly confirmed."""
    if not confirm:
        return pretty_json({"error": "confirm=true is required before deleting EverOS memories"})
    if memory_id and (user_id or session_id):
        return pretty_json({"error": "single delete by memory_id cannot include user_id or session_id"})
    if not memory_id:
        if not user_id:
            return pretty_json({"error": "batch delete requires explicit user_id; default user_id is intentionally not used"})
        expected = delete_confirm_text(user_id, session_id)
        if confirm_scope_text != expected:
            return pretty_json({"error": f"confirm_scope_text must exactly match {expected!r}"})
    return pretty_json(make_client().delete_memories(memory_id=memory_id, user_id=user_id, session_id=session_id))


@mcp.tool(
    name="everos_get_task_status",
    title="Get EverOS Task Status",
    annotations={"readOnlyHint": True, "destructiveHint": False, "idempotentHint": True, "openWorldHint": True},
)
async def everos_get_task_status(task_id: str) -> str:
    """Check an asynchronous EverOS extraction task status."""
    return pretty_json(make_client().get_task_status(task_id))


@mcp.tool(
    name="everos_get_settings",
    title="Get EverOS Settings",
    annotations={"readOnlyHint": True, "destructiveHint": False, "idempotentHint": True, "openWorldHint": True},
)
async def everos_get_settings() -> str:
    """Get current EverOS memory-space settings."""
    return pretty_json(make_client().get_settings())


@mcp.tool(
    name="everos_update_settings",
    title="Update EverOS Settings",
    annotations={"readOnlyHint": False, "destructiveHint": False, "idempotentHint": True, "openWorldHint": True},
)
async def everos_update_settings(settings: dict[str, Any], strict: bool = True, return_diff: bool = True) -> str:
    """Update EverOS memory-space settings. Only supplied fields are changed."""
    return pretty_json(make_client().update_settings(settings, strict=strict, return_diff=return_diff))


@mcp.tool(
    name="everos_verify_session_ingest",
    title="Verify EverOS Session Ingest",
    annotations={"readOnlyHint": True, "destructiveHint": False, "idempotentHint": True, "openWorldHint": True},
)
async def everos_verify_session_ingest(
    verification_queries: list[str],
    user_id: str | None = None,
    session_id: str | None = None,
    scope: MemoryScope = "personal",
    memory_types: list[SearchMemoryType] | None = None,
    top_k: int = 5,
    timeout: float | None = None,
) -> WorkflowOutput:
    """Verify that an existing user/session is searchable by running read-only sample queries."""
    return verify_session_ingest(
        client=make_client(),
        user_id=user_id or default_user_id(),
        session_id=session_id,
        scope=scope,
        verification_queries=verification_queries,
        memory_types=memory_types,
        top_k=top_k,
        timeout=timeout,
    )


@mcp.tool(
    name="everos_save_and_verify",
    title="Save and Verify EverOS Memory",
    annotations={"readOnlyHint": False, "destructiveHint": False, "idempotentHint": False, "openWorldHint": True},
)
async def everos_save_and_verify(
    content: str,
    verification_query: str | None = None,
    verification_queries: list[str] | None = None,
    user_id: str | None = None,
    session_id: str | None = None,
    scope: MemoryScope = "personal",
    role: Literal["user", "assistant", "tool", "system"] | None = None,
    tool_call_id: str | None = None,
    flush: bool = True,
    flush_timeout: float | None = None,
    memory_types: list[SearchMemoryType] | None = None,
    top_k: int = 5,
    timeout: float | None = None,
) -> WorkflowOutput:
    """Queue one memory message, optionally flush, then verify searchability with sample queries."""
    return save_and_verify(
        client=make_client(),
        content=content,
        user_id=user_id or default_user_id(),
        session_id=session_id,
        scope=scope,
        role=role,
        tool_call_id=tool_call_id,
        flush=flush,
        flush_timeout=flush_timeout,
        verification_query=verification_query,
        verification_queries=verification_queries,
        memory_types=memory_types,
        top_k=top_k,
        timeout=timeout,
    )


@mcp.tool(
    name="everos_import_and_verify",
    title="Import and Verify EverOS Memories",
    annotations={"readOnlyHint": False, "destructiveHint": False, "idempotentHint": False, "openWorldHint": True},
)
async def everos_import_and_verify(
    messages: list[dict[str, Any]] | None = None,
    file_path: str | None = None,
    verification_queries: list[str] | None = None,
    user_id: str | None = None,
    session_id: str | None = None,
    scope: MemoryScope = "personal",
    dry_run: bool = False,
    batch_size: int = 50,
    flush: bool = True,
    flush_timeout: float | None = None,
    memory_types: list[SearchMemoryType] | None = None,
    top_k: int = 5,
    timeout: float | None = None,
) -> WorkflowOutput:
    """Batch-import messages or a local file, then flush/poll-compatible verify with sample queries."""
    return import_and_verify(
        client=make_client(),
        user_id=user_id or default_user_id(),
        session_id=session_id,
        messages=messages,
        file_path=file_path,
        scope=scope,
        dry_run=dry_run,
        batch_size=batch_size,
        flush=flush,
        flush_timeout=flush_timeout,
        verification_queries=verification_queries,
        memory_types=memory_types,
        top_k=top_k,
        timeout=timeout,
        workflow="import_and_verify",
    )


@mcp.tool(
    name="everos_batch_ingest",
    title="Batch Ingest EverOS Memories",
    annotations={"readOnlyHint": False, "destructiveHint": False, "idempotentHint": False, "openWorldHint": True},
)
async def everos_batch_ingest(
    messages: list[dict[str, Any]] | None = None,
    file_path: str | None = None,
    verification_queries: list[str] | None = None,
    user_id: str | None = None,
    session_id: str | None = None,
    scope: MemoryScope = "personal",
    dry_run: bool = False,
    batch_size: int = 50,
    flush: bool = True,
    flush_timeout: float | None = None,
    memory_types: list[SearchMemoryType] | None = None,
    top_k: int = 5,
    timeout: float | None = None,
) -> WorkflowOutput:
    """Dry-run or execute batched EverOS ingest with optional flush and verification report."""
    return import_and_verify(
        client=make_client(),
        user_id=user_id or default_user_id(),
        session_id=session_id,
        messages=messages,
        file_path=file_path,
        scope=scope,
        dry_run=dry_run,
        batch_size=batch_size,
        flush=flush,
        flush_timeout=flush_timeout,
        verification_queries=verification_queries,
        memory_types=memory_types,
        top_k=top_k,
        timeout=timeout,
        workflow="batch_ingest",
    )


def main() -> None:
    mcp.run()


if __name__ == "__main__":
    main()
