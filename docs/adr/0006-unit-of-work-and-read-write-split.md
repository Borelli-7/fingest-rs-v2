# 0006 — Unit of Work for writes, separate reader for queries

## Status

Accepted.

## Context

v1 wrote an expense and updated the wallet balance as two independent statements:

```
INSERT INTO expense ...
UPDATE wallet SET amount = amount - ...
```

A failure between them left the balance permanently wrong with no way to detect it. The
same shape caused the expense-deletion defect, where the row was removed and the balance was
simply never restored.

A single repository trait per aggregate does not fix this. `ExpenseRepository::insert` and
`WalletRepository::adjust_balance` are two calls, and nothing in either signature says they
must share a transaction. Whatever the naming, the caller is left holding the invariant.

The obvious correction — put every method on a transactional trait — has its own cost. A
read like "list this wallet's expenses" would then open a transaction, hold a pooled
connection for the duration of the request, and contribute to pool exhaustion for no
consistency benefit.

## Decision

Split the wallets port in two.

**`UnitOfWork` / `WalletTx`** for writes. `UnitOfWork::begin` returns a `Box<dyn WalletTx>`;
every mutating method lives on `WalletTx`, including `append_events` (see
[0003](0003-transactional-outbox.md)). `commit` takes `self: Box<Self>`, so committing
consumes the transaction and a dropped `WalletTx` rolls back. A use case that forgets to
commit loses the write rather than half-applying it.

`adjust_balance` takes a signed `&Money` delta. The aggregate computes it; no caller invents
one.

**`WalletReader`** for queries. Read-only, no transaction, each method a single statement.
`find_owned` returns `None` when the wallet is missing *or* not owned, so a caller cannot
accidentally leak existence; `wallet_exists` is the separate, deliberate call used to choose
between 403 and 404.

## Consequences

- The wallet balance invariant is enforced by the transaction boundary rather than by
  reviewer attention. `add_expense`, `update_expense` and `delete_expense` each insert or
  remove the row, adjust the balance, and append the event in one transaction.
- Reads never hold a transaction open, so a slow query consumes one connection and does not
  block writers.
- The split is not free: `WalletTx::find_owned` duplicates `WalletReader::find_owned`
  because a write path must re-read inside its own transaction. The signatures are identical
  and the implementations differ only in what they execute against.
- Two traits mean two test doubles. `fingest-wallets-core/src/testing.rs` provides a fake
  `UnitOfWork` that buffers writes until `commit`, which is what makes rollback testable
  without a database.
- The split is currently only in the wallets context, because it is the only one with a
  multi-statement invariant. `fingest-planning-pg` keeps a single repository trait and
  builds its `BudgetCreated` event inside the adapter, since the budget id does not exist
  until the row is written. That inconsistency is deliberate and documented at
  `BudgetRepository::insert`.
