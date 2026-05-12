from __future__ import annotations

import json
import re
import threading
import time
from pathlib import Path
from typing import Any, Optional

from .client import DEFAULT_BASE_URL, DEFAULT_MEMORY_TYPES, EverOSClient, EverOSError, EverOSTimeoutError
from .env import get_env
from .formatting import format_search_context, pretty_json
from .schemas import normalize_scope

try:
    from agent.memory_provider import MemoryProvider
except Exception:  # pragma: no cover - used outside Hermes during standalone tests
    from abc import ABC, abstractmethod

    class MemoryProvider(ABC):
        @property
        @abstractmethod
        def name(self) -> str: ...

        @abstractmethod
        def is_available(self) -> bool: ...

        @abstractmethod
        def initialize(self, session_id: str, **kwargs: Any) -> None: ...

        @abstractmethod
        def get_tool_schemas(self) -> list[dict[str, Any]]: ...

        def handle_tool_call(self, tool_name: str, args: dict[str, Any], **kwargs: Any) -> str:
            raise NotImplementedError(tool_name)

        def get_config_schema(self) -> list[dict[str, Any]]:
            return []

        def save_config(self, values: dict[str, Any], hermes_home: str) -> None:
            return None

        def system_prompt_block(self) -> str:
            return ""

        def prefetch(self, query: str, *, session_id: str = "") -> str:
            return ""

        def queue_prefetch(self, query: str, *, session_id: str = "") -> None:
            return None

        def sync_turn(self, user_content: str, assistant_content: str, *, session_id: str = "") -> None:
            return None

        def on_session_end(self, messages: list[dict[str, Any]]) -> None:
            return None

        def on_pre_compress(self, messages: list[dict[str, Any]]) -> str:
            return ""

        def on_memory_write(self, action: str, target: str, content: str, metadata: dict[str, Any] | None = None) -> None:
            return None

        def shutdown(self) -> None:
            return None


_DEFAULT_CONFIG = {
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
    "agentic_timeout": 60.0,
    "max_context_items": 8,
    "timeout": 10.0,
}

_CONTEXT_STRIP_RE = re.compile(r"<memory-context>[\s\S]*?</memory-context>|<everos-context>[\s\S]*?</everos-context>", re.I)
_TRIVIAL_RE = re.compile(r"^(ok|okay|thanks|thank you|got it|sure|yes|no|yep|nope|k|ty|thx|np)\.?$", re.I)


SAVE_SCHEMA = {
    "name": "everos_memory_save",
    "description": "Queue an explicit long-term memory message in EverOS and optionally request extraction; saved=true does not guarantee a structured memory is immediately searchable.",
    "parameters": {
        "type": "object",
        "properties": {
            "content": {"type": "string", "description": "Memory content to store."},
            "session_id": {"type": "string", "description": "Optional EverOS/Hermes session id."},
            "scope": {"type": "string", "enum": ["personal", "agent"], "description": "Memory scope. Default personal."},
            "role": {"type": "string", "enum": ["user", "assistant", "tool", "system"], "description": "Message role. role=tool is only valid with scope=agent."},
            "flush": {"type": "boolean", "description": "Trigger EverOS extraction immediately. Default true."},
        },
        "required": ["content"],
    },
}

SEARCH_SCHEMA = {
    "name": "everos_memory_search",
    "description": "Search EverOS long-term memory using keyword, vector, hybrid, or agentic retrieval.",
    "parameters": {
        "type": "object",
        "properties": {
            "query": {"type": "string", "description": "Search query."},
            "limit": {"type": "integer", "description": "Backward-compatible alias for top_k."},
            "top_k": {"type": "integer", "description": "Cloud top_k; -1 requests all matching results."},
            "method": {"type": "string", "enum": ["keyword", "vector", "hybrid", "agentic"], "description": "Retrieval method. Default hybrid."},
            "session_id": {"type": "string", "description": "Optional session filter."},
            "filters": {"type": "object", "description": "Optional Cloud v1 filters DSL. user_id is filled from provider config."},
            "memory_types": {"type": "array", "items": {"type": "string", "enum": ["episodic_memory", "profile", "raw_message", "agent_memory"]}, "description": "Optional EverOS search memory types."},
            "radius": {"type": "number", "description": "Optional vector radius for vector/hybrid/agentic retrieval."},
            "include_original_data": {"type": "boolean", "description": "Include Cloud original_data. Vectors remain stripped by default."},
            "include_vectors": {"type": "boolean", "description": "Keep embedding/vector fields for debugging only."},
            "response_format": {"type": "string", "enum": ["json", "markdown"], "description": "Output format."},
        },
        "required": ["query"],
    },
}

