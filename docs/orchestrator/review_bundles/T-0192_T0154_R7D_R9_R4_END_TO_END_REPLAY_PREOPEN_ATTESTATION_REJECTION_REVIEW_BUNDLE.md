# T-0192 — end-to-end replay attestation fixture attempt

## Required production path

`validate_producer_attestation` requires, in order: pinned control root;
attempt digest plus fixed policy; a fully committed R7C snapshot matching the
attempt; current trusted Cargo/Rustc evidence; BUILD_ATTESTED result; immutable
claim and matching owner proof; matching attestation digest; and a candidate
whose replay-open SHA, length, and stable identity match the attestation.

## Fixture result

This task added a test-only fixture builder that uses the real snapshot
producer, fixed tool resolver, pinned `control_create_json` writes, real
candidate create/evidence, and real `validate_producer_attestation` call. The
temporary fixture was rejected by the R7C snapshot producer with `reviewed
source snapshot authority is unavailable` before it could reach the reviewed
build validator. The end-to-end test is therefore marked ignored and does not
constitute acceptance evidence.

The test records the intended same-length candidate swap at the true
`candidate-replay-pre-open` seam, but the required positive production
validation could not be established from this fixture. No reparse attack is
claimed. The outside sentinel is created independently and no live worker,
promotion, daemon, recovery, or external mutation was invoked.

## Status

T-0192's mandatory positive fixture and replay validator rejection are **not
proven**. The unresolved prerequisite is a deterministic minimal workspace
that satisfies the full R7C completion-output/baseline snapshot authority,
not merely Cargo/source inventory. This bundle intentionally does not claim
completion.

## Changed files

- `src/reviewed_build.rs`
- `docs/orchestrator/review_bundles/T-0192_T0154_R7D_R9_R4_END_TO_END_REPLAY_PREOPEN_ATTESTATION_REJECTION_REVIEW_BUNDLE.md`
