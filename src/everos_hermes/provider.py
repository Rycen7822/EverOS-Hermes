from __future__ import annotations

import json
import hashlib
import os
import re
import threading
import time
from collections import OrderedDict
from pathlib import Path
from typing import Any, Optional

from .agent_visibility import audit_agent_visibility, build_agent_visibility_report
from .client import DEFAULT_BASE_URL, EverOSClient, EverOSError, EverOSTimeoutError
from .context_assembler import assemble_everos_context
from .env import get_env
from .flush_retry import flush_memories_with_retry
from .formatting import format_search_context, pretty_json
from .policy import should_skip_capture, should_skip_recall, stable_query_key
from .provider_schemas import provider_tool_schemas
from .provider_config import _DEFAULT_CONFIG, _as_bool, _load_config, _save_config
from .redaction import redact_text, sanitized_error_message
from .schemas import normalize_scope
from .tool_payloads import (
    flush_result_payload as _flush_result_payload,
    save_result_payload as _save_result_payload,
    timeout_payload as _timeout_payload,
)
from .trajectory import TrajectoryBuildResult, TrajectorySource, build_agent_trajectory_messages
from .workflows import import_and_verify, now_ms, save_and_verify, verify_session_ingest

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


_CONTEXT_STRIP_RE = re.compile(r"<memory-context>[\s\S]*?</memory-context>|<everos-context>[\s\S]*?</everos-context>", re.I)


