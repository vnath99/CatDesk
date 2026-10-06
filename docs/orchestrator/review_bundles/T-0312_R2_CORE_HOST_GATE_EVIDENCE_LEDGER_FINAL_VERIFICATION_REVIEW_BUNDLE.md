# T-0312-R2 Core-Host-Gate Evidence Ledger Final Verification

## Scope and result

T-0312-R2 is final verification and documentation reconciliation of the
existing T-0312/T-0312-R1 candidate. It makes no product-source change and
does not start T-0313. The original controller exhaustion was a command-window
event, not a source defect: its 120-second workers stopped the required
isolated release build after the R1 replay repair despite no compiler/linker
diagnostic. The repaired candidate is now verified locally and independently
with the exact fixed-target release command.

## Trust-boundary audit

The only approval route remains:

`CoreHostGateApprovalRequestV1` -> exact immutable reviewed typed artifact ->
canonical request SHA-256 -> revalidated `CoreHostGateReviewAuthorityV1` ->
fixed `APPROVE` receipt -> canonical ledger record.

The fixed purpose is `core-host-gate-approval-v1`; gates are only `T0223`,
`T0222`, and `T0152`. The request binds CatDesk/catdesk, observation identity
and digest, exact T-0224 session/record, current target SHA-256, runtime
identity, revision/time fields, and the independently completed/acknowledged
review artifact. The resolver in `src/delegated/autonomy_supervisor.rs` checks
the artifact membership and its exact request binding. Ordinary acknowledgement,
generic `independent_final_review`, provider, GUI, queue, wake,
source/promotion/build authority, caller booleans/prose, arbitrary gates,
paths, commands, screenshots, and uploaded receipt bytes cannot become host
gate acceptance authority.

## R1 replay-repair audit

`src/core_host_gate_evidence.rs` keeps the R1 same-gate replay guard: once a
ledger entry exists, a later revision with the same `review_record_id` is
refused. This closes the former possibility that a changed request (including
observation, target, T-0224 tuple, runtime identity, or authority identity)
could reuse a previous independent-review completion. The hostile regression
`mismatches_replay_and_noncanonical_protected_records_fail_closed` constructs a
changed observation at a higher revision with the earlier review record and
asserts finalization fails. Exact canonical repetition remains idempotent;
lower revisions, same-revision changed bytes, cross-gate and cross-context
replay remain fail-closed.

## Storage and read-only projection audit

Production selects the compiled fixed root
`C:\\ProgramData\\CatDesk\\CoreHostGateEvidence` and only
`t0223.json`, `t0222.json`, and `t0152.json`. Production finalization exposes
no caller-selected root, directory, filename, or storage path. The ledger uses
the pinned/no-follow `ProtectedDirectoryGuard` path, bounded regular-file reads,
atomic sibling replacement, protected reopen, and byte-for-byte canonical
revalidation. Alternate roots exist only in private `cfg(test)` seams; no real
ProgramData evidence was written.

The reader is read-only. It yields only validated fixed-ledger projections to
`read_fixed_core_host_acceptance_preflight`; missing or invalid records remain
non-authoritative. T-0293 remains the sole pure acceptance evaluator.

## Verification evidence

Independently completed post-repair verification recorded:

- `cargo build --release --locked --target-dir .catdesk/verification-targets/t0312`
  — exit 0, optimized release profile, 50.26 seconds;
- focused `core_host_gate_evidence` tests — exit 0;
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` — exit 0;
- `cargo test --workspace --all-targets --all-features` — exit 0 (892-test
  suite); and
- `git diff --check` — exit 0, with only pre-existing CRLF warnings.

R2 reran the required profile against the warm fixed target:

- `cargo build --release --locked --target-dir .catdesk/verification-targets/t0312`
  — exit 0, 0.31 seconds;
- `cargo fmt --all -- --check` — exit 0;
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` — exit 0;
- `cargo test --workspace --all-targets --all-features` — exit 0 (892 tests);
  and
- `git diff --check` — exit 0 (pre-existing CRLF warnings only).

The isolated target is release-equivalent compile/link evidence only. It was
not copied, promoted, deployed, or treated as reviewed-image authority.

## Attribution

- Original T-0312: `src/core_host_gate_evidence.rs`,
  `src/core_host_acceptance_preflight.rs`,
  `src/delegated/autonomy_supervisor.rs`, and `src/main.rs`.
- T-0312-R1: only `src/core_host_gate_evidence.rs` replay refusal and its
  hostile regression.
- T-0312-R2: `CATDESK_MILESTONES.md`, `.catdesk/current_plan.md`,
  `.catdesk/todo.md`, the original T-0312 bundle addendum, and this R2 bundle.

The extensive existing dirty worktree is preserved and is not attributed to
T-0312-R2.

## Live-state and prohibited-action audit

No host observation, production ProgramData write, daemon/release deployment,
supervisor activation, GUI/browser/wake/tunnel mutation, signing/provenance,
external-project action, or Git publication occurred. Live truth is unchanged:
T-0224 is accepted; T-0223 is `OPERATOR_BOOTSTRAP_REQUIRED`; T-0222/T-0139,
T-0152, and T-0155 are unaccepted.

## Exact next boundary

After independent final review accepts this candidate, T-0313 may implement
only a fixed product-derived host-observation capture surface. It must
remeasure supported lifecycle/GUI observations and feed only canonical
observations into this ledger. It must not infer acceptance from tests, review
metadata, GUI presentation, queue state, provider output, or caller input, and
it must not execute the parked T-0223 host bootstrap.

## Independent-final-review request

Verify the T-0311-R1 exact typed reviewed-artifact/request-digest chain, the
R1 same-review-record replay refusal and regression, fixed protected storage
with no caller path authority, read-only T-0293 projection, exact isolated
release evidence, and R2-only documentation attribution. Do not construe this
review as live-gate acceptance.
