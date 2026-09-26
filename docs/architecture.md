# Architecture

How FinGest v2 is put together, described with the [C4 model](https://c4model.com): system
context, containers, then components per bounded context. Each diagram names real crates,
traits and files — if a name here does not exist in the source, the document is wrong.

The decisions behind the structure are recorded separately in [adr/](adr/).

## Contents

- [Scope](#scope)
- [Level 1 — System context](#level-1--system-context)
- [Level 2 — Containers](#level-2--containers)
- [Level 3 — Components](#level-3--components)
  - [Identity](#identity)
  - [Catalog](#catalog)
  - [Wallets](#wallets)
  - [Planning](#planning)
  - [Events](#events)
- [Runtime views](#runtime-views)
- [Architecture patterns](#architecture-patterns)
- [Where the architecture is enforced](#where-the-architecture-is-enforced)
- [Startup and wiring](#startup-and-wiring)
- [Deliberate deviations](#deliberate-deviations)

## Scope

FinGest v2 is a single-process HTTP API over one PostgreSQL database. It is a re-architecture
of [finGest-rs](../../finGest-rs), preserving the wire contract while relocating the business
rules out of the handlers. v1's documentation describes a layered system; this one is
hexagonal, so the two documents are not comparable view-for-view.

Everything below describes the code as committed. Where the implementation deviates from what
the architecture would suggest, it is listed in [Deliberate deviations](#deliberate-deviations)
rather than smoothed over.

## Level 1 — System context

```mermaid
C4Context
    title System context — FinGest v2

    Person(client, "API client", "Web or mobile front end acting for an account holder or an admin")

    System(fingest, "FinGest v2 API", "Wallets, expenses, budgets and categories. Issues and verifies its own JWTs")

    SystemDb_Ext(postgres, "PostgreSQL 15", "Accounts, wallets, expenses, budgets, savings, and the event outbox")
    System_Ext(subscribers, "Event subscribers", "Receive domain events via the configured publisher plugins")

    Rel(client, fingest, "Calls", "HTTPS / JSON, Bearer JWT")
    Rel(fingest, postgres, "Reads and writes", "sqlx over TCP")
    Rel(fingest, subscribers, "Publishes domain events", "at-least-once, via outbox relay")
```

Authentication is self-contained: the API issues its own tokens with `JwtTokens`
(`crates/fingest-auth-jwt`) and verifies them with the same adapter. There is no external
identity provider — the sibling Java project in this repository uses Keycloak, this one does
not.

## Level 2 — Containers

One deployable process, `bins/fingest-api`. The boxes inside it are crates, drawn as
containers because the crate boundary is where the dependency rule is enforced
([ADR-0002](adr/0002-enforce-dependency-rule-with-a-test.md)).

Every arrow crossing the hexagon boundary points **inwards**. There is no arrow from a
`*-core` crate to an adapter, and that absence is the architecture.

```mermaid
C4Container
    title Containers — the fingest-api process

    Person(client, "API client", "Bearer JWT")

    Container_Boundary(proc, "fingest-api process") {
        Container(http, "fingest-http", "Rust · actix-web", "Routes, JwtAuth middleware, AuthenticatedUser extractor, the single ResponseError impl")
        Container(contracts, "fingest-contracts", "Rust · serde", "Frozen v1 wire types. Field names pinned by tests")
        Container(bootstrap, "fingest-bootstrap", "Rust", "Composition root. The only crate naming both a port and its adapter")

        Container(cores, "Bounded context cores", "Rust · pure", "fingest-identity-core, fingest-catalog-core, fingest-wallets-core, fingest-planning-core — use cases and port traits")
        Container(kernel, "fingest-kernel", "Rust · pure", "Money, Currency, DateRange, CategoryRef, Clock, DomainEvent, PortError")

        Container(pg, "Postgres adapters", "Rust · sqlx", "fingest-identity-pg, fingest-catalog-pg, fingest-wallets-pg, fingest-planning-pg")
        Container(authjwt, "fingest-auth-jwt", "Rust · bcrypt, jsonwebtoken", "BcryptHasher, JwtTokens")
        Container(events, "fingest-events", "Rust · sqlx, tokio", "PgOutboxReader, OutboxRelay, publishers")
        Container(plugins, "fingest-plugins", "Rust", "Plugin, Registry, PluginHost, FanOutPublisher")
    }

    ContainerDb_Ext(db, "PostgreSQL 15", "Relational", "Domain tables plus the outbox table")
    System_Ext(subscribers, "Event subscribers", "Whatever the enabled plugins reach")

    Rel(client, http, "HTTPS / JSON")
    Rel(http, contracts, "Serialises through")
    Rel(http, cores, "Invokes use cases")
    Rel(cores, kernel, "Uses value objects and ports")

    Rel(pg, cores, "Implements repository and transaction ports")
    Rel(authjwt, cores, "Implements hashing and token ports")
    Rel(events, kernel, "Implements EventPublisher")
    Rel(plugins, kernel, "Fans out to EventPublisher")

    Rel(bootstrap, http, "Registers routes and app_data")
    Rel(bootstrap, pg, "Constructs")
    Rel(bootstrap, authjwt, "Constructs")
    Rel(bootstrap, plugins, "Selects by name from PLUGINS")
    Rel(bootstrap, events, "Spawns the relay")

    Rel(pg, db, "sqlx")
    Rel(events, db, "Polls the outbox")
    Rel(events, subscribers, "Publishes")

    UpdateLayoutConfig($c4ShapeInRow="3", $c4BoundaryInRow="1")
```

`fingest-plugins` and `fingest-events` depend on `fingest-kernel` only. Neither knows what a
wallet is; they move `EventEnvelope` values.

## Level 3 — Components

Four bounded contexts, one diagram each. The shape repeats: an actix handler calls an
application service, the service calls a port trait, an adapter implements the trait. The
dashed boundary in each diagram is the edge of the hexagon — everything to its left compiles
without `sqlx`.

### Identity

Accounts, registration, login, token issuing and verification.
`crates/fingest-identity-core/src/{port,service}.rs`.

```mermaid
C4Component
    title Components — Identity

    Container_Boundary(core, "fingest-identity-core · pure") {
        Component(authsvc, "AuthService", "Use case", "register, login, verify_token")
        Component(usersvc, "UserService", "Use case", "list, update_name, delete")
        Component(accrepo, "AccountRepository", "Outbound port", "find, exists, insert, list, update_name, delete")
        Component(hasher, "PasswordHasher", "Outbound port", "hash, verify, verify_dummy")
        Component(issuer, "TokenIssuer / TokenVerifier", "Outbound port", "issue, verify")
    }

    Container_Boundary(adapters, "Adapters") {
        Component(httpauth, "auth.rs / users.rs", "actix handlers", "POST /api/auth/*, /resources/users/{login}")
        Component(pgacc, "PgAccountRepository", "sqlx", "fingest-identity-pg")
        Component(jwt, "BcryptHasher · JwtTokens", "bcrypt · jsonwebtoken", "fingest-auth-jwt, HS256")
    }

    ComponentDb_Ext(db, "account", "table", "login, first_name, last_name, password, admin")

    Rel(httpauth, authsvc, "calls")
    Rel(httpauth, usersvc, "calls")
    Rel(authsvc, accrepo, "uses")
    Rel(authsvc, hasher, "uses")
    Rel(authsvc, issuer, "uses")
    Rel(usersvc, accrepo, "uses")
    Rel(pgacc, accrepo, "implements")
    Rel(jwt, hasher, "implements")
    Rel(jwt, issuer, "implements")
    Rel(pgacc, db, "SELECT / INSERT / UPDATE / DELETE")

    UpdateLayoutConfig($c4ShapeInRow="3", $c4BoundaryInRow="1")
```

`PasswordHasher::verify_dummy` is a port method, not an implementation detail. `login` calls
it when no account matches, so a login for a non-existent account costs the same as one with
a wrong password. Putting it on the port is what makes the timing property testable without
bcrypt.

### Catalog

Categories: shared reference data, identified by the composite `(name, profit)`.
`crates/fingest-catalog-core/src/{port,service}.rs`.

```mermaid
C4Component
    title Components — Catalog

    Container_Boundary(core, "fingest-catalog-core · pure") {
        Component(catsvc, "CategoryService", "Use case", "list, create, rename, delete")
        Component(catrepo, "CategoryRepository", "Outbound port", "list, find, find_conflict, insert, rename, delete")
    }

    Container_Boundary(adapters, "Adapters") {
        Component(httpcat, "catalog.rs", "actix handlers", "/resources/categories — GET public, writes admin-only")
        Component(pgcat, "PgCategoryRepository", "sqlx", "fingest-catalog-pg")
    }

    ComponentDb_Ext(db, "category", "table", "composite key (name, profit)")

    Rel(httpcat, catsvc, "calls")
    Rel(catsvc, catrepo, "uses")
    Rel(pgcat, catrepo, "implements")
    Rel(pgcat, db, "SQL")

    UpdateLayoutConfig($c4ShapeInRow="2", $c4BoundaryInRow="1")
```

`find_conflict` exists separately from `find` because duplicate detection is
case-insensitive while lookup is exact, and a rename must be able to exclude the row being
renamed from its own conflict check. A conflict is `CatalogError::Conflict`, which maps to
409 — v1 returned 500 here (D11).

### Wallets

The only context with a multi-statement invariant, and the reason
[ADR-0006](adr/0006-unit-of-work-and-read-write-split.md) exists.
`crates/fingest-wallets-core/src/{port,service}.rs`.

```mermaid
C4Component
    title Components — Wallets

    Container_Boundary(core, "fingest-wallets-core · pure") {
        Component(wsvc, "WalletService", "Use case", "wallets, expenses, summary, highest_expense, counted_categories")
        Component(reader, "WalletReader", "Outbound port · read", "owner_exists, list_for_owner, find_owned, wallet_exists, list_expenses, highest_expense, find_expense")
        Component(uow, "UnitOfWork / WalletTx", "Outbound port · write", "begin, insert/update/delete, adjust_balance, append_events, commit")
    }

    Container_Boundary(adapters, "Adapters") {
        Component(httpwal, "wallets.rs", "actix handlers", "11 routes under /resources/users/{login}/wallets")
        Component(pgread, "PgWalletReader", "sqlx", "fingest-wallets-pg")
        Component(pguow, "PgUnitOfWork → PgWalletTx", "sqlx transaction", "fingest-wallets-pg")
    }

    ComponentDb_Ext(db, "wallet · expense · outbox", "tables", "balance and entries, plus the event rows")

    Rel(httpwal, wsvc, "calls")
    Rel(wsvc, reader, "queries")
    Rel(wsvc, uow, "mutates within one transaction")
    Rel(pgread, reader, "implements")
    Rel(pguow, uow, "implements")
    Rel(pgread, db, "SELECT")
    Rel(pguow, db, "BEGIN … COMMIT")

    UpdateLayoutConfig($c4ShapeInRow="2", $c4BoundaryInRow="1")
```

### Planning

Budgets and savings. `crates/fingest-planning-core/src/{port,service}.rs`.

```mermaid
C4Component
    title Components — Planning

    Container_Boundary(core, "fingest-planning-core · pure") {
        Component(bsvc, "BudgetService", "Use case", "list, create, update, delete")
        Component(brepo, "BudgetRepository", "Outbound port", "owner_exists, category_exists, list_with_spent, find_with_owner, insert, update, delete")
        Component(srepo, "SavingRepository", "Outbound port", "list_for_owner, insert, delete — no HTTP surface")
    }

    Container_Boundary(adapters, "Adapters") {
        Component(httpbud, "budgets.rs", "actix handlers", "4 routes under /resources/users/{login}/budgets")
        Component(pgbud, "PgBudgetRepository", "sqlx", "fingest-planning-pg")
    }

    ComponentDb_Ext(db, "budget · saving · outbox", "tables", "")

    Rel(httpbud, bsvc, "calls")
    Rel(bsvc, brepo, "uses")
    Rel(pgbud, brepo, "implements")
    Rel(pgbud, srepo, "implements")
    Rel(pgbud, db, "SQL, budget row and outbox row in one transaction")

    UpdateLayoutConfig($c4ShapeInRow="2", $c4BoundaryInRow="1")
```

`list_with_spent` filters on two independent windows — one bounding `start_date`, one
bounding `end_date` — and sums the owner's spending in the same query, reproducing v1's
behaviour exactly.

### Events

Cross-cutting. No bounded context depends on this; it depends on the kernel only.

```mermaid
C4Component
    title Components — Events

    Container_Boundary(write, "Write path · inside the caller's transaction") {
        Component(tx, "WalletTx::append_events / PgBudgetRepository::insert", "port method", "Appends EventEnvelope rows")
    }

    ComponentDb_Ext(outbox, "outbox", "table", "id, aggregate, event_type, payload JSONB, occurred_at, published_at")

    Container_Boundary(relaybox, "fingest-events") {
        Component(oreader, "PgOutboxReader", "sqlx", "fetch_unpublished, mark_published")
        Component(relay, "OutboxRelay", "tokio task", "drain_once every 5s, batch of 100")
        Component(pubs, "TracingPublisher · InProcessPublisher", "EventPublisher", "")
    }

    Container_Boundary(pluginbox, "fingest-plugins") {
        Component(fan, "FanOutPublisher", "EventPublisher", "Built by Registry::into_publisher")
    }

    System_Ext(subs, "Subscribers", "Must be idempotent")

    Rel(tx, outbox, "INSERT, same transaction as the state change")
    Rel(relay, oreader, "fetch_unpublished, then mark_published only after publish succeeds")
    Rel(oreader, outbox, "SELECT … WHERE published_at IS NULL")
    Rel(relay, fan, "publish(&envelopes)")
    Rel(fan, pubs, "fans out to every enabled plugin")
    Rel(pubs, subs, "delivers")

    UpdateLayoutConfig($c4ShapeInRow="3", $c4BoundaryInRow="2")
```

## Runtime views

### An authenticated request

`JwtAuth` wraps the whole `/resources/users` scope, so verification happens once per request
and `AuthenticatedUser` reuses the claims the middleware deposited rather than verifying a
second time.

```mermaid
sequenceDiagram
    autonumber
    participant C as Client
    participant M as JwtAuth middleware
    participant E as AuthenticatedUser
    participant H as Handler
    participant S as Application service
    participant P as Port adapter

    C->>M: GET /resources/users/bob/wallets<br/>Authorization: Bearer …
    alt header missing or token invalid
        M-->>C: 401 {"status":"401","message":"…"}
    else verified
        M->>M: TokenVerifier::verify → Claims<br/>stored in request extensions
        M->>H: forward
        H->>E: extract
        E->>E: require_self_or_admin("bob")
        alt caller is neither bob nor admin
            E-->>C: 403
        else allowed
            H->>S: WalletService::list_wallets("bob")
            S->>P: WalletReader::list_for_owner
            P-->>S: wallets
            S-->>H: wallets
            H-->>C: 200 [WalletDto]
        end
    end
```

### Recording an expense

One transaction covers the row, the balance and the event. This is the path v1 got wrong in
three separate ways.

```mermaid
sequenceDiagram
    autonumber
    participant H as wallets.rs handler
    participant S as WalletService
    participant T as WalletTx
    participant DB as PostgreSQL

    H->>S: add_expense(login, wallet_id, input)
    S->>T: UnitOfWork::begin
    T->>DB: BEGIN
    S->>T: find_owned(login, wallet_id)
    alt not owned or missing
        S-->>H: NotFound / Forbidden
        Note over T,DB: WalletTx dropped without commit → ROLLBACK
    else owned
        S->>T: category_exists(category)
        alt unknown category
            S-->>H: BadRequest
        else known
            S->>S: expense currency vs wallet currency
            alt mismatch
                S-->>H: BadRequest CurrencyMismatch (D8)
                Note over T,DB: dropped → ROLLBACK
            else match
                S->>T: insert_expense → expense_id
                S->>T: adjust_balance(wallet_id, signed delta)
                S->>T: append_events([ExpenseRecorded, WalletBalanceAdjusted])
                S->>T: commit
                T->>DB: COMMIT
                S-->>H: expense_id
                H-->>H: 201 + Location
            end
        end
    end
```

The delta passed to `adjust_balance` is computed by the aggregate from the category's
`profit` flag. No caller invents a sign.

### One outbox drain cycle

```mermaid
sequenceDiagram
    autonumber
    participant R as OutboxRelay
    participant O as PgOutboxReader
    participant F as FanOutPublisher
    participant DB as outbox table

    loop every OUTBOX_POLL_INTERVAL (5s)
        R->>O: fetch_unpublished(100)
        O->>DB: SELECT … WHERE published_at IS NULL ORDER BY id
        DB-->>O: rows
        alt none pending
            O-->>R: []
        else pending
            O-->>R: [(id, EventEnvelope)]
            R->>F: publish(&envelopes)
            alt a publisher fails
                F-->>R: PortError
                R->>R: log warn, rows stay unpublished
                Note over R,DB: retried next tick — at-least-once
            else all succeed
                F-->>R: Ok
                R->>O: mark_published(&ids)
                O->>DB: UPDATE outbox SET published_at = now()
            end
        end
    end
```

Relay errors are logged and retried on the next tick. The task is spawned detached, so a
broker outage never takes the API down with it.

## Architecture patterns

Each entry names the file that implements the pattern and, where one exists, the test that
keeps it true.

| Pattern | Where | What keeps it honest |
|---|---|---|
| **Hexagonal / ports & adapters** | `*-core` crates define traits; `*-pg`, `fingest-auth-jwt`, `fingest-http` implement or drive them | `crates/fingest-kernel/tests/dependency_rule.rs` · [ADR-0001](adr/0001-hexagonal-architecture.md) |
| **Enforced dependency rule** | Root `Cargo.toml` splits kernel-safe from adapter-only deps | The same test — it reads manifests, so it cannot be feature-flagged away · [ADR-0002](adr/0002-enforce-dependency-rule-with-a-test.md) |
| **Bounded contexts** | identity, catalog, wallets, planning — one crate each, no crate-to-crate edge between them | The compiler: an undeclared dependency does not build |
| **Shared kernel** | `fingest-kernel` — `Money`, `Currency`, `DateRange`, `CategoryRef`, `DomainEvent` | Kept minimal on purpose; anything context-specific belongs in that context |
| **Value objects** | `Currency` accepts 3 ASCII letters and uppercases them; `Money` rejects negatives; `DateRange` validates ordering | Construction is the only way in, so an invalid value has no representation (D9) |
| **Unit of Work** | `UnitOfWork` → `WalletTx`; `commit` takes `self: Box<Self>`, drop rolls back | `append_events` is on the transaction, so a write cannot commit without its event · [ADR-0006](adr/0006-unit-of-work-and-read-write-split.md) |
| **Read/write split (CQRS-lite)** | `WalletReader` for queries, `WalletTx` for mutations | A query physically cannot hold a transaction open |
| **Transactional outbox** | `migrations/20260906000000_outbox.sql`, `crates/fingest-events/src/relay.rs` | `published_at` is stamped only after a successful publish · [ADR-0003](adr/0003-transactional-outbox.md) |
| **Composition root** | `crates/fingest-bootstrap/src/lib.rs` — the only crate naming a port *and* its adapter | The dependency rule prevents any other crate from doing so |
| **Plugin registry + fan-out** | `fingest-plugins`: `Plugin`, `Registry`, `PluginHost`, `FanOutPublisher` | Unknown plugin name is `PluginError::Unknown` and stops startup · [ADR-0004](adr/0004-compile-time-plugin-registry.md) |
| **Published language** | `fingest-contracts` — every request/response shape, nothing else | Field-name tests in the crate; mapping lives in `fingest-http` so the wire format cannot follow the domain · [ADR-0005](adr/0005-frozen-wire-contracts.md) |
| **Clock injection** | `Clock` port in the kernel; `SystemClock` in production, `FixedClock` in tests | Time-dependent behaviour is deterministic without freezing the system clock |
| **Error translation at the boundary** | `crates/fingest-http/src/error.rs` — one `ResponseError` impl, `From` per context error | `PortError` other than `Conflict` collapses to a generic 500 and is logged, never echoed (D10) |
| **Policy objects on the extractor** | `AuthenticatedUser::require_admin`, `require_self_or_admin` | Authorization is a call in the handler, not a comment in a table |

## Where the architecture is enforced

| Rule | Mechanism | File |
|---|---|---|
| Kernel and `*-core` never touch an adapter crate | Test reading Cargo manifests | `crates/fingest-kernel/tests/dependency_rule.rs` |
| The dependency-rule test still checks something | `kernel_is_among_the_checked_crates` | same file |
| Savings stay unrouted | `savings_are_not_exposed` asserts 404 | `crates/fingest-http/src/resources.rs` |
| Every protected path needs a token | `every_protected_path_rejects_a_missing_token` | same file |
| Error body shape and status codes | Unit tests on `ApiError::error_response` | `crates/fingest-http/src/error.rs` |
| Wire field names | Serialisation tests | `crates/fingest-contracts/src/lib.rs` |
| SQL matches the schema | `cargo sqlx prepare --workspace --check -- --all-targets` | `.github/workflows/ci.yml` |
| No unsafe code | `unsafe_code = "forbid"` | root `Cargo.toml` |
| Lints are errors | `cargo clippy --workspace --all-targets -- -D warnings` | `.github/workflows/ci.yml` |

CI runs three jobs: `lint` (fmt + clippy), `test` (migrations, offline-metadata check, then
`cargo test --workspace` against a throwaway PostgreSQL 15 service container), and `docker`
(container build). There is no deployment pipeline.

## Startup and wiring

`fingest_bootstrap::run` (`crates/fingest-bootstrap/src/lib.rs`), in order:

1. `Config::from_env` — `from_source` is pure over its input, so configuration is testable
   without mutating process env. `JWT_SECRET` shorter than 32 characters is
   `ConfigError::WeakJwtSecret` and the process refuses to start (D12).
2. `init_tracing(log_level)`.
3. `connect` — `PgPool` with `db_max_connections`, then `sqlx::migrate!("../../migrations")`.
4. `plugin_host()` — registers `TracingPlugin` and `InProcessPlugin`, then
   `build(&config.plugins)` keeps only the names in `PLUGINS` and `into_publisher()`
   collapses them into a `FanOutPublisher`.
5. `spawn_outbox_relay` — detached `tokio` task polling every `OUTBOX_POLL_INTERVAL` (5s).
6. `Dependencies::build` — constructs every adapter, wraps each in `Arc<dyn Port>`, and puts
   the five application services plus `TokenVerifierRef` into actix `app_data`.
7. `HttpServer::new` — `Logger`, CORS (`GET POST PUT DELETE`, `Authorization` and
   `Content-Type`, credentials, `max_age` 3600), then `configure_routes`.

Environment variables and their defaults are listed under
[Configuration](../README.md#configuration) in the README.

## Deliberate deviations

Two places where the code does not follow the pattern the rest of the system uses. Each is
intentional; changing them would cost more than the consistency is worth.

**`saving` is modelled, persisted and unrouted.** `SavingRepository` and the `saving` table
exist; no HTTP route reaches them. This matches v1's surface, and
`savings_are_not_exposed` fails if anyone adds one by accident.

**`DateRange` uses sentinel dates for unbounded ranges.** Not `Option<NaiveDate>`. This
reproduces v1's filtering behaviour exactly, including at the boundaries, which a nullable
representation would have quietly changed.

---

See also: [README](../README.md) · [Deviations from v1](../README.md#deviations-from-v1) ·
[Decision records](adr/)
