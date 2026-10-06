# T-0381 — T-0324 SR3C fixed ordinary-worker artifact provider

## Scope and authority decision

T-0380 established the measured host condition
`USER_RELEASE_INVALID/RELEASE_ROOT_UNAVAILABLE`: no per-user release chain is
present, and neither a caller/workspace-parameterized reviewed-build API nor
the signed reviewed-main-image role may be repurposed as ordinary-worker
provenance. T-0381 adds the missing closed-world *reader* only. It creates no
artifact, signature, review decision, user-release state, supervisor state, or
live runtime change.

`ordinary_worker_artifact_provider.rs` provides exactly one production entry
point, `read_fixed_ordinary_worker_artifact_pair()`. It reopens only the
compiled `C:\ProgramData\CatDesk\ReviewedOrdinaryWorkerArtifactsV1` root on
Windows through pinned/no-follow `ProtectedDirectoryGuard` descent. There are
no production arguments for an image path, root, hash, generation, session,
record, review prose, endpoint, role, or workspace.

The root carries one bounded canonical record
`ordinary-worker-artifacts.v1.json` and two fixed child names:
`predecessor.exe` and `current.exe`. Missing production content is
unavailable/fail-closed; this ticket does not populate ProgramData.

## Exact provenance and domain separation

The canonical pair record is domain-separated as
`catdesk-ordinary-worker-artifact-provider-v1`, product `CatDesk`, and fixed
role `catdesk-ordinary-worker-v1`. It rejects unknown fields and noncanonical
bytes. Each descriptor binds:

- generation, immutable image SHA-256 and length;
- reviewed source snapshot ID;
- exact independently validated review session, record, authority digest,
  contract hash, completion digest, and remeasurement digest; and
- reviewed-build attestation ID/digest plus the attestation candidate
  SHA-256/length, which must equal the reopened image measurement.

The reader requires exact fixed predecessor/current artifact IDs, predecessor
generation lower than current, and distinct image identities. It recomputes
both image measurements through protected no-follow file opens before exposing
crate-private bytes to a future bootstrap boundary. Generic acknowledgement or
an `independent_final_review` label is not an input field and cannot act as
provenance. The workspace-scoped `validate_producer_attestation` API remains
unused because its caller-selected workspace and review inputs are not a safe
production authority. The signed reviewed-main-image/bootstrap/rotation role
remains separate and is not read by this provider.

## Refusal and mutation boundaries

The provider refuses missing predecessor/current, malformed/noncanonical
records, same or stale generation, identical images, bad image length/hash,
bad review/authority/attestation digests, attestation-to-image mismatch,
unexpected artifact ID, role/domain confusion, and protected reparse/path
substitution. It has no write, pipe, process, browser, wake, target, tunnel,
Program Files, UAC/signing, Scheduler/service, Git, or external-project path.

No production pair is claimed present and no live readiness or serving cutover
is claimed. The externally owned official Secure MCP runtime remains outside
this authority.

## Attribution and verification

T-0381-attributable changes are:

- `src/ordinary_worker_artifact_provider.rs` — fixed read-only pair carrier
  and deterministic hostile regressions.
- `src/user_worker_release.rs` — manifest validation visibility widened only
  to the crate-private provider, without changing its validation rules.
- `src/main.rs` — private module registration.
- This review bundle.

The pre-existing untracked `src/user_worker_release.rs` also contains the
accepted T-0374 through T-0380 release/prestate lineage; it is not otherwise
re-attributed to T-0381. All other dirty-worktree content is unattributed.

Verification results:

- `cargo test ordinary_worker_artifact_provider -- --nocapture` — PASS: 3
  focused pair/refusal/no-mutation tests.
- `cargo fmt --all -- --check` — PASS.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` — PASS.
- `cargo test --workspace --all-targets --all-features` — PASS: primary suite
  897 passed, 21 ignored; all target-specific test binaries passed.
- `git diff --check` — PASS; only pre-existing working-copy CRLF warnings,
  with no whitespace errors.

## Only permitted next boundary

Fresh independent final review must first accept this reader and the exact
provisioned pair. Only then may a separately reviewed zero-choice per-user
bootstrap consume the fixed predecessor then current descriptors, prepare the
two immutable generations, and atomically commit them through existing
generation-CAS release authority. It must not infer a predecessor, repair a
tampered committed state, activate a worker, or migrate the chat target.
