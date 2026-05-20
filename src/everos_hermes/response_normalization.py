from __future__ import annotations

from typing import Any

SEARCH_KEYS = (
    "episodes",
    "profiles",
    "raw_messages",
    "agent_memory",
    "agent_cases",
    "agent_skills",
    "cases",
    "skills",
    "items",
    "results",
    "memories",
)


def response_payload(response: Any) -> Any:
    return response.get("data", response) if isinstance(response, dict) else response


def response_data(response: Any) -> dict[str, Any]:
    data = response_payload(response)
    return data if isinstance(data, dict) else {}


def as_list(value: Any) -> list[Any]:
    if value is None:
        return []
    if isinstance(value, list):
        return value
    return [value]


def count_hits(response: Any) -> int:
    return _count_hits_value(response_payload(response))


def response_summary(response: Any) -> dict[str, Any]:
    data = response_payload(response)
    hit_count = count_hits(response)
    if isinstance(data, dict):
        return {"keys": sorted(str(key) for key in data.keys()), "hit_count": hit_count}
    if isinstance(data, list):
        return {"items": len(data), "hit_count": hit_count}
    return {"type": type(data).__name__, "hit_count": hit_count}


def _count_hits_value(value: Any) -> int:
    if isinstance(value, list):
        return len(value)
    if not isinstance(value, dict):
        return 0
    total = 0
    for key, child in value.items():
        if key in SEARCH_KEYS:
            total += _count_hits_value(child)
    return total
