from __future__ import annotations

import json
import os
from pathlib import Path
from typing import Any

from .client import DEFAULT_BASE_URL, DEFAULT_MEMORY_TYPES

_DEFAULT_CONFIG: dict[str, Any] = {
    "base_url": DEFAULT_BASE_URL,
    "user_id": "",
    "auto_recall": True,
    "auto_capture": True,
    "flush_after_turn": True,
    "search_method": "hybrid",
    "top_k": 5,
    "memory_types": DEFAULT_MEMORY_TYPES,
    "capture_agent_memory": False,
    "agent_capture_mode": "parallel",
    "agent_recall": False,
    "agent_memory_types": ["agent_memory"],
    "agent_flush_after_turn": True,
    "agent_visibility_verify_after_write": False,
    "agent_visibility_verify_after_flush": False,
    "agent_visibility_queries": [],
    "agent_visibility_top_k": 5,
    "agent_visibility_timeout": 30.0,
    "agent_visibility_get_page_size": 20,
    "agent_visibility_retry_flush_attempts": 1,
    "agentic_timeout": 60.0,
    "max_context_items": 8,
    "timeout": 10.0,
    "max_context_chars": 12000,
    "include_recent_raw": False,
    "recent_raw_top_k": 4,
    "profile_max_items": 3,
    "agent_skills_max_items": 4,
    "agent_cases_max_items": 4,
    "episodic_max_items": 6,
    "min_score": 0.0,
    "min_recall_query_chars": 8,
    "prefetch_cache_enabled": True,
    "prefetch_cache_ttl_seconds": 90,
    "agent_trajectory_on_session_end": True,
    "agent_trajectory_on_pre_compress": True,
    "agent_trajectory_on_delegation": True,
    "agent_summary_after_turn": True,
    "agent_max_messages": 80,
    "agent_max_message_chars": 8000,
    "agent_max_tool_result_chars": 6000,
    "agent_max_payload_chars": 60000,
    "agent_dedupe_entries": 256,
}

def _load_config(hermes_home: str) -> dict[str, Any]:
    config = dict(_DEFAULT_CONFIG)
    path = Path(hermes_home) / "everos.json"
    if path.exists():
        try:
            raw = json.loads(path.read_text(encoding="utf-8"))
            if isinstance(raw, dict):
                config.update({k: v for k, v in raw.items() if v is not None})
        except Exception:
            pass
    return _normalize_config(config)


_SECRET_CONFIG_KEYS = {"api_key"}


def _sanitize_config_values(values: dict[str, Any]) -> dict[str, Any]:
    return {str(key): value for key, value in (values or {}).items() if str(key) not in _SECRET_CONFIG_KEYS}


def _save_config(values: dict[str, Any], hermes_home: str) -> None:
    path = Path(hermes_home) / "everos.json"
    existing: dict[str, Any] = {}
    if path.exists():
        try:
            raw = json.loads(path.read_text(encoding="utf-8"))
            if isinstance(raw, dict):
                existing = raw
        except Exception:
            existing = {}
    existing.update(_sanitize_config_values(values or {}))
    text = json.dumps(_normalize_config(existing), ensure_ascii=False, indent=2, sort_keys=True) + "\n"
    path.parent.mkdir(parents=True, exist_ok=True)
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
    with os.fdopen(fd, "w", encoding="utf-8") as handle:
        handle.write(text)
    os.chmod(path, 0o600)


