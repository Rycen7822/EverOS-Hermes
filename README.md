<div align="center">

# EverOS-Hermes

**EverOS Cloud memory for Hermes Agent: one Python package that provides both a stdio MCP server and a Hermes memory provider.**

Search EverOS before a turn, capture completed conversations after a turn, and expose explicit EverOS memory tools without duplicating API keys in MCP config.

</div>

<br/>

<p align="center">
  <a href="README.md"><img src="https://img.shields.io/badge/Docs-README-f5c542?style=for-the-badge" alt="Documentation"></a>
  <a href="https://github.com/Rycen7822/EverOS-Hermes"><img src="https://img.shields.io/badge/GitHub-EverOS--Hermes-0969da?style=for-the-badge" alt="GitHub repository"></a>
  <a href="src/everos_hermes/mcp_server.py"><img src="https://img.shields.io/badge/MCP-9%20tools-2ea44f?style=for-the-badge" alt="MCP: nine tools"></a>
  <a href="integrations/hermes"><img src="https://img.shields.io/badge/Hermes-memory%20provider-5865F2?style=for-the-badge" alt="Hermes memory provider"></a>
  <a href="pyproject.toml"><img src="https://img.shields.io/badge/Runtime-Python%203.10%2B-blue?style=for-the-badge" alt="Python 3.10+"></a>
</p>

> EverOS-Hermes is for Hermes Agent users who want EverOS as a long-term memory backend.
> For local-only memory, Hermes' built-in provider may be simpler. For explicit EverOS API access only,
> the MCP server can be enabled without switching Hermes' memory provider.

## Why

Hermes has two different integration surfaces that are both useful for memory:

- **automatic memory provider hooks** for recall, capture, explicit memory writes, and session flushes;
- **MCP tools** for deliberate agent actions such as searching, saving, deleting, or checking EverOS tasks.

EverOS-Hermes keeps those surfaces in one small package:

1. a shared stdlib EverOS REST client;
2. a FastMCP stdio server with EverOS memory tools;
3. a thin Hermes `MemoryProvider` plugin that can be copied into `~/.hermes/plugins/everos`.

Secrets stay in the normal Hermes secret file, so users can edit `~/.hermes/.env` instead of embedding keys in MCP `env:` blocks.

## Features

- **Hermes memory provider**: set `memory.provider: everos` to recall and capture through EverOS.
- **Nine MCP tools**: save, add, flush, search, get, delete, task status, get settings, and update settings.
- **Dotenv fallback**: credential lookup is `process env` -> `$HERMES_HOME/.env` -> `~/.hermes/.env`.
- **Low dependency surface**: EverOS API client uses Python stdlib; MCP uses `mcp` / FastMCP.
- **Search-before-generation loop**: `prefetch()` calls EverOS hybrid search for episodic/profile memory.
- **Capture-after-generation loop**: `sync_turn()` stores user/assistant turns and can flush immediately.
- **Explicit memory mirroring**: Hermes `on_memory_write()` writes durable memory events to EverOS.
- **Safe secret hygiene**: examples use placeholders only; `.env` and local reference checkouts are ignored.

## Rust version

A feature-parity Rust port is available under [`rust-version/`](rust-version/). It keeps this Python version intact while adding a native `everos-hermes-rust` binary for the stdio MCP server plus a thin Hermes Python shim that delegates provider behavior to Rust.

Quick build:

```bash
cd rust-version
cargo build --release
```

See [`rust-version/README.md`](rust-version/README.md) for Rust MCP/provider configuration and verification commands.

## Install

| Variant | Best for | Runtime requirements | Status |
| --- | --- | --- | --- |
| Source / editable install | Normal use today, development, debugging | Python 3.10+, Hermes Agent, EverOS API key | Available |
| Hermes provider plugin | Automatic recall/capture in Hermes | Source package installed in Hermes' Python env | Available |
| stdio MCP server | Explicit EverOS tools in Hermes or another MCP client | Source package installed where MCP command runs | Available |
| Prebuilt package | One-command distribution | TBD | Planned, not yet published |

### Agent Self-Install Prompts

Copy one of these prompts into a coding agent if you want it to install EverOS-Hermes for itself.

Hermes provider + MCP, recommended:

```text
Install EverOS-Hermes from repo `https://github.com/Rycen7822/EverOS-Hermes` for Hermes Agent. Clone it to a stable local tools directory, run `python -m pip install -e .`, copy `integrations/hermes` to `~/.hermes/plugins/everos`, set `memory.provider: everos`, add MCP server `everos` with `python -m everos_hermes.mcp_server`, put `EVEROS_API_KEY` and optional `EVEROS_USER_ID` in `~/.hermes/.env` without committing secrets, then verify with `python -m pytest -q`, `hermes mcp test everos`, and a fresh Hermes session.
```

MCP-only install:

```text
Install only the EverOS-Hermes MCP server from repo `https://github.com/Rycen7822/EverOS-Hermes`. Clone it, run `python -m pip install -e .`, add MCP server `everos` using `python -m everos_hermes.mcp_server`, keep `EVEROS_API_KEY` in `~/.hermes/.env`, and verify with `hermes mcp test everos` plus `tools/list` showing the EverOS tools.
```

### Source / Editable Install

```bash
git clone https://github.com/Rycen7822/EverOS-Hermes.git
cd EverOS-Hermes
python -m pip install -e .
python -m pytest tests -q
```

If Hermes runs under a different Python environment than your shell, install the package with that interpreter instead.

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
```

Credential lookup order:

1. current process environment variables, for temporary shell overrides;
2. `$HERMES_HOME/.env`, for Hermes profiles or tests;
3. `~/.hermes/.env`, the default Hermes secret file.

