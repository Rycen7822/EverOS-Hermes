<div align="center">

# EverOS-Hermes

**EverOS Cloud memory for Hermes Agent: Python source plus a Rust prebuilt package for both stdio MCP tools and a Hermes memory provider.**

Use EverOS either as explicit MCP tools, or as an optional Hermes memory provider that can recall before a turn and capture completed user/assistant turns after the response.

</div>

<br/>

<p align="center">
  <a href="README.md"><img src="https://img.shields.io/badge/Docs-README-f5c542?style=for-the-badge" alt="Documentation"></a>
  <a href="https://github.com/Rycen7822/EverOS-Hermes"><img src="https://img.shields.io/badge/GitHub-EverOS--Hermes-0969da?style=for-the-badge" alt="GitHub repository"></a>
  <a href="src/everos_hermes/mcp_server.py"><img src="https://img.shields.io/badge/MCP-13%20tools-2ea44f?style=for-the-badge" alt="MCP: thirteen tools"></a>
  <a href="integrations/hermes"><img src="https://img.shields.io/badge/Hermes-memory%20provider-5865F2?style=for-the-badge" alt="Hermes memory provider"></a>
  <a href="rust-version/README.md"><img src="https://img.shields.io/badge/Runtime-Python%20%7C%20Rust-blue?style=for-the-badge" alt="Runtime: Python and Rust"></a>
  <a href="https://github.com/Rycen7822/EverOS-Hermes/releases"><img src="https://img.shields.io/badge/Rust%20Prebuilt-available-0969da?style=for-the-badge" alt="Rust prebuilt package available"></a>
</p>

> EverOS-Hermes is for Hermes Agent users who want EverOS as a long-term memory backend.
> MCP-only mode exposes tools but does not automatically search or save memory; provider mode adds
> Hermes' automatic memory hooks.

## Why

Hermes has two different integration surfaces that are both useful for memory:

- **automatic memory provider hooks** for recall, capture, explicit memory writes, and session flushes;
- **MCP tools** for deliberate agent actions such as searching, saving, deleting, or checking EverOS tasks.

EverOS-Hermes keeps those surfaces in one small package:

1. a shared stdlib EverOS REST client;
2. a FastMCP stdio server with EverOS memory tools;
3. a thin Hermes `MemoryProvider` plugin that can be copied into `~/.hermes/plugins/everos`;
4. a Rust runtime and Linux x86_64 prebuilt release package for fast local installs.

Secrets stay in the normal Hermes secret file, so users can edit `~/.hermes/.env` instead of embedding keys in MCP `env:` blocks.

## Features

- **Optional Hermes memory provider**: set `memory.provider: everos` when you want automatic recall/capture hooks.
- **Thirteen explicit MCP tools**: the nine Cloud v1 primitives plus batch/import/verify workflow helpers for safer migration and searchability checks.
- **Provider context engine**: both Python and Rust provider runtimes include structured agent trajectory capture, a budgeted context assembler, deterministic message ids, prefetch caching, opt-in session-scoped recent raw recall, and agent-scope visibility reports that distinguish raw queue/flush success from structured `agent_memory`/`agent_case`/`agent_skill` visibility.
- **Dotenv fallback**: credential lookup is `process env` -> `$HERMES_HOME/.env` -> `~/.hermes/.env`.
- **Two runtimes**: Python/FastMCP source implementation plus a Rust binary with a prebuilt Linux x86_64 package.
- **Cloud v1 contract**: personal and agent memory are supported; group, sender, and multimodal object storage endpoints are explicitly out of scope. See [`docs/everos_cloud_v1_contract.md`](docs/everos_cloud_v1_contract.md).
- **Configurable provider loop**: `auto_recall`, `auto_capture`, `flush_after_turn`, `agent_recall`, and `include_recent_raw` can be tuned in `$HERMES_HOME/everos.json`.
- **Safe secret hygiene**: examples use placeholders only; `.env` and local reference checkouts are ignored.

## Rust version

A Rust port is available under [`rust-version/`](rust-version/). It keeps this Python version intact while adding a native `everos-hermes-rust` binary for the stdio MCP server plus a thin Hermes Python shim that delegates provider behavior to Rust.

