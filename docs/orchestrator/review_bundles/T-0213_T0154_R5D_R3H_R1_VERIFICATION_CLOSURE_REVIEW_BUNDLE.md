# T-0213 R1 verification closure

## Scope and root cause

This is a corrective closure of the existing T-0213 fixed reviewed-image deployment and closed `--catdesk-reviewed-producer-service` entry work.  It does not redesign the dedicated-producer architecture, invoke provisioning, or change reviewed-build authority.

The fresh pre-edit reproduction was `cargo test`.  It ran 684 tests and failed exactly one test:

`reviewed_build::tests::adversarial_copy_evidence_and_replay_stay_bound_to_open_handles`

The failing assertion expected replay evidence to have the legitimate candidate length (12), while the observed replay object had the attacker candidate length (18).  This was a stale test expectation, not a deployment/service-entry product defect and not a task-attribution artifact.  The test deliberately arms the test-only `candidate-copy` hook.  Depending on real Windows sharing semantics, the hook can either be denied from replacing the pathname or can replace it after the handle-derived candidate evidence was collected.  The old test asserted only the denial branch.

## Changed files

- `src/reviewed_build.rs`
- `docs/orchestrator/review_bundles/T-0213_T0154_R5D_R3H_R1_VERIFICATION_CLOSURE_REVIEW_BUNDLE.md`

The test now records whether both the concrete candidate rename and attacker write succeeded.  It preserves the primary assertion that candidate evidence comes from the original opened handle.  If replacement succeeds, it asserts that a later pathname replay observes distinct attacker evidence, including the attacker SHA-256 and stable identity; if replacement is denied, it asserts replay remains the original evidence.  This accurately tests both Windows outcomes without granting later replay any authority from the original handle.

No T-0213 deployment policy code was changed in this closure: source image, Program Files destination, fixed service identity/mode/namespace policy, closed service arguments, stage-specific redacted outcomes, rollback semantics, and the final-link gate remain as implemented by the prior T-0213 work.

## Verification

Focused checks after the repair:

- `cargo fmt --check` — passed.
- `cargo test adversarial_copy_evidence_and_replay_stay_bound_to_open_handles -- --nocapture` — passed.
- `cargo test dedicated_producer_ -- --nocapture` — 10 passed.
- `cargo test replay_preopen_producer_attestation_rejects_same_length_swap -- --nocapture` — passed as an ordinary test.  This provider context reported the bounded toolchain-fixture environment-unavailable branch; it did not invoke a live worker.
- `cargo clippy --all-targets --all-features -- -D warnings` — passed.

Final verification:

- `cargo test` — 663 passed, 0 failed, 21 ignored.
- `git diff --check` — passed.

No live Program Files, SCM, service, account, ACL, namespace, or elevation mutation was performed.  No service was started, and no reviewed-build worker, promotion, recovery, tunnel, browser, Scheduler, external-project, or Git publication action was invoked.

## Remaining T-0212 host acceptance

T-0212 remains a separately approved, administrator-run host operation.  The administrator must use the existing zero-parameter fixed-policy command only after a clean read-only ABSENT preflight.  That operation must deploy the product-owned reviewed service image, create/configure the fixed `CatDeskReviewedProducer` service and restricted service SID, create the fixed secured namespace, and journal/recover each stage fail-closed.  A subsequent independent host-security acceptance must validate the actual service token/SID, IPC peer and Job membership, namespace owner/DACL/mandatory-label behavior, and retained output-handle provenance.  `REVIEWED_BUILD_FINAL_LINK_HANDOFF_REQUIRED` remains in force until that later live acceptance is positively completed.
