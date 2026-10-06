# T-0389 — T-0324 SR3K first pair evidence bootstrap closure

## Technical design before source decision

### Bootstrap circularity and trust boundary

T-0387 deliberately resolves a pair-purpose authority only from a *prior*
acknowledged `COMPLETED_VERIFIED` record whose immutable completion membership
contains the fixed `src/ordinary-worker-pair-approval-v1.json` artifact.
T-0388 correctly refused to create that first artifact because no two exact
ordinary-worker descriptor chains were available. The circularity is not
resolved by allowing a prospective pair artifact to cite its own completion:
that would replace independent evidence with self-blessing.

The one standing domain remains
`catdesk-ordinary-worker-pair-approval-v1`; this ticket creates no per-release
domain, signer, review mode, caller selector, or live writer. A legitimate
first artifact can only be constructed from two *earlier* independently
accepted ordinary-worker outputs whose bytes and provenance already satisfy
the T-0387 descriptor model.

### Candidate prior evidence and fixed binding model

The only candidate mechanisms considered are the existing reviewed-source
snapshot, reviewed-build attestation, and acknowledged-completion output
remeasurement seams. Each future fixed slot would need all of the following:

- an immutable ordinary-worker output whose SHA-256 and length are remeasured;
- a reviewed-source snapshot identity revalidated against that output's prior
  review record;
- the exact prior review session, record, authority, contract, completion, and
  remeasurement identities;
- a reviewed-build attestation ID/digest whose candidate SHA-256/length equals
  that same immutable output; and
- a fixed predecessor/current relation with strictly increasing generation and
  distinct image identities.

Selection must be product-owned and zero-choice. No caller may select an
output path, hash, generation, session, review, attestation, role, or purpose.
Exact replay would be idempotent only when all of those immutable bindings
match; changed bytes or any identity drift is a conflict, not a repair.

### Classification

**`FIRST_PAIR_EVIDENCE_PREREQUISITE_MISSING`**

The single concrete prerequisite is **two prior independently accepted,
remeasurable ordinary-worker reviewed-build outputs with complete per-output
source/review/attestation bindings**. Neither accepted T-0387 nor T-0388
contains such an output: both persisted contracts/plans declare no completion
artifact IDs; the fixed pair JSON is absent; and the workspace has no
`target/reviewed-builds` evidence root to remeasure. Their generic final-review
payloads are not descriptors or immutable output authority.

This is not an integration defect. T-0387 already has a bounded typed request
and revalidation path for valid prior evidence. It would be authority expansion
to change that path to accept historical task names, generic completion state,
review prose, `target/release`, Program Files, main-image/bootstrap data,
core-host data, or any caller-provided values.

## Source decision

No source or test change is permitted or needed. No
`src/ordinary-worker-pair-approval-v1.json` artifact is created. The existing
T-0387 hostile coverage continues to refuse self-reference/same-session
blessing, stale or replayed review output, same or reversed generation,
identical images, changed canonical bytes, source/review/attestation mismatch,
caller choice, and main-image/core-host cross-domain artifacts. Adding an
integration that manufactures either missing descriptor would defeat those
invariants.

## Prohibited-action audit

No release-store or ProgramData/Program Files write, pair provisioning,
worker/supervisor/process action, serving/wake/target/tunnel change, browser
action, signing/UAC operation, Git publication, or external-project mutation
occurred.

## Verification and next action

Repository verification after this documentation decision:

- `cargo fmt --all -- --check` — PASS (only the pre-existing environment
  canonicalization warning was printed).
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` —
  PASS (same warning only).
- `cargo test --workspace --all-targets --all-features` — PASS; the primary
  unit-test run enumerated 922 tests and all target-specific test binaries
  completed successfully.
- `git diff --check` — PASS; pre-existing working-copy CRLF warnings only and
  no whitespace errors.

The attributable diff must be this bundle only.

The exact next bounded action is not another audit: establish two prior,
independently accepted ordinary-worker reviewed-build outputs with the complete
fixed descriptor bindings above. A later ticket may then construct the first
canonical pair artifact and, only after independent acceptance, a separate
ticket may invoke the T-0387 remeasurement resolver.
