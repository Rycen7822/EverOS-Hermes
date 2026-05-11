<div align="center">

# EverOS-Hermes Rust Version

**Rust port of EverOS-Hermes: one native binary for the stdio MCP server plus a Rust-backed Hermes memory provider shim.**

This directory keeps the Python implementation intact and adds an independent Rust runtime under `rust-version/`.

</div>

## What was migrated

The Rust version implements the same user-facing surfaces as the Python version:

- EverOS REST client with Authorization-header auth and JSON request/response handling.
- Hermes-style dotenv lookup: process env -> `$HERMES_HOME/.env` -> `~/.hermes/.env`.
- Search-context formatting for episodes, profile facts, agent cases, and agent skills.
- Hermes memory-provider core behavior:
  - `is_available`
  - `initialize`
  - `system_prompt_block`
  - `prefetch`
  - `sync_turn`
  - `on_memory_write`
  - `on_session_end`
  - five explicit provider tools
- stdio MCP server with the same nine EverOS tools.
- A thin Python Hermes plugin shim, because Hermes' plugin API loads Python entrypoints. The shim delegates behavior to the Rust binary.

The original Python package remains in the repository root and is not removed.

## Project layout

```text
rust-version/
  Cargo.toml
  src/
    client.rs       # EverOS REST client
    env.rs          # Hermes dotenv lookup
    formatting.rs   # EverOS result -> prompt context formatting
    provider.rs     # Rust Hermes memory-provider core
    mcp.rs          # stdio MCP JSON-RPC server and tool handlers
    cli.rs          # binary CLI and provider helper commands
    main.rs
  integrations/hermes/
    __init__.py     # minimal Python shim for Hermes plugin API
    plugin.yaml
  scripts/package-release.sh  # reproducible prebuilt package builder
  tests/parity.rs              # parity + stdio integration tests
```

## Install

| Variant | Best for | Runtime requirements | Status |
| --- | --- | --- | --- |
| Rust prebuilt package | Normal Linux x86_64 use | Linux x86_64; no Rust toolchain | Available as a GitHub release asset |
| Rust from source | Other platforms, development, reproducible local builds | Rust toolchain | Available |
| Python shim for Hermes provider | Registering Rust provider hooks in Hermes | Python only because Hermes loads plugin entrypoints in Python | Included in both prebuilt and source trees |

### Rust Prebuilt Package

Release asset shape:

```text
https://github.com/Rycen7822/EverOS-Hermes/releases/download/v<version>/everos-hermes-rust-<version>-<target>.tar.gz
```

Current Linux x86_64 asset:

```text
everos-hermes-rust-0.1.0-x86_64-unknown-linux-gnu.tar.gz
```

Linux x86_64 install example:

```bash
VERSION=0.1.0
TARGET=x86_64-unknown-linux-gnu
INSTALL_DIR="$HOME/.local/share/everos-hermes"
ASSET="everos-hermes-rust-${VERSION}-${TARGET}.tar.gz"

mkdir -p "$INSTALL_DIR"
curl -L -o "/tmp/$ASSET" \
  "https://github.com/Rycen7822/EverOS-Hermes/releases/download/v${VERSION}/${ASSET}"
tar -xzf "/tmp/$ASSET" -C "$INSTALL_DIR" --strip-components=1
"$INSTALL_DIR/bin/everos-hermes-rust" --help
```

The archive contains:

```text
bin/everos-hermes-rust
integrations/hermes/__init__.py
integrations/hermes/plugin.yaml
README.md
INSTALL.md
```

Optional checksum verification:

```bash
curl -L -o "/tmp/$ASSET.sha256" \
  "https://github.com/Rycen7822/EverOS-Hermes/releases/download/v${VERSION}/${ASSET}.sha256"
(cd /tmp && sha256sum -c "$ASSET.sha256")
```

### Rust From Source

```bash
cd /home/xu/project/tools/EverOS-Hermes/rust-version
cargo build --release
cargo test --tests --no-fail-fast
```

The binary will be:

```text
/home/xu/project/tools/EverOS-Hermes/rust-version/target/release/everos-hermes-rust
```

Create a local release archive:

```bash
./scripts/package-release.sh
```

For development smoke tests, `target/debug/everos-hermes-rust` also works.

## Required secrets

Do not put secrets in committed config. Keep EverOS credentials in Hermes' dotenv file:

```bash
mkdir -p ~/.hermes
$EDITOR ~/.hermes/.env
```

Example:

