# 0002 — Enforce the dependency rule with a test

## Status

Accepted.

## Context

[0001](0001-hexagonal-architecture.md) only pays off while the interior of the hexagon stays
pure. The failure mode is undramatic: someone needs a timestamp in a core crate, reaches for
`tokio::time`, adds one line to a `Cargo.toml`, and the core crate now transitively depends
on a runtime. Nothing breaks that day. Months later the "pure" crate cannot be tested
without an async runtime and the boundary is decorative.

Three mechanisms were available:

1. **Convention** — write it in the README. This is what v1 did with its layering, and the
   layering did not hold.
2. **A lint or an external tool** — `cargo-deny` can ban crates, but the ban would be
   workspace-wide or need per-crate configuration maintained alongside the manifests.
3. **A test.**

## Decision

`crates/fingest-kernel/tests/dependency_rule.rs` reads the `Cargo.toml` of every crate whose
directory name is `fingest-kernel` or ends in `-core`, strips `#` comments, and fails if any
declared dependency key appears in `FORBIDDEN`:

```
sqlx, actix-web, actix-cors, tokio, jsonwebtoken, bcrypt, reqwest
```

Two supporting tests guard the test itself: `kernel_is_among_the_checked_crates` fails if the
directory filter stops matching anything, and `comment_mentions_do_not_trigger_a_violation`
pins the comment-stripping behaviour so a `# sqlx is banned here` note cannot fail the build.

The root `Cargo.toml` splits `[workspace.dependencies]` into a kernel-safe group and an
adapter-only group with the rule restated in a comment, so the constraint is visible at the
point of temptation as well as at the point of enforcement.

Only *declared* dependencies are scanned. A transitive leak into a pure crate is only
reachable through another pure crate, which the same test covers.

## Consequences

- The rule fails in CI, in the same `cargo test --workspace` run as everything else, with a
  message naming the manifest and the offending crate.
- No compilation of the checked crates is needed — the test reads text files, so it is fast
  and cannot be defeated by a feature flag.
- The check is name-based. Adding a new adapter technology (a Redis client, an HTTP client
  other than `reqwest`) requires adding it to `FORBIDDEN`; until someone does, a core crate
  could take that dependency unnoticed.
- Renaming a bounded-context crate away from the `-core` suffix would silently remove it
  from the check. The suffix is therefore load-bearing, not cosmetic.
