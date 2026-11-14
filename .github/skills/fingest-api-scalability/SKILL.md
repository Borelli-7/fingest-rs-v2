---
name: fingest-api-scalability
description: "Use when changes affect sustained load behavior: DB pool sizing, SQL plan/index coverage, outbox throughput, bounded queues/maps, and backpressure/retry semantics."
---

# FinGest API scalability (repository-local)

**Scope:** `fingest-rs-v2` runtime scalability, especially Postgres adapters and background event delivery.

## Procedure
1. Confirm bounded resources and backpressure:
   - DB pool cap is explicit (`DB_MAX_CONNECTIONS`);
   - in-memory abuse protections have maximum tracked keys and fail closed when full.
2. Confirm SQL scalability path:
   - run/extend existing plan/index tests instead of guessing.
3. Confirm outbox multi-replica behavior:
   - claim mechanism uses `FOR UPDATE SKIP LOCKED`;
   - lease/release/retry semantics remain at-least-once;
   - pass caps prevent infinite single-tick drain loops.
4. Evaluate retry/idempotency expectations for consumers before increasing concurrency/batch settings.

## Required evidence format
- **Scope statement:** target bottleneck path and expected scaling axis (QPS, backlog, tenants, rows).
- **Evidence citations (`file:line`):**
  - `crates/fingest-bootstrap/src/config.rs:11`, `:72`
  - `crates/fingest-bootstrap/src/lib.rs:142`, `:143`
  - `crates/fingest-events/src/outbox.rs:52`, `:56`, `:95`, `:105`
  - `crates/fingest-events/src/relay.rs:11`, `:14`, `:20`, `:84`, `:117`
  - `crates/fingest-http/src/rate_limit.rs:20`, `:47`, `:51`
- **Severity + confidence:** scalability risk + confidence 1-10.
- **Focused tests:** command list + outcomes.
- **Concise handoff:** task id, revision, changed files, commands/results, blockers.

## Focused command set
- `cargo test -p fingest-bootstrap --test schema_indexes`
- `cargo test -p fingest-events`
- `cargo test -p fingest-http rate_limit::tests`
- `cargo test --workspace` (only when scaling changes cross bounded contexts)
