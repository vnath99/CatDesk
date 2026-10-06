# T-0191 — pre-open output/replay validation

## Order map

`open_built_output` pins a baseline release child, fires
`built-output-child-pre-open` immediately before its second exact relative
open, compares the two opened object identities, then fires the retained
post-open seam. `built-output-release-descent` is between a baseline pinned
release directory and the authoritative second descent.

`open_candidate_for_replay` descends the candidate parent, fires the new
`candidate-replay-pre-open` immediately before its child open, then retains
the post-open replay seam. `evidence_from_open_regular` hashes the already-open
file. `validate_producer_attestation` calls replay open/evidence after
validating attempt/result/claim/attestation and rejects mismatched candidate
SHA, length, or stable identity.

## Concrete tests

| Test | Seam timing | Mutation | Result |
| --- | --- | --- | --- |
| `adversarial_release_parent_pre_descent_is_denied_or_fails_closed` | Before authoritative release descent | Rename `release` entry | Baseline directory pin denies rebind or open fails; outside sentinel unchanged |
| `adversarial_built_output_child_true_pre_open_is_denied_or_rejected` | Immediately before second child open | Rename genuine child and install attacker bytes | Baseline file sharing/identity prevents attacker acceptance; outside sentinel unchanged |

The T-0190 post-open, candidate-parent, create-new, copy, evidence, and replay
handle-binding tests remain in place.

## Product correction

The child pre-open boundary retains a baseline exact child handle and compares
its stable Windows identity to the second exact child open. Its no-write and
no-delete sharing makes a concrete directory-entry swap fail. The release
parent has the equivalent baseline pinned directory handle across the
pre-descent seam.

## Replay limitation

This pass adds the true `candidate-replay-pre-open` seam but does not build the
full durable attempt/result/attestation fixture required to invoke
`validate_producer_attestation` end-to-end during a candidate swap. The exact
T-0191 producer-attestation fixture requirement is therefore **not proven by
this bundle**; no claim is made that all three host-rejected windows are
closed. Live reparse substitution is likewise not claimed; R7C no-follow
relative classification remains simulated fail-closed coverage.

## Changed files and verification

- `src/reviewed_build.rs`
- `docs/orchestrator/review_bundles/T-0191_T0154_R7D_R9_R3_PREOPEN_OUTPUT_REPLAY_VALIDATION_CLOSURE_REVIEW_BUNDLE.md`

Completed checks: `cargo fmt --check`, strict clippy, focused
`cargo test reviewed_build -- --nocapture` (18 passed), and `git diff --check`.
No live build worker, promotion, reload, recovery, or external mutation ran.
