from __future__ import annotations

import subprocess
import sys
import zipfile
from pathlib import Path

import yaml

REPO_ROOT = Path(__file__).resolve().parents[1]
PLUGIN_MANIFEST = REPO_ROOT / "integrations" / "hermes" / "plugin.yaml"
RUST_PLUGIN_MANIFEST = REPO_ROOT / "rust-version" / "integrations" / "hermes" / "plugin.yaml"

EXPECTED_SKILL_REFERENCES = [
    "user-intent-runbooks.md",
    "memory-routing-policy.md",
    "agent-case-visibility.md",
    "agent-visibility-contract-audits.md",
    "plugin-triage-and-migration.md",
    "cleanup-and-verification.md",
]

def test_rust_prebuild_package_script_uses_python_skill_resources_as_canonical_source():
    script = (REPO_ROOT / "rust-version" / "scripts" / "package-release.sh").read_text(encoding="utf-8")

    assert "CANONICAL_PLUGIN_DIR" in script
    assert "../integrations/hermes" in script
    assert "git -C \"$REPO_ROOT\" ls-files" in script
    assert "integrations/hermes/resources/skills/everos-memory-curation" in script
    assert 'cp -R "$CANONICAL_SKILL_DIR"' not in script

def test_rust_prebuild_package_script_honors_target_and_normalizes_archive_metadata():
    script = (REPO_ROOT / "rust-version" / "scripts" / "package-release.sh").read_text(encoding="utf-8")

    assert 'cargo build --release --target "$TARGET" --bin everos-hermes-rust' in script
    assert 'target/$TARGET/release/everos-hermes-rust' in script
    assert "--owner=0" in script
    assert "--group=0" in script
    assert "--numeric-owner" in script
    assert "chmod 0755" in script
    assert "chmod 0644" in script

def test_rust_prebuild_package_script_stages_tracked_files_and_rejects_ignored_secrets():
    script = (REPO_ROOT / "rust-version" / "scripts" / "package-release.sh").read_text(encoding="utf-8")

    assert "git ls-files" in script
    assert 'cp -R "$ROOT/integrations/hermes"' not in script
    assert 'cp -R "$CANONICAL_SKILL_DIR"' not in script
    assert ".env" in script
    assert "Refusing to package" in script

def test_python_test_extra_ci_and_mypy_configuration_cover_supported_gates():
    pyproject = (REPO_ROOT / "pyproject.toml").read_text(encoding="utf-8")
    workflow_path = REPO_ROOT / ".github" / "workflows" / "ci.yml"
    workflow = yaml.safe_load(workflow_path.read_text(encoding="utf-8"))
    workflow_text = workflow_path.read_text(encoding="utf-8")

    assert '"PyYAML>=6"' in pyproject
    assert "[tool.mypy]" in pyproject
    assert "everos_hermes.provider" in pyproject
    assert "everos_hermes.mcp_server" in pyproject
    assert workflow["jobs"]["python"]
    assert workflow["jobs"]["rust"]
    for command in [
        "python -m ruff check .",
        "python -m pytest -q",
        "python -m mypy src/everos_hermes --ignore-missing-imports",
        "cargo fmt --all --check",
        "cargo test --quiet",
        "cargo clippy --quiet -- -D warnings",
        "scripts/package-release.sh",
        "sha256sum -c",
        "missing release resource",
    ]:
        assert command in workflow_text

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

    assert py_manifest["version"] == "0.3.0"
    assert rust_manifest["version"] == "0.3.0"
    assert 'version = "0.3.0"' in pyproject
    assert 'version = "0.3.0"' in cargo
