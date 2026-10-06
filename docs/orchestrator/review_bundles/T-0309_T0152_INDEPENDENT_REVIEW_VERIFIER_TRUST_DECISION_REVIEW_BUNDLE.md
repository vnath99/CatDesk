# T-0309 / T-0152 Independent-Review Verifier Trust Decision Review Bundle

## Scope and preserved boundary

T-0308 remains accepted only as a design-only `TRUST_MODEL_BLOCKER`. Its
conditional ProgramData ledger, closed capture, and finalizer are not
implemented or authorized by this review. T-0307 remains a read-only
reconciliation and does not authorize bootstrap or signing.

Current gate truth is unchanged: T-0224 is accepted; T-0223 is source-ready
but parked at the externally reviewed-image bootstrap prerequisite;
T-0222/T-0139 is source-ready but literal-host unaccepted; T-0152 is
unaccepted; T-0155 is unstarted. The order remains **T-0223 ->
T-0222/T-0139 -> T-0152 -> T-0155**.

**Sole classification: `EXISTING_AUTHORITIES_PURPOSE_BOUND_INADEQUATE`.**
There are accepted receipt/verification mechanisms, but none explicitly
authorizes the distinct domain `host-gate observation approval` with an exact
gate, product-derived observation digest, project/session/current-target
binding, and independent reviewer verdict. Reuse would be a trust-policy
extension, not an implementation convenience.

## Candidate-authority matrix

