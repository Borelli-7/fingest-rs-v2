---
name: fingest-api-security
description: Use when a change touches auth, authorization, secrets, identity/account checks, error bodies, or logging. Runs a security procedure with evidence; it does not request a full security audit agent.
---

# FinGest API security procedure (repository-local)

**Scope:** procedure for `fingest-rs-v2` code changes only. **Do not** run external security agents from this skill.

## Procedure
1. Verify auth/authorization path:
   - protected routes are scoped behind JWT middleware;
   - handlers enforce self-or-admin where required;
   - admin rights come from current stored account, not stale token claims.
2. Verify secret and credential handling:
   - `JWT_SECRET` length enforcement;
   - no credentials or secrets in logs/responses/docs output.
3. Verify tenant/data isolation:
   - path login checks and ownership gates on user/wallet/budget routes.
4. Verify error hygiene and PII handling:
   - storage/internal details are logged server-side but not leaked in HTTP error bodies.
5. Verify abuse controls:
   - auth rate limiting behavior and bounded key tracking are unchanged or intentionally updated.

## Required evidence format
- **Scope statement:** security-sensitive surface touched.
- **Evidence citations (`file:line`):**
  - `crates/fingest-http/src/resources.rs:14`, `:17`
  - `crates/fingest-http/src/extractor.rs:26`, `:41`, `:51`, `:86`
  - `crates/fingest-http/src/error.rs:66`, `:72`
  - `crates/fingest-bootstrap/src/config.rs:4`, `:33`, `:60`
  - `crates/fingest-http/src/rate_limit.rs:14`, `:44`, `:47`
- **Severity + confidence:** `severity: low|medium|high|critical`, `confidence: 1-10`.
- **Focused tests:** exact commands + results.
- **Concise handoff:** task id, revision, changed files, commands/results, blockers.

## Focused command set
- `cargo test -p fingest-http`
- `cargo test -p fingest-bootstrap config::tests -- --nocapture`
- `cargo test -p fingest-http auth::tests::verify_requires_a_bearer_token`
- `cargo test -p fingest-http wallets::tests::a_stale_admin_claim_does_not_grant_admin`

