from __future__ import annotations

import json
from typing import Any

VECTOR_KEYS = {"vector", "embedding", "embeddings"}


def compact_json(data: Any) -> str:
    return json.dumps(data, ensure_ascii=False, separators=(",", ":"))


def pretty_json(data: Any) -> str:
    return json.dumps(data, ensure_ascii=False, indent=2)


def strip_vectors(data: Any) -> Any:
    """Return a deep copy with embedding/vector payloads removed.

    EverOS debug/original-data responses can include large embedding vectors.
    Those are rarely useful to an LLM and can dominate MCP context, so callers
    should strip them unless a human explicitly requests vector debugging.
    """
    if isinstance(data, dict):
        return {key: strip_vectors(value) for key, value in data.items() if key not in VECTOR_KEYS}
    if isinstance(data, list):
        return [strip_vectors(item) for item in data]
    return data


def format_search_context(response: dict[str, Any], *, max_items: int = 5) -> str:
    """Convert flexible EverOS search/get responses into compact context text."""
    data = response.get("data", response) if isinstance(response, dict) else {}
    if not isinstance(data, dict):
        return ""

    lines: list[str] = []
    agent_memory_raw = data.get("agent_memory")
    agent_memory: dict[str, Any] = agent_memory_raw if isinstance(agent_memory_raw, dict) else {}
    episode_lines = _format_episodes(_as_list(data.get("episodes") or data.get("results") or data.get("memories")), max_items=max_items)
    profile_lines = _format_profiles(_as_list(data.get("profiles") or data.get("profile")), max_items=max_items)
    raw_lines = _format_raw_messages(_as_list(data.get("raw_messages")), max_items=max_items)
    agent_case_lines = _format_agent_cases(_as_list(data.get("agent_cases") or agent_memory.get("cases") or agent_memory.get("agent_cases")), max_items=max_items)
    agent_skill_lines = _format_agent_skills(_as_list(data.get("agent_skills") or agent_memory.get("skills") or agent_memory.get("agent_skills")), max_items=max_items)

    if episode_lines:
        lines.append("## Episodes")
        lines.extend(episode_lines)
    if profile_lines:
        lines.append("## Profile")
        lines.extend(profile_lines)
    if raw_lines:
        lines.append("## Raw Messages")
        lines.extend(raw_lines)
    if agent_case_lines:
        lines.append("## Agent Cases")
        lines.extend(agent_case_lines)
    if agent_skill_lines:
        lines.append("## Agent Skills")
        lines.extend(agent_skill_lines)

    if not lines:
        return ""
    return "# EverOS Memory\n" + "\n".join(lines)


def _format_episodes(items: list[Any], *, max_items: int) -> list[str]:
    lines: list[str] = []
    for item in items[:max_items]:
        if not isinstance(item, dict):
            text = str(item).strip()
            if text:
                lines.append(f"- {text[:500]}")
            continue
        subject = _first_text(item, "subject", "title", "topic", "type")
        summary = _first_text(item, "summary", "episode", "content", "memory", "text", "narrative")
        score = item.get("score") or item.get("relevance_score") or item.get("similarity")
        prefix = f"- {subject}: " if subject else "- "
        suffix = _format_score(score)
        body = (summary or compact_json(item))[:700]
        lines.append(f"{prefix}{body}{suffix}")
    return lines


def _format_profiles(items: list[Any], *, max_items: int) -> list[str]:
    lines: list[str] = []
    for item in items[:max_items]:
        if not isinstance(item, dict):
            text = str(item).strip()
            if text:
                lines.append(f"- {text[:500]}")
            continue
        profile_data = item.get("profile_data") if isinstance(item.get("profile_data"), dict) else item
        for key in ("explicit_info", "implicit_traits", "preferences", "facts", "traits"):
            value = profile_data.get(key) if isinstance(profile_data, dict) else None
            for fact in _as_list(value):
                text = _stringify_fact(fact)
                if text:
                    label = key.replace("_", " ")
                    lines.append(f"- {label}: {text[:500]}")
                    if len(lines) >= max_items:
                        return lines
        if not lines:
            lines.append(f"- {compact_json(item)[:700]}")
    return lines


def _format_raw_messages(items: list[Any], *, max_items: int) -> list[str]:
    lines: list[str] = []
    for item in items[:max_items]:
        if not isinstance(item, dict):
            text = str(item).strip()
            if text:
                lines.append(f"- {text[:500]}")
            continue
        role = _first_text(item, "role", "sender", "type")
        content = _first_text(item, "content", "text", "message", "summary") or compact_json(item)
        prefix = f"- {role}: " if role else "- "
        lines.append(f"{prefix}{content[:700]}")
    return lines


def _format_agent_cases(items: list[Any], *, max_items: int) -> list[str]:
    lines = []
    for item in items[:max_items]:
        if isinstance(item, dict):
            intent = _first_text(item, "task_intent", "intent", "name")
            approach = _first_text(item, "approach", "summary", "content")
            lines.append(f"- {intent}: {approach}" if intent else f"- {approach or compact_json(item)}")
    return lines


def _format_agent_skills(items: list[Any], *, max_items: int) -> list[str]:
    lines = []
    for item in items[:max_items]:
        if isinstance(item, dict):
            name = _first_text(item, "name", "title")
            desc = _first_text(item, "description", "content", "summary")
            lines.append(f"- {name}: {desc}" if name else f"- {desc or compact_json(item)}")
    return lines


def _as_list(value: Any) -> list[Any]:
    if value is None:
        return []
    if isinstance(value, list):
        return value
    return [value]


def _first_text(mapping: dict[str, Any], *keys: str) -> str:
    for key in keys:
        value = mapping.get(key)
        if isinstance(value, str) and value.strip():
            return value.strip()
    return ""


def _stringify_fact(value: Any) -> str:
    if isinstance(value, str):
        return value.strip()
    if isinstance(value, dict):
        return _first_text(value, "text", "content", "fact", "value", "summary") or compact_json(value)
    if value is None:
        return ""
    return str(value).strip()


def _format_score(score: Any) -> str:
    if score is None:
        return ""
    try:
        value = float(score)
        if 0 <= value <= 1:
            return f" [score={value:.2f}]"
        return f" [score={value:g}]"
    except Exception:
        return f" [score={score}]"