Rust context-engine parity is current for the provider hooks described below: `rust-version/` now includes matching `trajectory`, `policy`, and `context_assembler` modules, prefetch caching, session-scoped recent raw recall, deterministic message ids, and structured agent trajectory hooks for `sync_turn`, `on_pre_compress`, `on_session_end`, and `on_delegation`.

Quick build:

```bash
cd rust-version
cargo build --release
```

See [`rust-version/README.md`](rust-version/README.md) for Rust MCP/provider configuration and verification commands.

## Install

| Variant | Best for | Runtime requirements | Status |
| --- | --- | --- | --- |
| Rust prebuilt package | Normal Linux x86_64 installs, especially MCP-only use and quick Hermes provider setup | Linux x86_64; Hermes Agent for provider/MCP registration; no Rust toolchain | Available as a GitHub release asset |
| Python version | Editing, debugging, or using the original FastMCP/provider implementation | Python 3.10+, `pip`, Hermes Agent, EverOS API key | Available |
| Rust from source | Native local use on other platforms, development, and reproducible builds | Rust toolchain; Python only for the thin Hermes provider shim | Available |

EverOS-Hermes has two independent Hermes surfaces:

- **MCP server**: exposes explicit EverOS tools under `mcp_servers.everos`; it does not change Hermes' memory provider.
- **Memory provider**: enables automatic recall/capture hooks with `memory.provider: everos`; it does not register an MCP server by itself.

You may enable MCP-only, provider-only, or both. EverOS credentials are read from process env -> `$HERMES_HOME/.env` -> `~/.hermes/.env`; keep secrets there instead of duplicating real keys inside MCP `env:` blocks.

### Agent Self-Install Prompts

Copy exactly one matching prompt into Hermes, Codex, or another coding agent. The prompts deliberately use absolute paths for the installed binary and tell the agent to verify each surface separately.

Rust prebuilt package, recommended on Linux x86_64:

