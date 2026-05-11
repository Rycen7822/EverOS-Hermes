# EverOS-Hermes

EverOS-Hermes turns EverOS Cloud into both:

1. a local stdio MCP server with EverOS memory tools; and
2. a Hermes Agent memory provider plugin (`memory.provider: everos`).

It is based on EverOS v1 docs:

- API base URL: `https://api.evermind.ai`
- Auth: `Authorization: Bearer <EVEROS_API_KEY>`
- Core loop: search before generation, add conversation after generation, flush for immediate extraction.

## Repository layout

```text
src/everos_hermes/
  client.py        # stdlib EverOS REST client
  formatting.py    # response-to-context formatter
  mcp_server.py    # FastMCP stdio server
  provider.py      # Hermes MemoryProvider implementation
integrations/hermes/
  __init__.py      # thin plugin entrypoint for ~/.hermes/plugins/everos
  plugin.yaml
  README.md
tests/
```

## Install for local development

```bash
cd /home/xu/project/tools/EverOS-Hermes
python -m pip install -e .
python -m pytest tests -q
```

## Required config

Create an EverOS key at https://everos.evermind.ai/api-keys and put it in
`~/.hermes/.env`:

```bash
EVEROS_API_KEY=your_everos_api_key
# recommended for a stable single-user CLI identity
EVEROS_USER_ID=hermes_default
```

Optional:

```bash
EVEROS_BASE_URL=https://api.evermind.ai
EVEROS_TIMEOUT=10
```

The MCP server and Hermes provider resolve credentials in this order:

1. real process environment variables, so temporary shell overrides work;
2. `$HERMES_HOME/.env` when `HERMES_HOME` is set;
3. `~/.hermes/.env` by default.

This means the normal MCP config does not need an `env:` block for the EverOS key.

## Use as Hermes memory provider

```bash
cd /home/xu/project/tools/EverOS-Hermes
python -m pip install -e .
mkdir -p ~/.hermes/plugins
cp -r integrations/hermes ~/.hermes/plugins/everos
```

Add to `~/.hermes/config.yaml`:

```yaml
memory:
  provider: everos
```

Put secrets in `~/.hermes/.env` (not in git):

```bash
EVEROS_API_KEY=your_everos_api_key
EVEROS_USER_ID=hermes_default
```

Restart Hermes CLI/WebUI/gateway after changing the memory provider.

Provider behavior:

- `prefetch(query)` -> `POST /api/v1/memories/search` with `method="hybrid"`, `top_k=5`, `memory_types=["episodic_memory", "profile"]`.
- `sync_turn(user, assistant)` -> `POST /api/v1/memories`, then `POST /api/v1/memories/flush` by default.
- `on_memory_write()` mirrors explicit built-in memory writes to EverOS.
- `on_session_end()` flushes the session.

Hermes memory tools exposed by the provider:

- `everos_memory_save`
- `everos_memory_search`
- `everos_memory_get`
- `everos_memory_flush`
- `everos_memory_forget` (requires `confirm=true`)

## Use as MCP server

After installing the package, add this to `~/.hermes/config.yaml`:

```yaml
mcp_servers:
  everos:
    command: python
    args:
      - -m
      - everos_hermes.mcp_server
```

Keep `EVEROS_API_KEY` / `EVEROS_USER_ID` in `~/.hermes/.env` or the environment that launches Hermes. The server reads that file itself, so an MCP `env:` block is optional. If you must use an MCP `env:` block, put real non-placeholder values there and avoid committing secrets.
Then verify:

```bash
hermes mcp test everos
```

Or run the server manually for an MCP client:

```bash
EVEROS_API_KEY=your_key EVEROS_USER_ID=hermes_default python -m everos_hermes.mcp_server
```

MCP tools:

- `everos_save_memory`
- `everos_add_memories`
- `everos_flush_memories`
- `everos_search_memories`
- `everos_get_memories`
- `everos_delete_memories`
- `everos_get_task_status`
- `everos_get_settings`
- `everos_update_settings`

## Advanced provider config

Non-secret provider settings live in `$HERMES_HOME/everos.json`:

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

`EVEROS_USER_ID` overrides the config file. User templates support `{user_id}`, `{user_name}`, `{identity}`, and `{platform}` placeholders.

## Notes

- EverOS extraction is asynchronous by default; flushing makes newly added messages searchable sooner.
- Use `method="agentic"` only for complex multi-part retrieval because it is slower and more expensive than `hybrid`.
- Do not commit EverOS API keys.
