# T-0221 — Post-verification lease-expiry reconciliation

## Scope and reproduced defect

T-0220 showed a durable control-plane gap: a provider turn had terminated and
the verifier had recorded `PASSED`, but a later local finalization operation
could leave the session in `VERIFYING`.  On restart, the normal provider lease
check ran before that local bookkeeping and an expired lease prevented the
already-executed task from completing.

The pre-change sequence in `AutonomousControllerV1::finish_turn` was:

1. transition to `VERIFYING` and record verifier activity;
2. run the verifier and record its result;
3. enforce output attribution, create/validate the reviewed-source snapshot,
   mark queue progress, construct final review, write completion artifacts,
   finish accounting, transition terminal, and emit inbox evidence.

There was no durable boundary between steps 2 and 3.  In particular, the
reviewed-source snapshot and completion-artifact calls were fallible after a
successful verifier result, while a later `run_once` rejected an expired lease
before any recovery-only work could occur.

## Implemented durable boundary

`AutonomousPassedVerificationCheckpointV1` is written only after all of the
following have succeeded:

- terminal provider execution and a verifier `PASSED` result;
- non-empty authoritative diff;
- task-output attribution enforcement; and
- reviewed-source snapshot creation/validation, where the approved task
  requires one.

The checkpoint is stored in the session's `verification/passed-finalization.json`
and binds schema/session/project/approved contract hash/logical task/provider
identity and turn count, verification profile and summary, the bounded
authoritative diff plus SHA-256, and a SHA-256 digest of the immutable
attribution/snapshot expectation.  It is finalization evidence only: it is
not a provider launch permit, mutation permit, baseline-recapture permit, or
lease extension.

After the checkpoint exists, `run_once` recognizes `VERIFYING`,
`RECOVERING_AFTER_RESTART`, and the recoverable waiting states before the
ordinary lease gate.  It invokes only `finalize_passed_verification`; that
path does not select a task, capture a baseline, enter repair, rerun a
provider, or resample/create reviewed source snapshots.  It performs bounded
local queue/completion/final-review/accounting/inbox finalization using the
persisted verifier result and exact persisted diff.  A normal session with no
checkpoint still encounters the unchanged lease-expiry stop before provider
selection.

Checkpoint binding mismatch, unreadable/corrupt data, snapshot failure before
checkpoint creation, final-review failure, completion-artifact failure,
accounting failure, or inbox failure now records a fixed non-secret
`post_verification_finalization_pending` event and durable recoverable
escalation reason.  It does not silently abandon `VERIFYING` and does not
launch a provider.

## Exactly-once properties

`write_completion_artifacts` now treats an identical existing completion
artifact as a replay success and a differing existing artifact as a durable
conflict.  The existing review-inbox stable record identity remains
idempotent.  Therefore a retry after a local failure can reuse the same
checkpoint without overwriting completion evidence or creating another inbox
record.  A completed multi-task checkpoint is removed only after that exact
task is durably complete, so it cannot authorize a later graph task.

## Deterministic tests

`passed_verification_checkpoint_finalizes_after_expiry_without_a_second_turn`
reproduces the required shape with the deterministic provider/verifier seam:

1. provider terminal + verifier `PASSED` at lease-valid time;
2. final-review seam fails, leaving a durable checkpoint and recoverable
   waiting state;
3. a fresh controller (restart analogue) resumes at the expired lease time;
4. it reaches `COMPLETED_VERIFIED` without invoking the replacement provider
   or verifier;
5. persisted provider turn count is unchanged, completion artifacts exist,
   and repeated terminal ticks leave exactly one inbox record.

Existing focused coverage still proves expiry before provider launch stops at
`LEASE_EXPIRED`, failed verification enters the bounded repair path, task
output baselines are immutable/attributed, restart recovery preserves provider
continuity, and graph progression remains ordered.

## Changed files

- `src/delegated/autonomous_controller.rs`
- `src/delegated/autonomy_state.rs`
- `docs/orchestrator/review_bundles/T-0221_POST_VERIFICATION_LEASE_EXPIRY_RECONCILIATION_REVIEW_BUNDLE.md`

## Verification

