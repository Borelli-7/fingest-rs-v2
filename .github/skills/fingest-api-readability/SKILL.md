---
name: fingest-api-readability
description: "Use when refactoring or reviewing for maintainability/readability: explicit invariants, small handlers, clear ownership of errors, and concise handoffs for low-token coordination."
---

# FinGest API readability (repository-local)

**Scope:** readability/maintainability improvements in `fingest-rs-v2` without changing intended behavior.

## Procedure
1. Keep responsibilities clear:
   - handlers orchestrate and delegate to services;
   - contract DTOs stay in `fingest-contracts`;
   - composition/wiring remains in `fingest-bootstrap`.
2. Make invariants explicit in code, not comments alone:
   - use guard methods (`require_admin`, `require_self_or_admin`);
   - preserve centralized HTTP error shaping in `ApiError`.
3. Preserve explicit error propagation (`?`, typed `From` mappings), avoid broad catch-all fallbacks.
4. Prefer focused tests that pin intent (routing/auth/contract/dependency-rule), then include concise handoff notes for downstream coordinators.

## Required evidence format
- **Scope statement:** readability target and non-goals.
- **Evidence citations (`file:line`):**
  - `crates/fingest-http/src/resources.rs:9`, `:14`
  - `crates/fingest-http/src/extractor.rs:41`, `:51`
  - `crates/fingest-http/src/error.rs:8`, `:41`
  - `crates/fingest-contracts/src/lib.rs:1`, `:6`
  - `crates/fingest-kernel/tests/dependency_rule.rs:1`, `:70`
- **Severity + confidence:** maintainability risk + confidence 1-10.
- **Focused tests:** exact commands + outcomes.
- **Concise handoff:** task id, revision, changed files, commands/results, blockers.

## Focused command set
- `cargo test -p fingest-http`
- `cargo test -p fingest-contracts`
- `cargo test -p fingest-kernel --test dependency_rule`
- `cargo fmt --all --check` (when refactors touch multiple files)
