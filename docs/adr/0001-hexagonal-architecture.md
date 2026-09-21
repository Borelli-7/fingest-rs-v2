# 0001 — Hexagonal architecture over layered handlers

## Status

Accepted.

## Context

v1 was a three-tier application: `api/handlers` → `services` → `db`. The layering was a
naming convention, not a constraint. Nothing prevented a handler from running SQL, and
nothing prevented a business rule from living inside a handler where it would only execute
if that particular route was reached.

That is the root cause behind most of the defects listed in [Deviations from v1](../../README.md#deviations-from-v1),
not a set of independent bugs:

- `JwtAuth` was written but never attached to a route, so ~20 `/resources/*` endpoints were
  open (D1). The authorization rule existed in a middleware nobody invoked.
- `POST .../expenses` accepted an expense whose currency differed from the wallet's and
  silently left the balance untouched (D8). The invariant "a wallet's balance is the sum of
  its entries, in the wallet's currency" was written nowhere that could enforce it.
- `POST .../budgets` returned `201` with a fabricated `id: 1` and wrote no row (D7).
- Deleting an expense removed the row and left the balance permanently wrong.

Each of these is a rule that had no home. Fixing them individually in v1 was possible; it
would have left the next rule in the same position.

## Decision

Structure the system as ports and adapters. The interior of the hexagon is `fingest-kernel`
plus one `*-core` crate per bounded context (identity, catalog, wallets, planning). It holds
the domain types, the use cases, and the trait definitions for everything it needs from the
outside world. It compiles with no knowledge that Postgres, actix or JWTs exist.

Adapters live outside and depend inwards: `*-pg` crates implement the repository and
transaction ports, `fingest-auth-jwt` implements hashing and token ports, `fingest-http`
drives the use cases from actix handlers. `fingest-bootstrap` is the only crate that names
both a port and its concrete adapter.

Bounded contexts are separate crates rather than modules, so the compiler rejects a
dependency between two contexts that was not declared in a manifest.

## Consequences

- A business rule has exactly one place to live, and it executes regardless of which route
  reaches it. `WalletService::add_expense` cannot be bypassed by adding a second handler.
- Core-crate tests need no database and no mocking framework — the port doubles are real
  in-memory implementations under `*-core/src/testing.rs`.
- The crate count went from 1 to 16. Adding a field to a domain type can mean touching a
  core crate, a pg adapter and a contract crate. This is the cost paid for the compiler
  checking the boundaries.
- The interior cannot be allowed to drift back; see [0002](0002-enforce-dependency-rule-with-a-test.md).
- Some mechanical duplication is now deliberate: `CategoryRef` exists in the kernel and
  `CategoryDto` in the contracts crate, with the mapping in `fingest-http`. Collapsing them
  would let the wire format follow the domain around.