| Candidate | Exact source/purpose | Ownership and fixed transport | What it binds and replay/expiry semantics | Host-gate approval reuse? |
| --- | --- | --- | --- | --- |
| Canonical final-review inbox | `ReviewInboxRecord` / `canonical_inbox_records` in `src/stable_wake_core.rs`; terminal action is `independent_final_review`. `emit_delegated_review_inbox` in `src/delegated/autonomy_runtime.rs` creates a deterministic run/final-review/diff/current-target reference. | CatDesk autonomous/delegated state writer; fixed workspace `.catdesk/autonomy/review-inbox.json`, schema 1 and bounded/reparse-safe read. A reviewer acknowledgement is represented operationally by `unread == false`. | Binds record/project/session/state/next-action/reference/timestamp. Duplicate conflict is refused by canonical consumers; no signature, verifier key, verdict field, observation digest, or approval sequence exists. | No. It is a terminal-review workflow record, not an authenticated host-gate verdict. |
| Reviewed-promotion review authority | `AutonomySupervisorV1::resolve_reviewed_promotion_review_authority` in `src/delegated/autonomy_supervisor.rs`. | CatDesk autonomous state store and the same acknowledged inbox record; no public verifier root or signed receipt transport. | Requires one `catdesk` unread=false `COMPLETED_VERIFIED`/`independent_final_review` record with `artifacts/completion.json`, matching contract hash, passed verification/final review, and remeasured approved source-output baseline/current observations. It derives a digest and `ReviewedSourceSnapshotExpectedV1`. | No. Its accepted domain is a reviewed source snapshot used only by `reviewed_build`/`reviewed_build_promotion`; it does not collect, bind, or approve host observations/current wake target/generation. |
| Reviewed-build attestation | `ReviewedBuildAttestationV2`, `validate_producer_attestation`, and `validate_attestation` in `src/reviewed_build.rs`; producer string is `CATDESK_REVIEWED_BUILD_WORKER_V2`. | Fixed workspace reviewed-build control root, with attempt/claim/owner/result/attestation files and protected pinned-relative access. The worker produces it; it has no independent signing key. | Binds reviewed session/record digest, committed source snapshot, fixed Cargo/Rustc policy/identities, candidate path/hash/length/identity, generation, and recomputed attestation digest. Claim/owner/result mismatch or candidate drift fails closed. | No. It is candidate-build provenance, not a reviewer receipt or runtime observation authorization. |
| Reviewed-promotion authorization | `ReviewedPromotionAuthorizationV1`, `prepare_reviewed_promotion`, `confirm_reviewed_promotion`, and `run_reviewed_promotion_worker` in `src/daemon_reload.rs`. | Rust control plane writes fixed `.catdesk/promotion-control` preflight/authorization/claim/owner/result records; only fixed promotion worker/script consumes it. | Schema 1, one confirmation token, 120-second TTL, authorization/claim/transaction IDs, candidate/canonical/script/PowerShell hashes, review/source/attestation/toolchain binding, and create-once claim/rollback semantics. | No. It authorizes one candidate replacement transaction, not acceptance of host activation/GUI/sweep observations. |
| Reviewed-main-image product root | `REVIEWED_MAIN_IMAGE_PRODUCTION_PUBLIC_KEY`, `ReviewedMainImageEnvelopeV1`, and `verify_reviewed_main_image_envelope_for_policy` in `src/reviewed_build.rs`; T-0215/T-0217 bundle policy. | External/offline private signer; only the public Ed25519 root is compiled into CatDesk. Fixed ProgramData transport and Program Files accepted/rotation receipts hold canonical envelopes. | Signature covers product, purpose, root/version, monotonic epoch, exact policy SHA-256, payload hash/length, review/build ids. Exact accepted recovery is narrow; stale/same-epoch conflict fails closed. | No. Accepted purposes are exactly `reviewed-main-image-bootstrap` and `reviewed-main-image-rotation`; a host-gate purpose/policy would be rejected before signature validity could matter. |
| Stable-wake delivery receipt | `StableWakeDelivery::read_exact_sent_evidence` and terminal receipt validation in `src/stable_wake_delivery.rs`. | Stable-wake owner and fixed workspace wake state; schema 4 outer state, receipt schema 1, canonical inbox plus protected target readback. | Exactly one terminal record/delivery; positive browser send time, message digest, current target digest, target-drift/attention/duplicate/nonmonotonic-state refusal. | No. It establishes browser delivery of the T-0224 review event only; it contains no T-0223/T-0222 lifecycle, GUI, approval, or aggregate verdict semantics. |
| Autonomous controller verifier/final review | `AutonomousControllerV1` verifier boundary and `final_review` in `src/delegated/autonomous_controller.rs`. | CatDesk verifier implementation and durable completion artifacts; no explicit independent reviewer public key, signed verdict, or host-gate receipt transport. | Requires verification plus an independent completion artifact before `COMPLETED_VERIFIED`; used to emit the inbox request. | No. It verifies task completion/artifacts and triggers review workflow; it does not authenticate a later independent host-gate review. |

## Purpose and domain-separation proof

The sources deliberately reject the tempting substitutions:

1. The T-0215/T-0217 main-image root verifies a canonical envelope only after
   `product`, purpose, root version, and fixed bootstrap/rotation policy digest
   all match. A signature under that root with a host-gate domain cannot pass
   the existing verifier. Reusing it would require a new policy purpose and
   an external signing decision, which T-0309 must not create or request.
2. A reviewed-build attestation is recomputed from an already-approved source
   snapshot, fixed toolchain, worker ownership, and candidate bytes. It is not
   signed by an independent reviewer and cannot be made to approve a live
   listener, pipe, window, or GUI session by adding fields.
3. Promotion authorization is a short-lived Rust-issued execution capability.
   Its review binding exists only to protect one candidate/transaction and it
   expires or fails closed on candidate, preflight, claim, or attestation
   drift. Treating it as a durable host-acceptance verdict would invert its
   authority direction.
4. A schema-4 `SENT` receipt is exact evidence for the wake owner’s sole
   browser-submit responsibility. Its target/message/record binding cannot be
   reinterpreted as supervisor or GUI proof.
5. The promotion-review resolver is the narrowest plausible reusable
   independent-review mechanism, but its exact revalidation creates a
   `ReviewedSourceSnapshotExpectedV1` and supports only build/promotion calls.
   The acknowledged inbox bit is not an unscoped signed approval. Extending it
   to a separately named host-gate receipt is a new accepted policy domain.

