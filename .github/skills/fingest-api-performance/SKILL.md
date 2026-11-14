---
name: fingest-api-performance
description: Use when an API change may affect latency/throughput/allocations (handlers, SQL, outbox relay, rate limiting). Produces proportional measurement evidence instead of benchmark theater.
---

# FinGest API performance (repository-local)

**Scope:** `fingest-rs-v2` only. Focus on `crates/fingest-http`, `crates/fingest-*-pg`, `crates/fingest-events`, `crates/fingest-bootstrap`, and `tests/fingest_performance_test.jmx`.

## Procedure
1. Define the hot path and expected impact before measuring (for example: `GET /resources/users/{login}/wallets/{id}/summary` or outbox drain throughput).
2. Measure proportionally:
   - **Unit/integration path:** run only the affected crate/tests first (`cargo test -p <crate>`).
   - **DB query shape/indexability:** use existing schema-plan tests in `crates/fingest-bootstrap/tests/schema_indexes.rs`.
   - **API-level load only when justified:** run `jmeter -n -t tests/fingest_performance_test.jmx -JPORT=8080 -l results.jtl`.
3. Inspect Rust allocation/ownership and async behavior:
   - Avoid avoidable clones and repeated serialization in hot loops.
   - Keep blocking work off async request paths; use `tokio::task::spawn_blocking` (or a bounded dedicated worker pool) for blocking I/O/CPU sections.
   - Use `tokio::spawn` for async background tasks only, with explicit lifecycle/cancellation expectations.
4. For outbox paths, verify bounded behavior (`batch_size`, `max_batches_per_pass`, retry on failure) before proposing tuning.

## Required evidence format
- **Scope statement:** endpoint/crate/path under analysis.
- **Evidence citations (`file:line`):** include concrete anchors such as:
  - `crates/fingest-events/src/relay.rs:11`, `:14`, `:30`, `:88`, `:117`
  - `crates/fingest-bootstrap/tests/schema_indexes.rs:49`, `:65`, `:79`
  - `crates/fingest-bootstrap/src/lib.rs:33`, `:142`, `:158`
- **Severity + confidence:** `impact: low|medium|high`, `confidence: 1-10`.
- **Focused tests:** exact commands run and observed result.
- **Concise handoff:** task id, revision, changed files, commands/results, blockers.

## Focused command set
- `cargo test -p fingest-bootstrap --test schema_indexes`
- `cargo test -p fingest-events`
- `cargo test -p fingest-http`
- `jmeter -n -t tests/fingest_performance_test.jmx -JPORT=8080 -l results.jtl` (only when endpoint-level load evidence is needed)
