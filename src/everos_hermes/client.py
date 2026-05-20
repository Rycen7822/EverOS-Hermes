from __future__ import annotations

import json
import urllib.error
import urllib.parse
import urllib.request
from typing import Any, Mapping

from .env import get_env
from .formatting import strip_vectors
from .redaction import sanitized_error_message
from .schemas import (
    build_filters,
    normalize_rank_order,
    normalize_scope,
    settings_diff,
    validate_delete_request,
    validate_get_params,
    validate_messages,
    validate_search_params,
    validate_settings_update,
)

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
                status_code = int(getattr(resp, "status", 0) or 0)
                if not raw:
                    return {"ok": True, "status_code": status_code}
                parsed = json.loads(raw)
                if isinstance(parsed, dict):
                    parsed.setdefault("status_code", status_code)
                    return parsed
                return {"data": parsed, "status_code": status_code}
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
        agent: bool | None = None,
        scope: str | None = None,
    ) -> dict[str, Any]:
        resolved_scope = normalize_scope(scope, agent)
        validate_messages(messages, resolved_scope)
        body: dict[str, Any] = {
            "user_id": user_id,
            "session_id": session_id,
            "messages": messages,
            "async_mode": async_mode,
        }
        path = "/api/v1/memories/agent" if resolved_scope == "agent" else "/api/v1/memories"
        return self.request_json("POST", path, body)

    def flush_memories(
        self,
        *,
        user_id: str,
        session_id: str | None = None,
        agent: bool | None = None,
        scope: str | None = None,
        timeout: float | None = None,
    ) -> dict[str, Any]:
        resolved_scope = normalize_scope(scope, agent)
        path = "/api/v1/memories/agent/flush" if resolved_scope == "agent" else "/api/v1/memories/flush"
        return self.request_json("POST", path, {"user_id": user_id, "session_id": session_id}, timeout=timeout)

    def get_memories(
        self,
        *,
        user_id: str | None = None,
        session_id: str | None = None,
        filters: dict[str, Any] | None = None,
        memory_type: str = "episodic_memory",
        page: int = 1,
        page_size: int = 20,
        rank_by: str = "timestamp",
        rank_order: str = "desc",
    ) -> dict[str, Any]:
        normalized_rank_order = normalize_rank_order(rank_order)
        validate_get_params(memory_type, page, page_size, rank_by, normalized_rank_order)
        resolved_filters = build_filters(user_id=user_id, session_id=session_id, filters=filters)
        return self.request_json(
            "POST",
            "/api/v1/memories/get",
            {
                "memory_type": memory_type,
                "filters": resolved_filters,
                "page": page,
                "page_size": page_size,
                "rank_by": rank_by,
                "rank_order": normalized_rank_order,
            },
        )

    def search_memories(
        self,
        *,
        query: str,
        user_id: str | None = None,
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
        resolved_memory_types = list(memory_types or DEFAULT_MEMORY_TYPES)
        normalized_method = method.strip().lower()
        validate_search_params(normalized_method, resolved_memory_types, top_k, radius)
        resolved_filters = build_filters(user_id=user_id, session_id=session_id, filters=filters)
        effective_timeout = 60.0 if normalized_method == "agentic" and timeout is None else timeout
        body = {
            "query": query,
            "filters": resolved_filters,
            "method": normalized_method,
            "memory_types": resolved_memory_types,
            "top_k": top_k,
            "radius": radius,
            "include_original_data": include_original_data,
        }
        response = self.request_json("POST", "/api/v1/memories/search", body, timeout=effective_timeout)
        return response if include_vectors else strip_vectors(response)

    def delete_memories(
        self,
        *,
        memory_id: str | None = None,
        user_id: str | None = None,
        session_id: str | None = None,
    ) -> dict[str, Any]:
        validate_delete_request(memory_id=memory_id, user_id=user_id, session_id=session_id)
        body: dict[str, Any]
        if memory_id:
            body = {"memory_id": memory_id}
            mode = "single"
        else:
            body = {"user_id": user_id, "session_id": session_id}
            mode = "batch"
        response = self.request_json("POST", "/api/v1/memories/delete", body)
        if response.get("status_code") == 204:
            response.update({"deleted": True, "mode": mode})
        return response

    def get_task_status(self, task_id: str) -> dict[str, Any]:
        quoted = urllib.parse.quote(task_id, safe="")
        return self.request_json("GET", f"/api/v1/tasks/{quoted}")

    def get_settings(self) -> dict[str, Any]:
        return self.request_json("GET", "/api/v1/settings")

    def update_settings(self, settings: dict[str, Any], *, strict: bool = True, return_diff: bool = True) -> dict[str, Any]:
        validated = validate_settings_update(settings, strict=strict)
        before = self.get_settings() if return_diff else {}
        response = self.request_json("PUT", "/api/v1/settings", validated)
        after = self.get_settings() if return_diff else response
        if return_diff:
            response["diff"] = settings_diff(before, after, validated)
            response["updated"] = after.get("data", after)
        return response


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
    safe_detail = sanitized_error_message(detail or str(exc.reason))
    return EverOSError(f"EverOS API error {safe_detail}")


def _timeout_error(method: str, path: str) -> EverOSTimeoutError:
    return EverOSTimeoutError(
        f"EverOS request timed out during {method.upper()} {_normalize_path(path)}. "
        "The server may still be processing the request; search existing memories or check a prior task/request id before retrying."
    )