GET_SCHEMA = {
    "name": "everos_memory_get",
    "description": "Get structured EverOS memories by type for the configured user.",
    "parameters": {
        "type": "object",
        "properties": {
            "memory_type": {"type": "string", "enum": ["episodic_memory", "profile", "agent_case", "agent_skill"], "description": "Memory type to retrieve."},
            "page": {"type": "integer", "description": "Page number starting at 1."},
            "page_size": {"type": "integer", "description": "Items per page, 1-100."},
            "session_id": {"type": "string", "description": "Optional session filter."},
            "filters": {"type": "object", "description": "Optional Cloud v1 filters DSL. user_id is filled from provider config."},
            "rank_by": {"type": "string", "description": "Rank field. Default timestamp."},
            "rank_order": {"type": "string", "enum": ["asc", "desc"], "description": "Rank order."},
        },
    },
}

FLUSH_SCHEMA = {
    "name": "everos_memory_flush",
    "description": "Force EverOS memory extraction for the configured user/session. Timeout errors are retryable; search/status checks should happen before retrying.",
    "parameters": {
        "type": "object",
        "properties": {
            "session_id": {"type": "string", "description": "Optional session id."},
            "scope": {"type": "string", "enum": ["personal", "agent"], "description": "Memory scope to flush."},
            "timeout": {"type": "number", "description": "Optional per-call timeout in seconds."},
        },
    },
}