```text
Install EverOS-Hermes for this Hermes Agent from `https://github.com/Rycen7822/EverOS-Hermes`. If `uname -s` is Linux and `uname -m` is x86_64/amd64, prefer the latest Rust prebuilt release asset named `everos-hermes-rust-<version>-x86_64-unknown-linux-gnu.tar.gz`; otherwise use the Rust-from-source path. Download both the `.tar.gz` and `.sha256`, verify with `sha256sum -c` before extracting, and install the extracted package directory to `$HOME/.local/share/everos-hermes`. Do not assume the binary is on PATH; use the absolute binary `$HOME/.local/share/everos-hermes/bin/everos-hermes-rust`. Put `EVEROS_API_KEY` and optional `EVEROS_USER_ID` only in `$HERMES_HOME/.env` or `~/.hermes/.env`; if the key is missing, ask the user rather than inventing one. If explicit MCP tools are desired, register `everos` with command `$HOME/.local/share/everos-hermes/bin/everos-hermes-rust` and arg `mcp` (for example `hermes mcp add everos --command "$HOME/.local/share/everos-hermes/bin/everos-hermes-rust" --args mcp`, or the equivalent YAML). If automatic memory-provider hooks are desired, copy `$HOME/.local/share/everos-hermes/integrations/hermes` to `$HERMES_HOME/plugins/everos` or `~/.hermes/plugins/everos`, set/update `EVEROS_HERMES_RUST_BIN` to an absolute path such as `/home/you/.local/share/everos-hermes/bin/everos-hermes-rust` (do not put `$HOME` in the `.env` value), then run `hermes config set memory.provider everos`. Memory-provider plugins are selected by `memory.provider`; do not rely on `plugins.enabled` for this provider. Verify with the absolute binary `--help`, `provider is-available --hermes-home <Hermes home>`, `hermes mcp test everos` if MCP was enabled, and a fresh Hermes session or restart for provider hooks.
```

Python/source version, for editing or debugging the source implementation:

```text
Install the Python/source version of EverOS-Hermes from `https://github.com/Rycen7822/EverOS-Hermes`, not the Rust prebuilt package. Clone it to a stable local tools directory and run `python -m pip install -e .` using the same Python environment Hermes will use; if Hermes resolves a different Python, use absolute interpreter paths in config. Put `EVEROS_API_KEY` and optional `EVEROS_USER_ID` only in `$HERMES_HOME/.env` or `~/.hermes/.env`; if the key is missing, ask the user. For explicit MCP tools, prefer the installed console script `everos-mcp` when it resolves to the same environment (`hermes mcp add everos --command everos-mcp`), otherwise configure YAML with an absolute Python command and args `-m everos_hermes.mcp_server`. For provider hooks, copy the repo's `integrations/hermes` directory to `$HERMES_HOME/plugins/everos` or `~/.hermes/plugins/everos` and run `hermes config set memory.provider everos`. Verify importability, `python -m pytest tests -q`, `hermes mcp test everos` if MCP was enabled, and a fresh Hermes session or restart for provider hooks.
```

Rust from source, for platform-specific native builds:

```text
Build EverOS-Hermes Rust from source by cloning `https://github.com/Rycen7822/EverOS-Hermes`, then run `cd rust-version && cargo build --release && cargo test --tests --no-fail-fast`. Install `rust-version/target/release/everos-hermes-rust` to `$HOME/.local/share/everos-hermes/bin/everos-hermes-rust` and copy `rust-version/integrations/hermes` to `$HOME/.local/share/everos-hermes/integrations/hermes`. Use the absolute binary path for MCP (`hermes mcp add everos --command "$HOME/.local/share/everos-hermes/bin/everos-hermes-rust" --args mcp`) and set/update `EVEROS_HERMES_RUST_BIN` to the same absolute binary path before enabling `memory.provider: everos`; do not put `$HOME` or `$INSTALL_DIR` in the `.env` value. Keep secrets in `$HERMES_HOME/.env` or `~/.hermes/.env`. Verify the binary `--help`, `provider is-available --hermes-home <Hermes home>`, `hermes mcp test everos` if MCP was enabled, and a fresh Hermes session or restart for provider hooks.
```

### Rust Prebuilt Package

The Rust prebuilt package is published as a GitHub release asset for Linux x86_64. Use the Python or Rust-from-source paths below for other hosts.

Release asset shape:

```text
https://github.com/Rycen7822/EverOS-Hermes/releases/download/v<version>/everos-hermes-rust-<version>-<target>.tar.gz
```

Current Linux x86_64 asset:

```text
everos-hermes-rust-0.2.1-x86_64-unknown-linux-gnu.tar.gz
```

Verified install flow:

```bash
VERSION=0.2.1
TARGET=x86_64-unknown-linux-gnu
PKG_NAME="everos-hermes-rust-${VERSION}-${TARGET}"
ASSET="${PKG_NAME}.tar.gz"
INSTALL_DIR="$HOME/.local/share/everos-hermes"
TMPDIR="$(mktemp -d)"

curl -fL -o "/tmp/$ASSET" \
  "https://github.com/Rycen7822/EverOS-Hermes/releases/download/v${VERSION}/${ASSET}"
curl -fL -o "/tmp/$ASSET.sha256" \
  "https://github.com/Rycen7822/EverOS-Hermes/releases/download/v${VERSION}/${ASSET}.sha256"
(cd /tmp && sha256sum -c "$ASSET.sha256")

tar -xzf "/tmp/$ASSET" -C "$TMPDIR"
rm -rf "$INSTALL_DIR"
mkdir -p "$(dirname "$INSTALL_DIR")"
mv "$TMPDIR/$PKG_NAME" "$INSTALL_DIR"
"$INSTALL_DIR/bin/everos-hermes-rust" --help
```

Optional PATH convenience:

```bash
mkdir -p "$HOME/.local/bin"
ln -sfn "$INSTALL_DIR/bin/everos-hermes-rust" "$HOME/.local/bin/everos-hermes-rust"
```

MCP registration for Hermes:

```bash
hermes mcp add everos --command "$INSTALL_DIR/bin/everos-hermes-rust" --args mcp
hermes mcp test everos
```

Equivalent MCP YAML:

```yaml
mcp_servers:
  everos:
    command: /home/you/.local/share/everos-hermes/bin/everos-hermes-rust
    args:
      - mcp
