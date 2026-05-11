from __future__ import annotations

import json
import time
from typing import Any, Literal

from mcp.server.fastmcp import FastMCP

from .client import DEFAULT_BASE_URL, DEFAULT_MEMORY_TYPES, EverOSClient, EverOSError, EverOSTimeoutError
from .env import get_env
from .formatting import format_search_context, pretty_json, strip_vectors

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
]

RetrievalMethod = Literal["keyword", "vector", "hybrid", "agentic"]
ResponseFormat = Literal["json", "markdown"]


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


def _flush_result_payload(response: dict[str, Any]) -> dict[str, Any]:
    data = response.get("data", {}) if isinstance(response, dict) else {}
    payload: dict[str, Any] = {"ok": True}
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


def _timeout_payload(operation: str, exc: EverOSTimeoutError) -> dict[str, Any]:
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
    flush_requested: bool,
    flush_result: dict[str, Any] | None = None,
    flush_error: EverOSTimeoutError | None = None,
) -> dict[str, Any]:
    data = result.get("data", {}) if isinstance(result, dict) else {}
    status = data.get("status", "") if isinstance(data, dict) else ""
    task_id = data.get("task_id", "") if isinstance(data, dict) else ""
    payload: dict[str, Any] = {
        "saved": True,
        "message_queued": True,
        "extraction_requested": bool(task_id or status in {"queued", "processing", "success"} or flush_requested),
        "searchable": None,
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
    uid = user_id or default_user_id()
    client = make_client()
    result = client.add_memories(
        user_id=uid,
        session_id=session_id,
        messages=[{"role": "user", "timestamp": now_ms(), "content": content}],
        async_mode=async_mode,
        agent=False,
    )
    flush_result = None
    flush_error = None
    if flush:
        try:
            flush_result = client.flush_memories(user_id=uid, session_id=session_id, agent=False, timeout=flush_timeout)
        except EverOSTimeoutError as exc:
            flush_error = exc
    return pretty_json(
        _save_result_payload(
            result=result,
            user_id=uid,
            session_id=session_id,
            flush_requested=flush,
            flush_result=flush_result,
            flush_error=flush_error,
        )
    )


@mcp.tool(
    name="everos_add_memories",
    title="Add EverOS Memory Messages",
    annotations={"readOnlyHint": False, "destructiveHint": False, "idempotentHint": False, "openWorldHint": True},
)
async def everos_add_memories(
    messages: list[dict[str, Any]],
    user_id: str | None = None,
    session_id: str | None = None,
    async_mode: bool = True,
    agent: bool = False,
    flush: bool = False,
    flush_timeout: float | None = None,
) -> str:
    """Add one or more personal or agent-trajectory messages to EverOS.

    messages must follow EverOS v1 schema: role, timestamp (Unix ms), and content.
    For agent=True, EverOS /api/v1/memories/agent is used and role can also be tool.
    """
    uid = user_id or default_user_id()
    client = make_client()
    result = client.add_memories(user_id=uid, session_id=session_id, messages=messages, async_mode=async_mode, agent=agent)
    if flush:
        try:
            flush_result = client.flush_memories(user_id=uid, session_id=session_id, agent=agent, timeout=flush_timeout)
            return pretty_json({"ok": True, "add": result, "flush": _flush_result_payload(flush_result)})
        except EverOSTimeoutError as exc:
            return pretty_json({"ok": True, "add": result, "flush": _timeout_payload("flush", exc)})
    return pretty_json(result)


@mcp.tool(
    name="everos_flush_memories",
    title="Flush EverOS Memories",
    annotations={"readOnlyHint": False, "destructiveHint": False, "idempotentHint": True, "openWorldHint": True},
)
async def everos_flush_memories(user_id: str | None = None, session_id: str | None = None, agent: bool = False, timeout: float | None = None) -> str:
    """Trigger EverOS boundary detection and memory extraction immediately.

    If the client times out, the response is a retryable structured error;
    search/status checks should be attempted before issuing another flush.
    """
    uid = user_id or default_user_id()
    try:
        return pretty_json(make_client().flush_memories(user_id=uid, session_id=session_id, agent=agent, timeout=timeout))
    except EverOSTimeoutError as exc:
        return pretty_json(_timeout_payload("flush", exc))


@mcp.tool(
    name="everos_search_memories",
    title="Search EverOS Memories",
    annotations={"readOnlyHint": True, "destructiveHint": False, "idempotentHint": True, "openWorldHint": True},
)
async def everos_search_memories(
    query: str,
    user_id: str | None = None,
    session_id: str | None = None,
    method: RetrievalMethod = "hybrid",
    top_k: int = 5,
    memory_types: list[str] | None = None,
    include_original_data: bool = False,
    include_vectors: bool = False,
    response_format: ResponseFormat = "json",
) -> str:
    """Search EverOS memory using keyword, vector, hybrid, or agentic retrieval.

    Defaults follow EverOS guidance: method=hybrid, memory_types=[episodic_memory, profile], top_k=5.
    Use method=agentic only for complex multi-part queries because it is slower and more expensive.
    """
    uid = user_id or default_user_id()
    resolved_types = list(memory_types or DEFAULT_MEMORY_TYPES)
    response = make_client().search_memories(
        query=query,
        user_id=uid,
        session_id=session_id,
        method=method,
        memory_types=resolved_types,
        top_k=top_k,
        include_original_data=include_original_data,
        include_vectors=include_vectors,
    )
    if not include_vectors:
        response = strip_vectors(response)
    return _render(response, response_format)


@mcp.tool(
    name="everos_get_memories",
    title="Get EverOS Memories",
    annotations={"readOnlyHint": True, "destructiveHint": False, "idempotentHint": True, "openWorldHint": True},
)
async def everos_get_memories(
    user_id: str | None = None,
    session_id: str | None = None,
    memory_type: Literal["episodic_memory", "profile", "agent_case", "agent_skill"] = "episodic_memory",
    page: int = 1,
    page_size: int = 20,
    response_format: ResponseFormat = "json",
) -> str:
    """Retrieve structured EverOS memories by memory_type with pagination."""
    uid = user_id or default_user_id()
    response = make_client().get_memories(user_id=uid, session_id=session_id, memory_type=memory_type, page=page, page_size=page_size)
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
) -> str:
    """Delete EverOS memory by exact memory_id, or batch-delete by user/session when explicitly confirmed."""
    if not confirm:
        return pretty_json({"error": "confirm=true is required before deleting EverOS memories"})
    if not memory_id and not (user_id or default_user_id()):
        return pretty_json({"error": "memory_id or user_id is required"})
    uid = user_id or (None if memory_id else default_user_id())
    return pretty_json(make_client().delete_memories(memory_id=memory_id, user_id=uid, session_id=session_id))


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
async def everos_update_settings(settings: dict[str, Any]) -> str:
    """Update EverOS memory-space settings. Only supplied fields are changed."""
    return pretty_json(make_client().update_settings(settings))


def main() -> None:
    mcp.run()


if __name__ == "__main__":
    main()
