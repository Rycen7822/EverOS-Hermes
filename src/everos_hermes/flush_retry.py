from __future__ import annotations

from typing import Any

from .client import EverOSError, EverOSTimeoutError


def flush_memories_with_retry(
    client: Any,
    *,
    user_id: str,
    session_id: str | None,
    scope: str,
    timeout: float | None = None,
    max_attempts: int = 2,
    include_timeout: bool = True,
) -> tuple[dict[str, Any], int]:
    attempts = max(1, int(max_attempts))
    for attempt in range(1, attempts + 1):
        try:
            kwargs: dict[str, Any] = {"user_id": user_id, "session_id": session_id, "scope": scope}
            if include_timeout or timeout is not None:
                kwargs["timeout"] = timeout
            return client.flush_memories(**kwargs), attempt
        except EverOSTimeoutError:
            raise
        except EverOSError as exc:
            if attempt < attempts and is_transient_send_error(exc):
                continue
            raise
    raise EverOSError("EverOS flush retry exhausted")


def is_transient_send_error(exc: BaseException) -> bool:
    message = str(exc).lower()
    return "error sending request" in message