FORGET_SCHEMA = {
    "name": "everos_memory_forget",
    "description": "Delete an EverOS memory by id. Requires confirm=true because this is destructive.",
    "parameters": {
        "type": "object",
        "properties": {
            "memory_id": {"type": "string", "description": "Exact EverOS memory id to delete."},
            "confirm": {"type": "boolean", "description": "Must be true to delete."},
        },
        "required": ["memory_id", "confirm"],
    },
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
    existing.update(values or {})
    path.write_text(json.dumps(_normalize_config(existing), ensure_ascii=False, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def _normalize_config(config: dict[str, Any]) -> dict[str, Any]:
    out = dict(_DEFAULT_CONFIG)
    out.update(config or {})
    for key in ("auto_recall", "auto_capture", "flush_after_turn", "capture_agent_memory", "agent_recall", "agent_flush_after_turn"):
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
        out["max_context_items"] = max(1, min(50, int(out.get("max_context_items", 8))))
    except Exception:
        out["max_context_items"] = 8
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


def _tool_error(message: str) -> str:
    return json.dumps({"error": message}, ensure_ascii=False)


def _timeout_payload(operation: str, exc: EverOSTimeoutError) -> dict[str, Any]:
    return {
        "ok": False,
        "operation": operation,
        "error": str(exc),
        "retryable": bool(getattr(exc, "retryable", True)),
        "suggested_next_actions": list(getattr(exc, "suggested_next_actions", [])),
    }


def _flush_result_payload(response: dict[str, Any]) -> dict[str, Any]:
    data = response.get("data", {}) if isinstance(response, dict) else {}
    payload: dict[str, Any] = {"ok": True}
    if isinstance(data, dict):
        for key in ("status", "request_id", "task_id", "message"):
            if data.get(key):
                payload[key] = data[key]
    return payload


def _save_result_payload(
    *,
    result: dict[str, Any],
    user_id: str,
    session_id: str | None,
    scope: str = "personal",
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


def _now_ms() -> int:
    return int(time.time() * 1000)


def _clean_content(text: str) -> str:
    return _CONTEXT_STRIP_RE.sub("", text or "").strip()


class EverOSMemoryProvider(MemoryProvider):
    def __init__(self) -> None:
        self._config = dict(_DEFAULT_CONFIG)
        self._client: Optional[EverOSClient] = None
        self._api_key = ""
        self._base_url = DEFAULT_BASE_URL
        self._user_id = ""
        self._session_id = ""
        self._hermes_home = ""
        self._platform = "cli"
        self._write_enabled = True
        self._active = False
        self._threads: list[threading.Thread] = []
        self._last_write_status: dict[str, Any] = {}
        self._last_flush_status: dict[str, Any] = {}
        self._last_agent_write_status: dict[str, Any] = {}
        self._last_agent_flush_status: dict[str, Any] = {}

    @property
    def name(self) -> str:
        return "everos"

    def is_available(self) -> bool:
        return bool(get_env("EVEROS_API_KEY", ""))

    def get_config_schema(self) -> list[dict[str, Any]]:
        return [
            {
                "key": "api_key",
                "description": "EverOS API key",
                "secret": True,
                "required": True,
                "env_var": "EVEROS_API_KEY",
                "url": "https://everos.evermind.ai/api-keys",
            },
            {
                "key": "user_id",
                "description": "Default EverOS user_id (optional; gateway user_id is used when present)",
                "required": False,
                "default": "",
                "env_var": "EVEROS_USER_ID",
            },
            {
                "key": "base_url",
                "description": "EverOS API base URL",
                "required": False,
                "default": DEFAULT_BASE_URL,
                "env_var": "EVEROS_BASE_URL",
            },
        ]

    def save_config(self, values: dict[str, Any], hermes_home: str) -> None:
        _save_config(values, hermes_home)

    def initialize(self, session_id: str, **kwargs: Any) -> None:
        self._hermes_home = kwargs.get("hermes_home") or str(Path.home() / ".hermes")
        self._config = _load_config(self._hermes_home)
        self._api_key = get_env("EVEROS_API_KEY", "", hermes_home_path=self._hermes_home)
        self._base_url = get_env("EVEROS_BASE_URL", self._config["base_url"], hermes_home_path=self._hermes_home)
        self._session_id = session_id
        self._platform = str(kwargs.get("platform") or "cli")
        self._user_id = self._resolve_user_id(kwargs)
        agent_context = kwargs.get("agent_context", "")
        self._write_enabled = agent_context not in ("cron", "flush", "subagent")
        self._active = bool(self._api_key)
        self._client = None
        if self._active:
            self._client = EverOSClient(api_key=self._api_key, base_url=self._base_url, timeout=self._config["timeout"])

    def _resolve_user_id(self, kwargs: dict[str, Any]) -> str:
        template = get_env("EVEROS_USER_ID", "", hermes_home_path=self._hermes_home) or str(self._config.get("user_id") or "").strip()
        gateway_user_id = str(kwargs.get("user_id") or "").strip()
        identity = str(kwargs.get("agent_identity") or "default").strip() or "default"
        user_name = str(kwargs.get("user_name") or "").strip()
        platform = str(kwargs.get("platform") or "cli").strip() or "cli"
        if not template:
            return gateway_user_id or f"hermes_{identity}"
        values = {
            "user_id": gateway_user_id or identity,
            "user_name": user_name or gateway_user_id or identity,
            "identity": identity,
            "platform": platform,
        }
        try:
            return template.format(**values)
        except Exception:
            return template

    def system_prompt_block(self) -> str:
        if not self._active:
            return ""
        return (
            "# EverOS Memory\n"
            f"EverOS memory provider is active for user_id `{self._user_id}`. "
            "Use EverOS memory context silently when relevant. "
            "Explicit tools available: everos_memory_search, everos_memory_save, everos_memory_get, everos_memory_flush, everos_memory_forget."
        )

    def prefetch(self, query: str, *, session_id: str = "") -> str:
        if not self._active or not self._config["auto_recall"] or not self._client or not query.strip():
            return ""
        query_text = query[:1000]
        formatted_sections: list[str] = []
        try:
            response = self._client.search_memories(
                query=query_text,
                user_id=self._user_id,
                session_id=None,
                method=self._config["search_method"],
                memory_types=self._config["memory_types"],
                top_k=self._config["top_k"],
                include_original_data=False,
                timeout=self._config["timeout"],
            )
            formatted = format_search_context(response, max_items=self._config["max_context_items"])
            if formatted:
                formatted_sections.append(formatted)
        except Exception as exc:
            self._record_status("prefetch.personal", False, exc)
        if self._config.get("agent_recall"):
            try:
                agent_response = self._client.search_memories(
                    query=query_text,
                    user_id=self._user_id,
                    session_id=None,
                    method=self._config["search_method"],
                    memory_types=self._config["agent_memory_types"],
                    top_k=self._config["top_k"],
                    include_original_data=False,
                    timeout=self._config["agentic_timeout"],
                )
                formatted = format_search_context(agent_response, max_items=self._config["max_context_items"])
                if formatted:
                    formatted_sections.append(formatted)
            except Exception as exc:
                self._record_status("prefetch.agent", False, exc)
        if not formatted_sections:
            return ""
        return "<everos-context>\n" + "\n\n".join(formatted_sections) + "\n</everos-context>"

    def queue_prefetch(self, query: str, *, session_id: str = "") -> None:
        return None

    def get_tool_schemas(self) -> list[dict[str, Any]]:
        return [SAVE_SCHEMA, SEARCH_SCHEMA, GET_SCHEMA, FLUSH_SCHEMA, FORGET_SCHEMA]

    def handle_tool_call(self, tool_name: str, args: dict[str, Any], **kwargs: Any) -> str:
        if not self._active or not self._client:
            return _tool_error("EverOS provider is not active. Set EVEROS_API_KEY and memory.provider: everos.")
        try:
            if tool_name == "everos_memory_save":
                return self._tool_save(args)
            if tool_name == "everos_memory_search":
                return self._tool_search(args)
            if tool_name == "everos_memory_get":
                return self._tool_get(args)
            if tool_name == "everos_memory_flush":
                return self._tool_flush(args)
            if tool_name == "everos_memory_forget":
                return self._tool_forget(args)
            return _tool_error(f"Unknown EverOS memory tool: {tool_name}")
        except EverOSError as exc:
            return _tool_error(str(exc))
        except Exception as exc:
            return _tool_error(f"EverOS tool failed: {exc}")

    def _tool_save(self, args: dict[str, Any]) -> str:
        content = str(args.get("content") or "").strip()
        if not content:
            return _tool_error("content is required")
        session_id = str(args.get("session_id") or self._session_id or "") or None
        scope = normalize_scope(str(args.get("scope") or "personal"))
        role = str(args.get("role") or ("tool" if scope == "agent" else "user")).strip() or "user"
        result = self._client.add_memories(
            user_id=self._user_id,
            session_id=session_id,
            messages=[{"role": role, "timestamp": _now_ms(), "content": content}],
            async_mode=True,
            scope=scope,
        )
        flush_result = None
        flush_error = None
        flush_requested = _as_bool(args.get("flush", True), True)
        if flush_requested:
            try:
                flush_result = self._client.flush_memories(user_id=self._user_id, session_id=session_id, scope=scope)
            except EverOSTimeoutError as exc:
                flush_error = exc
        return json.dumps(
            _save_result_payload(
                result=result,
                user_id=self._user_id,
                session_id=session_id,
                scope=scope,
                flush_requested=flush_requested,
                flush_result=flush_result,
                flush_error=flush_error,
            ),
            ensure_ascii=False,
        )

    def _tool_search(self, args: dict[str, Any]) -> str:
        query = str(args.get("query") or "").strip()
        if not query:
            return _tool_error("query is required")
        requested_top_k = args.get("top_k", args.get("limit", self._config["top_k"]))
        limit = _top_k(requested_top_k, self._config["top_k"])
        method = str(args.get("method") or self._config["search_method"]).strip().lower()
        if method not in {"keyword", "vector", "hybrid", "agentic"}:
            method = self._config["search_method"]
        memory_types = args.get("memory_types") if isinstance(args.get("memory_types"), list) else self._config["memory_types"]
        response_format = str(args.get("response_format") or "json")
        response = self._client.search_memories(
            query=query,
            user_id=self._user_id,
            session_id=str(args.get("session_id") or "") or None,
            filters=args.get("filters") if isinstance(args.get("filters"), dict) else None,
            method=method,
            memory_types=[str(item) for item in memory_types],
            top_k=limit,
            radius=_float_or_none(args.get("radius")),
            include_original_data=_as_bool(args.get("include_original_data", False), False),
            include_vectors=_as_bool(args.get("include_vectors", False), False),
            timeout=60.0 if method == "agentic" else self._config["timeout"],
        )
        if response_format == "markdown":
            return format_search_context(response, max_items=self._config["max_context_items"]) or pretty_json(response)
        return pretty_json(response)

    def _tool_get(self, args: dict[str, Any]) -> str:
        response = self._client.get_memories(
            user_id=self._user_id,
            session_id=str(args.get("session_id") or "") or None,
            filters=args.get("filters") if isinstance(args.get("filters"), dict) else None,
            memory_type=str(args.get("memory_type") or "episodic_memory"),
            page=_int_between(args.get("page", 1), 1, 10000, 1),
            page_size=_int_between(args.get("page_size", 20), 1, 100, 20),
            rank_by=str(args.get("rank_by") or "timestamp"),
            rank_order=str(args.get("rank_order") or "desc"),
        )
        return pretty_json(response)

    def _tool_flush(self, args: dict[str, Any]) -> str:
        try:
            response = self._client.flush_memories(
                user_id=self._user_id,
                session_id=str(args.get("session_id") or self._session_id or "") or None,
                scope=normalize_scope(str(args.get("scope") or "personal")),
                timeout=_float_or_none(args.get("timeout")),
            )
        except EverOSTimeoutError as exc:
            return pretty_json(_timeout_payload("flush", exc))
        return pretty_json(response)

    def _tool_forget(self, args: dict[str, Any]) -> str:
        if not _as_bool(args.get("confirm", False), False):
            return _tool_error("confirm=true is required before deleting an EverOS memory")
        memory_id = str(args.get("memory_id") or "").strip()
        if not memory_id:
            return _tool_error("memory_id is required")
        response = self._client.delete_memories(memory_id=memory_id)
        return pretty_json(response)

    def sync_turn(self, user_content: str, assistant_content: str, *, session_id: str = "") -> None:
        if not self._active or not self._write_enabled or not self._config["auto_capture"] or not self._client:
            return
        clean_user = _clean_content(user_content)
        clean_assistant = _clean_content(assistant_content)
        if not clean_user or not clean_assistant or _TRIVIAL_RE.match(clean_user):
            return
        sid = session_id or self._session_id
        now = _now_ms()
        personal_messages = [
            {"role": "user", "timestamp": now, "content": clean_user},
            {"role": "assistant", "timestamp": now + 1, "content": clean_assistant},
        ]
        agent_messages = _build_agent_trajectory_messages(clean_user, clean_assistant, now_ms=now + 2)

        def _run() -> None:
            mode = str(self._config.get("agent_capture_mode", "parallel"))
            write_personal = mode != "agent_only"
            write_agent = bool(self._config.get("capture_agent_memory")) and mode != "off"
            if write_personal:
                try:
                    result = self._client.add_memories(user_id=self._user_id, session_id=sid, messages=personal_messages, async_mode=True, scope="personal")
                    self._last_write_status = {"ok": True, "scope": "personal", "task_id": _task_id(result)}
                    if self._config["flush_after_turn"]:
                        flush = self._client.flush_memories(user_id=self._user_id, session_id=sid, scope="personal")
                        self._last_flush_status = {"ok": True, "scope": "personal", "status": _status(flush)}
                except Exception as exc:
                    self._record_status("sync_turn.personal", False, exc)
            if write_agent:
                try:
                    result = self._client.add_memories(user_id=self._user_id, session_id=sid, messages=agent_messages, async_mode=True, scope="agent")
                    self._last_agent_write_status = {"ok": True, "scope": "agent", "task_id": _task_id(result)}
                    if self._config.get("agent_flush_after_turn"):
                        flush = self._client.flush_memories(user_id=self._user_id, session_id=sid, scope="agent")
                        self._last_agent_flush_status = {"ok": True, "scope": "agent", "status": _status(flush)}
                except Exception as exc:
                    self._record_status("sync_turn.agent", False, exc, agent=True)

        self._start_thread(_run, "everos-sync-turn")

    def on_memory_write(self, action: str, target: str, content: str, metadata: dict[str, Any] | None = None) -> None:
        if action not in ("add", "replace", "update") or not content or not self._active or not self._write_enabled or not self._client:
            return
        text = f"Hermes {target} memory: {content.strip()}"

        def _run() -> None:
            try:
                result = self._client.add_memories(
                    user_id=self._user_id,
                    session_id=self._session_id,
                    messages=[{"role": "user", "timestamp": _now_ms(), "content": text}],
                    async_mode=True,
                    scope="personal",
                )
                if self._config["flush_after_turn"]:
                    self._client.flush_memories(user_id=self._user_id, session_id=self._session_id, scope="personal")
                return result
            except Exception as exc:
                self._record_status("memory_write.personal", False, exc)
                return None

        self._start_thread(_run, "everos-memory-write")

    def on_session_end(self, messages: list[dict[str, Any]]) -> None:
        if not self._active or not self._write_enabled or not self._client or not self._session_id:
            return
        try:
            self._client.flush_memories(user_id=self._user_id, session_id=self._session_id, scope="personal")
            if self._config.get("capture_agent_memory") and self._config.get("agent_flush_after_turn"):
                self._client.flush_memories(user_id=self._user_id, session_id=self._session_id, scope="agent")
        except Exception as exc:
            self._record_status("session_end.flush", False, exc)
            return None

    def on_pre_compress(self, messages: list[dict[str, Any]]) -> str:
        return ""

    def on_session_switch(self, new_session_id: str, *, parent_session_id: str = "", reset: bool = False, **kwargs: Any) -> None:
        if new_session_id:
            self._session_id = new_session_id

    def shutdown(self) -> None:
        threads = list(self._threads)
        self._threads = []
        for thread in threads:
            if thread.is_alive():
                thread.join(timeout=5.0)

    def _start_thread(self, target: Any, name: str) -> None:
        self._threads = [t for t in self._threads if t.is_alive()]
        thread = threading.Thread(target=target, daemon=True, name=name)
        self._threads.append(thread)
        thread.start()

    def _record_status(self, operation: str, ok: bool, exc: Exception | None = None, *, agent: bool = False) -> None:
        status = {"ok": ok, "operation": operation}
        if exc is not None:
            status["error"] = _redact_error(str(exc))
        if agent:
            self._last_agent_write_status = status
        else:
            self._last_write_status = status
        self._write_debug_log(status)

    def _write_debug_log(self, payload: dict[str, Any]) -> None:
        if not self._hermes_home:
            return
        try:
            path = Path(self._hermes_home) / "everos.log"
            safe = {k: v for k, v in payload.items() if k in {"ok", "operation", "error", "scope", "status", "request_id", "task_id"}}
            path.parent.mkdir(parents=True, exist_ok=True)
            with path.open("a", encoding="utf-8") as fh:
                fh.write(json.dumps(safe, ensure_ascii=False, sort_keys=True) + "\n")
        except Exception:
            return


def _int_between(value: Any, low: int, high: int, default: int) -> int:
    try:
        return max(low, min(high, int(value)))
    except Exception:
        return default


def _top_k(value: Any, default: int) -> int:
    try:
        parsed = int(value)
    except Exception:
        return default
    if parsed == -1:
        return -1
    return max(0, min(100, parsed))


def _float_or_none(value: Any) -> float | None:
    if value is None or value == "":
        return None
    try:
        parsed = float(value)
    except Exception:
        return None
    return parsed if parsed > 0 else None


def _task_id(response: dict[str, Any]) -> str:
    data = response.get("data", {}) if isinstance(response, dict) else {}
    return str(data.get("task_id") or "") if isinstance(data, dict) else ""


def _status(response: dict[str, Any]) -> str:
    data = response.get("data", {}) if isinstance(response, dict) else {}
    return str(data.get("status") or "") if isinstance(data, dict) else ""


def _build_agent_trajectory_messages(user_content: str, assistant_content: str, *, now_ms: int) -> list[dict[str, Any]]:
    user_summary = _truncate_for_memory(user_content, 4000)
    assistant_summary = _truncate_for_memory(assistant_content, 4000)
    return [
        {"role": "user", "timestamp": now_ms, "content": f"Task request: {user_summary}"},
        {
            "role": "assistant",
            "timestamp": now_ms + 1,
            "content": "Agent response summary: "
            f"{assistant_summary}\nOutcome: completed_or_partial\nReusable lesson hint: capture approach, correction, and verification if useful.",
        },
    ]


def _truncate_for_memory(text: str, limit: int) -> str:
    text = (text or "").strip()
    if len(text) <= limit:
        return text
    return text[:limit] + "…"


def _redact_error(text: str) -> str:
    text = re.sub(r"sk-[A-Za-z0-9_-]+", "sk-***", text or "")
    text = re.sub(r"(?i)(api[_-]?key|authorization|token|secret)=\S+", r"\1=***", text)
    return text[:500]


def register(ctx: Any) -> None:
    ctx.register_memory_provider(EverOSMemoryProvider())
