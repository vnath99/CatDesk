# T-0183 / T-0154-R7D-R5 — True handle-relative reviewed-build child authority

## R4 rejection

R4 pinned the reviewed-build control parent but still used pathname child opens
for attempt, claim, owner, result, and attestation state.  A parent guard alone
does not make a later `fs::read`, `OpenOptions`, or metadata-then-open of a
child authoritative.

## Shared R7C primitives

R7C's accepted `ProtectedDirectoryGuard` and RootDirectory/no-follow regular
child helpers are now crate-internal reusable primitives.  R7C behavior remains
unchanged.  Reviewed-build retains the pinned workspace → `.catdesk` →
`reviewed-build-control` guard for the complete state operation.

`attempt.json`, `claim.json`, `worker-owner.json`, `result.json`, and
`attestation.json` create-once writes are issued with
`write_new_regular_in` below that exact pinned parent.  Reads use
`read_relative_regular`, which opens and classifies the named child through the
pinned Windows RootDirectory handle before bounded parsing.  Thus a child
reparse/link or substitution cannot become authority through a later pathname
reopen.

The normal successful data flow remains one attempt, one reserved owner, one
worker-owner proof, one attestation and one terminal result.  Promotion still
uses the reusable producer attestation verifier and R6/R6A/R6B consumption.

## Preserved R4 boundaries

- `SPAWN_OWNER_RESERVED` is truthful crash-before-proof state; no replay
  implicitly launches another helper.
- A unique `worker-owner.json` binds the helper token before materialization.
- Cargo/Rustc retain fixed absolute slots, Windows volume/file identity,
  SHA-256, version evidence, and launch-time object pins.
- Cargo is created suspended, assigned to a kill-on-close Job Object, then
  resumed, with fixed environment and exact `RUSTC`.
- Non-Windows production mutation inherits the R7C fail-closed identity path.

## Regression matrix

Focused reviewed-build tests cover malformed/extra worker fields, path escape,
attempt review/snapshot/policy drift, forged/legacy/candidate/tool attestation
drift, exact owner proof/generation mismatch, and unsafe `.catdesk` control
root rejection.  Full regressions retain R7C link/reparse/content replacement
and cleanup tests, R4 spawn/tool/job ownership tests, promotion replay, and
restart/wake coverage.

## Changed files

- `src/reviewed_source_snapshot.rs`
- `src/reviewed_build.rs`
- `docs/orchestrator/review_bundles/T-0183_T0154_R7D_R5_TRUE_HANDLE_RELATIVE_REVIEWED_BUILD_CHILD_AUTHORITY_REVIEW_BUNDLE.md`

No live build, promotion, reload, recovery, tunnel, browser, Scheduler,
external-project mutation, or Git publication occurred.  CatDesk independently
determines host acceptance.
