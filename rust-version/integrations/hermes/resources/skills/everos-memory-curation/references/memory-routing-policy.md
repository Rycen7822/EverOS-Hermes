# EverOS Memory Routing Policy

Load this reference when deciding whether information belongs in personal memory, agent memory, a Hermes skill, a reusable case, or nowhere. This keeps durable memory useful and prevents stale task logs from being saved.

## Recall Decision Rules

Before asking the user to repeat context, recall when prior work, recurring project conventions, previous decisions, or likely skill/case/session context may matter.

Preferred recall order:

1. Load the most relevant skill first if a skill trigger matches.
2. Use `session_search` for prior task transcripts and stale task history.
3. Use EverOS memory search for durable profile/fact/case recall.
4. Use local files for source-of-truth project state.

Do not treat memory as current system state. For current time, filesystem, git, process, versions, or config, use tools against the live environment.

## Write Decision Rules

Save only durable, reusable information: user preferences/corrections, stable conventions, reusable workflows, solved-problem cases, API/tool quirks, or explicit remember requests.

Do not save task progress, commit SHAs, PR/issue numbers, package hashes, pressure-test cycle IDs, random session IDs, one-off smoke results, raw transcripts, raw tool output, duplicated source text, or anything likely to become stale within a week.

If uncertain, skip saving or ask briefly.

## Target Selection

Choose the narrowest durable target:

1. **Hermes skill** — reusable procedures with triggers, steps, pitfalls, and verification.
2. **EverOS agent case** — compact solved-problem diagnostics and future-agent decision support.
3. **EverOS agent skill** — procedural content when Cloud structured visibility supports it.
4. **Hermes local memory** — compact stable facts, environment quirks, and durable agent notes.
5. **Hermes user profile** — user preferences, style, habits, and personal conventions.
6. **Session transcript only** — ordinary task history; do not duplicate it into durable memory.

Prefer skill over memory when the content contains steps. Prefer memory over skill when the content is a short fact or preference.

## Reusable Case Format

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

Create or patch a skill when the workflow has repeatable steps, commands, validation, pitfalls, and is likely to recur. Check existing skills first, ask before creating/deleting a skill, and patch loaded skills immediately if stale, incomplete, or wrong.

## Default Policy

Default unknown-user policy is conservative. If automatic capture or agent recall is enabled by configuration, keep recall volume bounded, curate deliberately after complex tasks, and clean noisy captures instead of relying on blanket capture alone.

## Post-task Proactive Curation Checklist

Do not wait for the user to explicitly ask for memory after a complex or iterative task. Before the final response, decide whether the work produced a reusable workflow, solved-problem case, stable quirk, or durable preference; save only compact durable content and verify `agent_visibility` when agent-scope visibility matters.
