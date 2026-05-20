from __future__ import annotations

import importlib.util
import json
import types
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

EXPECTED_SKILL_REFERENCES = sorted(path.name for path in (PLUGIN_SKILL.parent / "references").glob("*.md"))
assert len(EXPECTED_SKILL_REFERENCES) == 9

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

EXPECTED_SKILL_DESCRIPTION = (
    "Use proactively when complex or iterative work may produce durable EverOS/Hermes memory: "
    "recall, save, verify, clean, compress, or migrate reusable workflows, debugging "
    "lessons, tool/API quirks, and agent cases without saving noisy task logs."
)


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
        ref_path = PLUGIN_SKILL.parent / "references" / ref_name
        assert ref_path.exists() and len(ref_path.read_text(encoding="utf-8")) > 500
    assert "### Agent Case Trajectory Recipe" not in text
    assert not LEGACY_REPO_SKILL.exists(), "EverOS curation guidance must be plugin-bundled, not a separate repo skill."


def test_rust_plugin_skill_resources_are_byte_identical_to_python_canonical_tree():
    canonical_skill_dir = PLUGIN_SKILL.parent
    rust_skill_dir = RUST_PLUGIN_SKILL.parent

    canonical_files = {
        path.relative_to(canonical_skill_dir): path
        for path in canonical_skill_dir.rglob("*")
        if path.is_file() and "__pycache__" not in path.parts
    }
    rust_files = {
        path.relative_to(rust_skill_dir): path
        for path in rust_skill_dir.rglob("*")
        if path.is_file() and "__pycache__" not in path.parts
    }

    assert sorted(canonical_files) == sorted(rust_files)
    for rel_path in sorted(canonical_files):
        assert canonical_files[rel_path].read_bytes() == rust_files[rel_path].read_bytes(), rel_path


