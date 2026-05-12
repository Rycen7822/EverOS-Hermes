from __future__ import annotations

import hashlib
import json
import re
from typing import Any, Mapping


_TRIVIAL_RECALL = {"ok", "okay", "k", "yes", "no", "done", "thanks", "thank you", "hi", "hello", "ping"}
_TRIVIAL_CAPTURE = _TRIVIAL_RECALL | {"ack", "roger"}
_REAL_SHORT_TASKS = {"继续", "下一步", "继续下一步", "继续下一步实验"}
_RELEVANT_CONFIG_KEYS = (
    "max_context_chars",
    "include_recent_raw",
    "recent_raw_top_k",
    "profile_max_items",
    "agent_skills_max_items",
    "agent_cases_max_items",
    "episodic_max_items",
    "min_score",
    "agent_recall",
    "agent_memory_types",
)


def should_skip_recall(query: str, *, session_id: str, config: Mapping[str, Any]) -> tuple[bool, str]:
    normalized = _normalize_query(query)
    if not normalized:
        return True, "empty_query"
    session_reason = _session_skip_reason(session_id)
    if session_reason:
        return True, session_reason
    if normalized in _REAL_SHORT_TASKS:
        return False, ""
    min_chars = _int_config(config, "min_recall_query_chars", 8)
    if len(normalized) < min_chars and normalized in _TRIVIAL_RECALL:
        return True, "trivial_query"
    return False, ""


def should_skip_capture(user_content: str, assistant_content: str, *, session_id: str, config: Mapping[str, Any]) -> tuple[bool, str]:
    user = _normalize_query(user_content)
    assistant = _normalize_query(assistant_content)
    if not user or not assistant:
        return True, "empty_turn"
    session_reason = _session_skip_reason(session_id)
    if session_reason:
        return True, session_reason
    if user in _REAL_SHORT_TASKS:
        return False, ""
    if user in _TRIVIAL_CAPTURE:
        return True, "trivial_turn"
    return False, ""


def stable_query_key(query: str, *, session_id: str, config: Mapping[str, Any]) -> str:
    relevant = {key: config.get(key) for key in _RELEVANT_CONFIG_KEYS if key in config}
    payload = {
        "query": _normalize_query(query),
        "session_id": session_id or "",
        "config": relevant,
    }
    text = json.dumps(payload, ensure_ascii=False, sort_keys=True, separators=(",", ":"), default=str)
    return hashlib.sha256(text.encode("utf-8")).hexdigest()


def _session_skip_reason(session_id: str) -> str:
    session = (session_id or "").strip().lower()
    if session.startswith("temp:"):
        return "temporary_session"
    if session.startswith("internal:"):
        return "internal_session"
    return ""


def _normalize_query(value: str) -> str:
    return re.sub(r"\s+", " ", (value or "").strip().lower())


def _int_config(config: Mapping[str, Any], key: str, default: int) -> int:
    try:
        return int(config.get(key, default))
    except (TypeError, ValueError):
        return default
