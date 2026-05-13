# EverOS Hermes plugin

This directory is the single Hermes plugin entrypoint for EverOS-Hermes.
Copy it to `$HERMES_HOME/plugins/everos` or `~/.hermes/plugins/everos`.

It supports both Hermes loader paths from the same plugin directory:

- Standalone plugin loading via `hermes plugins enable everos` registers the `everos` toolset and the bundled skill `everos:everos-memory-curation`.
- Memory-provider loading via `hermes config set memory.provider everos` registers automatic EverOS recall/capture hooks.

Use both settings for normal operation.

## Install from this repository

```bash
cd /path/to/EverOS-Hermes
python -m pip install -e .

mkdir -p "${HERMES_HOME:-$HOME/.hermes}/plugins"
rm -rf "${HERMES_HOME:-$HOME/.hermes}/plugins/everos"
cp -R integrations/hermes "${HERMES_HOME:-$HOME/.hermes}/plugins/everos"

hermes plugins enable everos
hermes config set memory.provider everos
```

Set credentials in `$HERMES_HOME/.env`, `~/.hermes/.env`, or the process environment:

```bash
EVEROS_API_KEY=your_everos_api_key
EVEROS_USER_ID=hermes_default
# optional:
EVEROS_BASE_URL=https://api.evermind.ai
```

Restart Hermes CLI/WebUI/gateway after changing plugin, provider, or secret configuration.

## Registered toolset

The standalone plugin registers these tool names under the `everos` toolset:

- `everos_memory_save`
- `everos_memory_search`
- `everos_memory_get`
- `everos_memory_flush`
- `everos_memory_forget`
- `everos_memory_save_and_verify`
- `everos_memory_import_and_verify`
- `everos_memory_verify_session`

If the memory provider is also active, Hermes skips duplicate tool schemas and routes the same tool names through the active provider instance. If the standalone plugin is enabled without `memory.provider: everos`, the plugin lazily initializes its own provider for tool calls.

## Bundled skill

The operator/curation runbook is bundled at:

```text
resources/skills/everos-memory-curation/SKILL.md
```

Load it by qualified name:

```text
/skill everos:everos-memory-curation
```

Do not install it separately into `~/.hermes/skills/` unless you intentionally want a user-local fork outside this plugin.

## Advanced config

Advanced non-secret settings live at `$HERMES_HOME/everos.json`. Common settings:

```json
{
  "auto_recall": true,
  "auto_capture": true,
  "capture_agent_memory": true,
  "agent_recall": true,
  "agent_flush_after_turn": true,
  "search_method": "hybrid",
  "top_k": 2,
  "max_context_items": 2,
  "max_context_chars": 3000
}
```

`EVEROS_USER_ID` overrides `everos.json`. It can use `{user_id}`, `{user_name}`, `{identity}`, and `{platform}` placeholders when Hermes initializes the provider.