```bash
EVEROS_API_KEY=your_everos_api_key
EVEROS_USER_ID=hermes_default
# Optional:
EVEROS_BASE_URL=https://api.evermind.ai
EVEROS_TIMEOUT=10
# Optional for the Hermes Python shim if the binary is not on PATH:
EVEROS_HERMES_RUST_BIN=/home/xu/project/tools/EverOS-Hermes/rust-version/target/release/everos-hermes-rust
```

Lookup order for EverOS API settings is:

1. process environment variables;
2. `$HERMES_HOME/.env`;
3. `~/.hermes/.env`.

## Use as MCP server

Build the binary, then add this to `~/.hermes/config.yaml`:

```yaml
mcp_servers:
  everos:
    command: /home/xu/project/tools/EverOS-Hermes/rust-version/target/release/everos-hermes-rust
    args:
      - mcp
```

Manual launch:

```bash
/home/xu/project/tools/EverOS-Hermes/rust-version/target/release/everos-hermes-rust mcp
```

When configured in Hermes, this is a stdio child process. Hermes starts it when MCP is loaded/tested and restarts it on `/reload-mcp` or process restart.

### MCP tools

| Tool | Purpose | Read-only? |
| --- | --- | --- |
| `everos_save_memory` | Save one explicit text memory, then optionally flush. | No |
| `everos_add_memories` | Add one or more user/assistant/tool messages. | No |
| `everos_flush_memories` | Trigger boundary detection and extraction immediately. | No |
| `everos_search_memories` | Search with keyword, vector, hybrid, or agentic retrieval. | Yes |
| `everos_get_memories` | Retrieve structured memories by type and page. | Yes |
| `everos_delete_memories` | Delete by memory id or confirmed user/session scope. | No, destructive |
| `everos_get_task_status` | Check an async extraction task. | Yes |
| `everos_get_settings` | Read EverOS memory-space settings. | Yes |
| `everos_update_settings` | Update EverOS memory-space settings. | No |

## Use as Hermes memory provider

Hermes currently loads memory-provider plugins through Python entrypoints. The Rust version therefore includes a minimal Python shim at `rust-version/integrations/hermes` that registers a provider and shells out to the Rust binary for all behavior.

Build the Rust binary first:

```bash
cd /home/xu/project/tools/EverOS-Hermes/rust-version
cargo build --release
```

Install/copy the plugin shim:

```bash
mkdir -p ~/.hermes/plugins
cp -r /home/xu/project/tools/EverOS-Hermes/rust-version/integrations/hermes ~/.hermes/plugins/everos
```

If the binary is not on PATH, set this in `~/.hermes/.env` or the Hermes process environment:

```bash
EVEROS_HERMES_RUST_BIN=/home/xu/project/tools/EverOS-Hermes/rust-version/target/release/everos-hermes-rust
```

Then set the provider:

```yaml
memory:
  provider: everos
```

Restart Hermes CLI/WebUI/gateway after changing the provider. MCP tools and the memory provider are independent surfaces; you may enable either or both.

### Provider tools

| Tool | Purpose |
| --- | --- |
| `everos_memory_save` | Save an explicit long-term memory and optionally flush. |
| `everos_memory_search` | Search EverOS memory for the configured user. |
| `everos_memory_get` | Retrieve structured memories by type and page. |
| `everos_memory_flush` | Force EverOS extraction for the user/session. |
| `everos_memory_forget` | Delete a memory by id; requires `confirm=true`. |

Advanced non-secret provider settings remain compatible with the Python version and live in `$HERMES_HOME/everos.json`.

## Provider CLI helpers

The Python shim calls these commands internally, but they are useful for debugging:

```bash
# Availability check
./target/release/everos-hermes-rust provider is-available --hermes-home ~/.hermes

# List provider tool schemas
./target/release/everos-hermes-rust provider tool-schemas

# Run a prefetch manually
./target/release/everos-hermes-rust provider prefetch \
  --state-json '{"session_id":"s1","hermes_home":"/home/xu/.hermes","platform":"cli","agent_identity":"default"}' \
  --query 'coffee preference'
```

## Development and verification

```bash
cd /home/xu/project/tools/EverOS-Hermes/rust-version
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --tests --no-fail-fast
```

The test suite includes:

- dotenv fallback parity;
- EverOS client HTTP request construction through a local TCP mock server;
- search default/filter parity;
- context formatter parity;
- provider availability/user-id/tool-schema parity;
- provider save tool behavior;
- real binary stdio MCP initialize + tools/list smoke test.

## Security notes

- No real EverOS API key is committed here.
- `.env` and build artifacts are ignored.
- The MCP server never logs to stdout; stdout is reserved for MCP protocol frames.
- Destructive deletion requires explicit `confirm=true`.
- The Python shim is intentionally thin; EverOS logic lives in Rust.
