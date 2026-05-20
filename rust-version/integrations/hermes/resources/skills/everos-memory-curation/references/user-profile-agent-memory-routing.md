# User Profile ↔ Agent Memory Routing

Use this reference when USER PROFILE is near its character budget or the user asks to move content into agent memories.

## Durable pattern

- Keep USER PROFILE for short, must-follow user preferences that should be injected every session.
- Move task-class procedures, workflow references, and solved-problem cases to EverOS agent memories or Hermes skills.
- Prefer Hermes skills for repeatable workflows with steps and verification.
- Prefer EverOS agent cases for compact solved-problem patterns, diagnostics, and future-agent decision support.
- Prefer reference files under the governing skill for session-specific detail or workflow knowledge that should not become a catalog-level skill.

## Safe migration sequence

1. Classify entries: must-follow preference, workflow/procedure, solved-problem pattern, stable environment fact, or skip.
2. Write the compact skill/reference/agent case before removing detail from USER PROFILE.
3. Flush and verify agent visibility with the same `user_id` and `session_id` used for the save.
4. If visibility remains `not_visible`, keep a short USER PROFILE/local-memory fallback for mandatory preferences.
5. Remove or compress only entries recoverable from skills, references, memories, or source files.
6. Report USER PROFILE usage and agent-memory visibility status after cleanup.

## Known EverOS behavior

Agent-scope writes can return queued/extracted while structured `agent_memory`, `agent_case`, or `agent_skill` search/get still misses. Treat this as a visibility state, not proof the content is useless. Use skill/local-memory fallbacks for rules that must reliably affect future behavior.

A real multi-message trajectory is more reliable for Cloud-visible `agent_case` extraction than a single compact assistant note. Use user intent → assistant diagnosis/action → tool verification with `tool_call_id` → assistant fix/lesson. When calling the Cloud API directly, include epoch-millisecond `timestamp` on every message. A result with visible `agent_case` but no `agent_skill` is `partial` visibility and is still useful for reusable case curation.

If automatic capture and agent recall are enabled, keep recall volume bounded and clean noisy captures rather than disabling capture by default.

## Good USER PROFILE target

Keep USER PROFILE comfortably below capacity by storing only high-priority preferences. Do not fill it with project-specific workflow details if those details can live in a skill reference or agent case.
