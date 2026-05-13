# EverOS Cloud v1 capability contract for EverOS-Hermes

EverOS-Hermes targets EverOS Cloud v1 personal and agent memory workflows for Hermes Agent. The integration treats EverOS as an asynchronous memory system, not as a synchronous key-value store: accepting a message is not the same as making a structured memory immediately searchable.

## Base URL and auth

- Base URL: `https://api.evermind.ai`
- Auth: `Authorization: Bearer ...` (examples and smoke summaries redact real tokens as `Bearer ***`)
- All supported Cloud endpoints are under `/api/v1`.
- v0 endpoints are deprecated and must not be used by this integration.

## Endpoint whitelist

## Hermes workflow helpers

The Hermes MCP/provider surface may expose high-level helpers that compose the whitelisted endpoints below. These helpers are not new EverOS Cloud endpoints; they are local orchestration wrappers:

- `everos_batch_ingest` / `everos_memory_import_and_verify`: dry-run or execute batched `add_memories`, optional `flush`, and sample `search` verification. Reports include input/queued/failed counts, batch status, warnings, metrics, flush status, verification hits/misses, and suggested next actions. Dry-run validates supplied message timestamps as integer epoch milliseconds; execution may adaptively split multi-message batches that return Cloud `403 Forbidden`.
- `everos_verify_session_ingest` / `everos_memory_verify_session`: read-only verification using `POST /api/v1/memories/search` for one or more sample queries.
- `everos_save_and_verify` / `everos_memory_save_and_verify`: one-message `add_memories`, optional `flush`, and searchability verification.

Workflow helpers should return a stable envelope with at least `ok`, `workflow`, `status`, `retryable`, and `suggested_next_actions`. They must not call group/sender/object-storage/multimodal endpoints, and dry-run mode must not send write or flush HTTP requests.

### POST /api/v1/memories

Scope: personal memory.

Request body:

```json
{
  "user_id": "user_123",
  "session_id": "session_123",
  "messages": [{"role": "user", "timestamp": 1711900000000, "content": "...", "message_id": "msg_123"}],
  "async_mode": true
}
```

Message fields:

- `role`, integer epoch-millisecond `timestamp`, and non-empty `content` are required. ISO datetime strings are rejected by Hermes validators before write workflows send HTTP.
- `message_id` is an optional idempotency key. EverOS-Hermes preserves caller-provided `message_id` values and validates that they are non-empty strings when present; provider lifecycle writes generate deterministic ids for retry-safe personal and agent writes.
- In agent scope, `role="tool"` additionally requires a non-empty `tool_call_id`.

Response notes:

- A successful response means the raw messages were accepted/queued.
- If the Cloud response includes `task_id`, callers should poll task status or search later.
- `searchable` remains unknown until search confirms a structured memory exists.

### POST /api/v1/memories/agent

Scope: agent memory / agent trajectory.

Request body is the same shape as personal add, but roles may include `tool` for summarized tool results, corrections, and reusable agent lessons. EverOS Cloud requires `tool_call_id` whenever `role="tool"`; Hermes MCP/provider wrappers default agent single-message saves to a non-tool role and reject missing `tool_call_id` before HTTP when the caller explicitly asks for `role="tool"`.

A structured agent trajectory write can preserve assistant tool calls and tool-result linkage while staying under provider caps. Example message list:

```json
[
  {
    "role": "user",
    "timestamp": 1711900000000,
    "content": "Debug the timeout regression.",
    "message_id": "eh_user_1",
    "source": "session_end"
  },
  {
    "role": "assistant",
    "timestamp": 1711900001000,
    "content": "I will inspect the failing test and rerun it.",
    "message_id": "eh_assistant_1",
    "source": "session_end",
    "tool_calls": [{"id": "call_1", "function": {"name": "terminal", "arguments": "{\"command\":\"pytest tests/test_timeout.py -q\"}"}}]
  },
  {
    "role": "tool",
    "timestamp": 1711900002000,
    "content": "1 failed; timeout branch reproduced.",
    "message_id": "eh_tool_1",
    "source": "session_end",
    "tool_call_id": "call_1"
  }
]
```

Response notes are the same as personal add. EverOS-Hermes wraps agent-scope primitive write responses with `agent_visibility.status="unchecked"` because raw queue acceptance is not structured visibility.

### POST /api/v1/memories/flush

Scope: personal memory extraction.

Request body:

```json
{"user_id": "user_123", "session_id": "session_123"}
```

Response notes:

- Flush triggers boundary detection/extraction for the target user/session.
- Timeout is retryable but does not prove extraction failed; callers should search or check task/request status before retrying.

### POST /api/v1/memories/agent/flush

Scope: agent memory extraction.

Request body:

```json
{"user_id": "user_123", "session_id": "session_123"}
```

Response notes match personal flush, but apply to agent memory. EverOS-Hermes retries one transient request-send failure before returning an agent flush result; timeout responses remain retryable guidance rather than proof of failure.

### POST /api/v1/memories/search

Searches personal and/or agent memory according to `memory_types`.

Request body:

```json
{
  "query": "debug MCP timeout",
  "filters": {"user_id": "user_123", "AND": [{"session_id": "session_123"}]},
  "method": "hybrid",
  "memory_types": ["episodic_memory", "profile", "agent_memory"],
  "top_k": 5,
  "radius": 0.5,
  "include_original_data": false
}
```

Status and lifecycle notes:

