# FinGest v2

Personal finance API — a re-architected rewrite of [finGest-rs](../finGest-rs) using
hexagonal (ports & adapters) architecture.

Same domain and the same wire contract, minus a set of defects that are enumerated in
[Deviations from v1](#deviations-from-v1).

## Why a rewrite

v1's `JwtAuth` middleware was defined but never attached to a route, so every `/resources/*`
endpoint was reachable without a token. Fixing that in place was possible; the rewrite exists
because several other defects shared a root cause — business rules that lived in handlers,
where nothing forced them to run.

## Architecture

```
bins/fingest-api            thin binary
crates/
  fingest-kernel            Money, Currency, DateRange, CategoryRef, Clock, DomainEvent   [pure]
  fingest-identity-core     Account, Password, Claims, auth + user use cases              [pure]
  fingest-catalog-core      Category                                                      [pure]
  fingest-wallets-core      Wallet, Expense, Summary, UnitOfWork port                     [pure]
  fingest-planning-core     Budget, Saving                                                [pure]
  fingest-*-pg              sqlx adapters
  fingest-auth-jwt          bcrypt + jsonwebtoken adapters
  fingest-events            publishers + transactional outbox relay
  fingest-plugins           compile-time plugin registry
  fingest-contracts         frozen HTTP wire types
  fingest-http              actix routes, extractors, error mapping
  fingest-bootstrap         composition root
```

The dependency rule — no `*-core` crate may reference `sqlx`, `actix-web` or `tokio`, not even
as a dev-dependency — is enforced by a test, not a convention:
`crates/fingest-kernel/tests/dependency_rule.rs`.

[docs/architecture.md](docs/architecture.md) covers the same ground as C4 context, container
and per-context component diagrams, with the patterns catalogue and the runtime views.

## Endpoints

26 routes. v1's README documented 16 and listed `/resources/users` where the code registered
`/resources/users/{login}`.

`self-or-admin` means the caller's token subject must match `{login}`, or the caller is an admin.

### Authentication
| Method | Path | Access |
|---|---|---|
| POST | `/api/auth/register` | public |
| POST | `/api/auth/login` | public |
| GET | `/api/auth/verify` | any valid token |

### Capabilities
| Method | Path | Access |
|---|---|---|
| GET | `/api/capabilities` | public |

Reports `available` (compiled in), `enabled` (selected by `PLUGINS`) and `capabilities`
(feature names the enabled plugins declare). Public because a client needs it before it has
a token. It exposes build configuration, never data.

### Categories
| Method | Path | Access |
|---|---|---|
| GET | `/resources/categories` | public |
| POST | `/resources/categories` | admin |
| PUT | `/resources/categories/{name}/{profit}` | admin |
| DELETE | `/resources/categories/{name}/{profit}` | admin |

### Users
| Method | Path | Access |
|---|---|---|
| GET | `/resources/users/{login}` | admin (lists all accounts) |
| PUT | `/resources/users/{login}?field=firstName\|lastName` | self-or-admin |
| DELETE | `/resources/users/{login}` | self-or-admin |

### Wallets
| Method | Path | Access |
|---|---|---|
| GET | `/resources/users/{login}/wallets` | self-or-admin |
| POST | `/resources/users/{login}/wallets` | self-or-admin |
| PUT | `/resources/users/{login}/wallets/{id}` | self-or-admin |
| DELETE | `/resources/users/{login}/wallets/{id}` | self-or-admin |
| GET | `/resources/users/{login}/wallets/{id}/summary` | self-or-admin |
| GET | `/resources/users/{login}/wallets/{id}/highest_expense` | self-or-admin |
| GET | `/resources/users/{login}/wallets/{id}/counted_categories` | self-or-admin |

### Expenses
| Method | Path | Access |
|---|---|---|
| GET | `/resources/users/{login}/wallets/{id}/expenses` | self-or-admin |
| POST | `/resources/users/{login}/wallets/{id}/expenses` | self-or-admin |
| PUT | `/resources/users/{login}/wallets/{id}/expenses/{expense_id}` | self-or-admin |
| DELETE | `/resources/users/{login}/wallets/{id}/expenses/{expense_id}` | self-or-admin |

### Budgets
| Method | Path | Access |
|---|---|---|
| GET | `/resources/users/{login}/budgets` | self-or-admin |
| POST | `/resources/users/{login}/budgets` | self-or-admin |
| PUT | `/resources/users/{login}/budgets/{budget_id}` | self-or-admin |
| DELETE | `/resources/users/{login}/budgets/{budget_id}` | self-or-admin |

`saving` is modelled and persisted but intentionally unrouted, matching v1's surface. A test
asserts no `/savings` path resolves.

## Deviations from v1

Everything not listed here is byte-identical to v1, including error body shape
(`{"status":"404","message":"..."}`, where `status` is a string) and field names.

| # | Endpoint | v1 | v2 |
|---|---|---|---|
| D1 | ~20 `/resources/*` routes | 200, no token needed | 401 without a Bearer token |
| D2 | `GET /resources/users/{login}` | always 401 (middleware dead) | 200 admin / 403 otherwise |
| D3 | `DELETE /users/{login}`, `GET .../budgets` | always 401 | work per policy |
| D4 | `GET .../budgets` | strictly self, admin denied | self **or** admin |
| D5 | `.../counted_categories` | `{}` | count of spending entries per category |
| D6 | `.../summary` | balance only, range ignored | per-category totals honouring the range |
| D7 | `POST .../budgets` | 201 with fabricated `id:1`, nothing written | 201, row persisted |
| D8 | `POST .../expenses`, currency ≠ wallet | 201, balance silently unchanged | 400 `CurrencyMismatch` |
| D9 | any `currency` field | any string accepted | 3 ASCII letters, uppercased; else 400 |
| D10 | 500 from a DB failure | raw driver message in the body | generic message; detail logged |
| D11 | duplicate / in-use category | 500 | 409 |
| D12 | `JWT_SECRET` under 32 chars | accepted | process refuses to start |
| D13 | non-money route, malformed JSON | `"The amount is invalid"` | `"Invalid request data"` |
| D14 | `GET /api/capabilities` | no such route | reports enabled plugins and declared capabilities |
| D16 | `PUT .../wallets/{id}` with a different `amount.currency` | accepted, existing entries orphaned | 400 `CurrencyMismatch`; amount changes are recorded as `WalletBalanceAdjusted` |

Two further fixes change no status code and so have no D-number:

- **Deleting an expense restores the wallet balance.** v1 removed the row and left the balance
  permanently wrong.
- **A login for an unknown account still performs a password hash comparison.** v1 returned
  early, so response latency revealed which logins existed.

## Getting started

Requires Rust 1.98.1 (pinned in `rust-toolchain.toml`) and PostgreSQL 15.

```bash
export POSTGRES_PASSWORD="$(openssl rand -base64 24)"
docker compose up -d db

cat > .env <<EOF
DATABASE_URL=postgres://postgres:${POSTGRES_PASSWORD}@localhost:5432/moneymanager
JWT_SECRET=$(openssl rand -base64 32)
RUST_LOG=info
EOF

cargo run -p fingest-api
```

Migrations run automatically at startup. The seeded development accounts are
`admin`/`password123`, `user1`/`user123`, `user2`/`user456` — v1 stored these as plaintext
while login used bcrypt, so **none of them could actually log in there**. The seed
migration here stores bcrypt hashes, which is why it diverges byte-for-byte from v1's
copy: pointing this build at a database already migrated by v1 will fail sqlx's
checksum validation, so use a fresh database.

### Configuration

| Variable | Default | Notes |
|---|---|---|
| `HOST` / `PORT` | `127.0.0.1` / `8080` | |
| `DATABASE_URL` | — | required |
| `DB_MAX_CONNECTIONS` | `10` | |
| `JWT_SECRET` | — | required, minimum 32 characters |
| `JWT_EXPIRATION_HOURS` | `24` | |
| `CORS_ALLOWED_ORIGIN` | `http://localhost:8081` | |
| `PLUGINS` | `tracing` | comma-separated; `tracing`, `in-process`. Empty disables publishing |
| `RUST_LOG` | `info` | |

## Testing

```bash
cargo test --workspace          # unit + integration; needs DATABASE_URL for the -pg crates
cargo clippy --workspace --all-targets -- -D warnings
cargo sqlx prepare --workspace --check -- --all-targets
```

Adapter tests use `#[sqlx::test]`, which provisions a throwaway database per test. Core-crate
tests need no database and no mocking framework — the port doubles are real in-memory
implementations, and the fake `UnitOfWork` buffers writes until commit so rollback is testable.

Contract and load suites:

```bash
npx newman@6 run tests/postman_collection.json --env-var base_url=http://localhost:8080
jmeter -n -t tests/fingest_performance_test.jmx -JPORT=8080 -l results.jtl
```

## Events

State changes append to an `outbox` table inside the same transaction. A background relay
polls every 5 seconds and hands batches to the configured publishers.

Delivery is **at-least-once** — rows are marked published only after a successful publish, so a
broker outage leaves them pending rather than dropping them. Subscribers must be idempotent.

The drain cycle, including its failure path, is diagrammed in
[docs/architecture.md](docs/architecture.md#one-outbox-drain-cycle).

## Docker

```bash
docker build -t fingest-rs-v2 .
docker compose up
```

The image is ~102 MB, runs as uid 10001, and contains no `.env` — v1 copied `sample.env` to
`/app/.env`, shipping a publicly known `JWT_SECRET` inside every image. `docker-compose.yml`
requires `POSTGRES_PASSWORD` and `JWT_SECRET` to be supplied.

## Documentation

| Document | Covers |
|---|---|
| [docs/architecture.md](docs/architecture.md) | C4 context, container and component views; architecture patterns and where each one is enforced |
| [docs/adr/](docs/adr/) | Decision records — why hexagonal, why the dependency rule is a test, why the outbox, why compile-time plugins |

## License

MIT — see [LICENSE](LICENSE).
