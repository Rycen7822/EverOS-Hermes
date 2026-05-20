from __future__ import annotations

import re
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]






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
    for required in ["MCP-12%20tools", "<p align=\"center\">", "style=for-the-badge", "Hermes-single%20plugin"]:
        assert required in text


def test_readme_includes_agent_self_install_prompts_with_restart_reminder():
    text = (ROOT / "README.md").read_text(encoding="utf-8")

    assert "## Agent Self-Install Prompts" in text
    for required in ["hermes plugins enable everos", "everos:everos-memory-curation", "reload, reset, or restart Hermes Agent"]:
        assert required in text
