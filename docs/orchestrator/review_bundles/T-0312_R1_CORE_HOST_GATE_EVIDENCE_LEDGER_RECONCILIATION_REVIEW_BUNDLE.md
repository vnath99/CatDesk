# T-0312-R1 — Core Host Gate Evidence Ledger Reconciliation

## Outcome

T-0312-R1 initially classified the original controller terminal condition
correctly, then found one concrete replay defect in the candidate. The original controller reached
`repair_budget_exhausted` because the exact isolated release command repeatedly
outlived its then-120-second execution window. That timeout is not evidence of
a compiler, linker, trust-boundary, or ledger defect.

## Exact source attribution

Original T-0312 attribution remains limited to:

- `src/core_host_gate_evidence.rs` — fixed canonical ledger/finalizer/reader;
- `src/core_host_acceptance_preflight.rs` — read-only ledger projection;
- `src/delegated/autonomy_supervisor.rs` — T-0311-R1 authority deserialization;
- `src/main.rs` — module registration.

R1-only attribution is the one guard and regression in
`src/core_host_gate_evidence.rs`, plus this reconciliation bundle and the
minimal queue, milestone, plan, and original-bundle updates. The accumulated
dirty worktree is preserved and not attributed to T-0312-R1.

## Trust-boundary re-audit

The only gates are `T0223`, `T0222`, and `T0152`. Finalization requires the
canonical product-derived `CoreHostGateApprovalRequestV1`, the exact
T-0311-R1 request-bound `CoreHostGateReviewAuthorityV1`, and its exact
`APPROVE` receipt. Request, observation, T-0224 tuple, current target,
build/generation/host session, revision, request digest, and authority identity
are recomputed and must agree.

ACK-only, generic `independent_final_review`, provider, GUI, queue, wake,
source/promotion/build authority, caller boolean/prose, arbitrary gate,
screenshot, uploaded receipt bytes, path, and command have no authority path.
T-0293 remains the sole acceptance evaluator.

## Protected storage and replay audit

Production selects only the compiled
`C:\ProgramData\CatDesk\CoreHostGateEvidence` root and fixed
`t0223.json`/`t0222.json`/`t0152.json` child names. There is no production
root/directory/filename input. `ProtectedDirectoryGuard` pins the ProgramData
chain and uses no-follow handle-relative operations. Atomic temporary-sibling
replacement is followed by protected reopen and canonical-byte validation;
test-only temporary-root seams never access real ProgramData.

Exact canonical repeat is idempotent. Lower revisions, same-revision changed
bytes, cross-gate/cross-observation/cross-target/cross-T-0224/cross-runtime/
cross-reviewer reuse, malformed/oversized/noncanonical/partial state, root or
reparse substitution, and invalid receipt/authority remain fail-closed. The
reader is read-only and projects only validated project/session/T-0224 record/
target values into preflight.

### Bounded R1 repair

Before the repair, the same gate could accept a higher revision with a changed
request while retaining the previously consumed `review_record_id`. Although
the real T-0311-R1 resolver rejects that combination, the ledger must preserve
the same request-bound boundary itself. The finalizer now refuses any later
same-gate record that reuses the existing review record; the hostile regression
uses a changed observation and higher revision with the original review record.

## Verification evidence

Independent ChatGPT verification after the original escalation recorded:

- `cargo build --release --locked --target-dir .catdesk/verification-targets/t0312`
  — exit 0, optimized release profile, 50.23 seconds;
- `cargo fmt --all -- --check` — exit 0;
- strict clippy — exit 0; and
- full workspace all-target/all-feature tests — exit 0 (main suite: 892 tests,
  871 passing plus expected ignored/non-main test accounting).

Before the repair, R1 reran the exact warmed isolated command locally: exit 0,
optimized release profile, 0.32 seconds. After the repair, fmt, strict clippy,
and all workspace all-target/all-feature tests (892 tests) pass. The exact
isolated release command was rerun twice after the source change and each time
reached final crate compilation but exceeded this worker's 120-second window
without a compiler/linker diagnostic. The repaired candidate therefore awaits
that exact longer-window verification; no isolated output is deployment,
promotion, or reviewed-image authority.

## Live-state and next-boundary audit

No host observation, ProgramData production write, daemon/release action,
supervisor activation, GUI/browser/wake/tunnel mutation, signing/provenance,
external-project, or Git publication occurred. Live truth remains T-0224
accepted; T-0223 `OPERATOR_BOOTSTRAP_REQUIRED`; T-0222/T-0139, T-0152, and
T-0155 unaccepted.

T-0313, if separately approved, may define only a fixed product-derived
observation capture path feeding this ledger; it must not infer acceptance from
tests, reviews, GUI, queue, or provider state, and it must not start a host
action.

## Independent final-review request

Review the request-bound T-0311-R1 chain, fixed protected storage/no caller
path authority, fail-closed replay/read semantics, exact release verification,
and R1-only attribution. Do not treat this reconciliation as live-gate
acceptance.
