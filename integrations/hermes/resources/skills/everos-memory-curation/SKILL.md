---
name: everos-memory-curation
description: Use when deciding whether to recall, save, verify, clean, compress, or migrate Hermes/EverOS memories; routes durable knowledge into skills, agent cases, personal memory, or skip decisions without saving noisy task logs.
version: 1.0.5
author: Hermes Agent
license: MIT
metadata:
  hermes:
    tags: [everos, memory, recall, curation, agent-memory, skills]
    related_skills: [hermes-agent, hermes-agent-skill-authoring]
---

# EverOS Memory Curation

## Overview

Use this skill as the gatekeeper before any Hermes/EverOS memory recall or write. The goal is to keep future context useful: preserve reusable cases, workflows, user preferences, and stable environment facts; avoid saving task-progress logs, transient identifiers, and one-off verification output.

This skill does not replace tool instructions. It tells the agent how to decide whether memory action is warranted and where the information should go.

## When to Use

Load this skill before:

- Searching prior conversations or EverOS memory for cross-session context.
- Saving, importing, cleaning, compressing, or deleting Hermes/EverOS memory.
- Deciding whether a completed task produced a reusable case or skill.
- Handling `scope="agent"`, `agent_case`, `agent_skill`, or `agent_memory` visibility.
- The user says: remember this, recall, memory, EverOS, agent memory, case, skill, clean up, compress, migration, profile, or durable memory.

Do not use this for simple one-turn answers where no recall/write/cleanup is needed.

## Recall Decision Rules

Before asking the user to repeat context, recall when any of these are true:

1. The user references prior work: "last time", "we did", "as before", "continue", "remember".
2. The task depends on a known project, convention, or recurring workflow.
3. The current request may be affected by previous decisions or stable environment facts.
4. You suspect there is a relevant skill, case, or session transcript.

Preferred recall order:

1. Load the most relevant skill first if a skill trigger matches.
2. Use `session_search` for prior task transcripts and stale task history.
3. Use EverOS memory search for durable profile/fact/case recall.
4. Use local files (`read_file`, `search_files`) for source-of-truth project state.

Do not treat memory as current system state. For current time, filesystem, git, process, versions, or config, use tools against the live environment.

## Write Decision Rules

Only save memory when the information is durable and likely useful later.

Save when:

- The user states a preference, correction, stable identity detail, or durable convention.
- A reusable workflow, pitfall, or API/tool quirk was discovered.
- A difficult debugging path produced a stable diagnostic/fix pattern.
- A complex task produced a reusable case that future work can consult.
- The user explicitly asks to remember something.

Do not save:

- Task progress or completion logs.
- Commit SHAs, PR/issue numbers, package hashes, transient artifact paths, or test run counts.
- Pressure-test cycle IDs, random session IDs, temporary markers, or one-off smoke results.
- Long raw transcripts, raw tool output, or duplicated source text.
- Anything likely to become stale within a week, unless it is a stable convention or tool quirk.

If uncertain, skip saving or ask briefly.

## Target Selection

Choose the narrowest durable target:

1. **Hermes skill** — for reusable procedures with triggers, steps, pitfalls, and verification.
2. **EverOS agent case** — for a compact solved-problem case that future agents can reference, especially diagnostics and pitfalls.
3. **EverOS agent skill** — when Cloud supports structured visibility and the content is explicitly procedural.
4. **Hermes local memory (`memory` tool)** — for compact stable facts, environment quirks, and durable agent notes.
5. **Hermes user profile (`memory` tool target=`user`)** — for user preferences, style, habits, and personal conventions.
6. **Session transcript only** — for ordinary task history; do not duplicate it into durable memory.

Prefer skill over memory when the content contains steps. Prefer memory over skill when the content is a short fact or preference.

## Reusable Case Format

When saving a case, compress it to this shape:

```text
Agent case: <short title>
Trigger: <when future agents should look at this>
Problem: <observable symptoms>
Diagnosis: <minimal path that found the cause>
Fix: <stable resolution or decision>
Verification: <how success was checked>
Pitfalls: <what not to infer or do>
Reusable value: <why this matters later>
```

Keep cases short. Do not include raw logs, long diffs, secrets, SHAs, package hashes, or full command transcripts.

## Skill Creation / Patching Rules

Create or patch a skill when:

- The workflow has 3+ repeatable steps.
- It includes commands, validation, and known pitfalls.
- It is likely to recur across sessions.
- Existing skills do not already cover it.

Before creating a new skill:

1. Check whether an existing skill should be patched instead.
2. Ask for confirmation if creating or deleting a skill.
3. Use frontmatter with `name`, `description`, `version`, `author`, `license`, and `metadata.hermes.tags`.
4. Include triggers, steps, pitfalls, and verification checklist.

Patch loaded skills immediately if they are stale, incomplete, or wrong.

## EverOS Agent Memory Visibility

Current EverOS-Hermes behavior has three layers. Do not collapse them:

1. **Queue/flush accepted** — the Cloud task can succeed and `flush` can report `extracted`.
2. **Structured agent memory visible** — `agent_memory` search returns `cases` or `skills`, or `everos_get_memories(memory_type="agent_case")` / `agent_skill` returns rows.
3. **Provider recall injected** — Hermes must also have agent recall enabled and may need a fresh session/gateway restart after config changes.

