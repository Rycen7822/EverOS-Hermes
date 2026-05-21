---
name: everos-memory-curation
description: "Use proactively when complex or iterative work may produce durable EverOS/Hermes memory: recall, save, verify, clean, compress, or migrate reusable workflows, debugging lessons, tool/API quirks, and agent cases without saving noisy task logs."
version: 1.0.11
author: Hermes Agent
license: MIT
metadata:
  hermes:
    tags: [everos, memory, recall, curation, agent-memory, skills]
    related_skills: [hermes-agent, hermes-agent-skill-authoring]
---

# EverOS Memory Curation

## Overview

Use this plugin-bundled skill as the lightweight router for EverOS-Hermes memory work. Keep this entrypoint small: load it first, then load only the reference document that matches the user's intent. Do not load every reference by default.

EverOS-Hermes has two user-facing plugin surfaces:

- `memory.provider: everos` enables automatic recall/capture hooks.
- `plugins.enabled: [everos]` exposes explicit `everos_memory_*` tools and this `everos:everos-memory-curation` skill.

The compatibility MCP server can still exist for legacy/advanced callers, but the default Hermes user path is the single plugin.

## When to Use

Load this skill before:

- Searching prior conversations or EverOS memory for cross-session context.
- Saving, importing, cleaning, compressing, or deleting Hermes/EverOS memory.
- Deciding whether a completed task produced a reusable case or skill.
- Proactively curating after complex or iterative tool-using tasks, debugging sessions, plugin/config migrations, or reusable workflow discoveries.
- Handling `scope="agent"`, `agent_case`, `agent_skill`, `agent_memory`, or `agent_visibility`.
- Troubleshooting missing EverOS tools, inactive provider state, plugin installation, or memory migration.

Do not use this for simple one-turn answers where no recall/write/cleanup is needed.

## Post-task Proactive Curation

Do not wait for the user to say "remember this" after complex/iterative tasks, debugging sessions, plugin/config migrations, or reusable workflow discoveries. Before the final response, decide whether a durable lesson belongs in a Hermes skill, an EverOS agent case/memory, personal memory, or an explicit skip decision.

For agent-scope saves, prefer a compact reusable case and load `references/memory-routing-policy.md` plus `references/agent-case-visibility.md` when visibility matters. Skip task logs, SHAs, one-off test output, raw transcripts, and anything likely to become stale within a week.

## Reference Loading Rule

Pick the smallest reference that answers the current question:

1. Plain-language memory actions (`remember`, `recall`, `forget`, `session history`) -> `references/user-intent-runbooks.md`.
2. Whether/where to save information -> `references/memory-routing-policy.md`.
3. Agent memory visibility, trajectories, agent cases, or `tool_call_id` rules -> `references/agent-case-visibility.md`.
4. Agent visibility contract audits, especially Python/Rust identity parity or installed plugin runtime import-path checks -> `references/agent-visibility-contract-audits.md`.
5. Missing tools, plugin/provider setup, imports, exports, or migrations -> `references/plugin-triage-and-migration.md`.
6. Cleanup, compaction, destructive delete verification, or final checklists -> `references/cleanup-and-verification.md`.

If the task spans multiple areas, load multiple references deliberately and say why.

## Reference Map

- `references/user-intent-runbooks.md` — concise remember/recall/forget/session-history runbooks, including when to prefer `everos_memory_save_and_verify`.
- `references/memory-routing-policy.md` — recall/write decision rules, target selection, reusable case format, skill patching, and default policy.
- `references/agent-case-visibility.md` — `scope="agent"`, `agent_case`, `agent_skill`, `agent_memory`, `tool_call_id`, trajectory, and visibility checks.
- `references/agent-visibility-contract-audits.md` — audit pattern for Python/Rust `agent_visibility` identity contracts, installed plugin resource sync, runtime import paths, and evidence-file memory-curation notes.
- `references/plugin-triage-and-migration.md` — plugin/provider/MCP surface triage plus links to deeper API/profile/migration references.
- `references/cleanup-and-verification.md` — cleanup/compression checklist, common pitfalls, and final verification checklist.

Existing specialized references remain available under `references/` and should be loaded only when their topic is directly needed.

## Always-On Guardrails

- Do not make up memories, sessions, observation ids, timestamps, or visibility states.
- Do not save secrets, raw transcripts, transient task progress, PR numbers, commit SHAs, package hashes, or one-off verification output.
- Use skills for repeatable procedures; use memory for stable facts/preferences; use session transcripts for temporary task progress.
- Treat queued/flush success as acceptance, not proof of structured memory visibility. Verify with search/get when visibility matters.
- For destructive operations, search candidates first and delete exact ids only with explicit confirmation.
