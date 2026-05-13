# EverOS Agent Memory Visibility

Load this reference when working with scope="agent", agent_case, agent_skill, agent_memory, trajectory capture, visibility verification, or the queued-versus-visible distinction.

## EverOS Agent Memory Visibility

Current EverOS-Hermes behavior has three layers. Do not collapse them:

1. **Queue/flush accepted** — the Cloud task can succeed and `flush` can report `extracted`.
2. **Structured agent memory visible** — `agent_memory` search returns `cases` or `skills`, or `everos_get_memories(memory_type="agent_case")` / `agent_skill` returns rows.
3. **Provider recall injected** — Hermes must also have agent recall enabled and may need a fresh session/gateway restart after config changes.

For `scope="agent"` writes:

1. A successful queue/flush response is not enough.
2. Check `agent_visibility` when available. Current EverOS-Hermes workflow reports include `verification_user_id`, `verification_session_id`, and per-check `user_id` / `session_id`; use these fields as the controller-side identity for follow-up checks.
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

### Identity Discipline for Controller Verification

Do not invent or override `user_id` for agent-scope writes unless the user explicitly tells you to test a different account. Use the active provider/default identity, and record the exact `user_id`, `session_id`, `scope`, marker, and visible case/skill id in the evidence file. A controller must verify using the same `user_id` and `session_id`; a case saved under `hermes-agent` will not be found by default `hermes_default` checks even when it is actually visible.

When a subagent claims a memory write succeeded, require these fields before trusting it:

- `scope` and exact `user_id` used for save/flush/search/get;
- exact `session_id` and marker query;
- `agent_visibility_status` plus `verification_user_id` / `verification_session_id` if returned;
- visible `agent_case` or `agent_skill` id, or an explicit `not_visible` result;
- controller-side recheck using that same identity.

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
