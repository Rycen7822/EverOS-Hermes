from __future__ import annotations

import json
import urllib.error
import urllib.parse
import urllib.request
from typing import Any, Mapping

from .env import get_env
from .formatting import strip_vectors

DEFAULT_BASE_URL = "https://api.evermind.ai"
DEFAULT_TIMEOUT = 10.0
DEFAULT_MEMORY_TYPES = ["episodic_memory", "profile"]


class EverOSError(RuntimeError):
    """Raised when an EverOS API request fails."""


class EverOSTimeoutError(EverOSError):
    """Raised when an EverOS request times out but may still be processing."""

    retryable = True
    suggested_next_actions = [
        "search existing memories before retrying, because the server may have completed the request after the client timed out",
        "if the operation returned a task_id or request_id earlier, check that status before issuing another write/flush",
        "retry with a longer timeout only if search/status checks do not show the expected result",
    ]


class EverOSClient:
    """Small stdlib-only client for EverOS v1 memory APIs.

    EverOS docs: https://docs.evermind.ai/llms.txt
    Base URL: https://api.evermind.ai
    Auth: Authorization: Bearer <EVEROS_API_KEY>
    """

    def __init__(self, api_key: str | None = None, base_url: str | None = None, timeout: float | None = None):
        self.api_key = (api_key if api_key is not None else get_env("EVEROS_API_KEY", "")).strip()
        self.base_url = _normalize_base_url(base_url or get_env("EVEROS_BASE_URL", DEFAULT_BASE_URL))
        self.timeout = float(timeout if timeout is not None else get_env("EVEROS_TIMEOUT", str(DEFAULT_TIMEOUT)))
        if not self.api_key:
            raise EverOSError("EVEROS_API_KEY is required. Create one at https://everos.evermind.ai/api-keys")

    def request_json(self, method: str, path: str, body: Mapping[str, Any] | None = None, *, timeout: float | None = None) -> dict[str, Any]:
        url = f"{self.base_url}{_normalize_path(path)}"
        headers = {
            "Authorization": f"Bearer {self.api_key}",
            "Content-Type": "application/json",
            "Accept": "application/json",
        }
        data = None if body is None else json.dumps(_drop_none(dict(body)), ensure_ascii=False).encode("utf-8")
        req = urllib.request.Request(url, data=data, headers=headers, method=method.upper())
        try:
            with urllib.request.urlopen(req, timeout=self.timeout if timeout is None else timeout) as resp:
                raw = resp.read().decode("utf-8")
                if not raw:
                    return {}
                parsed = json.loads(raw)
                return parsed if isinstance(parsed, dict) else {"data": parsed}
        except urllib.error.HTTPError as exc:
            raise _http_error_to_everos_error(exc) from exc
        except urllib.error.URLError as exc:
            if isinstance(exc.reason, TimeoutError):
                raise _timeout_error(method, path) from exc
            raise EverOSError(f"EverOS request failed: {exc.reason}") from exc
        except TimeoutError as exc:
            raise _timeout_error(method, path) from exc
        except json.JSONDecodeError as exc:
            raise EverOSError(f"EverOS returned invalid JSON from {url}: {exc}") from exc

    def add_memories(
        self,
        *,
        user_id: str,
        messages: list[dict[str, Any]],
        session_id: str | None = None,
        async_mode: bool = True,
        agent: bool = False,
    ) -> dict[str, Any]:
        body: dict[str, Any] = {
            "user_id": user_id,
            "session_id": session_id,
            "messages": messages,
            "async_mode": async_mode,
        }
        path = "/api/v1/memories/agent" if agent else "/api/v1/memories"
        return self.request_json("POST", path, body)

    def add_group_memories(
        self,
        *,
        group_id: str,
        messages: list[dict[str, Any]],
        group_meta: dict[str, Any] | None = None,
        async_mode: bool = True,
    ) -> dict[str, Any]:
        return self.request_json(
            "POST",
            "/api/v1/memories/group",
            {"group_id": group_id, "group_meta": group_meta, "messages": messages, "async_mode": async_mode},
        )

    def flush_memories(self, *, user_id: str, session_id: str | None = None, agent: bool = False, timeout: float | None = None) -> dict[str, Any]:
        path = "/api/v1/memories/agent/flush" if agent else "/api/v1/memories/flush"
        return self.request_json("POST", path, {"user_id": user_id, "session_id": session_id}, timeout=timeout)

    def flush_group_memories(self, *, group_id: str, timeout: float | None = None) -> dict[str, Any]:
        return self.request_json("POST", "/api/v1/memories/group/flush", {"group_id": group_id}, timeout=timeout)

    def get_memories(
        self,
        *,
        user_id: str | None = None,
        group_id: str | None = None,
        session_id: str | None = None,
        filters: dict[str, Any] | None = None,
        memory_type: str = "episodic_memory",
        page: int = 1,
        page_size: int = 20,
        rank_by: str = "timestamp",
        rank_order: str = "desc",
    ) -> dict[str, Any]:
        resolved_filters = _build_filters(user_id=user_id, group_id=group_id, session_id=session_id, filters=filters)
        return self.request_json(
            "POST",
            "/api/v1/memories/get",
            {
                "memory_type": memory_type,
                "filters": resolved_filters,
                "page": page,
                "page_size": page_size,
                "rank_by": rank_by,
                "rank_order": rank_order,
            },
        )

    def search_memories(
        self,
        *,
        query: str,
        user_id: str | None = None,
        group_id: str | None = None,
        session_id: str | None = None,
        filters: dict[str, Any] | None = None,
        method: str = "hybrid",
        memory_types: list[str] | None = None,
        top_k: int = 5,
        radius: float | None = None,
        include_original_data: bool = False,
        include_vectors: bool = False,
        timeout: float | None = None,
    ) -> dict[str, Any]:
        resolved_filters = _build_filters(user_id=user_id, group_id=group_id, session_id=session_id, filters=filters)
        body = {
            "query": query,
            "filters": resolved_filters,
            "method": method,
            "memory_types": list(memory_types or DEFAULT_MEMORY_TYPES),
            "top_k": top_k,
            "radius": radius,
            "include_original_data": include_original_data,
        }
        response = self.request_json("POST", "/api/v1/memories/search", body, timeout=timeout)
        return response if include_vectors else strip_vectors(response)

    def delete_memories(
        self,
        *,
        memory_id: str | None = None,
        user_id: str | None = None,
        group_id: str | None = None,
        session_id: str | None = None,
    ) -> dict[str, Any]:
        if memory_id:
            body = {"memory_id": memory_id}
        else:
            body = {"user_id": user_id, "group_id": group_id, "session_id": session_id}
        return self.request_json("POST", "/api/v1/memories/delete", body)

    def get_task_status(self, task_id: str) -> dict[str, Any]:
        quoted = urllib.parse.quote(task_id, safe="")
        return self.request_json("GET", f"/api/v1/tasks/{quoted}")

    def get_settings(self) -> dict[str, Any]:
        return self.request_json("GET", "/api/v1/settings")

    def update_settings(self, settings: dict[str, Any]) -> dict[str, Any]:
        return self.request_json("PUT", "/api/v1/settings", settings)


