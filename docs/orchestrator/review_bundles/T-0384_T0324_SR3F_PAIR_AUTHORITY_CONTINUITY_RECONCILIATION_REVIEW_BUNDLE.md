# T-0384 — T-0324 SR3F fixed-pair authority continuity reconciliation

## Technical design and classification

**Classification: `FIXED_PAIR_AUTHORITY_ABSENT`**

### Observed continuity discrepancy

The persisted session
`adc-t0382-r1-t0324-sr3d-fixed-ordinary-worker-pair-provisioning-20260911`
is marked `COMPLETED_VERIFIED`, while no separately named T-0382-R1 canonical
bundle exists. This is a continuity question, not an authority grant. The
session's completion status, generic final-review metadata, verification
success, and session transcript cannot substitute for a completed immutable
ordinary-worker predecessor/current pair.

### Accepted evidence chain

- T-0381 provides only the fixed read-only carrier
  `read_fixed_ordinary_worker_artifact_pair()`. It validates content already
  present at the compiled product-owned root; it does not produce it.
- The canonical T-0382 bundle classifies the prerequisite
  `UPSTREAM_ARTIFACT_AUTHORITY_MISSING` and explicitly records that no
  provisioner or writer was added.
- The T-0382-R1 persisted contract repeats that same conditional boundary.
  Its plan has `completionArtifactIds: []`. Its final-review payload is only
  `{result: "reviewed locally", verificationStatus: "PASSED"}`; it contains
  no pair descriptor, image bytes, digest, length, generation, review binding,
  or build-attestation binding.
- The persisted R1 diagnostic records the independent worker's explicit
  conclusion `UPSTREAM_ARTIFACT_AUTHORITY_MISSING`: T-0381 has no accepted
  upstream producer or immutable reviewed pair source. Its attributable change
  is a test-only regression that pins this classification. The R1 attributable
  diff contains no fixed-pair record, pair images, or provisioner.
- T-0383 independently reached the same result from the canonical review
  surface and made no source change.

No accepted evidence supplies two distinct exact identities with predecessor
generation lower than current, exact immutable bytes, SHA-256/length, reviewed
source identity, independently validated review session/record/authority
digest, and reviewed-build attestation ID/digest. Therefore the apparent gap
is resolved as absent authority, not an authority recoverable from session
status alone and not a basis to infer a continuity defect in product state.

### Trust invariants

Only a complete, independently accepted fixed pair with every binding above
could be canonicalized. Generic `COMPLETED_VERIFIED`, verification output,
session-only records, generic final-review labels, review prose, a stale or
mismatched descriptor, or replayed/unreviewed artifacts remain insufficient.
No caller choice may select an image, path, generation, hash, review identity,
or build attestation. T-0384 creates no provider, writer, signer, provenance,
or runtime authority.

### Exact next boundary

A separate accepted authority ticket must establish the missing fixed,
product-owned predecessor/current pair and its exact reviewed provenance. Only
then may a narrowly scoped canonicalization or zero-choice provisioning design
be reviewed. Per-user bootstrap, supervisor activation, serving cutover,
target/wake migration, and Secure MCP mutation remain outside this ticket.

## Source and test decision

No source or test change is made. A regression that treats a session's
completion state as durable pair authority would violate the invariants above;
the existing T-0382 test-only classification regression already proves the
reader-only path has no provisioner. There is no canonicalization seam for
T-0384 to exercise for replay/idempotence because there is no exact accepted
pair to canonicalize.

## Evidence paths

- `docs/orchestrator/review_bundles/T-0381_T0324_SR3C_FIXED_ORDINARY_WORKER_ARTIFACT_PROVIDER_REVIEW_BUNDLE.md`
- `docs/orchestrator/review_bundles/T-0382_T0324_SR3D_FIXED_ORDINARY_WORKER_PAIR_PROVISIONING_REVIEW_BUNDLE.md`
- `.catdesk/autonomy/adc-t0382-r1-t0324-sr3d-fixed-ordinary-worker-pair-provisioning-20260911/{contract,plan,state,events}.json*`
- `.catdesk/autonomy/adc-t0382-r1-t0324-sr3d-fixed-ordinary-worker-pair-provisioning-20260911/artifacts/completion.json`
- `.catdesk/autonomy/diagnostics/codex-autonomy-adc-t0382-r1-t0324-sr3d-fixed-ordinary-worker-pair-provisioning-20260911-codex-cli-turn-1.json`
- `docs/orchestrator/review_bundles/T-0383_T0324_SR3E_FIXED_ORDINARY_WORKER_PAIR_HOST_LIVE_ACCEPTANCE_REVIEW_BUNDLE.md`

## Verification and attribution

T-0384-attributable content is limited to this reconciliation bundle. The
dirty worktree, prior T-0381/T-0382 sources, and earlier review bundles remain
unattributed. The authoritative attributable diff is one new documentation
file (107 additions, 0 deletions): this bundle; it contains no product source,
test, runtime, or protected state change.

Verification after readback:

- `cargo fmt --all -- --check` — PASS.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` —
  PASS.
- `cargo test --workspace --all-targets --all-features` — PASS: 919 primary
  unit tests passed; target-specific test binaries also passed (with the
  existing explicitly ignored tests remaining ignored).
- `git diff --check` — PASS; no whitespace errors. Output contained only
  pre-existing working-copy CRLF warnings.

## Prohibited-action audit and residual risk

No ProgramData or user-release mutation, release prepare/activate/commit,
worker launch, supervisor switch, browser/wake/target change, Secure MCP
change, signing/UAC action, Git publication, or external-project action was
performed. The residual risk is intentionally fail-closed: no product-owned
accepted exact pair exists for the T-0381 carrier, so host bootstrap and live
cutover remain unauthorized.
