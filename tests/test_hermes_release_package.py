from __future__ import annotations

import subprocess
import sys
import zipfile
from pathlib import Path

import yaml

REPO_ROOT = Path(__file__).resolve().parents[1]
PLUGIN_MANIFEST = REPO_ROOT / "integrations" / "hermes" / "plugin.yaml"
RUST_PLUGIN_MANIFEST = REPO_ROOT / "rust-version" / "integrations" / "hermes" / "plugin.yaml"

SKILL_REF_DIR = REPO_ROOT / "integrations" / "hermes" / "resources" / "skills" / "everos-memory-curation" / "references"
EXPECTED_SKILL_REFERENCES = sorted(path.name for path in SKILL_REF_DIR.glob("*.md"))
assert len(EXPECTED_SKILL_REFERENCES) == 9

def test_rust_prebuild_package_script_stages_canonical_tracked_resources():
    script = (REPO_ROOT / "rust-version" / "scripts" / "package-release.sh").read_text(encoding="utf-8")

    for required in [
        "CANONICAL_PLUGIN_DIR",
        "../integrations/hermes",
        "git -C \"$REPO_ROOT\" ls-files",
        "integrations/hermes/resources/skills/everos-memory-curation",
        'cargo build --release --target "$TARGET" --bin everos-hermes-rust',
        "--numeric-owner",
        "Refusing to package",
    ]:
        assert required in script
    for forbidden in ['cp -R "$ROOT/integrations/hermes"', 'cp -R "$CANONICAL_SKILL_DIR"']:
        assert forbidden not in script

def test_python_wheel_includes_single_plugin_manifest_and_skill_resources(tmp_path):
    pyproject = (REPO_ROOT / "pyproject.toml").read_text(encoding="utf-8")
    assert "references/*.md" not in pyproject
    assert all(f"references/{ref_name}" in pyproject for ref_name in EXPECTED_SKILL_REFERENCES)

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
        "share/everos-hermes/integrations/hermes/resources/skills/everos-memory-curation/references/memory-routing-policy.md",
    ]
    for suffix in required_suffixes:
        assert any(name.endswith(suffix) for name in names), suffix
    assert not any("__pycache__" in name or name.endswith((".pyc", ".pyo")) for name in names)
    forbidden_fragments = (".env", "secret", "token", "private", "scratch")
    assert not any(any(fragment in name.lower() for fragment in forbidden_fragments) for name in names)

def test_project_versions_match_plugin_manifest_version():
    pyproject = (REPO_ROOT / "pyproject.toml").read_text(encoding="utf-8")
    cargo = (REPO_ROOT / "rust-version" / "Cargo.toml").read_text(encoding="utf-8")
    py_manifest = yaml.safe_load(PLUGIN_MANIFEST.read_text(encoding="utf-8"))
    rust_manifest = yaml.safe_load(RUST_PLUGIN_MANIFEST.read_text(encoding="utf-8"))
    py_version = next(line.split('"')[1] for line in pyproject.splitlines() if line.startswith("version = "))
    cargo_version = next(line.split('"')[1] for line in cargo.splitlines() if line.startswith("version = "))

    assert py_manifest["version"] == rust_manifest["version"] == py_version == cargo_version
