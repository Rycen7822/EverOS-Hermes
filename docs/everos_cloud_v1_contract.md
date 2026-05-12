# EverOS Cloud v1 capability contract for EverOS-Hermes

EverOS-Hermes targets EverOS Cloud v1 personal and agent memory workflows for Hermes Agent. The integration treats EverOS as an asynchronous memory system, not as a synchronous key-value store: accepting a message is not the same as making a structured memory immediately searchable.

## Base URL and auth

- Base URL: `https://api.evermind.ai`
- Auth: `Authorization: Bearer <api-key>`
- All supported Cloud endpoints are under `/api/v1`.
- v0 endpoints are deprecated and must not be used by this integration.

## Endpoint whitelist

### POST /api/v1/memories

Scope: personal memory.

Request body:

```json
{
  "user_id": "user_123",
  "session_id": "session_123",
  "messages": [{"role": "user", "timestamp": 1711900000000, "content": "..."}],
  "async_mode": true
}
```

Response notes:

- A successful response means the raw messages were accepted/queued.
- If the Cloud response includes `task_id`, callers should poll task status or search later.
- `searchable` remains unknown until search confirms a structured memory exists.

### POST /api/v1/memories/agent

Scope: agent memory / agent trajectory.

Request body is the same shape as personal add, but roles may include `tool` for summarized tool results, corrections, and reusable agent lessons. EverOS Cloud requires `tool_call_id` whenever `role="tool"`; Hermes MCP/provider wrappers default agent single-message saves to a non-tool role and reject missing `tool_call_id` before HTTP when the caller explicitly asks for `role="tool"`.

Response notes are the same as personal add.

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

Response notes match personal flush, but apply to agent memory.

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
