# EverOS User-Intent Runbooks

Load this reference when the user asks to remember, recall, forget, or summarize memory/session history. It adapts the concise Codex agentmemory remember/recall/forget/session-history pattern to EverOS-Hermes tools.

## User-Intent Runbooks

Agentmemory's small `remember`, `recall`, `forget`, and `session-history` skills are a useful operator pattern: convert the user's plain-language intent into one narrow memory workflow, call the provider tool, present only grounded results, and give a startup/config triage path if tools are missing. Apply the same pattern here with EverOS-Hermes tools.

### Remember / save this

Use when the user says "remember this", "save this", "record this", or gives a durable preference/decision/lesson.

1. Extract the core durable content. Preserve user wording for preferences and corrections; compress noisy logs into a stable lesson.
2. Extract 2-5 searchable terms for your own verification query planning. EverOS plugin tools do not expose an agentmemory-style `concepts` field, so use these terms in `verification_query` / `verification_queries`, not as invented tool arguments.
3. Choose the target:
   - personal durable preference/fact -> `everos_memory_save_and_verify(scope="personal", role="user" or "assistant")`;
   - reusable future-agent case -> use the Agent Case Trajectory Recipe below or `everos_memory_save_and_verify(scope="agent")` when a single message is enough;
   - repeatable procedure -> patch/create a Hermes skill first, then optionally save a compact agent case pointing to the skill.
4. Prefer `everos_memory_save_and_verify` over raw `everos_memory_save` when future retrieval matters. Set `verification_query` to the most specific searchable phrase.
5. Report what was saved, the target scope, and whether verification / `agent_visibility` was `visible`, `partial`, `not_visible`, or `unchecked`.

Do not save secrets, raw transcripts, one-off task status, package hashes, PR numbers, or commit SHAs. If the content is a procedure, the durable home is a skill, not personal/profile memory.

### Recall / what did we do

Use when the user says "recall", "remember when", "what did we do", "last time", "continue", or asks for prior context.

1. Build a specific search query from the user's terms plus project/tool names. If the query is broad, try 2-3 alternatives rather than assuming no memory exists.
2. Use `session_search` for detailed old task transcripts and EverOS `everos_memory_search` for durable facts/cases. For structured agent cases, also use `everos_memory_get(memory_type="agent_case")` or `agent_skill` when appropriate.
3. Default EverOS search shape: `everos_memory_search(query=<query>, method="hybrid", top_k=5, response_format="markdown")`; use `memory_types` to narrow only when the user asks for profile/raw/agent content.
4. Present results using the Search Result Presentation Contract below. Do not make up memories, sessions, or decisions that the tool did not return.
5. If no results are found, state that clearly and suggest alternative search terms or the source-of-truth file/config lookup you will use next.

### Forget / delete memory

Use when the user says "forget this", "delete memory", "remove that", or requests privacy cleanup.

1. This is destructive. First search or get the candidate memories using `everos_memory_search` / `everos_memory_get`; do not delete from a vague phrase alone.
2. Show candidate `memory_id`, memory type/scope, session if available, and a short preview. Ask for explicit confirmation unless the user already provided an exact `memory_id` and unambiguous delete instruction in the current turn.
3. Delete exact ids with `everos_memory_forget(memory_id=<id>, confirm=true)`. Do not invent a session-delete capability for the standalone plugin tool; broad/user/session deletes require the lower-level MCP tools and an explicit broad-delete confirmation.
4. Verify with targeted search/get after deletion. Report residual raw/profile UI behavior honestly; a successful delete can still leave Cloud aggregate profile/UI remnants.
5. If cleanup is about noisy auto-capture rather than one id, follow the Cleanup / Compression Checklist below before deleting anything.

### Session history / recent memory timeline

Use when the user asks for "session history", "what happened recently", "recent memory timeline", or an overview of prior work.

1. Prefer `session_search()` with no query for Hermes transcript recency; use EverOS only for durable extracted memories, not as the complete session log.
2. For EverOS, use `everos_memory_get(memory_type="episodic_memory", page_size=20, rank_order="desc")`; for reusable agent lessons, separately check `everos_memory_get(memory_type="agent_case", page_size=20, rank_order="desc")`.
3. Present a reverse-chronological timeline with source labels: `session transcript`, `episodic_memory`, `agent_case`, or `agent_skill`.
4. Do not make up sessions. If EverOS only contains extracted fragments, say that it is not a full transcript and offer `session_search` for detail.

## Search Result Presentation Contract

When presenting recalled memory, use a grounded format:

- Group by source/scope: personal/profile/episodic, agent_case/agent_skill/agent_memory, raw_message, or session transcript.
- For each item, show title or short summary, memory type, session/source if available, and why it matched.
- Highlight high-confidence reusable items first: user preferences, project conventions, verified agent cases, and maintained skills.
- Label uncertain or raw-message results as lower confidence.
- Do not make up memories, observation ids, sessions, timestamps, or Cloud visibility states.
- If no results are returned, say so and suggest 2-3 alternative queries or source files to inspect next.
