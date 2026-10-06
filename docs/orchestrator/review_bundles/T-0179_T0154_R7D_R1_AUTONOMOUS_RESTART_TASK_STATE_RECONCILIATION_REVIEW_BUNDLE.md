# T-0179 / T-0154-R7D-R1 — Autonomous Restart Task-State Reconciliation

## Reproduced durable shape

The restart wedge was reproduced with the durable state shape reported by host
acceptance: the session is `QUEUED`, its current logical task is
`WORKER_RUNNING`, `providerTurnCount` is non-zero, and the canonical Codex
thread plus stale durable handle are retained.  In that shape no task is
dependency-ready, while the old controller-only recovery occurred after the
runtime host thread preflight.  A supported restart could therefore reject
with `no dependency-satisfied autonomous task is ready` before the controller
was able to inspect stale ownership.

## Reconciliation boundary

`reconcile_restart_task_state` in `src/delegated/autonomous_controller.rs` is
the one durable decision boundary. It is called by:

- `autonomy_runtime::start_or_tick` before host Codex preflight;
- `AutonomousControllerV1::run_once` before ready-task selection; and
- supervisor `start` and `resume` control mutations once they have durably
  placed the session in `QUEUED`.

The classifier changes only the current queue entry from `WORKER_RUNNING` to
`READY`. It never recreates or changes the task id, contract binding,
completion-artifact ids, baseline, provider turn count, repair count, canonical
thread, or provider history.

It requeues only when one exact accounting record for the current task proves
a `CodexProviderActive` / `ProviderLifecycle` span is `INTERRUPTED`, the
recorded provider session matches the bound canonical thread, and the ledger is
complete. Open provider/reviewer ownership, absent/multiple/corrupt evidence,
ended-task contradictions, thread drift, incomplete activity evidence, or any
unresolved tool-call evidence remain fail-closed. A persisted handle id alone
is intentionally not liveness evidence: it may be retained after an
authoritative interruption solely to resume the exact canonical Codex thread.

`PENDING_RECOVERY` is surfaced as a bounded supervisor response. Runtime
preflight returns a fixed reconciliation-pending error and launches no provider
when ownership is live or unknown. Exact replay after a successful requeue is
`AlreadyReady`; it neither increments a turn count nor launches another turn.

## Deterministic coverage

The controller regressions cover:

- the live `QUEUED + WORKER_RUNNING + INTERRUPTED` shape, exact canonical
  thread reuse, idempotent second reconciliation, and immutable baseline
  equality;
- an open provider span, which remains pending;
- interrupted provider evidence accompanied by a tool call, which remains
  outcome-unknown and pending; and
- the existing controller continuation regression, now seeded with the exact
  interrupted durable accounting record before it is allowed to requeue.

The durable ledger parser itself rejects corrupt records; missing, duplicate,
truncated, reviewer-owned, session-mismatched, and thread-mismatched evidence
all reach the same pending/fail-closed boundary without relaunch.

## Changed files

- `src/delegated/autonomous_controller.rs`
- `src/delegated/autonomy_runtime.rs`
- `src/delegated/autonomy_supervisor.rs`
- `docs/orchestrator/review_bundles/T-0179_T0154_R7D_R1_AUTONOMOUS_RESTART_TASK_STATE_RECONCILIATION_REVIEW_BUNDLE.md`

## Verification evidence

Completed in this provider workspace:

- focused `cargo test restart_reconciliation -- --nocapture`;
- `cargo test` — 613 passed, 18 ignored, plus 2 PowerShell recovery fixtures.
- `cargo fmt --check`;
- `cargo clippy --all-targets --all-features -- -D warnings`;
- configured `cargo build --release`; and
- `git diff --check`.

The working tree contains pre-existing unrelated changes; only the listed
restart-reconciliation files are attributable to this task. No live build
worker, promotion, reload, recovery fault injection, tunnel/browser/Scheduler
action, external-project mutation, or Git publication was invoked. CatDesk
independently performs final verification and diff capture.

R7C reviewed-source authority, R6/R6A/R6B promotion replay protections,
Qwen fallback semantics, wake behavior, redaction, external Secure MCP
non-ownership, and dirty-workspace compatibility are unchanged.
