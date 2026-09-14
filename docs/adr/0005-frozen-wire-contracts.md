# 0005 — Freeze the v1 wire contract in its own crate

## Status

Accepted.

## Context

v2 is a rewrite of a running API, not a new API. Existing clients must not have to change.
That constraint is in tension with the rewrite itself: v1's wire format has several shapes
nobody would choose today.

- The error body is `{"status":"404","message":"..."}` — `status` is a **string**.
- `UserDto` uses `firstName` / `lastName` while every other field in the system is
  snake_case.
- A budget is `dateRange` in a request body and `date_range` in a response body.

If the DTOs lived next to the domain types, a rename during ordinary refactoring would
change the wire format silently, and the break would surface in a client rather than in CI.

## Decision

`fingest-contracts` holds every type that appears in a request or response body, and
nothing else. It depends only on `fingest-kernel` (for `Money`, `DateRange`, `CategoryRef`)
and is depended on only by `fingest-http`. It contains no mapping logic — converting a
domain type to a DTO is done in `fingest-http`, because the contracts crate must not depend
on a bounded context or the wire format would follow the domain around.

Field names are pinned by tests in the crate, so renaming one fails the build rather than a
client. The v1 asymmetries are reproduced deliberately and commented as such, for example
`ErrorResponse::new(status: u16, ...)` which takes a number and stores
`status.to_string()`.

The deliberate departures from v1's wire behaviour are enumerated in
[Deviations from v1](../../README.md#deviations-from-v1). Everything not listed there is
byte-identical.

## Consequences

- The set of things clients can observe is a single crate that can be read in one sitting.
- A domain-level rename cannot leak to the wire, and a wire-level rename cannot happen by
  accident.
- The oddities are permanent until a v3 negotiates them away. New fields should follow the
  surrounding shape, not the shape that is correct in isolation — consistency with the
  neighbouring fields is what clients parse against.
- Two representations of the same idea exist (`CategoryRef` and `CategoryDto`) with an
  explicit mapping between them. That duplication is the point, not an oversight.