- `top_k=-1` asks Cloud for all matching results; prompt injection must still cap context separately. Out-of-range `top_k` values must be rejected before HTTP rather than silently coerced.
- `radius` is only valid for vector/hybrid/agentic retrieval; `0.0` is schema-valid and must be preserved instead of treated as absent.
- Vectors are stripped before returning data to Hermes unless `include_vectors=true` is explicitly requested for debugging.

### POST /api/v1/memories/get

Retrieves paginated structured memories.

Request body:

```json
{
  "memory_type": "episodic_memory",
  "filters": {"user_id": "user_123"},
  "page": 1,
  "page_size": 20,
  "rank_by": "timestamp",
  "rank_order": "desc"
}
```

Response notes:

- `page` is 1-based; invalid values must be rejected before HTTP rather than silently coerced.
- `page_size` is limited to 1..100 in this integration; invalid values must be rejected before HTTP rather than silently coerced.
- Get memory types differ from search memory types; see mapping below.

### POST /api/v1/memories/delete

Deletes a single memory or a scoped user/session batch.

Single delete body:

```json
{"memory_id": "memory_123"}
```

Batch delete body:

```json
{"user_id": "user_123", "session_id": "session_123"}
```

Safety notes:

- Single delete is mutually exclusive with `user_id`/`session_id`.
- Batch delete requires explicit `user_id`; provider/tool defaults are not used.
- Batch delete requires confirmation text: `delete user_id=<USER_ID> session_id=<SESSION_ID_OR_*>`.
- HTTP 204 is normalized to `{ "ok": true, "status_code": 204, "deleted": true }`.

### GET /api/v1/tasks/{task_id}

Reads asynchronous extraction task status.

Response notes:

- Known statuses include processing/queued, success, failed, and unknown/expired.
- Unknown or expired status does not prove extraction failed; check search results before retrying writes.

### GET /api/v1/settings

Reads memory-space settings such as timezone, creation/update timestamps, and `llm_custom_setting`.

### PUT /api/v1/settings

Updates memory-space settings.

Request body:

```json
{"timezone": "Asia/Tokyo", "llm_custom_setting": {"style": "concise"}}
```

Safety notes:

- Strict mode is default.
- Allowed fields: `timezone`, `llm_custom_setting`.
- Unknown fields are rejected unless an explicit unsafe passthrough mode is used for development.
- `timezone` must be an IANA timezone.
- `llm_custom_setting` must be an object.

## Out of scope endpoint blacklist

The following EverOS Cloud APIs are intentionally out of scope for this release and must not be exposed in default tools or provider calls:

- Group memory: `POST /api/v1/memories/group`, `POST /api/v1/memories/group/flush`
- Groups CRUD: `/api/v1/groups`
- Senders CRUD: `/api/v1/senders`
- Multimodal object signing/storage: `POST /api/v1/object/sign`, S3 upload, `objectKey` content item wrapping

This release does not implement group memory, sender attribution, or multimodal upload. README/release notes must not claim those capabilities.

## Memory type mapping

| Operation | Allowed types | Notes |
|---|---|---|
| search | `episodic_memory`, `profile`, `raw_message`, `agent_memory` | `agent_memory` may contain agent cases and skills. |
| get | `episodic_memory`, `profile`, `agent_case`, `agent_skill` | `agent_case` and `agent_skill` are paginated separately. |

Invalid cross-use is rejected: `agent_case`/`agent_skill` are not valid search types, and `agent_memory` is not a valid get type.

## Filter DSL

Supported filter fields:

- `user_id` scalar equality; required for personal/agent memory queries.
- `session_id` scalar equality or operator object.
- `timestamp` scalar or operator object using `eq`, `gt`, `gte`, `lt`, `lte`.
- `AND` / `OR` arrays containing nested filter objects.

Unknown fields are rejected even if Cloud would ignore them, because silent ignore would make Hermes believe a filter was applied when it was not. Top-level `user_id`/`session_id` parameters are merged with filters only when there is no conflict.

## Agent visibility envelope

Agent-scope helpers must distinguish four states:

```json
{
  "agent_visibility_status": "not_visible",
  "agent_raw_queued": true,
  "agent_flush": {"ok": true, "status": "success"},
  "agent_visibility_checks": [
    {"surface": "personal_search", "visible": true},
    {"surface": "agent_memory", "visible": false},
    {"surface": "agent_case", "visible": false},
    {"surface": "agent_skill", "visible": false}
  ]
}
```

Status meanings:

- `unchecked`: primitive write/flush path did not probe structured visibility;
- `not_visible`: raw/personal search may contain the text, but no agent structured surface was visible;
- `partial`: at least one agent structured surface was visible, but expected surfaces were still missing;
- `visible`: structured agent probes found data on the expected agent surfaces.

This is a local EverOS-Hermes contract layered over Cloud v1 endpoints. It does not add Cloud endpoints and must not call out-of-scope group/sender/object/multimodal APIs.

## Lifecycle payload contract

EverOS-Hermes write/flush tools return lifecycle information similar to:

```json
{
  "ok": true,
  "scope": "personal",
  "user_id": "user_123",
  "session_id": "session_123",
  "message_queued": true,
  "extraction_requested": true,
  "task_id": "task_123",
  "flush": {"ok": true, "status": "queued"},
  "searchable": null,
  "next_actions": ["search after extraction completes", "poll everos_get_task_status if task_id exists"]
}
```

`searchable=null` is the safe default. It becomes `true` only when a follow-up search confirms the target memory.
