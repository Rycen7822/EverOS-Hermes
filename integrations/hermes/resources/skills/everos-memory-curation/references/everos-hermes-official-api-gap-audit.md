# EverOS-Hermes Official API Scope Audit

Use this reference when auditing EverOS-Hermes against the official EverOS / Evermind v1 API docs. The goal is scope clarity, not feature expansion.

## Source of truth

Re-read the checked-in official API reference under `evermind_api_reference/` and the current implementation files before reporting endpoint coverage. Do not infer coverage from memory or older audit notes.

## Current product scope

EverOS-Hermes is a Hermes memory provider/plugin plus MCP compatibility surface, not a full EverOS v1 SDK. The maintained surfaces are:

- Personal memory add, flush, search, get, and delete.
- Agent memory add, flush, search/get visibility workflows, and agent trajectory capture.
- Task status and memory-space settings get/update.
- Hermes context assembly, recall/write policy controls, and verification workflows.

Group, sender, object-storage, multimodal, full filter DSL, and broad delete semantics remain product out-of-scope unless the contract docs change first. Do not add those surfaces while doing memory-provider cleanup or complexity reduction.

## Verification commands

```bash
python -m pytest tests/test_schemas.py tests/test_schema_snapshots.py -q
(cd rust-version && cargo test --test parity -- --test-threads=1)
```

## Audit workflow

1. Parse `evermind_api_reference/INDEX.md` and endpoint detail files.
2. Compare current Python and Rust client, MCP, provider, schema, and workflow code.
3. Classify each gap as implemented, exposed, intentionally out-of-scope, or product-scope change required.
4. Report scope decisions separately from unfinished work.

## Pitfall

Do not promise profile edit/reset as a missing plugin feature: the official reference used here does not expose a profile patch/reset endpoint. Long `Basic Information` / `Personality & Traits` remains a Cloud aggregate limitation unless new official APIs appear.
