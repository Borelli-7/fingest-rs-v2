# syntax=docker/dockerfile:1

# cargo-chef caches dependency compilation as its own layer, so a source-only change does
# not rebuild the tree. v1 faked this with a dummy main.rs and then ran `rm Cargo.lock` +
# `cargo update`, which both defeated the cache and silently unpinned every dependency.
#
# bookworm matches the debian:12-slim runtime. No 1.98.1 image is published yet, so the
# base is 1.98 and rust-toolchain.toml pulls the exact patch on first use.
FROM rust:1.98-slim-bookworm AS chef
WORKDIR /app
# The pinned toolchain must live in the base stage: `cargo chef cook` runs with only
# recipe.json copied in, so without this rustup would fall back to the image's 1.98.0 and
# fail the workspace `rust-version = 1.98.1` check. `rustup show` installs it once, cached.
COPY rust-toolchain.toml .
RUN rustup show
RUN cargo install cargo-chef --locked --version ^0.1

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json
COPY . .
# Queries are verified against the committed .sqlx metadata; no database at build time.
ENV SQLX_OFFLINE=true
RUN cargo build --release --locked -p fingest-api

FROM debian:12-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Unprivileged: v1 ran as root.
RUN useradd --system --uid 10001 --no-create-home fingest

WORKDIR /app
COPY --from=builder /app/target/release/fingest-api /usr/local/bin/fingest-api
COPY --from=builder /app/migrations /app/migrations

# No .env is baked in. v1 copied sample.env to /app/.env, shipping a publicly known
# JWT_SECRET inside the image; configuration must come from the environment.
USER fingest
EXPOSE 8080

CMD ["fingest-api"]
