from __future__ import annotations

import importlib.util
import json
from pathlib import Path

import yaml


REPO_ROOT = Path(__file__).resolve().parents[1]
PLUGIN_DIR = REPO_ROOT / "integrations" / "hermes"
PLUGIN_INIT = PLUGIN_DIR / "__init__.py"
PLUGIN_MANIFEST = PLUGIN_DIR / "plugin.yaml"
PLUGIN_SKILL = PLUGIN_DIR / "resources" / "skills" / "everos-memory-curation" / "SKILL.md"
RUST_PLUGIN_DIR = REPO_ROOT / "rust-version" / "integrations" / "hermes"
RUST_PLUGIN_INIT = RUST_PLUGIN_DIR / "__init__.py"
RUST_PLUGIN_MANIFEST = RUST_PLUGIN_DIR / "plugin.yaml"
RUST_PLUGIN_SKILL = RUST_PLUGIN_DIR / "resources" / "skills" / "everos-memory-curation" / "SKILL.md"
LEGACY_REPO_SKILL = REPO_ROOT / "skills" / "software-development" / "everos-memory-curation" / "SKILL.md"

EXPECTED_SKILL_REFERENCES = [
    "user-intent-runbooks.md",
    "memory-routing-policy.md",
    "agent-case-visibility.md",
    "plugin-triage-and-migration.md",
    "cleanup-and-verification.md",
]

EXPECTED_PLUGIN_TOOL_NAMES = {
    "everos_memory_save",
    "everos_memory_search",
    "everos_memory_get",
    "everos_memory_flush",
    "everos_memory_forget",
    "everos_memory_save_and_verify",
    "everos_memory_import_and_verify",
    "everos_memory_verify_session",
}


def _load_plugin_module(plugin_dir: Path = PLUGIN_DIR, module_name: str = "everos_hermes_plugin_contract_under_test"):
    init_path = plugin_dir / "__init__.py"
    spec = importlib.util.spec_from_file_location(
        module_name,
        init_path,
        submodule_search_locations=[str(plugin_dir)],
    )
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def _skill_bundle_text(skill_path: Path) -> str:
    ref_dir = skill_path.parent / "references"
    parts = [skill_path.read_text(encoding="utf-8")]
    for ref_name in EXPECTED_SKILL_REFERENCES:
        parts.append((ref_dir / ref_name).read_text(encoding="utf-8"))
    return "\n\n".join(parts)


class StandalonePluginCtx:
    """Subset of Hermes PluginContext used by standalone user plugins."""

    def __init__(self):
        self.tools = []
        self.skills = []
        self.hooks = []

    def register_tool(self, name, toolset, schema, handler, **kwargs):
        self.tools.append(
            {
                "name": name,
                "toolset": toolset,
                "schema": schema,
                "handler": handler,
                "kwargs": kwargs,
            }
        )

    def register_skill(self, name, path, description=""):
        self.skills.append({"name": name, "path": Path(path), "description": description})

    def register_hook(self, hook_name, callback):
        self.hooks.append({"name": hook_name, "callback": callback})


class MemoryProviderCollector:
    """Subset of plugins.memory._ProviderCollector used by memory.provider loading."""

    def __init__(self):
        self.provider = None

    def register_memory_provider(self, provider):
        self.provider = provider

    def register_tool(self, *args, **kwargs):
        raise AssertionError("memory-provider discovery must not register standalone plugin tools")

    def register_hook(self, *args, **kwargs):
        raise AssertionError("memory-provider discovery must not register standalone plugin hooks")

    def register_cli_command(self, *args, **kwargs):
        raise AssertionError("memory-provider discovery must not register CLI commands")


