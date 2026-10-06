# T-0385 — T-0324 SR3G fixed ordinary-worker pair authority establishment

## Technical design and classification

**Classification: `REVIEWED_ORDINARY_WORKER_PAIR_PREREQUISITE_MISSING`**

### Candidate upstream authority and domain separation

T-0384 is the authoritative current state: no accepted exact predecessor/current
ordinary-worker pair exists. This review considered only the accepted
reviewed-source snapshot and reviewed-build/producer-attestation mechanisms as
possible upstream evidence.

Those mechanisms are not an ordinary-worker pair authority:

- `create_or_validate_reviewed_source_snapshot(workspace, expected)` and
  `validate_committed_snapshot(workspace, expected)` bind immutable source
  bytes to a particular workspace and completion expectation. They do not
  select, build, or designate two ordinary-worker executables.
- `prepare_reviewed_build(workspace, review_record_id, authority_digest,
  expected)` and `validate_producer_attestation(workspace, session, record,
  authority_digest, expected, candidate_relative_path, candidate_sha256)` are
  host/workspace-scoped reviewed-build controls. Their attestation is for one
  candidate under that attempt's workspace-local
  `target/reviewed-builds/<attempt>/catdesk.exe`; it requires contextual review
  and candidate inputs and is not a fixed product-owned pair descriptor.
- T-0381's `read_fixed_ordinary_worker_artifact_pair()` is the correct
  zero-choice consumer, but it only validates a pair already supplied beneath
  its compiled protected root. It has no producer.

The reviewed-main-image/bootstrap/rotation authority is a separate signed
main-image role and is categorically excluded. Program Files images,
repository or `target/release` binaries, generic `COMPLETED_VERIFIED` state,
review prose, and session completion are not ordinary-worker provenance.

### Required future pair and zero-choice selection

The only valid future producer boundary must materialize exactly two fixed,
product-owned descriptors for the T-0381 schema: `predecessor` and `current`.
They must be distinct; predecessor generation must be lower than current; and
each must bind immutable reopened bytes, exact SHA-256 and length, reviewed
source snapshot identity, independently validated review session/record/
authority/contract/completion/remeasurement identities, and reviewed-build
attestation ID/digest and candidate measurement.

Production selection must be fixed by that accepted producer. No production
caller may choose a workspace, path, image, hash, generation, role, session,
record, review prose, or attestation. The pair's purpose and fixed
`catdesk-ordinary-worker-v1` role must remain distinct from main-image,
bootstrap, and rotation purpose domains.

### Hostile refusal cases

The absent prerequisite is retained for session-only or generic-review
evidence, stale or unreviewed source/build evidence, missing/mismatched
attestation, same generation, identical image, wrong role/domain, changed
bytes/hash/length, caller-selected values, partial pair state, or reparse
substitution. None of these can be converted to pair authority by
canonicalization or replay.

### Exact next boundary

A separate authority-producing ticket must establish an independently accepted
fixed ordinary-worker pair source that supplies both exact descriptors without
caller choice. Only after that source exists may a separate narrow
T-0381-compatible provisioning/canonicalization boundary be reviewed. This
ticket does not authorize bootstrap, activation, serving cutover, rollback
exercise, or target/wake migration.

## Source and test decision

No source or test change is made. The reviewed-source and reviewed-build APIs
cannot safely be made into a zero-choice pair producer by wrapping their
caller-selected workspace/review/candidate inputs; doing so would add a trust
root and violate T-0380 through T-0384. Existing T-0381/T-0382 deterministic
tests already refuse stale, malformed, same-generation, identical-image,
wrong-role/domain, attestation/image-mismatched, and reparse-substituted pair
records. T-0384 already records why session-only evidence cannot become pair
authority. There is no valid new production transition for T-0385 to test.

## Evidence chain

- T-0380 establishes that workspace-scoped reviewed-build authority and the
  signed reviewed-main-image role cannot bootstrap the missing user release.
- T-0381 defines the exact closed-world consumer and its required bindings.
- T-0382 records `UPSTREAM_ARTIFACT_AUTHORITY_MISSING`; it added no writer.
- T-0383 preserves that fail-closed source decision.
- T-0384 independently reconciles T-0382-R1 and records
  `FIXED_PAIR_AUTHORITY_ABSENT`, including the absence of any two exact
  accepted descriptors in persisted completion evidence.
- `src/reviewed_source_snapshot.rs` and `src/reviewed_build.rs` confirm that
  the candidate mechanisms are workspace/attempt scoped and cannot select two
  fixed ordinary-worker identities without a new authority decision.

## Verification and attribution

T-0385-attributable content is limited to this review bundle. There is no
product source, test, ProgramData, user-release, supervisor, process, or
external-runtime change. The authoritative attributable diff is one new
documentation file (121 additions, 0 deletions): this bundle.

Verification after readback:

- `cargo fmt --all -- --check` — PASS.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` —
  PASS.
- `cargo test --workspace --all-targets --all-features` — PASS: 919 primary
  unit tests passed; target-specific test binaries also passed, with existing
  explicitly ignored tests remaining ignored.
- `git diff --check` — PASS; no whitespace errors. Output contained only
  pre-existing working-copy CRLF warnings.

## Prohibited-action audit and residual risk

No real ProgramData write, user-release prepare/activate/commit, worker launch,
supervisor or serving switch, Program Files mutation, signing/UAC action,
browser/wake/target action, Secure MCP/tunnel action, Git publication, or
external-project mutation occurred. The remaining risk is deliberately fail
closed: without two exact independently accepted ordinary-worker artifacts,
the T-0381 carrier and all downstream bootstrap/cutover surfaces remain
unauthorized.
