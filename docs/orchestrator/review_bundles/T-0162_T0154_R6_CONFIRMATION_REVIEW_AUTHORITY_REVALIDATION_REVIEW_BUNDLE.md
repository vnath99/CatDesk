# T-0162 / T-0154-R6 — Confirmation Review-Authority Revalidation

## Defect reproduced

R5 preflight resolved an acknowledged `independent_final_review` inbox row, its
completed session, contract, completion artifact, and task-output baseline.
R5 confirmation only rechecked the review-inbox envelope.  Therefore a changed
contract, completion artifact, baseline, or attributed output after preflight
could still reach `confirm_reviewed_promotion` and schedule the detached worker.

## Implementation

`AutonomousSupervisorV1::resolve_reviewed_promotion_review_authority` is now
the single fail-closed authority resolver used by both `PREFLIGHT` and
`CONFIRM`.  It accepts only the exact review record id already supplied at
preflight (and persisted in protected preflight state); it never enumerates or
selects historical completed sessions.

The resolver requires one acknowledged record with project `catdesk`, state
`COMPLETED_VERIFIED`, action `independent_final_review`, and reference
`artifacts/completion.json`; its exact inactive completed session with a current
task; a reloaded contract whose recomputed `decision_hash()` equals the session
and baseline bindings; PASSED completion evidence with non-empty final review;
and a matching immutable baseline.

For an empty graph, the current task must equal `contract.taskId` and only the
contract artifact list applies.  For a non-empty graph, exactly one graph task
must match `currentTaskId`; there is no contract-level fallback.  Every
non-empty required artifact is safely rehashed with the completion-attribution
identity rules and must still differ from its immutable baseline observation.

The deterministic SHA-256 evidence digest binds record/session/action/reference,
recomputed contract hash, logical task, completion verification and final-review
digest, exact artifact list, baseline observations, and current output hashes.
Preflight persists it through the existing protected promotion preflight.
Confirmation recomputes it using the persisted record id and requires exact
session, record, and digest equality before authorization persistence or worker
scheduling.

## Changed files

- `src/delegated/autonomy_supervisor.rs`
- `src/delegated/autonomous_controller.rs`
- `docs/orchestrator/review_bundles/T-0162_T0154_R6_CONFIRMATION_REVIEW_AUTHORITY_REVALIDATION_REVIEW_BUNDLE.md`

`safe_task_output_hash` is reusable crate-local completion-attribution logic.
It treats only `NotFound` as absence and rejects link/reparse/special-file and
metadata ambiguity while retaining the 16 MiB content bound.

## Deterministic coverage

- `reviewed_promotion_authority_revalidates_completion_output_evidence` creates
  a valid acknowledged completed session/baseline/output through the durable
  store, resolves authority, changes the output and proves the digest changes,
  then deletes the output and proves rejection.
- `reviewed_promotion_dag_authority_never_falls_back_to_legacy_outputs` proves
  a graph task uses only its own approved artifact list and rejects a legacy or
  missing task id.
- Existing state-store validation continues to reject malformed baseline shape;
  the shared resolver uses those durable readers before current-file checks.

## Verification evidence

- `cargo fmt --check` — passed.
- `cargo clippy --all-targets --all-features -- -D warnings` — passed.
- `cargo test` — passed: 589 passed, 18 ignored; the two PowerShell recovery
  integration tests also passed.  The test binary reported 607 unit tests
  after this task's two additions.
- `git diff --check` — passed.
- `cargo build --release` — passed.  The initial optimized compilation exceeded
  the harness's 120-second foreground window, so it was allowed to complete
  under the same fixed Cargo invocation; a subsequent foreground invocation
  completed successfully in 0.28 seconds.

## Preserved boundaries

Candidate/canonical/build-script/trusted-PowerShell drift checks, protected
one-shot authorization, direct PowerShell refusal without authorization,
reload-success-is-not-review, reviewed-promotion/LKG authority, and detached
response-safe worker boundaries are unchanged.  This ticket does **not** solve
candidate source-to-build provenance; that remains blocked on T-0163.

No live promotion, reload, recovery, external project, tunnel, browser,
Scheduler, protected production state, or Git publication action occurred.

## Host acceptance after independent review

Build/load an isolated reviewed candidate.  Use the first-class reviewed-build
promotion preflight with the exact acknowledged review record id, then confirm
only while the protected preflight remains valid and the recomputed authority
digest is unchanged.  Do not use direct PowerShell promotion.  T-0163 source to
build provenance must be accepted before live promotion is authorized.