```

Hermes memory provider setup:

```bash
mkdir -p "${HERMES_HOME:-$HOME/.hermes}/plugins"
rm -rf "${HERMES_HOME:-$HOME/.hermes}/plugins/everos"
cp -R "$INSTALL_DIR/integrations/hermes" "${HERMES_HOME:-$HOME/.hermes}/plugins/everos"
```

Then add or update these settings:

```bash
# In ${HERMES_HOME:-$HOME/.hermes}/.env:
EVEROS_HERMES_RUST_BIN=/home/you/.local/share/everos-hermes/bin/everos-hermes-rust

# Run this to update config.yaml:
hermes config set memory.provider everos
```

Provider availability check:

```bash
"$INSTALL_DIR/bin/everos-hermes-rust" provider is-available --hermes-home "${HERMES_HOME:-$HOME/.hermes}"
```

Restart Hermes CLI/WebUI/gateway after changing memory provider config. MCP tools and the memory provider are independent surfaces; you may enable either or both.

### Python Version

Use this path when you want the editable Python implementation or want to debug FastMCP / provider behavior directly.

```bash
git clone https://github.com/Rycen7822/EverOS-Hermes.git
cd EverOS-Hermes
python -m pip install -e .
python -m pytest tests -q
```

If Hermes runs under a different Python environment than your shell, install the package with that interpreter instead.

Provider context-engine knobs shared by the Python and Rust runtimes can be placed in `$HERMES_HOME/everos.json` during development/debugging:

```json
{
  "max_context_chars": 12000,
  "prefetch_cache_enabled": true,
  "prefetch_cache_ttl_seconds": 120,
  "include_recent_raw": false,
  "recent_raw_top_k": 4,
  "agent_summary_after_turn": false,
  "agent_trajectory_on_session_end": true,
  "agent_trajectory_on_pre_compress": true,
  "agent_trajectory_on_delegation": true,
  "agent_max_messages": 80,
  "agent_max_message_chars": 8000,
  "agent_max_tool_result_chars": 6000,
  "agent_max_payload_chars": 60000
}
```

`include_recent_raw=true` is intentionally opt-in and session-scoped; without a session id, recent raw recall is skipped instead of running a global raw-message search.

MCP registration after installing the Python package:

```bash
hermes mcp add everos --command everos-mcp
```

If `everos-mcp` would resolve to the wrong environment, use YAML with an absolute interpreter path:

```yaml
mcp_servers:
  everos:
    command: /absolute/path/to/python
    args:
      - -m
      - everos_hermes.mcp_server
