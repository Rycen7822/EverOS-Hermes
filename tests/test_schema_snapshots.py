import json
from pathlib import Path
from typing import Any


def _snapshot_path(name: str) -> Path:
    return Path(__file__).parent / "contracts" / name


def _simplify_provider_property(schema: dict[str, Any]) -> dict[str, Any]:
    keep = {}
    for key in ("type", "enum"):
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
        item = {
            "name": schema["name"],
            "properties": {key: _simplify_provider_property(properties[key]) for key in sorted(properties)},
        }
        required = sorted(params.get("required", []))
        if required:
            item["required"] = required
        snapshot.append(item)
    return snapshot


def _annotation_profile(annotations: Any) -> str:
    raw = getattr(annotations, "__dict__", {}) if annotations is not None else {}
    return ":".join(
        [
            "read" if raw.get("readOnlyHint") else "write",
            "destructive" if raw.get("destructiveHint") else "safe",
            "idem" if raw.get("idempotentHint") else "nonidem",
            "open" if raw.get("openWorldHint") else "closed",
        ]
    )


def _output_shape(output_schema: dict[str, Any]) -> Any:
    required = sorted(output_schema.get("required", []))
    properties = sorted(output_schema.get("properties", {}).keys())
    if required == ["result"] and properties == ["result"]:
        return "result"
    if not required and properties == ["ok", "retryable", "status", "suggested_next_actions", "workflow"]:
        return "workflow"
    return {"required": required, "properties": properties}


def _mcp_snapshot() -> list[dict[str, Any]]:
    from everos_hermes import mcp_server

    tools = mcp_server.mcp._tool_manager._tools
    snapshot = []
    for name in mcp_server.TOOL_NAMES:
        tool = tools[name]
        parameters = tool.parameters
        output_schema = tool.output_schema or {}
        item = {
            "name": name,
            "annotation_profile": _annotation_profile(tool.annotations),
        }
        output_shape = _output_shape(output_schema)
        if output_shape != "result":
            item["output_shape"] = output_shape
        required = sorted(parameters.get("required", []))
        properties = sorted(parameters.get("properties", {}).keys())
        if required:
            item["required"] = required
        if properties:
            item["properties"] = properties
        snapshot.append(item)
    return snapshot


def test_provider_tool_schemas_match_snapshot():
    expected = json.loads(_snapshot_path("provider_tools.snapshot.json").read_text(encoding="utf-8"))
    assert _provider_snapshot() == expected


def test_mcp_tool_schemas_match_snapshot():
    expected = json.loads(_snapshot_path("mcp_tools.snapshot.json").read_text(encoding="utf-8"))
    assert _mcp_snapshot() == expected
