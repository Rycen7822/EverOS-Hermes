# EverOS Memory Routing Policy

Load this reference when deciding whether information belongs in personal memory, agent memory, a Hermes skill, a reusable case, or nowhere. This keeps durable memory useful and prevents stale task logs from being saved.

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