def _tool_error(message: str) -> str:
    return json.dumps({"error": redact_text(message)}, ensure_ascii=False)



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
        self._last_agent_write_status: dict[str, Any] = {}
        self._last_recall_status: dict[str, Any] = {}
        self._last_agent_visibility_status: dict[str, Any] = {}
        self._prefetch_cache: dict[str, dict[str, Any]] = {}
        self._prefetch_inflight: set[str] = set()
        self._prefetch_lock = threading.Lock()
        self._agent_saved_fingerprints: OrderedDict[str, float] = OrderedDict()

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
        with self._prefetch_lock:
            self._prefetch_cache.clear()
            self._prefetch_inflight.clear()
        self._last_recall_status = {}
        self._last_agent_visibility_status = {}
        self._agent_saved_fingerprints.clear()
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
        if not self._active or not self._config["auto_recall"] or not self._client:
            return ""
        sid = session_id or self._session_id
        skip, reason = should_skip_recall(query, session_id=sid, config=self._config)
        if skip:
            self._last_recall_status = {"ok": True, "skipped": True, "reason": reason, "cached": False}
            return ""
        query_text = query[:1000]
        cache_key = stable_query_key(query_text, session_id=sid, config=self._config)
        if self._config.get("prefetch_cache_enabled"):
            cached = self._get_prefetch_cache(cache_key)
            if cached is not None:
                self._last_recall_status = {**cached.get("status", {}), "ok": True, "cached": True}
                return str(cached.get("text") or "")
        text, status = self._search_prefetch_context(query_text, sid)
        if self._config.get("prefetch_cache_enabled"):
            self._set_prefetch_cache(cache_key, text, status)
        return text

    def queue_prefetch(self, query: str, *, session_id: str = "") -> None:
        if not self._active or not self._config["auto_recall"] or not self._client:
            return
        sid = session_id or self._session_id
        skip, reason = should_skip_recall(query, session_id=sid, config=self._config)
        if skip:
            self._last_recall_status = {"ok": True, "skipped": True, "reason": reason, "cached": False}
            return
        query_text = query[:1000]
        cache_key = stable_query_key(query_text, session_id=sid, config=self._config)
        with self._prefetch_lock:
            if cache_key in self._prefetch_inflight:
                return
            cached = self._prefetch_cache.get(cache_key)
            if cached and float(cached.get("expires_at", 0)) > time.monotonic():
                return
            self._prefetch_inflight.add(cache_key)

        def _run() -> None:
            try:
                text, status = self._search_prefetch_context(query_text, sid)
                if self._config.get("prefetch_cache_enabled"):
                    self._set_prefetch_cache(cache_key, text, status)
            finally:
                with self._prefetch_lock:
                    self._prefetch_inflight.discard(cache_key)

        self._start_thread(_run, "everos-prefetch")

    def _get_prefetch_cache(self, cache_key: str) -> dict[str, Any] | None:
        with self._prefetch_lock:
            cached = self._prefetch_cache.get(cache_key)
            if not cached:
                return None
            if float(cached.get("expires_at", 0)) <= time.monotonic():
                self._prefetch_cache.pop(cache_key, None)
                return None
            return dict(cached)

    def _set_prefetch_cache(self, cache_key: str, text: str, status: dict[str, Any]) -> None:
        ttl = float(self._config.get("prefetch_cache_ttl_seconds", 90))
        with self._prefetch_lock:
            self._prefetch_cache[cache_key] = {"text": text, "status": dict(status), "expires_at": time.monotonic() + ttl}

    def _search_prefetch_context(self, query_text: str, session_id: str) -> tuple[str, dict[str, Any]]:
        main_response, raw_response, warnings, errors, ok_count = self._search_for_context(query_text, session_id)
        result = self._assemble_context(main_response, raw_response)
        status = {
            "ok": ok_count > 0 or not errors,
            "cached": False,
            "hit_counts": result.hit_counts,
            "included_counts": result.included_counts,
            "dropped_counts": result.dropped_counts,
            "estimated_chars": result.estimated_chars,
            "warnings": warnings,
        }
        if errors and ok_count == 0:
            status["ok"] = False
            status["errors"] = errors
        self._last_recall_status = status
        return result.text, status

    def _search_for_context(
        self, query_text: str, session_id: str
    ) -> tuple[dict[str, Any] | None, dict[str, Any] | None, list[str], list[str], int]:
        main_response: dict[str, Any] | None = None
        raw_response: dict[str, Any] | None = None
        warnings: list[str] = []
        errors: list[str] = []
        ok_count = 0
        personal_types = [str(item) for item in self._config["memory_types"] if str(item) != "agent_memory"] or ["episodic_memory", "profile"]
        try:
            main_response = self._client.search_memories(
                query=query_text,
                user_id=self._user_id,
                session_id=None,
                method=self._config["search_method"],
                memory_types=personal_types,
                top_k=self._config["top_k"],
                include_original_data=False,
                timeout=self._config["timeout"],
            )
            ok_count += 1
        except Exception as exc:
            errors.append(f"personal:{sanitized_error_message(exc)}")
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
                main_response = _merge_agent_response(main_response, agent_response)
                ok_count += 1
            except Exception as exc:
                errors.append(f"agent:{sanitized_error_message(exc)}")
                self._record_status("prefetch.agent", False, exc, agent=True)
        if self._config.get("include_recent_raw"):
            if session_id:
                try:
                    raw_response = self._client.search_memories(
                        query=query_text,
                        user_id=self._user_id,
                        session_id=session_id,
                        method=self._config["search_method"],
                        memory_types=["raw_message"],
                        top_k=self._config["recent_raw_top_k"],
                        include_original_data=False,
                        timeout=self._config["timeout"],
                    )
                    ok_count += 1
                except Exception as exc:
                    errors.append(f"raw:{sanitized_error_message(exc)}")
                    self._record_status("prefetch.raw", False, exc)
            else:
                warnings.append("raw_recall_skipped_missing_session")
        return main_response, raw_response, warnings, errors, ok_count

    def _assemble_context(
        self,
        main_response: dict[str, Any] | None,
        raw_response: dict[str, Any] | None,
    ) -> Any:
        return assemble_everos_context(
            main_response=main_response,
            raw_response=raw_response,
            config=self._config,
            source="prefetch",
        )

    def get_tool_schemas(self) -> list[dict[str, Any]]:
        return provider_tool_schemas()

    def handle_tool_call(self, tool_name: str, args: dict[str, Any], **kwargs: Any) -> str:
        if not self._active or not self._client:
            return _tool_error("EverOS provider is not active. Set EVEROS_API_KEY and memory.provider: everos.")
        handlers = {
            "everos_memory_save": self._tool_save,
            "everos_memory_search": self._tool_search,
            "everos_memory_get": self._tool_get,
            "everos_memory_flush": self._tool_flush,
            "everos_memory_forget": self._tool_forget,
            "everos_memory_save_and_verify": self._tool_save_and_verify,
            "everos_memory_import_and_verify": self._tool_import_and_verify,
            "everos_memory_verify_session": self._tool_verify_session,
        }
        handler = handlers.get(tool_name)
        if not handler:
            return _tool_error(f"Unknown EverOS memory tool: {tool_name}")
        try:
            return handler(args)
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
        role = str(args.get("role") or ("assistant" if scope == "agent" else "user")).strip() or "user"
        message = {"role": role, "timestamp": now_ms(), "content": content}
        if args.get("tool_call_id"):
            message["tool_call_id"] = str(args.get("tool_call_id"))
        result = self._client.add_memories(
            user_id=self._user_id,
            session_id=session_id,
            messages=[message],
            async_mode=True,
            scope=scope,
        )
        flush_result = None
        flush_error = None
        flush_requested = _as_bool(args.get("flush", True), True)
        if flush_requested:
            try:
                flush_result, _attempt_count = flush_memories_with_retry(
                    self._client,
                    user_id=self._user_id,
                    session_id=session_id,
                    scope=scope,
                )
            except Exception as exc:
                flush_error = exc
        payload = _save_result_payload(
            result=result,
            user_id=self._user_id,
            session_id=session_id,
            scope=scope,
            flush_requested=flush_requested,
            flush_result=flush_result,
            flush_error=flush_error,
        )
        if scope == "agent":
            payload["agent_visibility"] = self._agent_visibility_after_write(
                session_id=session_id,
                texts=[content],
                markers=["tool_save"],
                agent_raw_queued=True,
                agent_flush=payload.get("flush") if isinstance(payload.get("flush"), dict) else None,
                flush_completed=flush_result is not None,
                record_status=True,
            )
        return json.dumps(payload, ensure_ascii=False)

    def _tool_search(self, args: dict[str, Any]) -> str:
        query = str(args.get("query") or "").strip()
        if not query:
            return _tool_error("query is required")
        method = str(args.get("method") or self._config["search_method"]).strip().lower()
        memory_types = args.get("memory_types") if isinstance(args.get("memory_types"), list) else self._config["memory_types"]
        response_format = str(args.get("response_format") or "json")
        response = self._client.search_memories(
            query=query,
            user_id=self._user_id,
            session_id=str(args.get("session_id") or "") or None,
            filters=args.get("filters") if isinstance(args.get("filters"), dict) else None,
            method=method,
            memory_types=[str(item) for item in memory_types],
            top_k=args.get("top_k", args.get("limit", self._config["top_k"])),
            radius=args.get("radius"),
            include_original_data=_as_bool(args.get("include_original_data", False), False),
            include_vectors=_as_bool(args.get("include_vectors", False), False),
            timeout=self._config["agentic_timeout"] if method == "agentic" else self._config["timeout"],
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
            page=args.get("page", 1),
            page_size=args.get("page_size", 20),
            rank_by=str(args.get("rank_by") or "timestamp"),
            rank_order=str(args.get("rank_order") or "desc"),
        )
        return pretty_json(response)

    def _tool_flush(self, args: dict[str, Any]) -> str:
        session_id = str(args.get("session_id") or self._session_id or "") or None
        scope = normalize_scope(str(args.get("scope") or "personal"))
        try:
            response, attempt_count = flush_memories_with_retry(
                self._client,
                user_id=self._user_id,
                session_id=session_id,
                scope=scope,
                timeout=args.get("timeout"),
            )
        except EverOSTimeoutError as exc:
            return pretty_json(_timeout_payload("flush", exc))
        if scope == "agent":
            flush_payload = _flush_result_payload(response, attempt_count=attempt_count)
            return pretty_json(
                {
                    "flush": flush_payload,
                    "agent_visibility": build_agent_visibility_report(agent_raw_queued=None, agent_flush=flush_payload, checks=[]),
                }
            )
        return pretty_json(response)

    def _tool_forget(self, args: dict[str, Any]) -> str:
        if not _as_bool(args.get("confirm", False), False):
            return _tool_error("confirm=true is required before deleting an EverOS memory")
        memory_id = str(args.get("memory_id") or "").strip()
        if not memory_id:
            return _tool_error("memory_id is required")
        response = self._client.delete_memories(memory_id=memory_id)
        return pretty_json(response)

    def _tool_save_and_verify(self, args: dict[str, Any]) -> str:
        content = str(args.get("content") or "").strip()
        if not content:
            return _tool_error("content is required")
        result = save_and_verify(
            client=self._client,
            content=content,
            user_id=self._user_id,
            session_id=str(args.get("session_id") or self._session_id or "") or None,
            scope=normalize_scope(str(args.get("scope") or "personal")),
            role=str(args.get("role") or "").strip() or None,
            tool_call_id=str(args.get("tool_call_id") or "").strip() or None,
            flush=_as_bool(args.get("flush", True), True),
            verification_query=str(args.get("verification_query") or "").strip() or None,
            verification_queries=args.get("verification_queries") if isinstance(args.get("verification_queries"), list) else None,
            top_k=args.get("top_k", self._config["top_k"]),
        )
        return pretty_json(result)

    def _tool_import_and_verify(self, args: dict[str, Any]) -> str:
        result = import_and_verify(
            client=self._client,
            user_id=self._user_id,
            session_id=str(args.get("session_id") or self._session_id or "") or None,
            messages=args.get("messages") if isinstance(args.get("messages"), list) else None,
            file_path=str(args.get("file_path") or "").strip() or None,
            scope=normalize_scope(str(args.get("scope") or "personal")),
            dry_run=_as_bool(args.get("dry_run", False), False),
            batch_size=args.get("batch_size", 50),
            flush=_as_bool(args.get("flush", True), True),
            verification_queries=args.get("verification_queries") if isinstance(args.get("verification_queries"), list) else None,
            top_k=args.get("top_k", self._config["top_k"]),
        )
        return pretty_json(result)

    def _tool_verify_session(self, args: dict[str, Any]) -> str:
        queries = args.get("verification_queries") if isinstance(args.get("verification_queries"), list) else []
        if not queries:
            return _tool_error("verification_queries is required")
        result = verify_session_ingest(
            client=self._client,
            user_id=self._user_id,
            session_id=str(args.get("session_id") or self._session_id or "") or None,
            scope=normalize_scope(str(args.get("scope") or "personal")),
            verification_queries=[str(query) for query in queries],
            memory_types=args.get("memory_types") if isinstance(args.get("memory_types"), list) else None,
            top_k=args.get("top_k", self._config["top_k"]),
        )
        return pretty_json(result)

    def sync_turn(self, user_content: str, assistant_content: str, *, session_id: str = "") -> None:
        if not self._active or not self._write_enabled or not self._config["auto_capture"] or not self._client:
            return
        clean_user = _clean_content(user_content)
        clean_assistant = _clean_content(assistant_content)
        sid = session_id or self._session_id
        skip, _reason = should_skip_capture(clean_user, clean_assistant, session_id=sid)
        if skip:
            return
        now = now_ms()
        personal_messages = _build_personal_turn_messages(clean_user, clean_assistant, session_id=sid, now_ms=now)
        agent_result = self._build_agent_summary_result(clean_user, clean_assistant, session_id=sid, now_ms=now + 2)

        def _run() -> None:
            mode = str(self._config.get("agent_capture_mode", "parallel"))
            write_personal = mode != "agent_only"
            write_agent = bool(self._config.get("capture_agent_memory")) and bool(self._config.get("agent_summary_after_turn")) and mode != "off"
            if write_personal:
                try:
                    result = self._client.add_memories(user_id=self._user_id, session_id=sid, messages=personal_messages, async_mode=True, scope="personal")
                    self._last_write_status = {"ok": True, "scope": "personal", "task_id": _task_id(result)}
                    if self._config["flush_after_turn"]:
                        self._client.flush_memories(user_id=self._user_id, session_id=sid, scope="personal")
                except Exception as exc:
                    self._record_status("sync_turn.personal", False, exc)
            if write_agent:
                self._write_agent_trajectory(agent_result, session_id=sid, flush_allowed=True, operation="sync_turn.agent")

        self._start_thread(_run, "everos-sync-turn")

    def _build_agent_summary_result(self, user_content: str, assistant_content: str, *, session_id: str, now_ms: int) -> TrajectoryBuildResult:
        messages = [
            {"role": "user", "timestamp": now_ms, "content": f"Task request: {_truncate_for_memory(user_content, 4000)}"},
            {
                "role": "assistant",
                "timestamp": now_ms + 1,
                "content": "Agent response summary: "
                f"{_truncate_for_memory(assistant_content, 4000)}\nOutcome: completed_or_partial\nReusable lesson hint: capture approach, correction, and verification if useful.",
            },
        ]
        return self._build_agent_trajectory_result(messages, source="sync_turn", session_id=session_id, now=now_ms, max_messages=2)

    def _agent_visibility_queries(self, texts: list[str], *, session_id: str | None, markers: list[str] | None = None) -> list[str]:
        configured = self._config.get("agent_visibility_queries")
        if isinstance(configured, list):
            queries = [str(query).strip() for query in configured if str(query).strip()]
            if queries:
                return queries
        queries = [str(text).strip()[:200] for text in texts if str(text).strip()]
        for marker in markers or []:
            marker = str(marker).strip()
            if marker:
                queries.append(marker)
        if session_id:
            queries.append(f"session:{session_id}")
        return queries[:2] or ["agent memory"]

    def _agent_visibility_after_write(
        self,
        *,
        session_id: str | None,
        texts: list[str],
        markers: list[str] | None,
        agent_raw_queued: bool,
        agent_flush: dict[str, Any] | None,
        flush_completed: bool,
        record_status: bool = False,
    ) -> dict[str, Any]:
        should_audit = bool(self._config.get("agent_visibility_verify_after_write")) or (
            flush_completed and bool(self._config.get("agent_visibility_verify_after_flush"))
        )
        if should_audit:
            visibility = audit_agent_visibility(
                client=self._client,
                user_id=self._user_id,
                session_id=session_id,
                queries=self._agent_visibility_queries(texts, session_id=session_id, markers=markers),
                top_k=int(self._config.get("agent_visibility_top_k", 5)),
                timeout=float(self._config.get("agent_visibility_timeout", 30.0)),
                get_page_size=int(self._config.get("agent_visibility_get_page_size", 20)),
            )
            visibility["agent_raw_queued"] = agent_raw_queued
            visibility["agent_flush"] = agent_flush
        else:
            visibility = build_agent_visibility_report(
                agent_raw_queued=agent_raw_queued,
                agent_flush=agent_flush,
                checks=[],
            )
        if record_status:
            self._last_agent_visibility_status = visibility
        return visibility

    def _write_agent_trajectory(self, result: TrajectoryBuildResult, *, session_id: str, flush_allowed: bool, operation: str) -> bool:
        if not result.messages or not self._client:
            return False
        if not self._remember_agent_fingerprint(result.fingerprint):
            self._last_agent_write_status = {"ok": True, "scope": "agent", "deduped": True, "operation": operation}
            return False
        queued = False
        try:
            add = self._client.add_memories(user_id=self._user_id, session_id=session_id, messages=result.messages, async_mode=True, scope="agent")
            queued = True
            self._last_agent_write_status = {
                "ok": True, "scope": "agent", "task_id": _task_id(add), "operation": operation, "output_count": len(result.messages)
            }
            flush_payload: dict[str, Any] | None = None
            if flush_allowed and self._config.get("agent_flush_after_turn"):
                flush, attempt_count = flush_memories_with_retry(
                    self._client,
                    user_id=self._user_id,
                    session_id=session_id,
                    scope="agent",
                    include_timeout=False,
                )
                flush_payload = _flush_result_payload(flush, attempt_count=attempt_count)
            self._agent_visibility_after_write(
                session_id=session_id,
                texts=[str(message.get("content") or "") for message in result.messages],
                markers=[operation],
                agent_raw_queued=True,
                agent_flush=flush_payload,
                flush_completed=flush_payload is not None,
                record_status=True,
            )
            return True
        except Exception as exc:
            if queued:
                return True
            self._agent_saved_fingerprints.pop(result.fingerprint, None)
            self._record_status(operation, False, exc, agent=True)
            return False

    def _remember_agent_fingerprint(self, fingerprint: str) -> bool:
        if fingerprint in self._agent_saved_fingerprints:
            self._agent_saved_fingerprints.move_to_end(fingerprint)
            return False
        self._agent_saved_fingerprints[fingerprint] = time.monotonic()
        max_entries = int(self._config.get("agent_dedupe_entries", 256))
        while len(self._agent_saved_fingerprints) > max_entries:
            self._agent_saved_fingerprints.popitem(last=False)
        return True

    def on_memory_write(self, action: str, target: str, content: str, metadata: dict[str, Any] | None = None) -> None:
        if action not in ("add", "replace", "update") or not content or not self._active or not self._write_enabled or not self._client:
            return
        text = f"Hermes {target} memory: {content.strip()}"

        def _run() -> None:
            try:
                result = self._client.add_memories(
                    user_id=self._user_id,
                    session_id=self._session_id,
                    messages=[{"role": "user", "timestamp": now_ms(), "content": text}],
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
        if self._config.get("capture_agent_memory") and self._config.get("agent_trajectory_on_session_end"):
            result = self._build_agent_trajectory_result(messages, source="session_end")
            self._write_agent_trajectory(result, session_id=self._session_id, flush_allowed=True, operation="session_end.agent")
        try:
            self._client.flush_memories(user_id=self._user_id, session_id=self._session_id, scope="personal")
        except Exception as exc:
            self._record_status("session_end.flush", False, exc)
            return None

    def on_pre_compress(self, messages: list[dict[str, Any]]) -> str:
        if not self._active or not self._write_enabled or not self._client or not self._session_id:
            return ""
        if not self._config.get("capture_agent_memory") or not self._config.get("agent_trajectory_on_pre_compress"):
            return ""
        result = self._build_agent_trajectory_result(messages, source="pre_compress")
        wrote = self._write_agent_trajectory(result, session_id=self._session_id, flush_allowed=False, operation="pre_compress.agent")
        if not wrote:
            return ""
        return f"EverOS captured {len(result.messages)} agent trajectory messages for session {self._session_id}; preserve task outcome and reusable tool lessons."

    def on_delegation(self, task: str, result: str, *, child_session_id: str = "", **kwargs: Any) -> None:
        if not self._active or not self._write_enabled or not self._client or not self._session_id:
            return
        if not self._config.get("capture_agent_memory") or not self._config.get("agent_trajectory_on_delegation"):
            return
        child = str(child_session_id or kwargs.get("session_id") or "").strip()
        prefix = f"[delegation child_session_id={child}] " if child else "[delegation] "
        now = now_ms()
        messages = [
            {"role": "user", "timestamp": now, "content": str(task or "").strip()},
            {"role": "assistant", "timestamp": now + 1, "content": prefix + str(result or "").strip()},
        ]
        built = self._build_agent_trajectory_result(messages, source="delegation", now=now)
        if child:
            for message in built.messages:
                if message.get("role") == "assistant":
                    message["child_session_id"] = child
        self._write_agent_trajectory(built, session_id=self._session_id, flush_allowed=True, operation="delegation.agent")

    def _build_agent_trajectory_result(
        self,
        messages: list[dict[str, Any]],
        *,
        source: TrajectorySource,
        session_id: str = "",
        now: int | None = None,
        max_messages: int | None = None,
    ) -> TrajectoryBuildResult:
        return build_agent_trajectory_messages(
            messages,
            session_id=session_id or self._session_id,
            source=source,
            now_ms=now if now is not None else now_ms(),
            max_messages=max_messages if max_messages is not None else self._config["agent_max_messages"],
            max_message_chars=self._config["agent_max_message_chars"],
            max_tool_result_chars=self._config["agent_max_tool_result_chars"],
            max_payload_chars=self._config["agent_max_payload_chars"],
        )

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
            status["error"] = sanitized_error_message(exc)
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
            fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_APPEND, 0o600)
            try:
                os.fchmod(fd, 0o600)
                with os.fdopen(fd, "a", encoding="utf-8") as fh:
                    fd = -1
                    fh.write(json.dumps(safe, ensure_ascii=False, sort_keys=True) + "\n")
            finally:
                if fd != -1:
                    os.close(fd)
        except Exception:
            return


