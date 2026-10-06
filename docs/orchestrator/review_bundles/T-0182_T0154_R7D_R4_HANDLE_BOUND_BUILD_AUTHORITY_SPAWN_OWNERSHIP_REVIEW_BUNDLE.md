# T-0182 / T-0154-R7D-R4 — Handle-bound build authority and spawn ownership

## Rejected T-0181 boundaries

Host review found three distinct gaps in the prior reviewed-build worker:

1. `CONFIRM` spawned the helper while its durable claim still said that no
   worker had been proven, then attempted to rewrite the claim afterwards.
   A fast helper could therefore observe an invalid pre-owner state.
2. The reviewed-build control root was created/read through ordinary pathname
   operations despite the accepted R7C handle-bound containment model.
3. Cargo/Rustc identity used length/mtime as part of the recorded identity and
   the Job Object was attached only after Cargo had been created.

This change is restricted to those boundaries.  The R7C snapshot schema and
all R6/R6A/R6B promotion evidence remain unchanged.

## Durable spawn-owner state machine

`CONFIRM` now creates one create-new `claim.json` with:

`SPAWN_OWNER_RESERVED(reviewAttemptId, ownerId, generation=1)`.

The closed helper receives both attempt and owner identifiers.  Before it can
materialize snapshot bytes or touch a build target it must prove the exact
reservation and create a unique `worker-owner.json`.  That create-new file is
the worker linearization point; duplicate helpers, stale owner tokens, malformed
claims, and owner mismatches fail before build mutation.  Only the helper that
owns that proof updates the claim to `WORKER_OWNED`.

| Durable evidence | Returned state | Replay behavior |
| --- | --- | --- |
| Reservation, no owner proof | `CLAIMED_PENDING_UNPROVEN` | never implicitly respawns |
| Exact owner proof and worker claim | `WORKER_OWNED_PENDING` | same attempt only |
| Valid result + attestation chain | `BUILD_ATTESTED` | idempotent validation |
| Failed/ambiguous result | `BUILD_FAILED_OR_AMBIGUOUS` | terminal; no replacement owner |

This closes the claim-before-spawn crash window without reporting a reservation
as a scheduled or owned worker.

## Containment and stable tool identity

The reviewed-build control root now holds the accepted R7C
`ProtectedDirectoryGuard` from the positively classified workspace through the
`.catdesk/reviewed-build-control` chain.  It creates one component at a time
and rejects a link/reparse/special/unsafe `.catdesk` intermediate; unsupported
platforms inherit R7C's fail-closed stable-identity behavior.

Cargo and Rustc remain selected only from fixed absolute host slots.  On
Windows each tool is opened with `FILE_FLAG_OPEN_REPARSE_POINT`, regular-file
classification, and a no-write/no-delete sharing pin.  The persisted identity
is volume serial plus file index, paired with SHA-256 and bounded `--version`
evidence—not length+mtime.  Worker startup retains an exact pinned Cargo/Rustc
file while launching and forcing `RUSTC`, so a same-length/mtime replacement
cannot satisfy the tool evidence or be swapped before launch.

## Pre-execution process ownership

The kill-on-close Job Object is allocated before Cargo creation.  Windows Cargo
is created suspended, assigned to that Job Object, and resumed through the
process handle only after attachment.  Thus Cargo cannot execute a build script
outside CatDesk's descendant-tree ownership.  Timeout kills the Job Object and
waits for Cargo; unavailable Job/attach/resume support fails closed.

The fixed `cargo build --release --locked` argv, isolated `CARGO_HOME` and
`CARGO_TARGET_DIR`, exact pinned `RUSTC`, cleared inherited environment, and
closed MCP/static decision shapes remain in force.

## Regression coverage

Focused production tests cover closed worker arguments, immutable
review/snapshot/policy digests, forged/legacy/candidate/tool attestation
rejection, exact claim ownership, reserved-owner proof mismatch/stale-generation
refusal, and unsafe `.catdesk` control-root rejection.  The full suite also
retains R7C byte-snapshot containment and replacement-race tests, R6 promotion
replay tests, and T-0179/T-0180 restart reconciliation tests.

## Changed files

- `src/reviewed_build.rs`
- `src/reviewed_source_snapshot.rs` (narrow crate-private export of the
  accepted directory guard)
- `src/delegated/autonomy_supervisor.rs`
- `src/main.rs`
- `docs/orchestrator/review_bundles/T-0182_T0154_R7D_R4_HANDLE_BOUND_BUILD_AUTHORITY_SPAWN_OWNERSHIP_REVIEW_BUNDLE.md`

## Verification

The implementation runs only local deterministic checks; it does not invoke a
reviewed build worker, daemon reload, promotion, recovery, tunnel, browser,
Scheduler, external project, or Git publication.  Final rust_full and Git
status/diff checks are recorded by the task handoff.  CatDesk independently
determines acceptance and later host-side live-build eligibility.
