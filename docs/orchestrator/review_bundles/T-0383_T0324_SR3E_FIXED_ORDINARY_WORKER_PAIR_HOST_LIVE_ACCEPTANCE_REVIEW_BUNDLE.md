# T-0383 — T-0324 SR3E fixed ordinary-worker pair host-live acceptance

## Technical design before implementation

### Current authority chain

1. T-0379 supplies a zero-choice, read-only protected-host prestate reader.
2. T-0380 recorded the measured fixed-root result
   `USER_RELEASE_INVALID/RELEASE_ROOT_UNAVAILABLE`; it forbids inventing a
   current/previous user-release chain.
3. T-0381 supplies a zero-choice, read-only carrier for exactly two ordinary
   worker artifacts at the compiled ProgramData root. It validates a pair only
   after an independently accepted producer has already placed it there.
4. The requested accepted T-0382-R1 provisioning artifact is absent from the
   durable review-bundle set. The available T-0382 SR3D bundle instead records
   `UPSTREAM_ARTIFACT_AUTHORITY_MISSING` and deliberately adds no provisioner.

### Exact SR3E boundary

The smallest safe SR3E boundary would normally be a read-only pair-adoption
preflight: reopen the fixed T-0381 predecessor/current pair, validate it, and
only then permit a later separately reviewed zero-choice per-user bootstrap to
prepare predecessor then current. That boundary is not implementable from the
accepted durable evidence here: there is no accepted fixed pair provisioner,
no exact pair, and no T-0382-R1 evidence that could authorize one.

T-0383 therefore selects **no source implementation**. Its fail-closed result
is `UPSTREAM_ARTIFACT_AUTHORITY_MISSING`, not a claim that the literal host
lacks a pair or that a future upstream mechanism is invalid.

### Fixed inputs, state invariants, and ownership

If the missing upstream authority later exists, the only paths remain the
compiled T-0381 ProgramData carrier, the trusted current-user release root,
and the fixed supervisor state. No caller-selected path, hash, generation,
review identity, endpoint, executable, or staging location is admissible.
The existing user-release state machine must retain atomic committed
current+previous state: predecessor generation is strictly lower than current,
both immutable manifest/image identities are revalidated before adoption, and
rollback retains only the exact predecessor. Partial one-worker pair state,
same-generation/digest drift, replay conflict, unsafe/reparse substitution,
or stale CAS must refuse before any user-release or supervisor mutation.

The reader, future bootstrap, and any later activation have separate roles.
T-0383 owns no host write, process, control-pipe, wake/browser, target,
Secure MCP, service, Scheduler, signing/UAC, Git, or external-project action.
The official Secure MCP runtime remains externally owned.

### Failure classifications and unaccepted claims

The only current classification is `UPSTREAM_ARTIFACT_AUTHORITY_MISSING`.
`FIXED_PAIR_PROVISIONER_READY` cannot be selected without the missing accepted
R1 evidence and exact pair; `FIXED_PAIR_PROVISIONER_INTEGRATION_DEFECT` cannot
be selected because no provisioner implementation is present to test.

Unaccepted: literal host pair presence, per-user bootstrap, a positive
T-0379 readiness result, serving cutover, supervisor activation, rollback
exercise, target migration, and event-driven wake parity.

## Source decision

No source or test code is changed by T-0383. Adding a preflight that treats an
absent upstream authority as an actionable pair source would either duplicate
the existing T-0381 reader or manufacture provenance. Existing T-0381/T-0382
hostile regressions already cover stale/mismatched/replayed artifact records,
partial pair membership, digest/attestation drift, and reparse refusal; the
existing user-release tests cover atomic current/previous activation, rollback,
and interruption residue. There is no additional safe transition exposed by
the missing authority to test here.

## Verification, attribution, and next action

Only this bundle is T-0383-attributable. Verification results:

- `cargo fmt --all -- --check` — PASS.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` — PASS.
- `cargo test --workspace --all-targets --all-features` — PASS: primary suite
  898 passed, 21 ignored; all target-specific test binaries passed.
- `git diff --check` — PASS; only pre-existing working-copy CRLF warnings,
  with no whitespace errors.

No literal host readback or mutation was attempted; repository evidence cannot
substitute for the absent T-0382-R1 authority artifact.

The exact next bounded action is to recover or independently establish the
missing accepted fixed upstream pair-provisioning authority and its exact
predecessor/current evidence. Only then may a separately reviewed zero-choice
pair-adoption/bootstrap preflight be designed; no live activation or cutover is
authorized by T-0383.