def _task_id(response: dict[str, Any]) -> str:
    data = response.get("data", {}) if isinstance(response, dict) else {}
    return str(data.get("task_id") or "") if isinstance(data, dict) else ""


def _build_personal_turn_messages(user_content: str, assistant_content: str, *, session_id: str, now_ms: int) -> list[dict[str, Any]]:
    messages = [
        {"role": "user", "timestamp": now_ms, "content": user_content},
        {"role": "assistant", "timestamp": now_ms + 1, "content": assistant_content},
    ]
    for index, message in enumerate(messages):
        message["message_id"] = _personal_message_id(
            session_id=session_id,
            role=str(message["role"]),
            index=index,
            timestamp=message["timestamp"],
            content=str(message["content"]),
        )
    return messages


def _personal_message_id(*, session_id: str, role: str, index: int, timestamp: int, content: str) -> str:
    payload = json.dumps(
        {
            "session_id": session_id,
            "role": role,
            "index": index,
            "timestamp": timestamp,
            "content": content,
        },
        ensure_ascii=False,
        sort_keys=True,
        separators=(",", ":"),
    )
    return "eh_" + hashlib.sha256(payload.encode("utf-8")).hexdigest()[:32]


def _merge_agent_response(main_response: dict[str, Any] | None, agent_response: dict[str, Any] | None) -> dict[str, Any]:
    merged = {"data": dict(_response_data(main_response))}
    agent_data = _response_data(agent_response)
    for key in ("agent_skills", "agent_cases"):
        if agent_data.get(key):
            merged["data"][key] = _as_list_copy(merged["data"].get(key)) + _as_list_copy(agent_data.get(key))
    if agent_data.get("agent_memory"):
        merged["data"]["agent_memory"] = _as_list_copy(merged["data"].get("agent_memory")) + _as_list_copy(agent_data.get("agent_memory"))
    elif agent_data.get("results") or agent_data.get("memories") or agent_data.get("episodes"):
        merged["data"]["agent_memory"] = _as_list_copy(merged["data"].get("agent_memory")) + _as_list_copy(
            agent_data.get("results") or agent_data.get("memories") or agent_data.get("episodes")
        )
    return merged


def _response_data(response: dict[str, Any] | None) -> dict[str, Any]:
    if not isinstance(response, dict):
        return {}
    data = response.get("data", response)
    return data if isinstance(data, dict) else {}


def _as_list_copy(value: Any) -> list[Any]:
    if value is None:
        return []
    if isinstance(value, list):
        return list(value)
    return [value]



def _truncate_for_memory(text: str, limit: int) -> str:
    text = (text or "").strip()
    if len(text) <= limit:
        return text
    return text[:limit] + "…"


def register(ctx: Any) -> None:
    ctx.register_memory_provider(EverOSMemoryProvider())