```

### Rust From Source

Build the Rust binary locally:

```bash
git clone https://github.com/Rycen7822/EverOS-Hermes.git
cd EverOS-Hermes/rust-version
cargo build --release
cargo test --tests --no-fail-fast
target/release/everos-hermes-rust --help
```

Create the same release archive shape locally:

```bash
./scripts/package-release.sh
```

See [`rust-version/README.md`](rust-version/README.md) for Rust-specific MCP/provider details.

## Required Secrets

Create an EverOS key at https://everos.evermind.ai/api-keys and store it in the normal Hermes dotenv file:

```bash
mkdir -p ~/.hermes
$EDITOR ~/.hermes/.env
```

Example values:

```bash
EVEROS_API_KEY=your_everos_api_key
EVEROS_USER_ID=hermes_default
# Optional:
EVEROS_BASE_URL=https://api.evermind.ai
EVEROS_TIMEOUT=10
# Required for the Rust provider shim unless the binary is discoverable on PATH:
EVEROS_HERMES_RUST_BIN=/home/you/.local/share/everos-hermes/bin/everos-hermes-rust
```

Credential lookup order:

1. current process environment variables, for temporary shell overrides;
2. `$HERMES_HOME/.env`, for Hermes profiles or tests;
3. `~/.hermes/.env`, the default Hermes secret file.

The MCP config does not need an `env:` block unless you intentionally want per-server overrides.

## Use as Hermes Memory Provider

Install either the Rust prebuilt package or the Python package first, then place the matching plugin shim in Hermes' memory-provider plugin directory.

Rust prebuilt provider shim:

```bash
INSTALL_DIR="$HOME/.local/share/everos-hermes"
HERMES_HOME="${HERMES_HOME:-$HOME/.hermes}"
mkdir -p "$HERMES_HOME/plugins"
rm -rf "$HERMES_HOME/plugins/everos"
cp -R "$INSTALL_DIR/integrations/hermes" "$HERMES_HOME/plugins/everos"
# Add or update this line in "$HERMES_HOME/.env" with an absolute path
# (dotenv values are not shell-expanded):
# EVEROS_HERMES_RUST_BIN=/home/you/.local/share/everos-hermes/bin/everos-hermes-rust
```

Python/source provider shim:

```bash
cd /path/to/EverOS-Hermes
python -m pip install -e .
HERMES_HOME="${HERMES_HOME:-$HOME/.hermes}"
mkdir -p "$HERMES_HOME/plugins"
rm -rf "$HERMES_HOME/plugins/everos"
cp -R integrations/hermes "$HERMES_HOME/plugins/everos"
```

Select the provider:

```bash
hermes config set memory.provider everos
```

Memory-provider discovery scans `$HERMES_HOME/plugins/everos` for a provider selected by `memory.provider`; this provider does not need a `plugins.enabled` entry. Restart Hermes CLI / WebUI / gateway, or start a fresh session after changing memory provider config.

### Provider Behavior

This section applies only when `memory.provider: everos` is enabled. It is separate from the MCP server.

| Hook | EverOS action |
| --- | --- |
| `prefetch(query)` | If `auto_recall=true`, searches EverOS before a turn, uses the budgeted context assembler, and can consume a lock-protected prefetch cache. Optional recent raw recall is session-scoped and off by default. |
| `sync_turn(user, assistant)` | If `auto_capture=true`, saves the completed user/assistant turn with deterministic `message_id` values; `flush_after_turn=true` makes extraction run immediately. Optional lightweight agent summaries require `agent_summary_after_turn=true`. |
| `on_memory_write()` | Mirrors explicit Hermes memory writes to EverOS. |
| `on_session_end()` | Can write structured agent trajectory first, then flush the active EverOS session; personal flush still runs even if agent trajectory write fails. |
| `on_pre_compress()` | Can save capped structured agent trajectory before context compression without flushing. |
| `on_delegation()` | Can save a task/result pair to agent scope with a `[delegation child_session_id=...]` prefix. |

Hermes provider tools exposed to the agent:

| Tool | Purpose |
| --- | --- |
| `everos_memory_save` | Queue an explicit memory message and optionally request extraction; `saved=true` does not guarantee immediate structured/profile recall. |
| `everos_memory_search` | Search EverOS memory for the configured user. |
| `everos_memory_get` | Retrieve structured memories by type and page. |
| `everos_memory_flush` | Force EverOS extraction for the user/session; accepts per-call `timeout` and returns retryable timeout guidance. |
| `everos_memory_forget` | Delete a memory by id; requires `confirm=true`. |
| `everos_memory_save_and_verify` | Queue one message, optionally flush, then run targeted search verification and return a structured queue/verification report. |
| `everos_memory_import_and_verify` | Dry-run or execute batched message/file import with warnings, per-batch status, optional flush, and verification queries. |
| `everos_memory_verify_session` | Read-only verification helper for an existing user/session/scope using sample search queries. |

Advanced non-secret provider settings live in `$HERMES_HOME/everos.json`:

```json
{
  "base_url": "https://api.evermind.ai",
  "user_id": "hermes_{identity}",
  "auto_recall": true,
  "auto_capture": true,
  "flush_after_turn": true,
  "search_method": "hybrid",
  "top_k": 5,
  "memory_types": ["episodic_memory", "profile"],
  "max_context_chars": 12000,
  "profile_max_items": 3,
  "agent_skills_max_items": 4,
  "agent_cases_max_items": 4,
  "episodic_max_items": 6,
  "min_score": 0.0,
  "min_recall_query_chars": 8,
  "include_recent_raw": false,
  "recent_raw_top_k": 4,
  "prefetch_cache_enabled": true,
  "prefetch_cache_ttl_seconds": 120,
  "capture_agent_memory": false,
  "agent_recall": false,
  "agent_summary_after_turn": true,
  "agent_trajectory_on_session_end": true,
  "agent_trajectory_on_pre_compress": true,
  "agent_trajectory_on_delegation": true,
  "agent_flush_after_turn": true,
  "agent_memory_types": ["agent_memory"],
  "agent_visibility_verify_after_write": false,
  "agent_visibility_verify_after_flush": false,
  "agent_visibility_queries": [],
  "agent_visibility_top_k": 5,
  "agent_visibility_timeout": 30.0,
  "agent_visibility_get_page_size": 20,
  "agent_visibility_retry_flush_attempts": 1,
  "agent_visibility_retry_flush_backoff_ms": 250,
  "agent_max_messages": 80,
  "agent_max_message_chars": 8000,
  "agent_max_tool_result_chars": 6000,
  "agent_max_payload_chars": 60000,
  "agent_dedupe_entries": 256,
  "timeout": 10.0
}
```

`EVEROS_USER_ID` overrides `everos.json`. Templates can use `{user_id}`, `{user_name}`, `{identity}`, and `{platform}`.

Agent visibility options are intentionally off by default for provider hooks. When `agent_visibility_verify_after_write` or `agent_visibility_verify_after_flush` is enabled, the provider probes personal search plus agent structured surfaces and records `agent_visibility_status` as `unchecked`, `not_visible`, `partial`, or `visible`; raw queue/flush success alone is not treated as structured visibility.

## Use as MCP Server

After installing either runtime, register exactly one `mcp_servers.everos` command. MCP-only mode does not run provider hooks; it only makes tools available for the model to call explicitly.

Rust prebuilt/source command, recommended when a Rust binary is installed:

```bash
INSTALL_DIR="$HOME/.local/share/everos-hermes"
hermes mcp add everos --command "$INSTALL_DIR/bin/everos-hermes-rust" --args mcp
```

Equivalent Rust MCP YAML:

```yaml
mcp_servers:
  everos:
    command: /home/you/.local/share/everos-hermes/bin/everos-hermes-rust
    args:
      - mcp
