# T-0160 / T-0154-R4 - Reviewed Promotion Authorization Gate

## R3 bypass

R3 correctly stopped native-reload receipts from minting LKG authority, but a
direct invocation of `promote-reviewed-catdesk-build.ps1 -Execute` could still
perform a successful Phase-1 handback, canonical swap, and final handback,
then write `reviewed-promotion.json` and LKG. Those facts are operational
identity evidence, not independent review approval.

## Authority hierarchy

Only the CatDesk Rust control plane may issue a one-shot protected promotion
authorization. Native receipt state, health, listener/PID/path/hash evidence,
Phase-1 state, a review bundle, caller fields, and direct PowerShell execution
are insufficient. The authorization binds a candidate-relative path and hash,
prior canonical hash, promotion-script hash, trusted-PowerShell identity hash,
generation, expiry, and a completed CatDesk independent-review session plus
digest. Confirmation rechecks candidate, canonical and script identities then
schedules a detached fixed-purpose worker. The opaque authorization ID is not
returned by MCP. The worker writes only a bounded durable
`PROMOTION_COMPLETED` or `PROMOTION_FAILED_OR_AMBIGUOUS` result; RESULT never
returns child stdout, stderr, paths, routes, or credentials.

`promote-reviewed-catdesk-build.ps1 -Execute` now returns the fixed
`REVIEWED_PROMOTION_AUTHORIZATION_REQUIRED` result before handoff, backup,
transaction, swap, reviewed-promotion authority, or LKG mutation when that
record is unavailable, stale, malformed, or bound to another candidate. The
read-only plan and `-ValidatePhase1` modes remain authorization-free.

The reviewed-promotion record now includes the exact authorization ID and the
schema-2 promotion transaction includes the same ID. A crash recovery can only
continue a transaction with the matching protected authorization; a candidate
or prior-hash mismatch fails closed. The authority record is written after
canonical handback, before LKG persistence and transaction cleanup.

## Changed paths

- `src/delegated/autonomy_supervisor.rs`: first-class
  `catdesk_reviewed_build_promotion` PREFLIGHT/CONFIRM/RESULT surface and exact
  `catdesk_daemon_reload` compatibility decisions.
- `src/daemon_reload.rs`, `src/main.rs`: bounded authorization persistence,
  confirmation revalidation, closed detached worker arguments, and worker
  dispatch.
- `src/operator_facade.rs`: fixed trusted-PowerShell promotion invocation.
- `scripts/promote-reviewed-catdesk-build.ps1`: protected authorization gate
  and transaction/authority binding.
- `scripts/catdesk-release-recovery.ps1`: requires authorization binding on
  reviewed-promotion LKG migration evidence.
- `scripts/test-promote-reviewed-catdesk-build.ps1` and
  `scripts/test-start-catdesk-stack.ps1`: direct-execute refusal and updated
  provenance fixtures.

## Deterministic evidence

The promotion fixture proves direct `-Execute` without a protected
authorization leaves canonical bytes and transaction state untouched. It also
continues to exercise bounded Phase-1 and promotion crash handling through
explicit test-only trusted authorization seams. The Rust worker-argument test
rejects extra script/program arguments.

Executed locally: `cargo fmt --check`; `cargo clippy --all-targets
--all-features -- -D warnings`; `cargo test` (587 passed, 18 ignored; 2
PowerShell integration tests passed); the fixed promotion, bootstrap, and
lifecycle PowerShell fixtures; and `git diff --check`.

No live promotion, reload, recovery, tunnel, browser, Scheduler,
external-project, protected-state, or Git-publication action occurred.

## Host acceptance

Build an isolated reviewed candidate, invoke only the first-class
`catdesk_reviewed_build_promotion` PREFLIGHT then CONFIRM surface (or its exact
cached compatibility decisions), and wait for redacted durable result evidence.
Never relay direct operator PowerShell. Verify the fixed canonical hash and
official runtime through the reviewed lifecycle surface before any recovery
operation.