def test_manifest_declares_single_standalone_plugin_surface():
    data = yaml.safe_load(PLUGIN_MANIFEST.read_text(encoding="utf-8"))

    assert data["name"] == "everos"
    assert data["kind"] == "standalone"
    assert set(data["provides_tools"]) == EXPECTED_PLUGIN_TOOL_NAMES
    assert data["provides_hooks"] == [
        "on_session_end",
        "on_memory_write",
        "on_pre_compress",
    ]
    assert "hooks" not in data, "Use provides_hooks; legacy hooks key is only documentation and is ignored by Hermes."
    assert any(
        item.get("name") == "EVEROS_API_KEY" for item in data.get("requires_env", []) if isinstance(item, dict)
    )


def test_plugin_bundles_curation_skill_instead_of_shipping_repo_level_skill():
    assert PLUGIN_SKILL.exists()
    text = PLUGIN_SKILL.read_text(encoding="utf-8")
    assert "name: everos-memory-curation" in text
    assert "## Reference Map" in text
    assert len(text) <= 6500, "SKILL.md must stay thin; heavy guidance belongs in references/*.md."
    for ref_name in EXPECTED_SKILL_REFERENCES:
        assert f"references/{ref_name}" in text
        ref_path = PLUGIN_SKILL.parent / "references" / ref_name
        assert ref_path.exists()
        assert len(ref_path.read_text(encoding="utf-8")) > 500
    assert "### Agent Case Trajectory Recipe" not in text
    assert not LEGACY_REPO_SKILL.exists(), "EverOS curation guidance must be plugin-bundled, not a separate repo skill."


def test_standalone_register_exposes_tools_and_plugin_skill_without_memory_provider_method(monkeypatch):
    monkeypatch.setenv("EVEROS_API_KEY", "sk-test")
    monkeypatch.setenv("EVEROS_USER_ID", "u1")
    module = _load_plugin_module()
    ctx = StandalonePluginCtx()

    module.register(ctx)

    registered_names = {entry["name"] for entry in ctx.tools}
    assert registered_names == EXPECTED_PLUGIN_TOOL_NAMES
    assert {entry["toolset"] for entry in ctx.tools} == {"everos"}
    for entry in ctx.tools:
        assert entry["schema"]["name"] == entry["name"]
        assert callable(entry["handler"])
        assert entry["kwargs"].get("requires_env") == ["EVEROS_API_KEY"]

    assert ctx.skills == [
        {
            "name": "everos-memory-curation",
            "path": PLUGIN_SKILL,
            "description": "Operate and curate EverOS-Hermes memory safely.",
        }
    ]


def test_memory_provider_register_path_still_registers_provider_only():
    module = _load_plugin_module()
    ctx = MemoryProviderCollector()

    module.register(ctx)

    assert ctx.provider is not None
    assert ctx.provider.name == "everos"
    assert {schema["name"] for schema in ctx.provider.get_tool_schemas()} == EXPECTED_PLUGIN_TOOL_NAMES


def test_plugin_tool_handler_lazy_initializes_provider_and_returns_json(monkeypatch, tmp_path):
    monkeypatch.setenv("EVEROS_API_KEY", "sk-test")
    monkeypatch.setenv("EVEROS_USER_ID", "u1")
    monkeypatch.setenv("HERMES_HOME", str(tmp_path))
    module = _load_plugin_module()
    ctx = StandalonePluginCtx()
    module.register(ctx)
    handlers = {entry["name"]: entry["handler"] for entry in ctx.tools}

    payload = json.loads(handlers["everos_memory_save"]({"content": ""}))

    assert payload["error"] == "content is required"


