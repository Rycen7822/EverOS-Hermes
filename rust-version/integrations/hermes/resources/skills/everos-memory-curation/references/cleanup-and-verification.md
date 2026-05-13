# EverOS Cleanup and Verification

Load this reference for noisy memory cleanup, compression, destructive deletion verification, common pitfalls, or final post-task curation checks.

## Cleanup / Compression Checklist

Before deleting or compressing memory:

- [ ] Confirm scope: personal vs agent, user_id, session_id, memory_id.
- [ ] Back up structured memories or the specific target set.
- [ ] Prefer exact `memory_id` or `session_id` deletes over broad deletes.
- [ ] If broad delete is requested, require explicit user intent.
- [ ] For profile section compression (`Basic Information`, `Personality & Traits`), load `references/everos-profile-compaction-limits.md` and verify provider prefetch behavior, not just Cloud UI/profile fields.
- [ ] Verify counts and targeted searches after cleanup.
- [ ] Expect raw/profile residuals; report them as Cloud limits, not as successful deletion.
- [ ] Re-seed only compact durable facts, not old task logs.

## Common Pitfalls

1. **Saving everything after every task.** This recreates context bloat. Save only durable reusable knowledge.
2. **Confusing transcript recall with durable memory.** Use `session_search` for old task details; do not promote them unless reusable.
3. **Treating queued agent memory as visible.** Always distinguish raw/queued from structured `agent_case`/`agent_skill` visibility; prefer the trajectory recipe when a visible `agent_case` is required.
4. **Writing procedures into personal memory.** Procedures belong in skills; memory should hold compact facts/preferences.
5. **Deleting without backup.** EverOS delete semantics can be surprising; always back up meaningful sets before cleanup.
6. **Relying on stale memory for live state.** Re-check git, files, config, time, and system state with tools.
7. **Saving secrets or long logs.** Redact secrets and summarize only stable lessons.

## Verification Checklist

For recall:

- [ ] Loaded this skill when memory action is involved.
- [ ] Used session/EverOS/file lookup instead of guessing.
- [ ] Distinguished durable facts from current state.

For writes:

- [ ] Content is durable and compact.
- [ ] Target is correct: skill, agent case, local memory, user profile, or skip.
- [ ] No SHAs, package hashes, pressure cycles, raw logs, or transient task progress.
- [ ] Visibility/search was verified if future retrieval matters.

For cleanup:

- [ ] Backup exists.
- [ ] Scope is explicit.
- [ ] Post-cleanup search/count verification was done.
- [ ] Remaining Cloud residuals are reported honestly.
