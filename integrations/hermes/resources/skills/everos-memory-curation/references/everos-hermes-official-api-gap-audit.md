# EverOS-Hermes Official API Gap Audit

Use this reference when auditing EverOS-Hermes against the official EverOS / Evermind v1 API docs, especially when the user asks what plugin features are incomplete.

## Source of truth

Local official API reference directory used in the audit:

- `/home/xu/project/tools/EverOS-Hermes/evermind_api_reference/INDEX.md`
- v1 endpoint detail files under `/home/xu/project/tools/EverOS-Hermes/evermind_api_reference/v1/`

Do not infer endpoint coverage from memory. Re-read the local reference and implementation files when doing a fresh audit.

## Current implementation shape observed in this audit

EverOS-Hermes is not a full EverOS v1 API wrapper. It primarily implements:

- Personal memory add/flush/search/get/delete.
- Agent memory add/flush/search/get visibility workflows.
- Task status.
- Memory-space settings get/update.
- Hermes provider context assembly, raw/profile/agent recall controls, and workflow helpers.

## Endpoint coverage snapshot

Implemented and exposed in Python/Rust MCP/provider surfaces:

- `POST /api/v1/memories` — personal add.
- `POST /api/v1/memories/agent` — agent add via `scope="agent"`.
- `POST /api/v1/memories/flush` — personal flush.
- `POST /api/v1/memories/agent/flush` — agent flush.
- `POST /api/v1/memories/get` — get structured memories.
- `POST /api/v1/memories/search` — search structured/raw/agent memory.
- `POST /api/v1/memories/delete` — single memory id and scoped user/session delete only.
- `GET /api/v1/tasks/{task_id}` — task status.
- `GET /api/v1/settings` and `PUT /api/v1/settings` — settings.

Partial / drift:

- `POST /api/v1/memories/group` and `POST /api/v1/memories/group/flush`: Rust low-level client methods exist, but Python client/MCP/provider and Rust MCP do not expose them. Current contract docs mark group memory out-of-scope.

Missing official v1 capabilities:

- Group memory full lifecycle.
- Groups CRUD: `POST /api/v1/groups`, `GET/PATCH /api/v1/groups/{group_id}`.
- Senders CRUD: `POST /api/v1/senders`, `GET/PATCH /api/v1/senders/{sender_id}`.
- Multimodal object signing: `POST /api/v1/object/sign`, S3 upload helper, objectKey content wrapping.
- Group/sender scoped filters and deletes.
- `content-item[]` message content; current validators require non-empty string content.
- Full official filters DSL: `group_id`, `in`, and some Rust-side `session_id` operators are missing/rejected.
- Delete semantics for `group_id`, `sender_id`, and official `__all__`/null/empty-string tri-state are not implemented.
- `timezone: null` in settings is not accepted by strict validators; current code requires a non-empty IANA timezone string.

## Existing tests/docs checked

Useful verification commands:

```bash
python -m pytest tests/test_cloud_contract.py tests/test_schemas.py tests/test_upgrade_contract.py -q
(cd rust-version && cargo test --tests --no-fail-fast)
```

Observed coverage:

- Python contract tests explicitly whitelist personal/agent/get/search/delete/task/settings and blacklist group/sender/object signing.
- `docs/everos_cloud_v1_contract.md` states group/sender/object-storage/multimodal are out-of-scope.
- Rust parity tests cover MCP/provider behavior, but the Rust low-level group methods create a small contract drift unless deliberately kept for future work.

## Recommended audit workflow

1. Load this skill and this reference.
2. Parse `evermind_api_reference/INDEX.md` for official v1 endpoints.
3. Search implementation paths in:
   - `src/everos_hermes/client.py`
   - `src/everos_hermes/mcp_server.py`
   - `src/everos_hermes/provider.py`
   - `src/everos_hermes/schemas.py`
   - `rust-version/src/client.rs`
   - `rust-version/src/mcp.rs`
4. Classify each endpoint as implemented, exposed, partial, intentionally out-of-scope, or missing.
5. Separately compare schema-level features: filters DSL, message content type, delete scope, settings nullability.
6. Run the Python and Rust tests above.
7. Report gaps as current product-scope decisions vs actual unfinished work.

## Pitfall

Do not promise profile edit/reset as a missing plugin feature: the official reference used here does not expose a profile patch/reset endpoint. Long `Basic Information` / `Personality & Traits` remains a Cloud aggregate limitation unless new official APIs appear.
