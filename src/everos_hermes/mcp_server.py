from __future__ import annotations

import json
import time
from typing import Any, Literal

from mcp.server.fastmcp import FastMCP

from .client import DEFAULT_BASE_URL, DEFAULT_MEMORY_TYPES, EverOSClient
from .env import get_env
from .formatting import format_search_context, pretty_json

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
) -> str:
    """Save one explicit text memory to EverOS.

    This is a convenience wrapper around POST /api/v1/memories. It stores the
    provided content as a user message for the target user_id, then optionally
    calls /api/v1/memories/flush so the memory becomes searchable immediately.
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
    if flush:
        client.flush_memories(user_id=uid, session_id=session_id, agent=False)
    data = result.get("data", {}) if isinstance(result, dict) else {}
    return pretty_json({"saved": True, "user_id": uid, "session_id": session_id, "status": data.get("status", ""), "task_id": data.get("task_id", "")})


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
) -> str:
    """Add one or more personal or agent-trajectory messages to EverOS.

    messages must follow EverOS v1 schema: role, timestamp (Unix ms), and content.
    For agent=True, EverOS /api/v1/memories/agent is used and role can also be tool.
    """
    uid = user_id or default_user_id()
    client = make_client()
    result = client.add_memories(user_id=uid, session_id=session_id, messages=messages, async_mode=async_mode, agent=agent)
    if flush:
        client.flush_memories(user_id=uid, session_id=session_id, agent=agent)
    return pretty_json(result)


@mcp.tool(
    name="everos_flush_memories",
    title="Flush EverOS Memories",
    annotations={"readOnlyHint": False, "destructiveHint": False, "idempotentHint": True, "openWorldHint": True},
)
async def everos_flush_memories(user_id: str | None = None, session_id: str | None = None, agent: bool = False) -> str:
    """Trigger EverOS boundary detection and memory extraction immediately."""
    uid = user_id or default_user_id()
    return pretty_json(make_client().flush_memories(user_id=uid, session_id=session_id, agent=agent))


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
    )
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
