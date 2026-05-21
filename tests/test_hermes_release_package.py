from __future__ import annotations

import subprocess
import sys
import zipfile
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
SKILL_REF_DIR = REPO_ROOT / "integrations" / "hermes" / "resources" / "skills" / "everos-memory-curation" / "references"
EXPECTED_SKILL_REFERENCES = sorted(path.name for path in SKILL_REF_DIR.glob("*.md"))


def test_python_wheel_includes_single_plugin_manifest_and_skill_resources(tmp_path):
    subprocess.run(
        [sys.executable, "-m", "pip", "wheel", ".", "--no-deps", "-w", str(tmp_path)],
        cwd=REPO_ROOT,
        text=True,
        capture_output=True,
        check=True,
    )
    wheel = next(tmp_path.glob("everos_hermes-*.whl"))
    with zipfile.ZipFile(wheel) as zf:
        names = set(zf.namelist())

    required_suffixes = [
        "share/everos-hermes/integrations/hermes/plugin.yaml",
        "share/everos-hermes/integrations/hermes/__init__.py",
        "share/everos-hermes/integrations/hermes/resources/skills/everos-memory-curation/SKILL.md",
    ]
    for suffix in required_suffixes:
        assert any(name.endswith(suffix) for name in names), suffix
    for ref_name in EXPECTED_SKILL_REFERENCES:
        assert any(name.endswith(f"/references/{ref_name}") for name in names), ref_name
    assert not any("__pycache__" in name or name.endswith((".pyc", ".pyo")) for name in names)
