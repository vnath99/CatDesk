# T-0312 — Core Host Gate Evidence Ledger Integration

## Scope and decision

T-0312 is repository-only integration of the accepted T-0311-R1 authority.
It does not capture a host observation or perform a host, browser, wake,
tunnel, GUI, daemon, or supervisor action. Live truth is unchanged: T-0224 is
accepted; T-0223 is `OPERATOR_BOOTSTRAP_REQUIRED`; T-0222/T-0139, T-0152, and
T-0155 are unaccepted.

T-0311 is accepted only through T-0311-R1. The original T-0311 generic
acknowledged-completion / caller-selected-workspace defects remain historical.

## Architecture and authority boundary

`src/core_host_gate_evidence.rs` is the only ledger/finalizer module. It
accepts a typed `CoreHostGateApprovalRequestV1`, typed `CoreHostGateApprovalV1`,
and exact `CoreHostGateReviewAuthorityV1`; it recomputes canonical request
SHA-256 and invokes the T-0311-R1 receipt validator before it forms a record.
The finalizer cannot accept a boolean, prose, path, command, provider output,
uploaded receipt bytes, wildcard gate, arbitrary gate string, screenshot, or
generic review record.

The fixed gates remain exactly `T0223`, `T0222`, and `T0152`. A record binds:

- schema 1, `CatDesk`, `catdesk`, and `core-host-gate-evidence-v1`;
- the complete canonical T-0311-R1 request: gate, product-derived observation
  ID/digest, canonical T-0224 session/review record, current target SHA-256,
  runtime build/generation/host-session, issue time, and revision;
- canonical request SHA-256, the exact purpose-bound reviewer authority, and
  the exact fixed `APPROVE` receipt; and
- finalization timestamp.

The authority chain is immutable reviewed typed artifact -> canonical request
digest -> remeasured acknowledged independent-review authority -> exact
approval receipt -> canonical ledger record. ACK-only, ordinary
`independent_final_review`, provider/GUI/queue/wake/source/promotion/build
state, or a caller assertion has no path into this chain.

## Storage and reader

Production resolves only the compiled root
`C:\ProgramData\CatDesk\CoreHostGateEvidence` and fixed names
`t0223.json`, `t0222.json`, and `t0152.json`. There is no production root,
directory, filename, or workspace parameter. `ProtectedDirectoryGuard` begins
from pinned ProgramData and descends fixed CatDesk/CoreHostGateEvidence
components with no-follow semantics. Writes use an internally generated
temporary sibling, flush, atomic replace, then protected reopen plus exact
canonical-byte revalidation. Tests use the private `cfg(test)` temporary-root
seam only; no real ProgramData state was read or written.

The reader is strictly read-only. Missing root/file remains Missing; malformed,
oversized, noncanonical, substituted, or invalid authority/receipt data is
Invalid. It projects only project/session/T-0224-record/target fields into
`read_fixed_core_host_acceptance_preflight`; that existing T-0293 evaluator is
still the sole acceptance engine.

## Replay and failure semantics

The exact same canonical record is idempotent. A lower revision is refused;
the same revision with different bytes is a conflict; cross-gate reuse of the
same request or reviewer review record is refused. Receipt validation rejects
changed purpose/product/project/gate/observation/T-0224 tuple/current target/
build/generation/host-session/revision/request digest/reviewer authority.
Partial crash residue is inert because only the fixed final name is read; any
partial or malformed final file is Invalid, never accepted.

## Tests

Focused ledger tests pass:

- exact finalization, protected readback projection, and idempotence;
- changed observation at same revision conflict; lower revision rollback;
  cross-gate replay; and malformed final JSON -> Invalid.

The reused T-0311-R1 tests cover exact typed artifact positive; ACK-only,
generic completion, absent artifact, unrelated authority, noncanonical/wrong
domain, changed gate/observation/T-0224 tuple/target/build/generation/revision,
and receipt request-digest mismatch. Existing T-0293 tests retain missing and
invalid downstream evidence as non-accepting.

## Changed files and attribution

T-0312 attribution is limited to:

- `src/core_host_gate_evidence.rs` (new fixed ledger/finalizer/reader/tests);
- `src/core_host_acceptance_preflight.rs` (read-only ledger projection);
- `src/delegated/autonomy_supervisor.rs` (authority deserialization for
  canonical ledger validation);
- `src/main.rs` (module registration);
- `.catdesk/todo.md`, `CATDESK_MILESTONES.md`, `.catdesk/current_plan.md`, and
  this bundle.

All other dirty-worktree files pre-existed and are not attributed to T-0312.

## Verification

Passed after the one attributable clippy repair: `cargo fmt --all -- --check`;
focused approval/evidence/preflight/protected-FS tests (5 ledger/approval, 9
preflight, and 5 protected-FS tests); strict
`cargo clippy --workspace --all-targets --all-features -- -D warnings`; and
`cargo test --workspace --all-targets --all-features` (892 tests). `git diff
--check` is rerun at handoff.

The exact required isolated release-equivalent command was invoked four times:
`cargo build --release --locked --target-dir .catdesk/verification-targets/t0312`.
Each invocation used the same workspace-contained disposable target, reached
final `Compiling catdesk v0.1.6`, and was stopped by this worker's 120-second
execution ceiling before it emitted a compiler/linker result. This is not a
source failure or a deployable artifact. A verifier with a sufficient command
window must complete that exact command before independent acceptance; it must
not use or replace the locked default release executable.

### T-0312-R1 reconciliation addendum

The original controller terminal condition is historical runner/protocol state,
not a source defect: repeated earlier attempts were limited to 120 seconds.
Independent ChatGPT subsequently observed the exact same command exit 0 in
50.23 seconds. T-0312-R1 then reran the warmed command successfully (release
profile, exit 0, 0.32 seconds) and reran fmt, strict clippy, the 892-test suite,
and diff check successfully. That initial no-source-change conclusion is
superseded by the bounded R1 repair: a higher revision could reuse the same
review record for a changed request. `core_host_gate_evidence.rs` now refuses
that same-record replay and has a focused regression. The repair passes fmt,
strict clippy, and the full 892-test suite; its isolated release rebuild again
exceeds this worker's 120-second window before a compiler/linker result, so the
repaired candidate awaits the exact longer-window verification.

## Exact T-0313 boundary

T-0313 must define and implement only the fixed product-derived observation
producer/capture surface. It may feed a canonical observed request to this
ledger only after it has remeasured the supported lifecycle/GUI state. It must
not manufacture acceptance from tests, review metadata, GUI presentation,
queue state, provider output, or user-supplied evidence; it must not execute
the parked T-0223 host bootstrap or any live acceptance action.

### T-0312-R2 final-verification addendum

The R1 source-changing replay repair has now received the required exact
release-equivalent evidence: independent verification recorded
`cargo build --release --locked --target-dir .catdesk/verification-targets/t0312`
as exit 0 in 50.26 seconds, and R2 reran that exact command successfully
against the warm fixed target. R2 also reran fmt, strict clippy, the complete
892-test workspace suite, and `git diff --check` successfully. This corrects
only the earlier runner-window status; the isolated target remains disposable
verification evidence, never reviewed-image, promotion, or deployment
authority. No R2 product source changed. Independent final review remains
requested, and no live-gate status changed.

## Prohibited-action audit

No live ProgramData evidence, host observation, daemon reload, supervisor
activation, GUI launch, browser/wake, target/registry, Secure MCP/tunnel,
signing/provenance/dedicated-producer, external-project, or Git publication
action occurred.
