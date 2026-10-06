# T-0180 / T-0154-R7D-R2 — Live Reviewer Ownership Reconciliation

## T-0179 false block

T-0179's shared restart reconciler treated every
`CatdeskVerificationReviewActive` accounting span as a current reviewer owner.
That incorrectly blocked a normal repaired task: an earlier verification span
had completed, a later Codex repair span was durably interrupted at restart,
and the queue remained `WORKER_RUNNING` despite no live reviewer.

## Exact reviewer-span status matrix

The shared `reconcile_restart_task_state` now treats reviewer evidence as:

| Reviewer span status | Restart result |
| --- | --- |
| `COMPLETED` | Historical evidence; does not block an otherwise exact interrupted-provider recovery. |
| `OPEN` | Live/unknown owner; returns `Pending`. |
| `INTERRUPTED` | Ambiguous reviewer outcome; returns `Pending`. |

All T-0179 provider requirements remain unchanged: one exact current-task
record, non-terminal provider turn, no truncated ledger, no tool-call
outcome-unknown evidence, canonical provider-session/thread match, an exact
interrupted provider lifecycle span, and no open activity span. The requeue
still changes only `WORKER_RUNNING` to `READY`.

## Deterministic tests

`src/delegated/autonomous_controller.rs` now covers:

- an historical completed reviewer span followed by a later interrupted Codex
  repair span; this requeues and preserves baseline bytes, task identity,
  contract/thread binding, repair accounting, and provider turn count;
- open reviewer plus interrupted provider remains pending; and
- interrupted reviewer plus interrupted provider remains pending.

Existing T-0179 coverage continues to assert missing/live/truncated/tool-call
and provider-thread mismatch evidence fails closed, while supervisor start and
resume use the same reconciliation helper without launching another provider
turn or rebinding the canonical Codex thread.

## Changed files

- `src/delegated/autonomous_controller.rs`
- `src/reviewed_source_snapshot.rs` — test-only serialization of existing
  process-global cleanup fault hooks. This fixes the independently observed
  full-suite flake without changing snapshot authority or production cleanup.
- `docs/orchestrator/review_bundles/T-0180_T0154_R7D_R2_LIVE_REVIEWER_OWNERSHIP_RECONCILIATION_REVIEW_BUNDLE.md`

## Verification evidence

Completed locally:

- `cargo test restart_reconciliation -- --nocapture` — 3 focused tests passed.
- `cargo fmt --check`;
- `cargo clippy --all-targets --all-features -- -D warnings`;
- `cargo test` — 614 passed, 18 ignored, plus 2 PowerShell recovery fixtures;
- configured `cargo build --release`; and
- `git diff --check`.

The working tree contains pre-existing unrelated changes; the classifier,
regressions, and this bundle are the T-0180-attributable changes. No live build
worker, promotion, reload, recovery fault injection, tunnel/browser/Scheduler
action, external-project mutation, or Git publication was performed. CatDesk
independently performs final verification and authoritative diff capture.
