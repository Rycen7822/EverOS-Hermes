# User Profile ↔ Agent Memory Routing

Use this reference when USER PROFILE is near its character budget or the user asks to move content into agent memories.

## Durable pattern

- Keep USER PROFILE for short, must-follow user preferences that should be injected every session.
- Move task-class procedures, workflow references, and solved-problem cases to EverOS agent memories or Hermes skills.
- Prefer Hermes skills for repeatable workflows with steps and verification.
- Prefer EverOS agent cases for compact solved-problem patterns, diagnostics, and future-agent decision support.
- Prefer reference files under the governing skill for session-specific detail or workflow knowledge that should not become a catalog-level skill.

## Safe migration sequence

1. Classify entries:
   - must-follow preference → keep as compact USER PROFILE fallback;
   - workflow/procedure → skill or skill reference;
   - solved-problem pattern → EverOS agent case;
   - environment fact → local memory only if stable and hard to rediscover;
   - task log/SHA/package hash/pressure cycle/raw transcript → do not save.
2. Write compact agent memory/reference first when moving content out of USER PROFILE.
3. Flush and verify agent visibility (`agent_memory` search plus `agent_case`/`agent_skill` get when available).
4. If `agent_visibility_status` remains `not_visible`, do not delete mandatory preferences entirely; keep a short USER PROFILE/local-memory fallback.
5. Remove or compress only entries that can be recovered from skills, references, or project/source files.
6. Report both USER PROFILE usage after cleanup and agent-memory visibility status.

## Known EverOS behavior

Agent-scope writes can return queued/extracted while structured `agent_memory`, `agent_case`, or `agent_skill` search/get still misses. Treat this as a visibility state, not proof the content is useless. Use skill/local-memory fallbacks for rules that must reliably affect future behavior.

A real multi-message trajectory is more reliable for Cloud-visible `agent_case` extraction than a single compact assistant note. Use user intent → assistant diagnosis/action → tool verification with `tool_call_id` → assistant fix/lesson. When calling the Cloud API directly, include epoch-millisecond `timestamp` on every message. A result with visible `agent_case` but no `agent_skill` is `partial` visibility and is still useful for reusable case curation.

For this user, automatic capture and agent recall are explicitly enabled in `/home/xu/.hermes/everos.json`; keep recall volume bounded and clean noisy captures rather than disabling capture by default.

## Good USER PROFILE target

Aim to keep USER PROFILE comfortably below capacity (roughly half-full or less) by storing only high-priority preferences. Do not fill it with project-specific workflow details if those details can live in a skill reference or agent case.
