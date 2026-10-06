# T-0380 — T-0324 SR3B user-release bootstrap diagnostics

## Measured state and scope

The independently accepted T-0379 reader was executed twice through its
zero-argument host surface. Both measurements returned
`classification=USER_RELEASE_INVALID`, with `current_release=null`,
`previous_release=null`, and `supervisor=null`. The external official Secure
MCP runtime remains separately `CONNECTED_VERIFIED`/local-MCP-ready on the
legacy 77-tool serving generation. This ticket does not change that runtime,
the user-release store, supervisor state, or T-0378 cutover state.

## Diagnostic refinement

`ProtectedHostPrestateV1` retains the compatibility coarse classification and
now adds optional non-secret `release_diagnostic`. The zero-input inspector
can distinguish:

- `RELEASE_ROOT_UNAVAILABLE`
- `PROTECTED_PATH_OR_REPARSE_REFUSED`
- `ACTIVE_STATE_MISSING` or `ACTIVE_STATE_MALFORMED`
- `ACTIVE_STATE_CHANGED_DURING_READBACK`
- `CURRENT_IMMUTABLE_RELEASE_MISSING` or
  `CURRENT_IMMUTABLE_RELEASE_MALFORMED_OR_TAMPERED`
- `PREVIOUS_IMMUTABLE_RELEASE_MISSING` or
  `PREVIOUS_IMMUTABLE_RELEASE_MALFORMED_OR_TAMPERED`

The output contains no path, raw OS error, secret, caller-provided digest,
endpoint, or process ID. Both current and previous entries still require
protected no-follow reopening, exact manifest-document digest, strict manifest
schema/generation/review/attestation fields, and exact worker image
SHA-256/length. A changing `active-state.json` is refused rather than raced.

## Bootstrap authority decision

No production bootstrap writer was added. This is deliberate and fail closed.
The accepted `UserWorkerReleaseStoreV1` prepare/activate machinery is a
test/host-adapter primitive; it validates an already supplied manifest and
image but is not a provenance issuer. The workspace reviewed-build API requires
a caller-selected workspace, review session/record, authority digest, candidate
relative path, and candidate digest. It is therefore not a fixed
protected-host ordinary-worker artifact provider and cannot safely be reused
to bootstrap a missing user-release chain.

Likewise, the reviewed main-image bridge supplies a fixed signed-image digest
for its established role but does not supply the exact reviewed-source,
independent-review, and reviewed-build attestation identities required by the
ordinary-worker manifest. Reinterpreting either source as authority for two
ordinary-worker releases would expand trust by inference.

The required later bootstrap contract is consequently exact:

1. a separately accepted fixed product-owned provider must expose two exact
   independently reviewed ordinary-worker artifacts, predecessor then current,
   each with immutable bytes, SHA-256/length, reviewed-source identity, review
   session/record/authority digest, and reviewed-build attestation ID/digest;
2. a zero-choice host adapter may then prepare both immutable generations and
   atomically commit predecessor followed by current through the existing
   expected-generation CAS, leaving predecessor as the sole rollback pointer;
3. missing predecessor, changed same-generation bytes, unreviewed artifacts,
   stale expected generation, committed tamper, or reparse substitution must
   refuse. Only inert interrupted staging may be reconciled; committed
   authority is never silently repaired.

No generation or rollback predecessor is inferred by T-0380. Until that
fixed reviewed artifact provider is independently accepted, the bootstrap
operation remains unavailable rather than accepting caller-provided paths,
hashes, images, or review prose.

## Changed files

- `src/user_worker_release.rs`: bounded diagnostic taxonomy, protected reader
  error classification, and its focused taxonomy regression; the existing
  untracked file also contains the accepted T-0379 release/prestate lineage,
  which is not re-attributed as a T-0380 implementation change.
- This review bundle.

All other dirty-worktree content remains unattributed.

## Deterministic verification

- `cargo test user_worker_release -- --nocapture` — PASS: 16 release and
  diagnostic regressions, including malformed state, tamper, reparse, stale
  CAS, interrupted staging, replay/idempotency, exact rollback, zero-argument
  grammar, bounded/redacted output, and no mutating pipe/process/tunnel/wake
  path.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` — PASS.
- `cargo fmt --all -- --check` — PASS.
- `cargo test --workspace --all-targets --all-features` — PASS: main suite
  894 passed, 21 ignored; all target-specific test binaries passed.
- `git diff --check` — PASS. It emitted only pre-existing working-copy CRLF
  warnings and no whitespace errors.

## Prohibited actions and next boundary

No user-release or supervisor write, serving swap, activation, process action,
Program Files/ProgramData change, UAC/signing, target/wake/browser action,
Secure MCP/tunnel action, Scheduler/service action, Git publication, or
external-project mutation occurred.

After independent review, execute only the zero-argument diagnostic reader to
obtain the exact current subtype. A positive
`READY_FOR_REVIEWED_ACTIVATION` remains a prerequisite for any future cutover.
Live serving cutover and paired target migration remain unauthorized.