def _normalize_config(config: dict[str, Any]) -> dict[str, Any]:
    out = dict(_DEFAULT_CONFIG)
    out.update(_sanitize_config_values(config or {}))
    for key in (
        "auto_recall",
        "auto_capture",
        "flush_after_turn",
        "capture_agent_memory",
        "agent_recall",
        "agent_flush_after_turn",
        "agent_visibility_verify_after_write",
        "agent_visibility_verify_after_flush",
        "include_recent_raw",
        "prefetch_cache_enabled",
        "agent_trajectory_on_session_end",
        "agent_trajectory_on_pre_compress",
        "agent_trajectory_on_delegation",
        "agent_summary_after_turn",
    ):
        out[key] = _as_bool(out.get(key), bool(_DEFAULT_CONFIG[key]))
    try:
        out["top_k"] = max(1, min(20, int(out.get("top_k", 5))))
    except Exception:
        out["top_k"] = 5
    try:
        out["timeout"] = max(1.0, min(60.0, float(out.get("timeout", 10.0))))
    except Exception:
        out["timeout"] = 10.0
    try:
        out["agentic_timeout"] = max(1.0, min(120.0, float(out.get("agentic_timeout", 60.0))))
    except Exception:
        out["agentic_timeout"] = 60.0
    try:
        out["agent_visibility_timeout"] = max(1.0, min(120.0, float(out.get("agent_visibility_timeout", 30.0))))
    except Exception:
        out["agent_visibility_timeout"] = 30.0
    try:
        out["max_context_items"] = max(1, min(50, int(out.get("max_context_items", 8))))
    except Exception:
        out["max_context_items"] = 8
    for key, low, high in (
        ("max_context_chars", 1000, 50000),
        ("recent_raw_top_k", 0, 20),
        ("profile_max_items", 0, 20),
        ("agent_skills_max_items", 0, 20),
        ("agent_cases_max_items", 0, 20),
        ("episodic_max_items", 0, 20),
        ("min_recall_query_chars", 0, 200),
        ("prefetch_cache_ttl_seconds", 1, 600),
        ("agent_max_messages", 1, 200),
        ("agent_max_message_chars", 100, 20000),
        ("agent_max_tool_result_chars", 100, 20000),
        ("agent_max_payload_chars", 1000, 200000),
        ("agent_dedupe_entries", 16, 4096),
        ("agent_visibility_top_k", 1, 20),
        ("agent_visibility_get_page_size", 1, 100),
        ("agent_visibility_retry_flush_attempts", 1, 5),
    ):
        try:
            out[key] = max(low, min(high, int(out.get(key, _DEFAULT_CONFIG[key]))))
        except Exception:
            out[key] = _DEFAULT_CONFIG[key]
    try:
        out["min_score"] = max(0.0, min(1.0, float(out.get("min_score", 0.0))))
    except Exception:
        out["min_score"] = 0.0
    method = str(out.get("search_method", "hybrid")).strip().lower()
    out["search_method"] = method if method in {"keyword", "vector", "hybrid", "agentic"} else "hybrid"
    memory_types = out.get("memory_types")
    if isinstance(memory_types, str):
        memory_types = [part.strip() for part in memory_types.split(",") if part.strip()]
    if not isinstance(memory_types, list) or not memory_types:
        memory_types = DEFAULT_MEMORY_TYPES
    out["memory_types"] = [str(item) for item in memory_types]
    agent_memory_types = out.get("agent_memory_types")
    if isinstance(agent_memory_types, str):
        agent_memory_types = [part.strip() for part in agent_memory_types.split(",") if part.strip()]
    if not isinstance(agent_memory_types, list) or not agent_memory_types:
        agent_memory_types = ["agent_memory"]
    out["agent_memory_types"] = [str(item) for item in agent_memory_types]
    visibility_queries = out.get("agent_visibility_queries")
    if isinstance(visibility_queries, str):
        visibility_queries = [part.strip() for part in visibility_queries.split(",") if part.strip()]
    if not isinstance(visibility_queries, list):
        visibility_queries = []
    out["agent_visibility_queries"] = [str(item) for item in visibility_queries if str(item).strip()]
    mode = str(out.get("agent_capture_mode", "parallel")).strip().lower()
    out["agent_capture_mode"] = mode if mode in {"parallel", "agent_only", "off"} else "parallel"
    out["base_url"] = str(out.get("base_url") or DEFAULT_BASE_URL).strip() or DEFAULT_BASE_URL
    out["user_id"] = str(out.get("user_id") or "").strip()
    return out


def _as_bool(value: Any, default: bool) -> bool:
    if isinstance(value, bool):
        return value
    if isinstance(value, str):
        lowered = value.strip().lower()
        if lowered in ("1", "true", "yes", "y", "on"):
            return True
        if lowered in ("0", "false", "no", "n", "off"):
            return False
    return default
