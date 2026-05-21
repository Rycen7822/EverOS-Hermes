from __future__ import annotations

from copy import deepcopy
from typing import Any, Literal
from zoneinfo import ZoneInfo, ZoneInfoNotFoundError

RetrievalMethod = Literal["keyword", "vector", "hybrid", "agentic"]
SearchMemoryType = Literal["episodic_memory", "profile", "raw_message", "agent_memory"]
GetMemoryType = Literal["episodic_memory", "profile", "agent_case", "agent_skill"]
RankOrder = Literal["asc", "desc"]
MemoryScope = Literal["personal", "agent"]

RETRIEVAL_METHODS = {"keyword", "vector", "hybrid", "agentic"}
SEARCH_MEMORY_TYPES = {"episodic_memory", "profile", "raw_message", "agent_memory"}
GET_MEMORY_TYPES = {"episodic_memory", "profile", "agent_case", "agent_skill"}
RANK_ORDERS = {"asc", "desc"}
ALLOWED_RANK_BY = {"timestamp", "created_at", "updated_at"}
ALLOWED_SETTINGS_FIELDS = {"timezone", "llm_custom_setting"}
_PERSONAL_ROLES = {"user", "assistant", "system"}
_AGENT_ROLES = {"user", "assistant", "tool", "system"}
_FILTER_FIELDS = {"user_id", "session_id", "timestamp", "AND", "OR"}
_FILTER_OPERATORS = {"eq", "gt", "gte", "lt", "lte"}


def normalize_scope(scope: str | None = None, agent: bool | None = None) -> MemoryScope:
    resolved = (scope or ("agent" if agent else "personal")).strip().lower()
    if resolved not in {"personal", "agent"}:
        raise ValueError("scope must be 'personal' or 'agent'")
    if agent is not None and scope is not None and bool(agent) != (resolved == "agent"):
        raise ValueError("scope conflicts with backward-compatible agent alias")
    return resolved  # type: ignore[return-value]


def validate_messages(messages: list[dict[str, Any]], scope: str) -> None:
    if not isinstance(messages, list) or not (1 <= len(messages) <= 500):
        raise ValueError("messages must contain 1..500 items")
    allowed_roles = _AGENT_ROLES if scope == "agent" else _PERSONAL_ROLES
    for index, message in enumerate(messages):
        if not isinstance(message, dict):
            raise ValueError(f"messages[{index}] must be an object")
        role = message.get("role")
        if role not in allowed_roles:
            raise ValueError(f"messages[{index}].role must be one of {sorted(allowed_roles)} for {scope} scope")
        tool_call_id = message.get("tool_call_id")
        if role == "tool" and (not isinstance(tool_call_id, str) or not tool_call_id.strip()):
            raise ValueError(f"messages[{index}].tool_call_id is required when role='tool'")
        message_id = message.get("message_id")
        if message_id is not None and (not isinstance(message_id, str) or not message_id.strip()):
            raise ValueError(f"messages[{index}].message_id must be a non-empty string when provided")
        content = message.get("content")
        if not isinstance(content, str) or not content.strip():
            raise ValueError(f"messages[{index}].content must be a non-empty string")
        timestamp = message.get("timestamp")
        if not isinstance(timestamp, int) or isinstance(timestamp, bool):
            raise ValueError(f"messages[{index}].timestamp must be an integer epoch millisecond value")


def validate_search_params(method: str, memory_types: list[str] | None, top_k: int, radius: float | None) -> None:
    normalized_method = method.strip().lower()
    if normalized_method not in RETRIEVAL_METHODS:
        raise ValueError(f"method must be one of {sorted(RETRIEVAL_METHODS)}")
    if not isinstance(top_k, int) or isinstance(top_k, bool) or top_k < -1 or top_k > 100:
        raise ValueError("top_k must be an integer in -1..100")
    if radius is not None:
        if normalized_method == "keyword":
            raise ValueError("radius is not supported for keyword retrieval")
        if not isinstance(radius, (int, float)) or isinstance(radius, bool) or radius < 0 or radius > 1:
            raise ValueError("radius must be between 0 and 1")
    if memory_types is not None:
        invalid = [item for item in memory_types if item not in SEARCH_MEMORY_TYPES]
        if invalid:
            raise ValueError(f"memory_types for search may only contain {sorted(SEARCH_MEMORY_TYPES)}; invalid: {invalid}")


def validate_get_params(memory_type: str, page: int, page_size: int, rank_by: str, rank_order: str) -> None:
    if memory_type not in GET_MEMORY_TYPES:
        raise ValueError(f"memory_type for get must be one of {sorted(GET_MEMORY_TYPES)}")
    if not isinstance(page, int) or isinstance(page, bool) or page < 1:
        raise ValueError("page must be >= 1")
    if not isinstance(page_size, int) or isinstance(page_size, bool) or page_size < 1 or page_size > 100:
        raise ValueError("page_size must be in 1..100")
    if rank_by not in ALLOWED_RANK_BY:
        raise ValueError(f"rank_by must be one of {sorted(ALLOWED_RANK_BY)}")
    if normalize_rank_order(rank_order) not in RANK_ORDERS:
        raise ValueError("rank_order must be 'asc' or 'desc'")


def normalize_rank_order(rank_order: str) -> RankOrder:
    value = str(rank_order or "").strip().lower()
    if value not in RANK_ORDERS:
        raise ValueError("rank_order must be 'asc' or 'desc'")
    return value  # type: ignore[return-value]


def validate_filters(filters: dict[str, Any]) -> None:
    if not isinstance(filters, dict):
        raise ValueError("filters must be an object")
    _validate_filter_node(filters, path="filters")
    if not filters.get("user_id"):
        raise ValueError("filters must include user_id for personal/agent memory queries")