For `scope="agent"` writes:

1. A successful queue/flush response is not enough.
2. Check `agent_visibility` when available.
3. Treat `not_visible` as a real state, not a failure of local write.
4. Treat `partial` as useful: a visible `agent_case` with no `agent_skill` is still a successful reusable case.
5. If structured agent memory is not visible but the content is important, use a fallback:
   - create/patch a Hermes skill for workflows;
   - save a compact local memory for stable facts/preferences;
   - or keep it in the session transcript if it is not durable.

### Agent Case Trajectory Recipe

When the user wants future agents or Cloud UI/search to see a case, prefer a real trajectory over a single compact assistant note:

1. `user`: task intent or problem statement.
2. `assistant`: diagnosis, action plan, or change made.
3. `tool`: concise verification result with a stable `tool_call_id`.
4. `assistant`: final fix/verification plus reusable lesson and pitfalls.

If calling the Cloud API directly, every message needs an epoch-millisecond `timestamp`. The tool-role message requires `tool_call_id`. This shape was verified to produce a visible `agent_case`; single assistant notes can remain `not_visible`.

Known constraints:

- `role="tool"` in agent scope requires `tool_call_id`.
- Agent-case extraction is more reliable with a real multi-message trajectory than with a single compact reference: send user intent, assistant diagnosis/action, tool result with `tool_call_id`, and assistant fix/verification/reusable lesson. Messages sent through the Cloud API need epoch-millisecond `timestamp` values. A single `scope="agent"` assistant note can remain `not_visible`, while this structured trajectory can produce a visible `agent_case`.
- `agent_visibility_status="partial"` can still mean success for case curation when `agent_case` is visible but `agent_skill` is empty.
- EverOS Cloud delete can return success while raw/profile residuals remain.
- EverOS personal `profile` may behave as a Cloud-side aggregate: exact profile id delete, user-level delete, group delete, and even deletion of profile `processed_episode_ids` / `explicit_info[].sources` / `implicit_traits[].sources` can all return success while `Basic Information` / `Personality & Traits` remain unchanged. See `references/everos-profile-compaction-limits.md` before promising profile compression.
- When auditing EverOS-Hermes against the official EverOS/Evermind API reference, use `references/everos-hermes-official-api-gap-audit.md` to distinguish implemented personal/agent memory surfaces from missing group/sender/multimodal/filter capabilities.
- When USER PROFILE is near capacity or the user asks to move content to agent memories, use `references/user-profile-agent-memory-routing.md`: keep only short must-follow preferences in USER PROFILE, move reusable workflows/cases to agent memory or skills, and keep a fallback when agent visibility is `not_visible`.
- Verify structured visibility after flush/delete when it matters.
- Broad auto-capture can pollute context; for this user it is explicitly enabled, so compensate with aggressive post-task curation and cleanup of noisy captures.

## Recommended Default Policy

Default for unknown users is conservative. For this user, the explicit current override is to enable automatic updates and agent memory capture/recall while keeping recall volume bounded:

```json
{
  "auto_capture": true,
  "capture_agent_memory": true,
  "agent_recall": true,
  "agent_flush_after_turn": true,
  "auto_recall": true,
  "agent_memory_types": ["agent_memory"],
  "memory_types": ["episodic_memory"],
  "top_k": 2,
  "max_context_items": 2,
  "max_context_chars": 3000
}
```

Operational implications:

- After changing these booleans, start a fresh Hermes session or restart the gateway so provider initialization sees the new config.
- Keep saving reusable cases deliberately, but prefer the trajectory recipe above when visibility matters.
- Monitor captures for noisy episodic/profile growth; delete task logs, SHAs, package hashes, pressure cycles, and raw transcripts when they appear.

Use automatic capture plus deliberate post-task curation; do not rely on blanket capture alone to create high-quality agent cases.

## Post-Task Curation Checklist

After a complex or iterative task, ask internally:

- [ ] Did this produce a reusable workflow? If yes, create/patch a skill.
- [ ] Did this produce a reusable solved-problem pattern, diagnostic path, migration pattern, or future-agent decision case? If yes, aggressively save a compact EverOS agent case; when Cloud visibility matters, use the multi-message trajectory recipe, then verify `agent_memory` search and `agent_case` get.
- [ ] Did this reveal a stable API/tool/cloud quirk? If yes, save a compact fact or case.
- [ ] Did the user state a durable preference or correction? If yes, save to user profile only if it must be injected every session; otherwise prefer a skill/reference/agent case.
- [ ] Is USER PROFILE carrying workflow detail better suited to skills/agent memories? If yes, migrate using `references/user-profile-agent-memory-routing.md` and keep only a short fallback.
- [ ] Is this only task progress, a log, a SHA, or a one-off result? If yes, do not save.
- [ ] If saved to EverOS agent scope, did structured visibility become `visible`, `partial`, or `not_visible`? Treat visible `agent_case` plus empty `agent_skill` as useful `partial` success.
- [ ] If visibility failed but the content is important, did you use a skill/local-memory fallback?

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
