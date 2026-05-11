# EverOS memory provider for Hermes Agent

This folder is the Hermes memory-provider plugin entrypoint for EverOS.
It mirrors the lightweight `agentmemory-main/integrations/hermes` pattern: copy the plugin into Hermes, set `memory.provider`, and run the MCP server if you also want explicit EverOS tools.

## What it does

- `prefetch()` searches EverOS before each LLM turn and injects relevant episode/profile context.
- `sync_turn()` stores completed user/assistant turns in EverOS and optionally flushes extraction.
- `on_memory_write()` mirrors explicit Hermes memory writes into EverOS.
- `on_session_end()` flushes the active EverOS session.
- Provider tools exposed to Hermes: `everos_memory_search`, `everos_memory_save`, `everos_memory_get`, `everos_memory_flush`, `everos_memory_forget`.

## Install

From this repository:

```bash
cd /home/xu/project/tools/EverOS-Hermes
python -m pip install -e .
mkdir -p ~/.hermes/plugins
cp -r integrations/hermes ~/.hermes/plugins/everos
```

Set credentials in `~/.hermes/.env` or your shell:

```bash
EVEROS_API_KEY=your_everos_api_key
# Optional but recommended for a stable single-user CLI identity:
EVEROS_USER_ID=hermes_default
# Optional:
EVEROS_BASE_URL=https://api.evermind.ai
```

Lookup order is: process environment, `$HERMES_HOME/.env`, then `~/.hermes/.env`.
The EverOS MCP server reads the dotenv file itself, so you normally do not need
to duplicate the key in an MCP `env:` block.

Then set Hermes config:

```yaml
memory:
  provider: everos
```

Restart Hermes (or gateway/WebUI) after changing provider config.

## Optional MCP server

After `python -m pip install -e .`, add this to `~/.hermes/config.yaml`:

```yaml
mcp_servers:
  everos:
    command: python
    args:
      - -m
      - everos_hermes.mcp_server
```

Keep `EVEROS_API_KEY` / `EVEROS_USER_ID` in `~/.hermes/.env` or the environment that launches Hermes. The server reads that file itself, so an MCP `env:` block is optional. If you must use an MCP `env:` block, put real non-placeholder values there and avoid committing secrets.

Then run:

```bash
hermes mcp test everos
```

If Hermes is already running, use `/reload-mcp` or restart the process.

## Config file

Advanced non-secret settings live at `$HERMES_HOME/everos.json`:

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
  "timeout": 10.0
}
```

`EVEROS_USER_ID` overrides `everos.json`. The value can use `{user_id}`, `{user_name}`, `{identity}`, and `{platform}` placeholders when the provider is initialized by Hermes.