def build_filters(
    *,
    user_id: str | None = None,
    session_id: str | None = None,
    filters: dict[str, Any] | None = None,
) -> dict[str, Any]:
    resolved: dict[str, Any] = deepcopy(filters or {})
    if user_id:
        existing = resolved.get("user_id")
        if existing is not None and existing != user_id:
            raise ValueError("filter user_id conflicts with user_id parameter")
        resolved["user_id"] = user_id
    if session_id:
        exact_values = _collect_exact_session_values(resolved)
        if exact_values and any(value != session_id for value in exact_values):
            raise ValueError("filter session_id conflicts with session_id parameter")
        if not _contains_field(resolved, "session_id"):
            clauses = list(resolved.get("AND") or [])
            clauses.append({"session_id": session_id})
            resolved["AND"] = clauses
    validate_filters(resolved)
    return resolved


def validate_delete_request(*, memory_id: str | None, user_id: str | None, session_id: str | None) -> None:
    if memory_id:
        if user_id or session_id:
            raise ValueError("single delete by memory_id cannot include user_id or session_id")
        return
    if not user_id:
        raise ValueError("batch delete requires explicit user_id")


def delete_confirm_text(user_id: str, session_id: str | None) -> str:
    return f"delete user_id={user_id} session_id={session_id or '*'}"


def validate_settings_update(settings: dict[str, Any], *, strict: bool = True) -> dict[str, Any]:
    if not isinstance(settings, dict) or not settings:
        raise ValueError("settings must be a non-empty object")
    if strict:
        unknown = sorted(set(settings) - ALLOWED_SETTINGS_FIELDS)
        if unknown:
            raise ValueError(f"Unknown settings fields {unknown}; allowed fields: {sorted(ALLOWED_SETTINGS_FIELDS)}")
    out: dict[str, Any] = {}
    for key, value in settings.items():
        if key == "timezone":
            if not isinstance(value, str) or not value.strip():
                raise ValueError("timezone must be an IANA timezone string")
            try:
                ZoneInfo(value)
            except ZoneInfoNotFoundError as exc:
                raise ValueError("timezone must be an IANA timezone string, e.g. Asia/Tokyo") from exc
            out[key] = value
        elif key == "llm_custom_setting":
            if not isinstance(value, dict):
                raise ValueError("llm_custom_setting must be an object")
            out[key] = value
        elif not strict:
            out[key] = value
    return out


def settings_diff(before: dict[str, Any], after: dict[str, Any], requested: dict[str, Any]) -> dict[str, dict[str, Any]]:
    before_data = _data_obj(before)
    after_data = _data_obj(after)
    diff: dict[str, dict[str, Any]] = {}
    for key in requested:
        old = before_data.get(key)
        new = after_data.get(key)
        if old != new:
            diff[key] = {"before": old, "after": new}
    return diff


def _data_obj(payload: dict[str, Any]) -> dict[str, Any]:
    data = payload.get("data", payload)
    return data if isinstance(data, dict) else {}


def _validate_filter_node(node: Any, *, path: str) -> None:
    if not isinstance(node, dict):
        raise ValueError(f"{path} must be an object")
    for key, value in node.items():
        if key not in _FILTER_FIELDS:
            raise ValueError(f"Unknown filter field {key!r} at {path}")
        if key in {"AND", "OR"}:
            if not isinstance(value, list) or not value:
                raise ValueError(f"{path}.{key} must be a non-empty array")
            for i, child in enumerate(value):
                _validate_filter_node(child, path=f"{path}.{key}[{i}]")
        elif key == "timestamp":
            _validate_filter_value(value, path=f"{path}.timestamp")
        elif key == "session_id":
            _validate_session_id_filter_value(value, path=f"{path}.session_id")
        else:
            _validate_filter_value(value, path=f"{path}.{key}")


def _validate_session_id_filter_value(value: Any, *, path: str) -> None:
    if isinstance(value, str) and value.strip():
        return
    if isinstance(value, dict):
        if set(value) != {"eq"}:
            raise ValueError(f"{path} operator object must be {{'eq': '<non-empty string>'}}")
        eq = value.get("eq")
        if isinstance(eq, str) and eq.strip():
            return
        raise ValueError(f"{path}.eq must be a non-empty string")
    raise ValueError(f"{path} must be a non-empty string or eq operator object")


def _validate_filter_value(value: Any, *, path: str) -> None:
    if isinstance(value, dict):
        unknown = sorted(set(value) - _FILTER_OPERATORS)
        if unknown:
            raise ValueError(f"Unknown filter operator(s) {unknown} at {path}")
        if not value:
            raise ValueError(f"{path} operator object cannot be empty")
    elif isinstance(value, (str, int, float)) and not isinstance(value, bool):
        return
    else:
        raise ValueError(f"{path} must be a scalar or operator object")


def _contains_field(node: Any, field: str) -> bool:
    if isinstance(node, dict):
        if field in node:
            return True
        return any(_contains_field(value, field) for value in node.values())
    if isinstance(node, list):
        return any(_contains_field(item, field) for item in node)
    return False


def _collect_exact_session_values(node: Any) -> list[str]:
    values: list[str] = []
    if isinstance(node, dict):
        session_value = node.get("session_id")
        if isinstance(session_value, str):
            values.append(session_value)
        elif isinstance(session_value, dict) and isinstance(session_value.get("eq"), str):
            values.append(session_value["eq"])
        for key, value in node.items():
            if key != "session_id":
                values.extend(_collect_exact_session_values(value))
    elif isinstance(node, list):
        for item in node:
            values.extend(_collect_exact_session_values(item))
    return values
