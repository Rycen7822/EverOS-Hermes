from __future__ import annotations

import re
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def test_python_source_mcp_tool_count_is_12():
    from everos_hermes import mcp_server

    tools = mcp_server.mcp._tool_manager._tools
    assert len(mcp_server.TOOL_NAMES) == 12
    assert len(tools) == 12
    assert set(tools) == set(mcp_server.TOOL_NAMES)


def test_provider_explicit_tool_schema_count_is_8(monkeypatch, tmp_path):
    from everos_hermes.provider import EverOSMemoryProvider

    monkeypatch.setenv("HERMES_HOME", str(tmp_path))
    provider = EverOSMemoryProvider()

    schemas = provider.get_tool_schemas()
    assert len(schemas) == 8
    assert all(schema["name"].startswith("everos_memory_") for schema in schemas)


def test_readme_uses_current_mcp_12_badge_and_tracked_files_have_no_stale_tool_aliases():
    text = (ROOT / "README.md").read_text(encoding="utf-8")
    tracked = subprocess.check_output(["git", "ls-files"], cwd=ROOT, text=True).splitlines()
    corpus = "\n".join(
        (ROOT / rel).read_text(encoding="utf-8", errors="ignore")
        for rel in tracked
        if rel != "tests/test_upgrade_contract.py" and (ROOT / rel).suffix in {".md", ".py", ".rs", ".toml", ".yaml", ".yml", ".json"}
    )
    for pattern in [r"MCP-9\b", r"MCP-13\b", r"13 tools", r"everos_batch_ingest"]:
        assert not re.search(pattern, corpus, flags=re.IGNORECASE), pattern
    for required in ["MCP-12%20tools", "<p align=\"center\">", "style=for-the-badge", "Hermes-single%20plugin", "Python package: `everos-hermes` `0.3.0`", "Rust crate/binary: `everos-hermes-rust` `0.3.0`"]:
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
