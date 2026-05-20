import json
from pathlib import Path
from typing import Any


def _snapshot_path(name: str) -> Path:
    return Path(__file__).parent / "contracts" / name


def _simplify_provider_property(schema: dict[str, Any]) -> dict[str, Any]:
    keep = {}
    for key in ("type", "enum", "default", "description"):
        if key in schema:
            keep[key] = schema[key]
    return keep


def _provider_snapshot() -> list[dict[str, Any]]:
    from everos_hermes.provider import EverOSMemoryProvider

    provider = EverOSMemoryProvider()
    snapshot = []
    for schema in provider.get_tool_schemas():
        params = schema["parameters"]
        properties = params.get("properties", {})
        snapshot.append(
            {
                "name": schema["name"],
                "description": schema.get("description", ""),
                "required": sorted(params.get("required", [])),
                "properties": {key: _simplify_provider_property(properties[key]) for key in sorted(properties)},
            }
        )
    return snapshot


def _description_summary(text: str) -> str:
    text = " ".join(str(text or "").strip().split())
    if ". " in text:
        return text.split(". ", 1)[0] + "."
    return text


def _annotations_dict(annotations: Any) -> dict[str, Any]:
    if annotations is None:
        return {}
    raw = getattr(annotations, "__dict__", {})
    return {key: raw[key] for key in sorted(raw) if raw[key] is not None}


def _mcp_snapshot() -> list[dict[str, Any]]:
    from everos_hermes import mcp_server

    tools = mcp_server.mcp._tool_manager._tools
    snapshot = []
    for name in mcp_server.TOOL_NAMES:
        tool = tools[name]
        parameters = tool.parameters
        snapshot.append(
            {
                "name": name,
                "title": tool.title,
                "description_summary": _description_summary(tool.description),
                "required": sorted(parameters.get("required", [])),
                "properties": sorted(parameters.get("properties", {}).keys()),
                "annotations": _annotations_dict(tool.annotations),
            }
        )
    return snapshot


def test_provider_tool_schemas_match_snapshot():
    expected = json.loads(_snapshot_path("provider_tools.snapshot.json").read_text(encoding="utf-8"))
    assert _provider_snapshot() == expected


def test_mcp_tool_schemas_match_snapshot():
    expected = json.loads(_snapshot_path("mcp_tools.snapshot.json").read_text(encoding="utf-8"))
    assert _mcp_snapshot() == expected