def test_rust_prebuild_shim_sends_sensitive_payloads_over_stdin_and_logs_background_failures(monkeypatch, tmp_path):
    module = _load_plugin_module(RUST_PLUGIN_DIR, "everos_hermes_rust_shim_stdio_under_test")
    fake_bin = tmp_path / "everos-hermes-rust"
    fake_bin.write_text("#!/bin/sh\nexit 0\n", encoding="utf-8")
    fake_bin.chmod(0o755)
    monkeypatch.setenv("EVEROS_HERMES_RUST_BIN", str(fake_bin))

    run_calls = []

    def fake_run(cmd, *, text, capture_output, timeout, check, input=None):
        run_calls.append({"cmd": cmd, "input": input, "timeout": timeout})
        if "tool-schemas" in cmd:
            stdout = "[]"
        elif "tool-call" in cmd:
            stdout = '{"ok":true}'
        else:
            stdout = "ok"
        return types.SimpleNamespace(returncode=0, stdout=stdout, stderr="")

    popen_calls = []

    class FakePopen:
        def __init__(self, cmd, **kwargs):
            popen_calls.append({"cmd": cmd, "kwargs": kwargs, "communicated": []})
            self.returncode = None

        def communicate(self, input=None, timeout=None):
            popen_calls[-1]["communicated"].append({"input": input, "timeout": timeout})
            self.returncode = 7
            bearer_token = "abc" + "+def/" + "ghi=~tail"
            quoted_value = "quoted," + "semi;" + "with]delimiters"
            email_secret = "email" + "-secret"
            key_secret = "key" + "-secret"
            client_id_secret = "client" + "-id-secret"
            credentials_blob = json.dumps({"client_email": email_secret, "private_key": key_secret}).replace('"', '\\"')
            arguments_blob = json.dumps({"credentials": {"client_email": email_secret, "client_id": client_id_secret}}).replace('"', '\\"')
            pretty_payload = "{\n  \"client_email\": \"" + email_secret + "\",\n  \"client_id\": \"" + client_id_secret + "\"\n}"
            return (
                "",
                "background failed tok"
                + "en=\""
                + quoted_value
                + "\" creden"
                + "tials=\""
                + credentials_blob
                + "\" Authorization: Bearer "
                + bearer_token
                + " arguments=\""
                + arguments_blob
                + "\" creden"
                + "tials="
                + pretty_payload
                + " request_id=req-shim"
            )


        def poll(self):
            return self.returncode

        def wait(self, timeout=None):
            self.returncode = 7
            return self.returncode

        def kill(self):
            self.returncode = -9

    monkeypatch.setattr(module.subprocess, "run", fake_run)
    monkeypatch.setattr(module.subprocess, "Popen", FakePopen)

    provider = module.EverOSRustMemoryProvider()
    provider.initialize("sess", hermes_home=str(tmp_path), user_id="user-secret", agent_context="ctx-secret")
    provider.save_config({"api_key": "config-secret", "base_url": "http://example.test"}, str(tmp_path))
    provider.prefetch("query secret", session_id="sess-q")
    provider.handle_tool_call("everos_memory_save", {"content": "tool secret"})
    provider.sync_turn("user secret", "assistant secret", session_id="sess-bg")

    joined_args = "\n".join("\0".join(call["cmd"]) for call in run_calls + popen_calls)
    for secret in ["config-secret", "query secret", "tool secret", "user secret", "assistant secret", "ctx-secret"]:
        assert secret not in joined_args
    assert all("--payload-stdin" in call["cmd"] for call in run_calls if "tool-schemas" not in call["cmd"])
    assert all(call["input"] and "secret" in call["input"] for call in run_calls if "tool-schemas" not in call["cmd"])
    assert popen_calls and popen_calls[0]["kwargs"].get("stdin") == module.subprocess.PIPE
    assert popen_calls[0]["communicated"] and "assistant secret" in popen_calls[0]["communicated"][0]["input"]

    provider.shutdown()
    log_path = tmp_path / "everos.log"
    log_text = log_path.read_text(encoding="utf-8")
    assert "background provider command failed" in log_text
    assert "[REDACTED]" in log_text
    assert "abc+def/ghi=~tail" not in log_text
    assert "quoted,semi;with]delimiters" not in log_text
    assert "email-secret" not in log_text
    assert "key-secret" not in log_text
    assert "client-id-secret" not in log_text
    assert "request_id=req-shim" in log_text
    assert log_path.stat().st_mode & 0o777 == 0o600


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
            "description": EXPECTED_SKILL_DESCRIPTION,
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
        "verification_user_id",
        "same `user_id` and `session_id`",
        "Do not invent or override `user_id`",
        "Do not make up memories",
    ]

    for skill_path in [PLUGIN_SKILL, RUST_PLUGIN_SKILL]:
        skill_text = skill_path.read_text(encoding="utf-8")
        bundle_text = _skill_bundle_text(skill_path)
        data = yaml.safe_load(skill_text.split("---", 2)[1])
        assert tuple(map(int, data["version"].split("."))) >= (1, 0, 8)
        assert data["description"] == EXPECTED_SKILL_DESCRIPTION
        assert len(data["description"]) <= 1024
        assert "## Post-task Proactive Curation" in skill_text
        assert "Do not wait for the user to say" in skill_text
        assert "references/memory-routing-policy.md" in skill_text
        assert len(skill_text) <= 6500
        assert "Existing specialized references remain available under `references/`" in skill_text
        for heavy_marker in ["### Remember / save this", "### Agent Case Trajectory Recipe", "## Cleanup / Compression Checklist"]:
            assert heavy_marker not in skill_text, f"{skill_path} kept heavy section {heavy_marker} in SKILL.md"
        for stale_marker in ["/home/xu", "For this user", "Rust low-level group methods exist"]:
            assert stale_marker not in bundle_text, f"{skill_path} kept stale reference marker {stale_marker}"
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
            "description": EXPECTED_SKILL_DESCRIPTION,
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
    assert "memory.provider everos" in combined
    assert "everos:everos-memory-curation" in combined
    assert "thin" in combined
    assert "references/user-intent-runbooks.md" in root_readme
    assert "references/memory-routing-policy.md" in root_readme
    assert "legacy ordinary skill" in root_readme
    assert "mcp_servers:" not in combined
    assert "Optional MCP server" not in combined
    assert "explicit MCP tools" not in root_readme
