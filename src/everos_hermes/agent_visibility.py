from __future__ import annotations

import time
from typing import Any

from .client import EverOSError
from .redaction import sanitized_error_message
from .response_normalization import count_hits, response_summary


def build_agent_visibility_report(
    *,
    agent_raw_queued: bool | None,
    agent_flush: dict[str, Any] | None,
    checks: list[dict[str, Any]],
    user_id: str | None = None,
    session_id: str | None = None,
) -> dict[str, Any]:
    normalized_checks = [dict(check) for check in checks]
    executed = bool(normalized_checks)
    hit_checks = [check for check in normalized_checks if int(check.get("hit_count") or 0) > 0]
    error_checks = [check for check in normalized_checks if check.get("status") == "error"]

    if not executed:
        structured_visible: bool | None = None
        status = "unchecked"
    elif hit_checks and len(hit_checks) == len(normalized_checks):
        structured_visible = True
        status = "visible"
    elif hit_checks:
        structured_visible = True
        status = "partial"
    elif error_checks:
        structured_visible = False
        status = "error"
    else:
        structured_visible = False
        status = "not_visible"

    report = {
        "agent_raw_queued": agent_raw_queued,
        "agent_flush": agent_flush,
        "agent_structured_visible": structured_visible,
        "agent_visibility_status": status,
        "agent_visibility_checks": normalized_checks,
    }
    if user_id is not None:
        report["verification_user_id"] = user_id
    if session_id is not None:
        report["verification_session_id"] = session_id
    return report


def workflow_status_from_agent_visibility(agent_visibility: dict[str, Any], fallback: str) -> str:
    status = agent_visibility.get("agent_visibility_status")
    if status == "visible":
        return "verified"
    if status == "partial":
        return "partially_verified"
    if status == "not_visible":
        return "agent_not_visible"
    if status == "error":
        return "agent_visibility_error"
    return fallback


def audit_agent_visibility(
    *,
    client: Any,
    user_id: str,
    session_id: str | None,
    queries: list[str],
    top_k: int = 5,
    timeout: float | None = None,
    get_page_size: int = 20,
    precomputed_search_responses: dict[str, dict[str, Any]] | None = None,
) -> dict[str, Any]:
    checks: list[dict[str, Any]] = []
    clean_queries = [str(query).strip() for query in queries if str(query).strip()]
    precomputed = precomputed_search_responses or {}

    for query in clean_queries:
        started = time.perf_counter()
        check: dict[str, Any] = {
            "kind": "search",
            "user_id": user_id,
            "session_id": session_id,
            "memory_types": ["agent_memory"],
            "query": query,
        }
        try:
            response = precomputed.get(query)
            if response is None:
                response = client.search_memories(
                    query=query,
                    user_id=user_id,
                    session_id=session_id,
                    method="hybrid",
                    memory_types=["agent_memory"],
                    top_k=top_k,
                    include_original_data=False,
                    include_vectors=False,
                    timeout=timeout,
                )
            _record_successful_check(check, response)
        except EverOSError as exc:
            check.update({"status": "error", "hit_count": 0, "error": sanitized_error_message(exc)})
        finally:
            check["latency_ms"] = _elapsed_ms(started)
        checks.append(check)

    for memory_type in ("agent_case", "agent_skill"):
        started = time.perf_counter()
        check = {"kind": "get", "user_id": user_id, "session_id": session_id, "memory_type": memory_type}
        try:
            response = client.get_memories(
                user_id=user_id,
                session_id=session_id,
                memory_type=memory_type,
                page=1,
                page_size=get_page_size,
            )
            _record_successful_check(check, response)
        except EverOSError as exc:
            check.update({"status": "error", "hit_count": 0, "error": sanitized_error_message(exc)})
        finally:
            check["latency_ms"] = _elapsed_ms(started)
        checks.append(check)

    return build_agent_visibility_report(
        agent_raw_queued=None,
        agent_flush=None,
        checks=checks,
        user_id=user_id,
        session_id=session_id,
    )


def _record_successful_check(check: dict[str, Any], response: dict[str, Any]) -> None:
    hit_count = count_hits(response)
    check.update({"status": "hit" if hit_count else "miss", "hit_count": hit_count, "response_summary": response_summary(response)})


def _elapsed_ms(started: float) -> float:
    return round((time.perf_counter() - started) * 1000.0, 3)
