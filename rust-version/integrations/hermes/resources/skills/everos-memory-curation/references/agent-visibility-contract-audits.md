# Agent Visibility Contract Audits

Use this reference when the task asks for a real-task audit of EverOS-Hermes `agent_visibility`, especially Python/Rust parity, installed plugin resource sync, or runtime import-path checks around identity propagation.

## What to verify

For `scope="agent"` verification reports, confirm the implementation and tests distinguish queue/flush acceptance from structured visibility and preserve the controller identity:

- top-level `agent_visibility.verification_user_id` equals the `user_id` used for verification;
- top-level `agent_visibility.verification_session_id` equals the `session_id` used for verification when one is supplied;
- every entry in `agent_visibility.agent_visibility_checks` carries the same `user_id` and `session_id`;
- checks cover agent-memory search plus direct `agent_case` and `agent_skill` get calls;
- workflow status maps `visible` -> `verified`, `partial` -> `partially_verified`, `not_visible` -> `agent_not_visible`, and `error` -> `agent_visibility_error`.

## Audit pattern

1. Inspect implementation sources before running tests:
   - Python: `src/everos_hermes/agent_visibility.py` and `src/everos_hermes/workflows.py`.
   - Rust: `rust-version/src/agent_visibility.rs` and `rust-version/src/workflows.rs`.
2. When auditing an installed Hermes plugin after an identity fix, verify the active runtime surfaces, not just repo source:
   - compare `integrations/hermes` to `$HERMES_HOME/plugins/everos` excluding `__pycache__` and report extra/missing/changed files plus hashes for `plugin.yaml`, `__init__.py`, `SKILL.md`, `references/agent-case-visibility.md`, and `references/memory-routing-policy.md`;
   - confirm `plugins.enabled` contains `everos`, `plugins.disabled` does not, `memory.provider` is `everos`, and required env names are present without printing secret values;
   - import `everos_hermes.agent_visibility` in the active Python environment and record `sys.executable`, `module_file`, package file, distribution version, and `direct_url.json` when available;
   - load the installed plugin entrypoint by absolute path and confirm it resolves `EverOSMemoryProvider`, `_skill_path()`, and the bundled skill exists.
3. Inspect the contract tests that assert identity fields:
   - Python workflow tests around `save_and_verify` and `verify_session_ingest` for agent scope.
   - Rust parity test for `mcp_save_and_verify_agent_scope_reports_structured_visibility`.
4. Run targeted tests only unless the user requested a full suite:
   - Python: the workflow tests asserting agent visibility identity propagation, plus `tests/test_agent_visibility.py` for helper-level behavior.
   - Rust: the single parity test asserting the same contract when Rust runtime parity is in scope.
5. Write a concise evidence file when requested or when the task is an audit:
   - inspected commit or source state;
   - installed plugin sync/config/import-path state when relevant;
   - implementation/test files and relevant line ranges;
   - exact commands and PASS/FAIL summary;
   - repository edit status;
   - memory-curation decision, explicitly saying whether an EverOS memory write tool was called.

## Curation rule

A clean audit that only confirms existing committed behavior should usually remain in the evidence file and final response, not durable memory. Save or patch a durable skill/case only if the audit reveals a new reusable workflow, stable API/tool quirk, or future-agent decision pattern.
