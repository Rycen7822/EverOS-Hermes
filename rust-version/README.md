# EverOS-Hermes Rust Version

The Rust runtime is the native backend for the same single Hermes plugin model used by the Python source tree.

It provides:

- `everos-hermes-rust`, a native binary used by the Python plugin shim.
- `integrations/hermes/`, a Hermes plugin directory that can be copied to `$HERMES_HOME/plugins/everos`.
- The bundled plugin skill `everos:everos-memory-curation` under `integrations/hermes/resources/skills/`.
- A thin `SKILL.md` router plus detailed skill references under `integrations/hermes/resources/skills/everos-memory-curation/references/`.
- Rust parity for provider behavior: structured agent trajectory capture, the budgeted context assembler, deterministic message ids, `prefetch_cache_enabled`, optional `include_recent_raw`, and hooks such as `agent_trajectory_on_session_end`, `agent_trajectory_on_pre_compress`, and delegation capture.
- A stdio compatibility command surface with MCP-12 tools for callers that already depend on it. Normal Hermes installs should use the plugin path below.

## Build

```bash
cd rust-version
cargo build --release
cargo test --tests --no-fail-fast
```

## Install as one Hermes plugin

Install the binary to a stable path and copy the plugin directory:

```bash
INSTALL_DIR="$HOME/.local/share/everos-hermes"
HERMES_HOME="${HERMES_HOME:-$HOME/.hermes}"

mkdir -p "$INSTALL_DIR/bin" "$HERMES_HOME/plugins"
cp target/release/everos-hermes-rust "$INSTALL_DIR/bin/everos-hermes-rust"
rm -rf "$HERMES_HOME/plugins/everos"
cp -R integrations/hermes "$HERMES_HOME/plugins/everos"
```

Put secrets and the absolute binary path in `$HERMES_HOME/.env` or `~/.hermes/.env`:

```bash
EVEROS_API_KEY=your_everos_api_key
EVEROS_USER_ID=hermes_default
EVEROS_HERMES_RUST_BIN=/home/you/.local/share/everos-hermes/bin/everos-hermes-rust
# optional:
EVEROS_BASE_URL=https://api.evermind.ai
```

Do not write `$HOME`, `~`, or `$INSTALL_DIR` inside `.env` values; Hermes dotenv parsing does not shell-expand them.

Enable both plugin roles:

```bash
hermes plugins enable everos
hermes config set memory.provider everos
```

Restart Hermes CLI/WebUI/gateway after changing plugin, provider, or secret configuration.

## How the plugin loads

Hermes has two loader paths, and the Rust shim supports both from the same plugin directory:

- `plugins.enabled: [everos]` imports the standalone plugin, registers the `everos` toolset, and registers the bundled skill as `everos:everos-memory-curation`.
- `memory.provider: everos` loads the same directory as a memory provider and delegates provider operations to `everos-hermes-rust`.

The standalone plugin registers tools only when the Rust binary can return schemas. If tools are missing, check `EVEROS_HERMES_RUST_BIN` and restart Hermes.

## Plugin tools

The Rust-backed plugin/provider surface matches the Python provider tool names:

- `everos_memory_save`
- `everos_memory_search`
- `everos_memory_get`
- `everos_memory_flush`
- `everos_memory_forget`
- `everos_memory_save_and_verify`
- `everos_memory_import_and_verify`
- `everos_memory_verify_session`

## Bundled skill

Load the runbook with:

```text
/skill everos:everos-memory-curation
```

The skill is stored at:

```text
integrations/hermes/resources/skills/everos-memory-curation/SKILL.md
```

The entry `SKILL.md` is intentionally small. It routes agents to the relevant reference file instead of loading all operator guidance every time. Current primary references are:

- `references/user-intent-runbooks.md`
- `references/memory-routing-policy.md`
- `references/agent-case-visibility.md`
- `references/agent-visibility-contract-audits.md`
- `references/plugin-triage-and-migration.md`
- `references/cleanup-and-verification.md`

## Compatibility server

The binary still includes the stdio compatibility server for existing automation and parity tests. That path is not the recommended Hermes setup path anymore; prefer the single plugin installation above so tools, automatic memory hooks, and the curation skill are co-located.

## Release package

`./scripts/package-release.sh` builds a tarball containing:

```text
bin/everos-hermes-rust
integrations/hermes/__init__.py
integrations/hermes/plugin.yaml
integrations/hermes/resources/skills/everos-memory-curation/SKILL.md
integrations/hermes/resources/skills/everos-memory-curation/references/*.md
README.md
INSTALL.md
```

Verify release packages by checking the archive checksum, running the binary `--help`, copying `integrations/hermes` to the Hermes plugin directory, enabling `everos`, selecting `memory.provider: everos`, checking the thin skill references exist, and starting a fresh Hermes session.