- `cargo test passed_verification_checkpoint_finalizes_after_expiry_without_a_second_turn -- --nocapture` — passed.
- `cargo test autonomous_controller -- --nocapture` — 34 passed, 0 failed.
- `cargo fmt --check` — passed.
- `cargo clippy --all-targets --all-features -- -D warnings` — passed.
- `cargo test` — 696 unit tests passed, 21 existing host-specific tests
  ignored, plus 4 integration/measure tests passed; 0 failed.
- `git diff --check` — passed.

The working tree already contained extensive unrelated controller/state
changes before this task, so repository-wide `git diff --stat` is not the
T-0221 attribution boundary.  The attributable implementation surface is the
passed-finalization checkpoint read/write/validation helpers, the
`finalize_passed_verification` recovery branch and its focused test, together
with this bundle.  No unrelated changes were reset, reverted, or claimed.

No daemon reload, wake dispatch, GUI activation, promotion, tunnel action,
external-project operation, or Git mutation occurred.  This is a local
controller/state repair only.  Independent CatDesk review remains required.

## R3 checkpoint-before-snapshot ordering correction

Independent review correctly identified that the earlier checkpoint ordering
still left a crash/failure window: it created or validated the reviewed-source
snapshot before persisting the passed-verification checkpoint.  That statement
in the earlier ``Implemented durable boundary`` section is superseded by this
section.

The corrected `AutonomousControllerV1::finish_turn` order is now:

1. terminal provider, verifier `PASSED`, non-empty authoritative diff, and
   task-output attribution complete;
2. capture the exact approved output hashes and baseline observations as a
   `ReviewedSourceSnapshotExpectedV1` expectation, without creating or
   validating CatDesk-owned snapshot evidence;
3. persist `AutonomousPassedVerificationCheckpointV1`, bound to the session,
   contract, logical task, provider turn, verification/diff hashes, and the
   complete expected output observations; and only then
4. enter deterministic finalization, which first hash-compares each current
   approved output to the persisted expectation and only then calls
   `create_or_validate_reviewed_source_snapshot`.

The durable state validator recomputes both the authoritative-diff SHA-256 and
the attribution digest over the persisted expected snapshot tuple. Corrupt,
rebound, malformed, or output-drifted checkpoint data is rejected. Recovery
does not recapture a baseline, rerun provider/verifier/repair, create source
expectations from current files, or mutate approved outputs. It only reads the
bound output identities; a mismatch leaves a bounded recoverable
`passed_verification_output_mismatched` result with no completion artifact or
inbox emission.

`checkpoint_precedes_snapshot_failure_and_expired_restart_finalizes_once`
uses a deterministic regular-file snapshot-root blocker after all release
source inputs are present. The provider and verifier complete, output
attribution succeeds, and the checkpoint with a concrete
`src/task-output.txt` SHA-256 expectation is durably present before snapshot
creation fails. After only the snapshot-root blocker is cleared, a fresh
controller at expired-lease time completes from the checkpoint without a
second provider/verifier turn and emits exactly one inbox record across repeat
ticks.

`checkpointed_output_hash_drift_fails_closed_without_a_second_turn` changes
the attributed output after that same checkpoint. Expired recovery remains
recoverable, has the identical provider-turn count, and creates neither
completion artifacts nor an inbox record.

R3-focused verification:

- `cargo test checkpoint_ -- --nocapture` — 3 passed.
- `cargo test checkpointed_output_hash_drift_fails_closed_without_a_second_turn -- --nocapture` — passed.
- `cargo test autonomous_controller -- --nocapture` — 36 passed.

Full rust_full result after the correction:

- `cargo fmt --check` — passed.
- `cargo clippy --all-targets --all-features -- -D warnings` — passed.
- `cargo test` — the unit-test phase passed 698 tests with 21 existing
  host-specific ignores, but the subsequent `tests/recovery_powershell.rs`
  integration fixture failed in the pre-existing
  `scripts/test-promote-reviewed-catdesk-build.ps1` assertion
  `future native reload receipt fails closed before canonical mutation`.
  This does not exercise the T-0221 controller/state finalization path, and
  no unrelated reload/promotion script was changed to mask it.
- `git diff --check` — passed.

The attributable R3 implementation remains confined to
`src/delegated/autonomous_controller.rs`, `src/delegated/autonomy_state.rs`,
and this review bundle; unrelated existing worktree changes remain outside
this task's attribution boundary.
