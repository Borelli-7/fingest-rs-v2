---
name: fingest-api-engineering
description: "Use when implementing/changing FinGest API behavior to enforce repository best practices: hexagonal boundaries, contract safety, focused verification, and explicit error propagation."
---

# FinGest API engineering (repository-local)

**Scope:** `fingest-rs-v2` only; no parent-repo edits, no production operations.

## Procedure
1. Keep boundaries intact: core crates stay adapter-free (`crates/fingest-kernel/tests/dependency_rule.rs`).
2. Preserve wire compatibility unless intentionally changing contract:
   - treat `crates/fingest-contracts` as shared API surface;
   - validate impacted routes and payloads against `tests/postman_collection.json`.
3. Use existing error mapping and propagation patterns (`ApiError` + `From<...>`), no silent fallbacks.
4. Validate with the smallest command set that covers the changed surface, then broaden only if needed.
5. For cross-repo/shared-contract consumers, explicitly state consumer impact assumptions (v1 compatibility clients, Postman contract suite, load scripts).

## Required evidence format
- **Scope statement:** what changed and which bounded context(s).
- **Evidence citations (`file:line`):**
  - `crates/fingest-kernel/tests/dependency_rule.rs:10`, `:70`
  - `crates/fingest-contracts/src/lib.rs:1`, `:6`, `:12`
  - `crates/fingest-http/src/error.rs:8`, `:41`, `:66`
  - `README.md:194`, `README.md:201`
- **Severity + confidence:** engineering risk + confidence 1-10.
- **Focused tests:** command list + outcomes.
- **Concise handoff:** task id, revision, changed files, commands/results, blockers.

## Focused command set
- `cargo test -p fingest-kernel --test dependency_rule`
- `cargo test -p fingest-contracts`
- `cargo test -p fingest-http`
- `npx newman@6 run tests/postman_collection.json --env-var base_url=http://localhost:8080` (for changed HTTP contract behavior)
- `cargo clippy --workspace --all-targets -- -D warnings` (only when change spans multiple crates)
