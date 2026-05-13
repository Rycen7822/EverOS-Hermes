from __future__ import annotations

import re
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def test_python_source_mcp_tool_count_is_13():
    from everos_hermes import mcp_server

    tools = mcp_server.mcp._tool_manager._tools
    assert len(mcp_server.TOOL_NAMES) == 13
    assert len(tools) == 13
    assert set(tools) == set(mcp_server.TOOL_NAMES)


def test_provider_explicit_tool_schema_count_is_8(monkeypatch, tmp_path):
    from everos_hermes.provider import EverOSMemoryProvider

    monkeypatch.setenv("HERMES_HOME", str(tmp_path))
    provider = EverOSMemoryProvider()

    schemas = provider.get_tool_schemas()
    assert len(schemas) == 8
    assert all(schema["name"].startswith("everos_memory_") for schema in schemas)


def test_readme_uses_current_mcp_13_badge_and_no_stale_mcp_9_wording():
    text = (ROOT / "README.md").read_text(encoding="utf-8")

    stale_patterns = [
        r"MCP-9\b",
        r"MCP-9%20tools",
        r"MCP:\s*nine tools",
        r"alt=\"MCP: nine tools\"",
    ]
    for pattern in stale_patterns:
        assert not re.search(pattern, text, flags=re.IGNORECASE), pattern
    assert "MCP-13%20tools" in text or "MCP-13 tools" in text
    assert "<p align=\"center\">" in text
    assert "style=for-the-badge" in text
    assert "Hermes-single%20plugin" in text
    assert "Python package: `everos-hermes` `0.3.0`" in text
    assert "Rust crate/binary: `everos-hermes-rust` `0.3.0`" in text


def test_readme_documents_thin_plugin_skill_references():
    text = (ROOT / "README.md").read_text(encoding="utf-8")

    for required in [
        "everos:everos-memory-curation",
        "SKILL.md` is intentionally thin",
        "references/user-intent-runbooks.md",
        "references/memory-routing-policy.md",
        "references/agent-case-visibility.md",
        "references/plugin-triage-and-migration.md",
        "references/cleanup-and-verification.md",
        "legacy ordinary skill",
    ]:
        assert required in text


def test_readme_documents_provider_context_engine_and_rust_parity():
    text = (ROOT / "README.md").read_text(encoding="utf-8")

    for required in [
        "structured agent trajectory",
        "context assembler",
        "include_recent_raw",
        "agent_trajectory_on_session_end",
        "prefetch_cache_enabled",
        "Rust context-engine parity is current",
    ]:
        assert required in text


def test_readme_includes_agent_self_install_prompts_with_restart_reminder():
    text = (ROOT / "README.md").read_text(encoding="utf-8")

    assert "## Agent Self-Install Prompts" in text
    assert "Copy one of these prompts into Hermes, Codex, Claude Code, or another coding agent" in text
    for required in [
        "Install EverOS-Hermes for this Hermes Agent from `https://github.com/Rycen7822/EverOS-Hermes`.",
        "hermes plugins enable everos",
        "hermes config set memory.provider everos",
        "everos:everos-memory-curation",
        "After installation and verification, tell the user to reload, reset, or restart Hermes Agent",
    ]:
        assert required in text


def test_cloud_contract_keeps_out_of_scope_endpoint_blacklist():
    text = (ROOT / "docs" / "everos_cloud_v1_contract.md").read_text(encoding="utf-8")

    for forbidden in [
        "/api/v1/memories/group",
        "/api/v1/groups",
        "/api/v1/senders",
        "/api/v1/object/sign",
    ]:
        assert forbidden in text
    assert "out of scope" in text.lower()


def test_cloud_contract_documents_message_id_and_structured_agent_trajectory():
    text = (ROOT / "docs" / "everos_cloud_v1_contract.md").read_text(encoding="utf-8")

    for required in [
        "message_id",
        "optional idempotency key",
        "structured agent trajectory",
        "tool_calls",
        "tool_call_id",
        "source",
    ]:
        assert required in text
