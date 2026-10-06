# T-0324 SR3N — reviewed-source authority evidence repair review

## Classification

**`REVIEWED_SOURCE_AUTHORITY_READY`**

SR3N-R3 corrects the sole prior evidence-contract defect before execution and
baselining. Its top-level `contract.json` and materialized `plan.json` each
declare exactly one `completionArtifactIds` member:

`docs/orchestrator/review_bundles/T-0324_SR3N_REVIEWED_SOURCE_AUTHORITY_EVIDENCE_REPAIR_REVIEW_BUNDLE.md`

The SR3N-R3 output baseline names that same sole artifact and records its
pre-execution SHA-256 as
`77a5d5ec6ff50a7a9e4e9286fc2fad915a98698c50a0627ebab413fdeddcaa8e`.
Neither persisted authority surface was edited during this review.

## Exact resolver behavior and evidence defect

`AutonomousSupervisorV1::resolve_reviewed_promotion_review_authority` obtains
the task's declared artifact list before loading baseline evidence and rejects
empty or mismatched membership. It then requires exactly one current
remeasurement per declared artifact. SR3L, SR3N, and SR3N-R1 lacked the required
membership; SR3N-R3 supplies the fixed one-member declaration and matching
baseline without changing resolver behavior.

This repairs evidence reachability, not trust policy: a later controller still
must independently remeasure the changed completion output, require
`COMPLETED_VERIFIED` plus an acknowledged review record, and bind the existing
reviewed-source snapshot to that evidence. Generic ACK, completion prose, raw
paths or hashes, mutable `target/release`, and host state remain insufficient.

## Current source review

The SR3L wake-target source basis has not materially drifted in its reviewed
boundary. `operator_facade.rs` still exposes the typed designated-target action
with only canonical conversation URL plus expected-old SHA-256 CAS input, and
delegates to `mcp::operator_update_designated_chat_target`. The latter still
canonicalizes the URL, verifies the CAS and pre-existing registry/wake
coherence under the shared lock, uses the paired registry/wake transaction,
and fails closed on stale/divergent/compensation ambiguity. Current source adds
the separately reviewed SR3M reviewed-build transport but does not alter this
bridge's source semantics.

No live paired target state was read, used as source acceptance evidence, or
mutated. Host state is not evidence and cannot replace the predeclared
completion artifact.

## Threat and failure analysis

Accepting a review bundle because prose calls it a completion artifact would
allow generic completion state or caller assertion to replace contract
membership. Editing the persisted contract or plan after approval would alter
the authority that the resolver hashes and baselines. Accepting a current
workspace file without declared membership would permit mutable-source
self-blessing. Each remains refused. SR3N-R3 instead supplies the membership
before baseline collection, with no caller-selected artifact, fallback path,
or invented baseline.

No source change is appropriate: weakening the resolver, adding a fallback
bundle path, or broadening raw build/hash authority would expand trust. The
predeclared artifact and its normal completion/review lifecycle are sufficient
for this repository source basis.

## Attribution and prohibited-action audit

T-0324 SR3N-R3 attribution is this review bundle only. No product source, test,
persisted `.catdesk` state, wake/target state, reviewed-build state,
ProgramData/Program Files state, worker/supervisor/process, browser,
tunnel/Secure MCP, signing/UAC, Git, or external-project state changed.

## Verification and next action

The following required repository checks passed after this bundle update:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `cargo test --workspace --all-targets --all-features` (923-test primary suite)
- `git diff --check` (only pre-existing CRLF conversion warnings)

The attributable output remains this one bundle. Existing dirty `src/mcp.rs`
and untracked `src/operator_facade.rs` are inherited work and are not SR3N
changes.

Independent final review is requested for this ready classification, the exact
one-member contract/plan/baseline evidence, and the unchanged source boundary.

Only after this SR3N-R3 record is completed and independently acknowledged may
`operator reviewed-build prepare --review-record-id <acknowledged-SR3N-R3-record>`
be invoked; CONFIRM may use only its returned opaque token, followed by RESULT.