def test_skill_includes_agentmemory_style_operator_runbooks_and_guardrails():
    expected_sections = [
        "## User-Intent Runbooks",
        "### Remember / save this",
        "### Recall / what did we do",
        "### Forget / delete memory",
        "### Session history / recent memory timeline",
        "## Tool Unavailable / Plugin Not Loaded Triage",
        "## Search Result Presentation Contract",
    ]
    expected_terms = [
        "everos_memory_save_and_verify",
        "everos_memory_search",
        "everos_memory_get",
        "everos_memory_forget",
        "memory.provider: everos",
        "plugins.enabled",
        "agent_visibility",
        "Do not make up memories",
    ]

    for skill_path in [PLUGIN_SKILL, RUST_PLUGIN_SKILL]:
        skill_text = skill_path.read_text(encoding="utf-8")
        bundle_text = _skill_bundle_text(skill_path)
        data = yaml.safe_load(skill_text.split("---", 2)[1])
        assert data["version"] >= "1.0.7"
        assert len(skill_text) <= 6500
        for ref_name in EXPECTED_SKILL_REFERENCES:
            assert f"references/{ref_name}" in skill_text
        for heavy_marker in ["### Remember / save this", "### Agent Case Trajectory Recipe", "## Cleanup / Compression Checklist"]:
            assert heavy_marker not in skill_text, f"{skill_path} kept heavy section {heavy_marker} in SKILL.md"
        for section in expected_sections:
            assert section in bundle_text, f"{skill_path} missing {section}"
        for term in expected_terms:
            assert term in bundle_text, f"{skill_path} missing {term}"


def test_rust_plugin_manifest_and_resources_match_single_plugin_contract():
    data = yaml.safe_load(RUST_PLUGIN_MANIFEST.read_text(encoding="utf-8"))

    assert data["name"] == "everos"
    assert data["kind"] == "standalone"
    assert data["runtime"] == "rust"
    assert set(data["provides_tools"]) == EXPECTED_PLUGIN_TOOL_NAMES
    assert data["provides_hooks"] == [
        "on_session_end",
        "on_memory_write",
        "on_pre_compress",
    ]
    assert RUST_PLUGIN_SKILL.exists()
    assert "### Agent Case Trajectory Recipe" in _skill_bundle_text(RUST_PLUGIN_SKILL)


def test_rust_standalone_register_exposes_plugin_skill_and_tools_when_binary_surface_is_available(monkeypatch):
    module = _load_plugin_module(RUST_PLUGIN_DIR, "everos_hermes_rust_plugin_contract_under_test")
    fake_schemas = [
        {"name": name, "description": f"{name} description", "parameters": {"type": "object", "properties": {}}}
        for name in sorted(EXPECTED_PLUGIN_TOOL_NAMES)
    ]
    monkeypatch.setattr(module.EverOSRustMemoryProvider, "get_tool_schemas", lambda self: fake_schemas)
    monkeypatch.setattr(module.EverOSRustMemoryProvider, "is_available", lambda self: True)
    ctx = StandalonePluginCtx()

    module.register(ctx)

    assert {entry["name"] for entry in ctx.tools} == EXPECTED_PLUGIN_TOOL_NAMES
    assert {entry["toolset"] for entry in ctx.tools} == {"everos"}
    assert ctx.skills == [
        {
            "name": "everos-memory-curation",
            "path": RUST_PLUGIN_SKILL,
            "description": "Operate and curate EverOS-Hermes memory safely.",
        }
    ]


def test_rust_memory_provider_register_path_still_registers_provider_only():
    module = _load_plugin_module(RUST_PLUGIN_DIR, "everos_hermes_rust_memory_provider_contract_under_test")
    ctx = MemoryProviderCollector()

    module.register(ctx)

    assert ctx.provider is not None
    assert ctx.provider.name == "everos"


def test_install_docs_describe_one_plugin_not_separate_mcp_and_skill_setup():
    root_readme = (REPO_ROOT / "README.md").read_text(encoding="utf-8")
    plugin_readme = (PLUGIN_DIR / "README.md").read_text(encoding="utf-8")
    rust_readme = (REPO_ROOT / "rust-version" / "README.md").read_text(encoding="utf-8")
    combined = root_readme + "\n" + plugin_readme + "\n" + rust_readme

    assert "One Hermes plugin" in root_readme
    assert "single Hermes plugin" in rust_readme
    assert "hermes plugins enable everos" in combined
    assert "everos:everos-memory-curation" in combined
    assert "mcp_servers:" not in combined
    assert "Optional MCP server" not in combined
    assert "explicit MCP tools" not in root_readme