```

Python/source console-script command after `python -m pip install -e .`:

```bash
hermes mcp add everos --command everos-mcp
```

Equivalent Python MCP YAML, useful when you need an absolute interpreter path:

```yaml
mcp_servers:
  everos:
    command: /absolute/path/to/python
    args:
      - -m
      - everos_hermes.mcp_server
```

If `python`, `everos-mcp`, or the Rust binary would resolve to the wrong environment, use an absolute command path. The MCP config does not need an `env:` block unless you intentionally want per-server overrides.

Verify:

```bash
hermes mcp test everos
```

Manual stdio launch for another MCP client:

```bash
/home/you/.local/share/everos-hermes/bin/everos-hermes-rust mcp
# or, for the Python runtime:
everos-mcp
```

When configured in Hermes, the stdio MCP server is launched as a Hermes-managed child process. It starts when Hermes loads MCP servers and exits/restarts with Hermes or `/reload-mcp`.

## MCP Operations

The MCP server exposes thirteen tools:

| Tool | Purpose | Read-only? |
| --- | --- | --- |
| `everos_save_memory` | Queue one explicit text memory message, then optionally flush; response separates queue/extraction/searchability state. For agent scope, `role=tool` requires `tool_call_id`; default agent role is non-tool, and the response includes `agent_visibility` with `unchecked` unless a workflow performs structured verification. | No |
| `everos_add_memories` | Add one or more messages to personal or agent scope; optional `message_id` is preserved for idempotent retries; legacy `agent` alias remains supported but conflicts with `scope`. Agent-scope primitive responses include unchecked visibility metadata. | No |
| `everos_flush_memories` | Trigger personal or agent extraction immediately; supports per-call `timeout`, retryable timeout responses, and one retry for transient request-send failures. Agent flush returns flush status plus unchecked visibility metadata. | No |
| `everos_search_memories` | Search with keyword, vector, hybrid, or agentic retrieval; exposes `filters`, `radius`, `top_k=-1`, `timeout`, and agentic fallback; vector fields are stripped unless `include_vectors=true`. | Yes |
| `everos_get_memories` | Retrieve structured memories with `filters`, pagination, `rank_by`, and `rank_order`. | Yes |
| `everos_delete_memories` | Delete exactly one `memory_id` or a confirmed user/session batch; batch delete requires `confirm_scope_text`. | No, destructive |
| `everos_get_task_status` | Check an asynchronous extraction task. | Yes |
| `everos_get_settings` | Read EverOS memory-space settings. | Yes |
| `everos_update_settings` | Update whitelisted EverOS settings fields and return a before/after diff. | No |
| `everos_batch_ingest` | Dry-run or execute batched ingest, optionally flush, and return per-batch plus verification status; workflow reports metrics and adaptively splits Cloud 403 batches. | No |
| `everos_verify_session_ingest` | Read-only search verification for an existing user/session/scope. | Yes |
| `everos_save_and_verify` | Queue one message, optionally flush, then verify recall with one or more search queries. | No |
| `everos_import_and_verify` | Batch-import messages or a local file with dry-run validation, optional flush, verification report, metrics, and adaptive split-on-403 behavior. | No |

Common search call shape:

```json
{
  "query": "user coffee preference",
  "method": "hybrid",
  "top_k": 5,
  "memory_types": ["episodic_memory", "profile"],
  "filters": {"user_id": "hermes_default", "AND": [{"session_id": "optional-session"}]},
  "radius": 0.5,
  "include_original_data": false,
  "include_vectors": false,
  "timeout": 10,
  "fallback_to_hybrid": true,
  "response_format": "markdown"
}
```

Use `method="agentic"` only for complex multi-part retrieval because it is slower and more expensive than `hybrid`. Even when `include_original_data=true`, embedding/vector fields are removed by default to avoid flooding context; set `include_vectors=true` only for debugging.

Search/get type mapping is intentionally split: `search` accepts `episodic_memory`, `profile`, `raw_message`, and `agent_memory`; `get` accepts `episodic_memory`, `profile`, `agent_case`, and `agent_skill`. `top_k=-1` is allowed for Cloud search, but Markdown rendering still caps prompt context separately. Numeric public arguments are validated rather than silently coerced: invalid `top_k`, `page`, or `page_size` fails before HTTP, while schema-valid `radius=0` is preserved.

Delete safety is stricter than raw CRUD: single delete uses `memory_id` only, while batch delete requires an explicit `user_id`, `confirm=true`, and `confirm_scope_text` exactly matching `delete user_id=<id>` or `delete user_id=<id> session_id=<session>`.

Settings updates are restricted to the documented settings whitelist and return a diff. Unknown keys are rejected before the request is sent.

Workflow import helpers validate `messages[].timestamp` locally: when supplied, it must be an integer epoch-millisecond value such as `1712052000000`, not an ISO datetime string. Dry-run reports `warnings` plus `metrics` (`total_messages`, batch counts, content length, and estimated payload bytes). During execution, if EverOS Cloud returns `403 Forbidden` for a multi-message batch, the helper records the failed oversized batch, splits it in half, retries the child batches, and returns `split_count`, `payload_bytes`, `split_reason`, and a small-batch recommendation in `suggested_next_actions`.

Agent-scope workflow helpers (`everos_save_and_verify`, `everos_verify_session_ingest`, and import/batch verification when `scope="agent"`) return an `agent_visibility` object. The status values are:

- `unchecked`: raw message queueing/flush was attempted, but no structured visibility probe was run;
- `not_visible`: personal/raw search may show the queued content, but agent structured surfaces are still empty;
- `partial`: at least one agent structured probe returned data, but not all expected agent surfaces are visible;
- `visible`: agent structured probes found the memory on the expected agent surfaces.

This distinction avoids treating a successful queue or flush response as proof that `agent_memory`, `agent_case`, or `agent_skill` is already searchable.

## Runtime Modes

| Mode | Enable with | Automatic behavior | Use when |
| --- | --- | --- | --- |
| MCP-only | `mcp_servers.everos` | None. The model must call EverOS tools explicitly. | You want manual search/save/delete tools without changing Hermes memory. |
| Provider-only | `memory.provider: everos` | Optional recall before each turn, capture after each completed user/assistant turn, and flush on session end. | You want EverOS as Hermes' memory backend. |
| Both | Both config blocks | Provider hooks plus explicit MCP tools. | You want automatic memory and manual EverOS controls. |

For lower latency or stricter control, keep `auto_capture=true` but set `auto_recall=false` in `$HERMES_HOME/everos.json`; the agent can still search manually through `everos_memory_search` or `everos_search_memories`.

## Project Layout

| Path | Purpose |
| --- | --- |
| `src/everos_hermes/client.py` | Stdlib EverOS v1 REST client and API error handling. |
| `src/everos_hermes/env.py` | Hermes dotenv lookup helpers for secrets and endpoint overrides. |
| `src/everos_hermes/formatting.py` | EverOS response to compact prompt/Markdown formatting for MCP/tool output. |
| `src/everos_hermes/context_assembler.py` | Python provider context assembler for profile/skills/cases/episodes/recent raw sections under a global budget. |
| `src/everos_hermes/policy.py` | Lightweight recall/capture skip policy and stable prefetch cache key helpers. |
| `src/everos_hermes/trajectory.py` | Structured agent trajectory conversion, redaction/capping, tool-call linkage, and stable message ids. |
| `src/everos_hermes/mcp_server.py` | FastMCP stdio server and thirteen MCP tools. |
| `src/everos_hermes/workflows.py` | Shared batch/import/save-and-verify workflow helpers used by MCP and provider tools. |
| `src/everos_hermes/provider.py` | Hermes `MemoryProvider` implementation. |
| `integrations/hermes/` | Thin plugin entrypoint and Hermes-specific install notes. |
| `scripts/` | Release packaging and smoke-test helpers, including `everos_agent_visibility_smoke.py` for MCP fake-server visibility checks. |
| `tests/` | Client, provider, and MCP tool tests with fake clients / HTTP. |
| `agentmemory-main/` | Local reference checkout; intentionally ignored and not vendored. |

## Development

Run the current verification suite without leaving cache artifacts:

```bash
PYTHONDONTWRITEBYTECODE=1 python -m pytest -p no:cacheprovider tests -q
PYTHONDONTWRITEBYTECODE=1 python -m py_compile src/everos_hermes/*.py integrations/hermes/__init__.py
```

MCP smoke-test pattern:

```bash
python -m everos_hermes.mcp_server
# from an MCP client: initialize, then tools/list; expect the thirteen EverOS tools above
```

Agent visibility fake-server smoke for the Rust MCP binary:

```bash
cd /home/xu/project/tools/EverOS-Hermes
python scripts/everos_agent_visibility_smoke.py \
  --binary rust-version/target/debug/everos-hermes-rust \
  --mode build-tree \
  --output .tmp_everos_visibility_smoke/build_tree_summary.json
```

The smoke script drives MCP stdio, uses a local fake EverOS Cloud, redacts authorization headers in its JSON summary, and verifies `not_visible`/`partial`/`visible`, unchecked primitive agent saves, local `role=tool` validation, and transient agent-flush retry behavior.

Repository hygiene before commits:

```bash
git check-ignore -v agentmemory-main .env .pytest_cache
git diff --check
```

## Security Notes

- Do not commit EverOS API keys, `.env`, MCP `env:` blocks with real credentials, or generated cache directories.
- The client sends `Authorization: Bearer ...` only at request time; examples use placeholders only, and smoke summaries redact it as `Bearer ***`.
- `everos_delete_memories` and `everos_memory_forget` are destructive and require explicit confirmation flags.
- EverOS extraction is asynchronous by default; flushing makes newly added messages searchable sooner but can add API work.

## Status

- Python package: available.
- Hermes memory provider plugin: available.
- stdio MCP server: available.
- Rust prebuilt package / release artifacts: available on GitHub Releases.