The MCP config does not need an `env:` block unless you intentionally want per-server overrides.

## Use as Hermes Memory Provider

Install the package, then copy the plugin entrypoint into Hermes' plugin directory:

```bash
cd /path/to/EverOS-Hermes
python -m pip install -e .
mkdir -p ~/.hermes/plugins
cp -r integrations/hermes ~/.hermes/plugins/everos
```

Set the provider in `~/.hermes/config.yaml`:

```yaml
memory:
  provider: everos
```

Restart Hermes CLI / WebUI / gateway, or start a fresh session after changing memory provider config.

### Provider Behavior

| Hook | EverOS action |
| --- | --- |
| `prefetch(query)` | `POST /api/v1/memories/search` with `method="hybrid"`, `top_k=5`, and `memory_types=["episodic_memory", "profile"]`. |
| `sync_turn(user, assistant)` | `POST /api/v1/memories`, then `POST /api/v1/memories/flush` by default. |
| `on_memory_write()` | Mirrors explicit Hermes memory writes to EverOS. |
| `on_session_end()` | Flushes the active EverOS session. |

Hermes provider tools exposed to the agent:

| Tool | Purpose |
| --- | --- |
| `everos_memory_save` | Save an explicit long-term memory and optionally flush. |
| `everos_memory_search` | Search EverOS memory for the configured user. |
| `everos_memory_get` | Retrieve structured memories by type and page. |
| `everos_memory_flush` | Force EverOS extraction for the user/session. |
| `everos_memory_forget` | Delete a memory by id; requires `confirm=true`. |

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
  "capture_agent_memory": false,
  "timeout": 10.0
}
```

`EVEROS_USER_ID` overrides `everos.json`. Templates can use `{user_id}`, `{user_name}`, `{identity}`, and `{platform}`.

## Use as MCP Server

After installing the package, add this to `~/.hermes/config.yaml`:

```yaml
mcp_servers:
  everos:
    command: python
    args:
      - -m
      - everos_hermes.mcp_server
```

Equivalent console-script command after installation:

```yaml
mcp_servers:
  everos:
    command: everos-mcp
```

If `python` or `everos-mcp` would resolve to the wrong environment, use an absolute interpreter or executable path.

Verify:

```bash
hermes mcp test everos
```

Manual stdio launch for another MCP client:

```bash
python -m everos_hermes.mcp_server
# or
everos-mcp
```

When configured in Hermes, the stdio MCP server is launched as a Hermes-managed child process. It starts when Hermes loads MCP servers and exits/restarts with Hermes or `/reload-mcp`.

## MCP Operations

The MCP server exposes nine tools:

| Tool | Purpose | Read-only? |
| --- | --- | --- |
| `everos_save_memory` | Save one explicit text memory, then optionally flush. | No |
| `everos_add_memories` | Add one or more user/assistant/tool messages. | No |
| `everos_flush_memories` | Trigger boundary detection and extraction immediately. | No |
| `everos_search_memories` | Search with keyword, vector, hybrid, or agentic retrieval. | Yes |
| `everos_get_memories` | Retrieve structured memories with pagination. | Yes |
| `everos_delete_memories` | Delete by memory id or confirmed user/session scope. | No, destructive |
| `everos_get_task_status` | Check an asynchronous extraction task. | Yes |
| `everos_get_settings` | Read EverOS memory-space settings. | Yes |
| `everos_update_settings` | Update supplied EverOS settings fields. | No |

Common search call shape:

```json
{
  "query": "user coffee preference",
  "method": "hybrid",
  "top_k": 5,
  "memory_types": ["episodic_memory", "profile"],
  "response_format": "markdown"
}
```

Use `method="agentic"` only for complex multi-part retrieval because it is slower and more expensive than `hybrid`.

## How It Works

```text
Hermes user turn
  -> EverOS provider prefetch(query)
  -> EverOS hybrid search injects compact memory context
  -> LLM response
  -> EverOS provider sync_turn(user, assistant)
  -> EverOS add memories
  -> optional EverOS flush for near-immediate extraction
```

The MCP tools are separate from automatic provider hooks. Enable both when you want automatic memory plus explicit EverOS control tools.

## Project Layout

| Path | Purpose |
| --- | --- |
| `src/everos_hermes/client.py` | Stdlib EverOS v1 REST client and API error handling. |
| `src/everos_hermes/env.py` | Hermes dotenv lookup helpers for secrets and endpoint overrides. |
| `src/everos_hermes/formatting.py` | EverOS response to compact prompt/Markdown formatting. |
| `src/everos_hermes/mcp_server.py` | FastMCP stdio server and nine MCP tools. |
| `src/everos_hermes/provider.py` | Hermes `MemoryProvider` implementation. |
| `integrations/hermes/` | Thin plugin entrypoint and Hermes-specific install notes. |
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
# from an MCP client: initialize, then tools/list; expect the nine EverOS tools above
```

Repository hygiene before commits:

```bash
git check-ignore -v agentmemory-main .env .pytest_cache
git diff --check
```

## Security Notes

- Do not commit EverOS API keys, `.env`, MCP `env:` blocks with real credentials, or generated cache directories.
- The client sends `Authorization: Bearer <EVEROS_API_KEY>` only at request time.
- `everos_delete_memories` and `everos_memory_forget` are destructive and require explicit confirmation flags.
- EverOS extraction is asynchronous by default; flushing makes newly added messages searchable sooner but can add API work.

## Status

- Python package: available.
- Hermes memory provider plugin: available.
- stdio MCP server: available.
- Prebuilt package / release artifacts: planned, not yet published.
