# 0003 — Transactional outbox for event publishing

## Status

Accepted.

## Context

State changes need to be observable outside the process — for logging today, for a broker
later. The naive approach is to publish from the use case after the write:

```
tx.commit().await?;
publisher.publish(&events).await?;
```

This has two failure modes that cannot both be avoided without a shared transaction:

- Publish after commit: the process dies between the two lines and the event is lost, while
  the state change is durable. Observers permanently disagree with the database.
- Publish before commit: the transaction rolls back and observers were told about a state
  change that never happened.

A distributed transaction across Postgres and a broker would solve it and is not available
here — nor desirable.

## Decision

Events are written to an `outbox` table inside the same transaction as the state change that
produced them. `WalletTx::append_events` is part of the transaction port, so a use case
physically cannot commit the write without the event, or the event without the write.

`migrations/20260906000000_outbox.sql`:

```sql
CREATE TABLE IF NOT EXISTS outbox (
    id BIGSERIAL PRIMARY KEY,
    aggregate TEXT NOT NULL,
    event_type TEXT NOT NULL,
    payload JSONB NOT NULL,
    occurred_at TIMESTAMPTZ NOT NULL,
    published_at TIMESTAMPTZ
);
CREATE INDEX IF NOT EXISTS idx_outbox_unpublished
    ON outbox (id) WHERE published_at IS NULL;
```

`OutboxRelay` (`crates/fingest-events/src/relay.rs`) polls every 5 seconds
(`OUTBOX_POLL_INTERVAL` in `fingest-bootstrap`), reads up to 100 unpublished rows, publishes
them, and only then stamps `published_at`. It is spawned detached: a publishing outage must
not stop the API from serving requests.

## Consequences

- Delivery is **at-least-once**. If the publish succeeds but `mark_published` fails, the
  batch is re-delivered on the next pass. **Subscribers must be idempotent.** This is the
  price of not losing events, and it is not negotiable without a distributed transaction.
- Ordering is by `id` within a batch, and batches are sequential, but nothing guarantees a
  subscriber processes them in order.
- Events are visible to observers up to one poll interval after they are durable. Any
  read-your-writes expectation must go through the API, not through events.
- The partial index means the relay's scan cost is proportional to the backlog, not to the
  table size, so retaining published rows is cheap. Nothing currently prunes them; a growing
  `outbox` table is a known and accepted operational cost.
- `FanOutPublisher` aborts a batch on the first publisher failure (see
  [0004](0004-compile-time-plugin-registry.md)), so a single failing subscriber holds up the
  rest rather than being marked delivered.
