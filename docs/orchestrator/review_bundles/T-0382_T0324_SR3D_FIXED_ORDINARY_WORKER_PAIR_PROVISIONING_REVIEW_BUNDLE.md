# T-0382 — T-0324 SR3D fixed ordinary-worker pair provisioning

## Classification

`UPSTREAM_ARTIFACT_AUTHORITY_MISSING`

## Authority analysis

The accepted T-0381 boundary is a fixed, read-only carrier. Its sole
production entry point reopens a canonical pair only after an independently
accepted producer has already placed `ordinary-worker-artifacts.v1.json`,
`predecessor.exe`, and `current.exe` beneath the compiled
`C:\ProgramData\CatDesk\ReviewedOrdinaryWorkerArtifactsV1` root. It does not
contain an upstream ordinary-worker artifact producer, a signed pair envelope,
an independently revalidated review-completion source, or an immutable source
from which it can derive predecessor/current bytes.

The accepted reviewed-build APIs remain deliberately workspace- and
caller-review-identity-bound. The signed reviewed-main-image bootstrap/rotation
authority is domain-separated and does not supply an ordinary-worker pair.
The missing per-user release root is likewise not a predecessor source.
Therefore none of those surfaces may be repurposed as a fixed pair
provisioner without manufacturing provenance or adding a new trust root.

No provisioner was implemented. In particular, T-0382 did not write
ProgramData, introduce caller-selected path/hash/generation/review identity,
create a signing or provenance authority, reinterpret generic independent
review acknowledgement, or perform a live release/runtime action.

## Deterministic classification regression

The T-0381 artifact-provider test module now pins this reader-only condition:
without an accepted fixed pair source it classifies
`UPSTREAM_ARTIFACT_AUTHORITY_MISSING` and verifies that the provider source has
no protected-FS atomic-write/create primitive. This is test-only; it adds no
production parameter, writer, or host authority.

Existing T-0381 regressions continue to cover a valid fixed pair in a private
temporary seam and refusal of missing members, stale/equal generations,
identical identities, tampered bytes, malformed review identity,
attestation/image mismatch, role confusion, noncanonical records, and reparse
substitution. They exercise no real ProgramData root and no runtime action.

## Attribution and verification

T-0382-attributable changes are limited to:

- `src/ordinary_worker_artifact_provider.rs`: one test-only classification
  regression; no production provisioner.
- This review bundle.

The existing untracked artifact-provider and user-worker-release sources carry
accepted T-0381 and earlier lineage and are not otherwise re-attributed. All
other dirty-worktree content remains unattributed.

Verification results:

- `cargo test ordinary_worker_artifact_provider -- --nocapture` — PASS: 4
  focused provider/classification tests.
- `cargo fmt --all -- --check` — PASS.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` — PASS.
- `cargo test --workspace --all-targets --all-features` — PASS: primary suite
  898 passed, 21 ignored; all target-specific test binaries passed.
- `git diff --check` — PASS; only pre-existing working-copy CRLF warnings,
  with no whitespace errors.

## Exact next boundary

A separate authority ticket must first establish an independently accepted,
fixed product-owned upstream source for the exact two ordinary-worker artifacts
and their provenance bindings. Only after that source is independently reviewed
may a separate zero-choice provisioner use protected no-follow atomic writes to
populate the T-0381 carrier. A later, separately reviewed per-user bootstrap
must consume that pair; live readiness, serving cutover, wake/target migration,
and Secure MCP mutation remain unauthorized.