Thus no discovered primitive can currently authorize
`(gate, observationDigest, projectId, sessionId, t0224RecordId,
currentTargetSha256, buildIdentity, generation, hostSession, approvalTime)`
without an explicit purpose expansion. Mechanism existence is not policy
authorization.

## Exact parked decision and next step

Implementation remains parked. The single next decision is an independently
reviewed trust-policy choice between these abstract alternatives, with no key,
secret, signature, host action, or implementation selected by this ticket:

1. **Existing-review-policy extension:** explicitly authorize a separately
   named `core-host-gate-approval-v1` receipt domain on the existing
   acknowledged-review authority, define its fixed product-derived observation
   binding and replay semantics, and confirm that acknowledgement cannot be a
   caller-editable acceptance assertion for this domain.
2. **Separate independent receipt authority:** explicitly authorize an
   independently authenticated verifier root/service and one fixed receipt
   transport for the same domain, including how the product reads only a
   bound verdict without receiving reviewer-controlled paths/prose/hashes.

No recommendation is made between them. Only after one is accepted may a
later implementation ticket apply the T-0308 conditional ledger design:
`src/core_host_gate_evidence.rs`, bounded reader integration in
`src/core_host_acceptance_preflight.rs`, and only the closed surfaces/tests
identified there. Migration remains absence-only and the pure preflight remains
the sole acceptance engine.

## Fail-closed consequences

- Missing, unsigned, wrong-domain, wrong-key, expired, replayed, duplicate,
  or conflicting candidate receipt remains `Missing`/invalid live evidence.
- A valid image signature, build attestation, promotion token, wake receipt,
  or reviewed completion record for another purpose never reaches T-0223,
  T-0222, or T-0152 evaluator inputs.
- No observer can supply `accepted=true`, arbitrary prose, a screenshot/file,
  path, target, hash, command, or arbitrary review response as evidence.
- T-0223 host bootstrap, T-0222 GUI proof, and T-0152 remain distinct future
  live steps; no documentation or source test changes their status.

## Attribution, verification, and prohibited-action audit

T-0309 attribution is documentation only:

- `CATDESK_MILESTONES.md`
- `.catdesk/current_plan.md`
- `docs/orchestrator/review_bundles/T-0309_T0152_INDEPENDENT_REVIEW_VERIFIER_TRUST_DECISION_REVIEW_BUNDLE.md`

The accumulated dirty worktree is preserved; unrelated files are not
attributed. No key generation, signature/signing request, secret access,
artifact/candidate creation, protected-host read, bootstrap/promotion/reload,
supervisor/GUI/browser/wake/target/tunnel action, external-project action,
unrestricted shell, Git publication, or product-source implementation occurred.

Documentation-only verification completed against the unchanged product
candidate:

- `cargo fmt --all -- --check` — PASS (Cargo emitted only the pre-existing
  `could not canonicalize path C:\\Users\\Volap` warning).
- `cargo test --workspace --all-targets --all-features reviewed_main_image` —
  PASS: 3 focused tests.
- `cargo test --workspace --all-targets --all-features reviewed_build` — PASS:
  66 focused tests; 3 explicitly host/toolchain-dependent tests remained
  ignored.
- `cargo test --workspace --all-targets --all-features stable_wake_delivery`
  — PASS: the 13 focused receipt/ownership tests passed in every applicable
  target.
- `cargo test --workspace --all-targets --all-features
  core_host_acceptance_preflight` — PASS: 9 focused fail-closed tests.
- `cargo test --workspace --all-targets --all-features reviewed_promotion` —
  PASS: 3 focused review-binding tests.
- `git diff --check` — recorded after this bundle update.

No product-source file changed. An independent review should confirm the sole
classification, strict purpose separation, lack of an existing host-gate
receipt domain, the documentation-only attribution, and the parked
implementation boundary.
