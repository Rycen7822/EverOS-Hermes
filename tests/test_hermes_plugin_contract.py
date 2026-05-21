from __future__ import annotations

import importlib.util
from pathlib import Path

import yaml


REPO_ROOT = Path(__file__).resolve().parents[1]
PLUGIN_DIR = REPO_ROOT / "integrations" / "hermes"
PLUGIN_MANIFEST = PLUGIN_DIR / "plugin.yaml"
PLUGIN_SKILL = PLUGIN_DIR / "resources" / "skills" / "everos-memory-curation" / "SKILL.md"
LEGACY_REPO_SKILL = REPO_ROOT / "skills" / "software-development" / "everos-memory-curation" / "SKILL.md"


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



class StandalonePluginCtx:
    """Subset of Hermes PluginContext used by standalone user plugins."""

    def __init__(self):
        self.tools = []
        self.skills = []

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


def test_standalone_register_exposes_tools_and_plugin_skill_without_memory_provider_method():
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

    assert ctx.skills[0]["name"] == "everos-memory-curation"
    assert ctx.skills[0]["path"] == PLUGIN_SKILL
    assert ctx.skills[0]["description"]


def test_memory_provider_register_path_still_registers_provider_only():
    module = _load_plugin_module()
    ctx = MemoryProviderCollector()

    module.register(ctx)

    assert ctx.provider is not None
    assert ctx.provider.name == "everos"


def test_skill_includes_operator_runbook_and_guardrail_anchors():
    skill_text = PLUGIN_SKILL.read_text(encoding="utf-8")
    bundle_text = skill_text + (PLUGIN_SKILL.parent / "references" / "agent-case-visibility.md").read_text(encoding="utf-8")
    data = yaml.safe_load(skill_text.split("---", 2)[1])

    assert data["name"] == "everos-memory-curation"
    assert data["description"]
    assert "## Reference Map" in skill_text
    assert not LEGACY_REPO_SKILL.exists()
    assert len(data["description"]) <= 1024
    assert len(skill_text) <= 6500
    for anchor in ["everos_memory_save_and_verify", "agent_visibility", "Do not invent or override `user_id`", "Do not make up memories"]:
        assert anchor in bundle_text
