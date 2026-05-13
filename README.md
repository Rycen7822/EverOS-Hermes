<div align="center">

# EverOS-Hermes

**One Hermes plugin for EverOS Cloud memory: automatic recall/capture hooks, explicit EverOS tools, and a thin bundled curation skill.**

Use EverOS through a single Hermes plugin directory, with compatibility surfaces for stdio MCP and Rust prebuilt installs.

</div>

<br/>

<p align="center">
  <a href="README.md"><img src="https://img.shields.io/badge/Docs-README-f5c542?style=for-the-badge" alt="Documentation"></a>
  <a href="https://github.com/Rycen7822/EverOS-Hermes"><img src="https://img.shields.io/badge/GitHub-EverOS--Hermes-0969da?style=for-the-badge" alt="GitHub repository"></a>
  <a href="src/everos_hermes/mcp_server.py"><img src="https://img.shields.io/badge/MCP-13%20tools-2ea44f?style=for-the-badge" alt="MCP: thirteen tools"></a>
  <a href="integrations/hermes"><img src="https://img.shields.io/badge/Hermes-single%20plugin-5865F2?style=for-the-badge" alt="Hermes single plugin"></a>
  <a href="rust-version/README.md"><img src="https://img.shields.io/badge/Runtime-Python%20%7C%20Rust-blue?style=for-the-badge" alt="Runtime: Python and Rust"></a>
  <a href="https://github.com/Rycen7822/EverOS-Hermes/releases"><img src="https://img.shields.io/badge/Rust%20Prebuilt-available-0969da?style=for-the-badge" alt="Rust prebuilt package available"></a>
</p>

> EverOS-Hermes is for Hermes Agent users who want EverOS as a long-term memory backend.
> Enable `plugins.enabled: [everos]` for standalone tools and the bundled skill, and
> set `memory.provider: everos` for automatic recall/capture hooks.

## What changed

EverOS-Hermes is packaged as one Hermes plugin directory instead of three separate things to install and remember:

1. Hermes memory provider hooks for automatic recall, capture, explicit memory-write mirroring, session-end flush, and pre-compression trajectory capture.
2. Standalone plugin tools such as `everos_memory_search`, `everos_memory_save`, `everos_memory_get`, and verification/import helpers.
3. The read-only plugin skill `everos:everos-memory-curation`, bundled under `integrations/hermes/resources/skills/`.

Current versions and surfaces:

- Hermes plugin manifest: `0.3.0`.
- Python package: `everos-hermes` `0.3.0`.
- Rust crate/binary: `everos-hermes-rust` `0.3.0`.
- Standalone provider/plugin tools: 8 `everos_memory_*` tools.
- Stdio compatibility server: MCP-13 tools for existing stdio integrations.

The stdio compatibility server and Rust binary remain in the repository for existing integrations and release packages. Normal Hermes users should install and enable the plugin rather than configuring a separate server and copying a separate skill.

Rust context-engine parity is current: the Python and Rust provider paths both cover structured agent trajectory capture, the budgeted context assembler, deterministic message ids, prefetch caching through `prefetch_cache_enabled`, optional session-scoped raw recall through `include_recent_raw`, and trajectory hooks such as `agent_trajectory_on_session_end`, `agent_trajectory_on_pre_compress`, and delegation capture.

## Install the Python plugin from source

```bash
git clone https://github.com/Rycen7822/EverOS-Hermes.git
cd EverOS-Hermes
python -m pip install -e .

HERMES_HOME="${HERMES_HOME:-$HOME/.hermes}"
mkdir -p "$HERMES_HOME/plugins"
rm -rf "$HERMES_HOME/plugins/everos"
cp -R integrations/hermes "$HERMES_HOME/plugins/everos"

hermes plugins enable everos
hermes config set memory.provider everos
```

Set credentials only in the Hermes secret file or the process environment:

```bash
# ${HERMES_HOME:-$HOME/.hermes}/.env
EVEROS_API_KEY=your_everos_api_key
EVEROS_USER_ID=hermes_default
# optional:
EVEROS_BASE_URL=https://api.evermind.ai
```

Restart Hermes CLI/WebUI/gateway after changing plugin, provider, or secret configuration. Already-running sessions do not retroactively reload plugin tools, bundled skills, or memory-provider hooks.

