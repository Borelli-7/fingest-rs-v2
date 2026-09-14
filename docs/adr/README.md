# Architecture decision records

One file per decision that would be expensive to reverse. Each records the constraint that
forced the choice, so a later reader can tell whether the constraint still holds.

Format: Status / Context / Decision / Consequences. Superseding a record means adding a new
one and marking the old one `Superseded by NNNN`, not editing it.

| # | Title | Status |
|---|---|---|
| [0001](0001-hexagonal-architecture.md) | Hexagonal architecture over layered handlers | Accepted |
| [0002](0002-enforce-dependency-rule-with-a-test.md) | Enforce the dependency rule with a test | Accepted |
| [0003](0003-transactional-outbox.md) | Transactional outbox for event publishing | Accepted |
| [0004](0004-compile-time-plugin-registry.md) | Compile-time plugin registry, no dynamic loading | Accepted |
| [0005](0005-frozen-wire-contracts.md) | Freeze the v1 wire contract in its own crate | Accepted |
| [0006](0006-unit-of-work-and-read-write-split.md) | Unit of Work for writes, separate reader for queries | Accepted |

See [../architecture.md](../architecture.md) for the C4 views these decisions produced.
