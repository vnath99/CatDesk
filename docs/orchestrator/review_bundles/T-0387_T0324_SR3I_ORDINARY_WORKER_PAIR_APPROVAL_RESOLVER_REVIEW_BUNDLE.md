# T-0387 — T-0324 SR3I ordinary-worker pair approval resolver

## Design before implementation

This ticket implements one reusable review-authority domain, not a per-release
trust mechanism. The fixed values are schema version `1`, product `CatDesk`,
project `catdesk`, purpose `catdesk-ordinary-worker-pair-approval-v1`, role
`catdesk-ordinary-worker-v1`, and immutable reviewed artifact identity
`src/ordinary-worker-pair-approval-v1.json`.

The canonical pair request contains exactly predecessor and current descriptor
objects. Each descriptor binds its fixed slot identity, generation, immutable
image SHA-256/length, reviewed-source snapshot ID, review session/record/
authority/contract/completion/remeasurement identities, and reviewed-build
attestation ID/digest plus exact candidate SHA-256/length. The canonical
request digest is SHA-256 of the deterministic JSON encoding after strict
shape validation. Predecessor generation must be lower than current; both
image identity pairs must be distinct.

Resolution reuses the acknowledged `COMPLETED_VERIFIED` completion
remeasurement. It additionally requires exactly one current completion output
at the fixed artifact identity, remeasures its membership and bytes, parses
only bounded canonical typed JSON, recomputes the request digest, and derives
a purpose-bound reviewer-authority digest. The resolver takes only workspace,
review-record ID, and the already typed request; no production API chooses
purpose, role, paths, hashes, generations, review identities, or attestations.

Generic ACK, unrelated review completion, stale/replayed output, changed
request, same/reversed generation, identical image, any provenance mismatch,
main-image or core-host artifact, caller choice, partial pair, malformed or
noncanonical data, and reparse/output drift must fail closed. Exact replay is
idempotent only when every artifact/request/review/measurement binding is
unchanged.

The implementation adds no pair writer, ProgramData/Program Files state,
per-user release mutation, signing key, UAC flow, worker/supervisor switch,
or external runtime ownership. Its next bounded action is to establish the
first exact independently reviewed approval artifact and pair bytes for later
T-0381-compatible provisioning.

## Implementation summary and audit trail

- `src/ordinary_worker_pair_approval.rs` defines the bounded typed request,
  descriptor, reviewed artifact parser/canonicalizer, request digest, and
  purpose-bound authority validator. It exposes no pair writer, storage root,
  executable path, command, or runtime operation.
- `src/delegated/autonomy_supervisor.rs` adds the exact read-only resolver.
  It begins with the existing acknowledged-completion remeasurement, requires
  one current output at the compiled artifact ID, hashes/re-hashes its bytes,
  verifies canonical typed binding, then independently revalidates each
  descriptor's prior reviewed completion, source snapshot, and reviewed-build
  attestation before deriving the pair-purpose authority digest.
- `src/main.rs` registers the private module only. No MCP tool or caller-facing
  selector is added.

The durable audit trail is the existing review inbox record, reviewed
completion artifact membership and measured output SHA-256, descriptor review
records and remeasurement digests, validated reviewed-source snapshots, and
validated reviewed-build attestations. The derived authority digest is scoped
to `catdesk-ordinary-worker-pair-approval-v1` plus one canonical pair request;
it is not a signature, pair byte object, provisioning receipt, or activation
authorization.

## Hostile regression matrix

| Case | Coverage/result |
| --- | --- |
| Unknown field, malformed JSON, trailing/noncanonical bytes | Typed parser refuses. |
| Wrong schema/product/project/purpose/role or cross-domain main-image role | Request/artifact validation refuses. |
| Changed source/review/attestation/image request fields | Canonical request digest and original typed artifact no longer bind. |
| Same/reversed generation or identical image | Pair relation refuses. |
| Attestation candidate length/hash mismatch | Descriptor validation refuses. |
| Generic ACK, stale/replayed review, changed/removal completion output | Existing supervisor remeasurement rejects before purpose resolution; the new resolver invokes that exact seam and rechecks fixed artifact output membership/bytes. |
| Descriptor review/source/build mismatch | Resolver independently revalidates review remeasurement, source snapshot, and attestation candidate measurement. |
| Caller-selected paths/hashes/generations/purpose/role | No production API field exists; source regression asserts no writer/runtime boundary in this module. |
| Partial pair or reparse-substituted future carrier | This resolver has no carrier access; T-0381 remains the protected no-follow consumer and refuses it separately. |
| Exact immutable replay | Same canonical artifact/request derives the same request digest; no state is written. |

## Source attribution, prohibited actions, and next boundary

T-0387 changes only `src/ordinary_worker_pair_approval.rs`,
`src/delegated/autonomy_supervisor.rs`, `src/main.rs`, and this bundle. Earlier
dirty-worktree source and T-0380 through T-0386 content remain unattributed.
The authoritative attributable diff was captured with `git diff --unified=0`:
the new typed module and this bundle are new files; the existing-file hunks are
limited to the private module declaration plus the pair authority type,
fixed-artifact resolver, and descriptor provenance remeasurement helper. No
unrelated dirty-worktree hunk is attributed to T-0387.
No fixed pair production write, ProgramData/Program Files or per-user release
write, worker/supervisor launch or switch, serving/wake/target/tunnel change,
browser action, Git publication, or external-project mutation occurred.

The exact next bounded action is to create and independently review the first
fixed `src/ordinary-worker-pair-approval-v1.json` completion artifact whose
two descriptor provenance chains and immutable pair bytes are already valid.
Only a separate T-0381-compatible fixed-pair provisioning ticket may consume
the resulting authority; bootstrap and live cutover remain unauthorized.

## Verification

Focused regressions passed:

- `cargo test ordinary_worker_pair_approval -- --nocapture` — PASS: canonical
  artifact/replay, malformed/noncanonical/cross-domain/refusal, changed-request
  authority binding, and no writer/runtime path tests.
- `cargo test reviewed_promotion_authority_revalidates_completion_output_evidence
  -- --nocapture` — PASS: inherited acknowledged-review output-drift/removal
  remeasurement seam used by the pair resolver.

Contract-approved verification after formatting:

- `cargo fmt --all -- --check` — PASS.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` —
  PASS.
- `cargo test --workspace --all-targets --all-features` — PASS: 922 primary
  unit tests passed; target-specific test binaries also passed, with existing
  explicitly ignored tests remaining ignored.
- `git diff --check` — PASS; no whitespace errors. Output contained only
  pre-existing working-copy CRLF warnings.
