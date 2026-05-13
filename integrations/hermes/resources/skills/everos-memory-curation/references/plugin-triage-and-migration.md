# EverOS Plugin Triage and Migration

Load this reference when EverOS tools are missing, the provider is inactive, plugin-bundled skills do not appear, or a migration/import/export task is involved.

## Tool Unavailable / Plugin Not Loaded Triage

If an EverOS tool is missing, returns "provider is not active", or the skill is loadable but plugin tools are absent, diagnose the plugin state before advising the user:

1. Confirm plugin installation: `$HERMES_HOME/plugins/everos` or a project plugin under the actual runtime cwd.
2. Confirm standalone plugin enablement: `plugins.enabled` contains `everos` and `plugins.disabled` does not.
3. Confirm automatic memory provider selection: `memory.provider: everos`.
4. Confirm secrets: `EVEROS_API_KEY` is in `$HERMES_HOME/.env`, profile `.env`, or process environment; do not print the key value.
5. Restart or start a fresh Hermes session after config/plugin/secret changes. Plugin discovery and memory-provider initialization are not retroactive inside an already-running conversation.
6. For Rust packages, verify `EVEROS_HERMES_RUST_BIN` is an absolute path to the binary; Hermes dotenv values are not shell-expanded.
7. Distinguish surfaces in the report:
   - standalone plugin enabled -> explicit tools and `everos:everos-memory-curation` skill;
   - `memory.provider: everos` -> automatic recall/capture hooks;
   - compatibility MCP server -> legacy/advanced stdio surface, not the default plugin path.

## Related deep references

These existing references remain available for specialized investigations:

- `references/user-profile-agent-memory-routing.md` — user/profile versus agent memory routing details.
- `references/everos-hermes-official-api-gap-audit.md` — official API coverage and known gaps.
- `references/everos-profile-compaction-limits.md` — profile compaction limitations and Cloud residual behavior.

Load those only after this triage reference indicates the task needs deeper API/profile analysis.
