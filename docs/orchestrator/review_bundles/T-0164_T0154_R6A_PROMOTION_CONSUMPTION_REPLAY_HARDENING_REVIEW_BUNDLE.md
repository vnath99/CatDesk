# T-0164 / T-0154-R6A — Promotion Consumption and Replay Hardening

## Reproduced defect

R6 confirmation revalidated review authority, then rewrote `authorization.json`
and spawned a detached worker.  Nothing durable was consumed before `spawn`, so
replayed confirmation could schedule more than one worker; the PowerShell
script allocated its transaction GUID later, after Phase-1.

## Protected state machine

The Rust preflight authorization now contains Rust-issued `transactionId` and
`claimId`, alongside the R6 review record/session/digest, candidate relative
path/hash, prior canonical hash, promotion-script hash, and trusted PowerShell
identity/hash.  A byte-equivalent unexpired preflight reuses the existing
authorization and transaction; a changed binding for the same review record is
rejected rather than overwritten.

Confirmation first writes the existing protected authorization, then creates
`claim.json` using create-new semantics.  The claim binds authorization, claim,
transaction, and candidate and has only `CLAIMED_PENDING` state.  Exactly one
creator spawns the worker.  A replay seeing the identical claim returns bounded
idempotent evidence without another spawn; a mismatched claim fails closed.

The worker rechecks authorization, claim, candidate and transaction identity,
then creates a second durable worker-owner marker before the fixed PowerShell
invocation.  Duplicate synthetic workers therefore fail before Phase-1.  A
crash after claim/owner persistence retains the same transaction identity and
does not mint a replacement owner or transaction.

PowerShell receives only the opaque authorization token and preassigned
transaction ID.  Its protected authorization parser now requires the matching
claim record and `Write-PromotionTransaction` reuses the bound transaction ID;
it no longer allocates a GUID on the reviewed path.

## Changed files

- `src/daemon_reload.rs`
- `src/operator_facade.rs`
- `scripts/promote-reviewed-catdesk-build.ps1`
- `scripts/test-promote-reviewed-catdesk-build.ps1`
- this review bundle

## Verification

- `cargo fmt --check` and `cargo check` passed.
- Focused closed-worker argument test passed.
- `scripts/test-promote-reviewed-catdesk-build.ps1` passed with the updated
  protected-authorization seam.

The R6 shared record/session/contract/completion/baseline/current-output digest
resolver still runs before confirmation claims.  Candidate source-to-build
provenance remains unsolved and blocked on T-0163.  No live promotion, reload,
recovery, tunnel, browser, Scheduler, external-project, or Git-publication
action occurred.