## How Hermes loads it

Hermes currently has two loader paths, and this plugin supports both from the same directory:

- `plugins.enabled: [everos]` lets the standalone PluginManager import `integrations/hermes/__init__.py`, register the `everos` toolset, and register `everos:everos-memory-curation`.
- `memory.provider: everos` lets Hermes' memory-provider discovery load the same directory and register `EverOSMemoryProvider` for automatic memory hooks.

Both settings are recommended for the full EverOS-Hermes experience. If you only set `memory.provider`, automatic recall/capture works but plugin-bundled skills and plugin-registered tools may not load through the standalone plugin manager. If you only enable the plugin, explicit tools work but automatic memory hooks are not active.

## Plugin tools

The plugin registers these tool names under the `everos` toolset:

- `everos_memory_save`
- `everos_memory_search`
- `everos_memory_get`
- `everos_memory_flush`
- `everos_memory_forget`
- `everos_memory_save_and_verify`
- `everos_memory_import_and_verify`
- `everos_memory_verify_session`

When the memory provider is also active, Hermes deduplicates matching tool schemas and routes calls through the active provider instance. When only the standalone plugin is enabled, the plugin lazily initializes its own provider instance for tool calls.

## Bundled skill

Load the curation router explicitly when operating EverOS memory:

```text
/skill everos:everos-memory-curation
```

The skill lives inside the plugin at:

```text
integrations/hermes/resources/skills/everos-memory-curation/SKILL.md
```

`SKILL.md` is intentionally thin. It keeps the trigger description, quick routing table, reference map, and always-on guardrails under a small context budget. Detailed guidance is split into references:

- `references/user-intent-runbooks.md` — remember/recall/forget/session-history runbooks.
- `references/memory-routing-policy.md` — personal memory vs agent case vs skill routing.
- `references/agent-case-visibility.md` — `scope="agent"`, trajectories, `tool_call_id`, and `agent_visibility` checks.
- `references/plugin-triage-and-migration.md` — install, provider/plugin/MCP triage, migration pointers.
- `references/cleanup-and-verification.md` — cleanup, destructive delete verification, and final checklists.

Do not copy the skill into `~/.hermes/skills/` as a separate editable skill unless you intentionally want a user-local fork outside this plugin. If a legacy ordinary skill already exists, prefer the qualified plugin skill name above or keep the local copy synchronized to avoid stale bare-name loads.

## Configuration

Advanced non-secret settings live at `$HERMES_HOME/everos.json`:

```json
{
  "auto_recall": true,
  "auto_capture": true,
  "flush_after_turn": true,
  "capture_agent_memory": true,
  "agent_recall": true,
  "agent_flush_after_turn": true,
  "search_method": "hybrid",
  "top_k": 2,
  "max_context_items": 2,
  "max_context_chars": 3000,
  "memory_types": ["episodic_memory"],
  "agent_memory_types": ["agent_memory"],
  "prefetch_cache_enabled": true,
  "include_recent_raw": false,
  "agent_trajectory_on_session_end": true,
  "agent_trajectory_on_pre_compress": true,
  "agent_visibility_verify_after_flush": false
}
```

`EVEROS_USER_ID` overrides `everos.json`. The value can use `{user_id}`, `{user_name}`, `{identity}`, and `{platform}` placeholders when the provider is initialized by Hermes.

## Development and tests

```bash
python -m pytest tests -q
python -m compileall -q src integrations rust-version/integrations tests
```

Focused plugin contract:

```bash
python -m pytest tests/test_hermes_plugin_contract.py -q
```

Rust parity:

```bash
cd rust-version
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --tests --no-fail-fast
```

The Rust port remains under `rust-version/` for native release builds and parity testing. Existing compatibility users can still invoke the compatibility server entrypoint directly, but that is no longer the recommended Hermes setup path.

## Cloud v1 coverage

EverOS-Hermes intentionally implements the Hermes memory-provider subset of the EverOS Cloud v1 surface:

- personal memories and agent memories;
- search, get, delete, settings, task status, flush;
- import/save/verify workflows;
- provider-side context assembly and agent trajectory capture.

Group memory, senders, multimodal object upload, and the full filters DSL remain outside the default plugin scope. See `docs/everos_cloud_v1_contract.md` for the endpoint-level contract.
