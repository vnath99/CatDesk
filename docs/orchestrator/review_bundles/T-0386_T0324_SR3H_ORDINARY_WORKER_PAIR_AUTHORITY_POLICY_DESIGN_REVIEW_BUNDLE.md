# T-0386 — T-0324 SR3H ordinary-worker pair authority policy design

## Technical design and classification

**Classification: `OPERATOR_ORDINARY_WORKER_TRUST_DECISION_REQUIRED`**

### Candidate authority source and domain separation

T-0385 establishes that the required pair is absent. The only accepted
independent-review mechanism relevant to a future pair is the exact
acknowledged-completion remeasurement in
`AutonomousSupervisorV1::resolve_reviewed_promotion_review_authority`.
That mechanism validates a unique acknowledged `COMPLETED_VERIFIED` record,
matching contract, completed verification, immutable task-output membership,
and current output remeasurement. It is necessary review evidence, not a
blanket release authorization.

The existing `core-host-gate-approval-v1` resolver demonstrates why purpose
separation is mandatory: generic remeasurement is insufficient until one fixed
reviewed artifact, itself a current immutable completion output, parses and
binds the exact canonical request for that distinct purpose. Its approval
cryptography and generic review metadata therefore do not authorize an
ordinary-worker pair.

The proposed future policy domain is
`catdesk-ordinary-worker-pair-approval-v1`, with fixed product `CatDesk`,
project `catdesk`, fixed role `catdesk-ordinary-worker-v1`, and one fixed
reviewed artifact identity such as
`src/ordinary-worker-pair-approval-v1.json`. This is design vocabulary only:
it is not a present artifact, source schema, authority resolver, provider,
signature, or production input.

### Required canonical approval artifact and digest

If an operator explicitly approves this new purpose, the artifact must be
strictly typed, canonical, bounded, and part of the selected reviewed
completion's immutable artifact set. It must carry a canonical pair-request
digest over all of the following fixed fields:

- schema/product/project/purpose/fixed ordinary-worker role;
- exact predecessor and current descriptor identities, with predecessor
  generation strictly lower than current and distinct immutable image
  SHA-256/length pairs;
- each descriptor's reviewed-source snapshot ID;
- each descriptor's independently validated review session ID, record ID,
  authority digest, contract hash, completion-review digest, and remeasurement
  digest; and
- each descriptor's reviewed-build attestation ID/digest and attestation
  candidate SHA-256/length, equal to its immutable image measurement.

The future resolver would first perform the existing acknowledged-completion
remeasurement, then require exactly one current artifact with that fixed ID,
verify its immutable completion membership and artifact SHA-256, parse only
the bounded typed shape, recompute the pair-request digest, and derive a
purpose-bound reviewer-authority digest. A T-0381-compatible provider could
consume only that exact authority plus those exact reopened bytes. Neither the
review record nor the artifact is a caller choice.

### Replay, conflict, and hostile refusal matrix

| Input or state | Required result |
| --- | --- |
| Generic ACK or unrelated `independent_final_review` completion | Refuse: workflow/review state is not pair approval. |
| Stale/replayed review or changed canonical pair request | Refuse: remeasurement, completion membership, and pair-request digest diverge. |
| Same generation or identical predecessor/current image | Refuse: ordering/distinctness invariant fails. |
| Mismatched source/review/attestation identity or attestation candidate measurement | Refuse: descriptor provenance is incomplete or divergent. |
| Signed main-image envelope, bootstrap/rotation receipt, Program Files image | Refuse: separate role/domain. |
| Caller-selected path, hash, generation, role, review, or attestation | Refuse: no caller choice exists in the future production API. |
| Partial pair, malformed/noncanonical artifact, unknown field, or reparse substitution | Refuse before any carrier or user-release transition. |

Exact replay may only be idempotent when the immutable artifact bytes,
canonical request digest, review remeasurement, both descriptors, and their
open-file measurements all match. Lower generation, same-revision changed
bytes, or any one-sided pair state must fail closed.

### Routine operating model and exact next boundary

After a separate operator trust decision and independent implementation review,
routine ordinary-worker releases could use this fixed reviewed-pair authority
without recurring UAC or a signing clerk: the trusted current-user release
store would only adopt an already accepted predecessor/current pair. This does
not alter product-root signed main-image bootstrap/rotation, which remains a
separate exceptional authority.

The exact next bounded action is an operator trust-policy decision that either
explicitly authorizes this new ordinary-worker-pair purpose and its fixed
artifact/request schema, or retains the current missing-prerequisite state.
Only after that decision may a separate implementation ticket add the typed
resolver and deterministic hostile regressions.

## Authority matrix

| Surface | Available now | Pair authority? |
| --- | --- | --- |
| T-0374-R1 release state | Atomic current/previous persistence and validation | No; validates supplied releases only. |
| T-0380 diagnostics | Fixed read-only absence/refusal projection | No. |
| T-0381 carrier | Fixed no-follow pair reader | No; consumes an already accepted pair. |
| Generic acknowledged completion remeasurement | Exact review/completion/output evidence | Necessary but insufficient. |
| `core-host-gate-approval-v1` artifact/resolver | Exact authority for three fixed host gates | No; different fixed purpose and request domain. |
| Reviewed main-image/bootstrap/rotation | Signed main-image authority | No; separate role/domain. |

## Source and test decision

No source or test change is made. No already-accepted authority explicitly
covers `catdesk-ordinary-worker-pair-approval-v1`; therefore an implementation
would create, rather than repair, authority. Existing T-0381/T-0382 and
core-host-gate regressions demonstrate the relevant fail-closed patterns, but
they cannot prove a policy that has not been authorized.

## Verification and attribution

T-0386-attributable content is limited to this design bundle. It creates no
key, signature, producer, pair artifact, protected-host state, user-release
mutation, worker launch, serving switch, live gate acceptance, or external
runtime change. The authoritative attributable diff is one new documentation
file (135 additions, 0 deletions): this bundle.

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

No ProgramData/user-release write, supervisor change, browser/wake/target
action, Secure MCP/tunnel action, Program Files mutation, UAC/signing action,
Git publication, or external-project mutation occurred. The residual risk is
deliberately fail closed: absent an operator policy decision and exact reviewed
pair artifact, nothing may populate T-0381 or bootstrap a user release.