def _normalize_base_url(url: str) -> str:
    url = (url or DEFAULT_BASE_URL).strip().rstrip("/")
    parsed = urllib.parse.urlparse(url)
    if parsed.scheme not in ("http", "https") or not parsed.netloc:
        raise EverOSError(f"Invalid EVEROS_BASE_URL: {url!r}")
    return url


def _normalize_path(path: str) -> str:
    return path if path.startswith("/") else f"/{path}"


def _drop_none(obj: dict[str, Any]) -> dict[str, Any]:
    return {k: v for k, v in obj.items() if v is not None}


def _build_filters(
    *,
    user_id: str | None = None,
    group_id: str | None = None,
    session_id: str | None = None,
    filters: dict[str, Any] | None = None,
) -> dict[str, Any]:
    resolved = dict(filters or {})
    if user_id is not None:
        resolved["user_id"] = user_id
    if group_id is not None:
        resolved["group_id"] = group_id
    if session_id:
        clauses = list(resolved.get("AND") or [])
        clauses.append({"session_id": session_id})
        resolved["AND"] = clauses
    return resolved


def _http_error_to_everos_error(exc: urllib.error.HTTPError) -> EverOSError:
    raw = ""
    try:
        raw = exc.read().decode("utf-8")
    except Exception:
        raw = ""
    detail = raw
    try:
        parsed = json.loads(raw) if raw else {}
        if isinstance(parsed, dict):
            bits = [str(exc.code)]
            code = parsed.get("code")
            message = parsed.get("message")
            request_id = parsed.get("request_id")
            if code:
                bits.append(str(code))
            if message:
                bits.append(str(message))
            if request_id:
                bits.append(f"request_id={request_id}")
            detail = ": ".join(bits)
    except Exception:
        pass
    return EverOSError(f"EverOS API error {detail or exc.reason}")


def _timeout_error(method: str, path: str) -> EverOSTimeoutError:
    return EverOSTimeoutError(
        f"EverOS request timed out during {method.upper()} {_normalize_path(path)}. "
        "The server may still be processing the request; search existing memories or check a prior task/request id before retrying."
    )
