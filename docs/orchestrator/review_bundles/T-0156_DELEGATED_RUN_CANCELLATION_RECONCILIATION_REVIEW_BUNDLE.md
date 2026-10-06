# T-0156 Delegated-Run Cancellation Reconciliation Review Bundle

## Purpose

T-0153-R1 exposed a durable lifecycle deadlock: a delegated Qwen worker terminated after a provider continuation failure while the run remained `CANCEL_REQUESTED`, leaving the workspace reserved and preventing the next reliability ticket from starting. The preserved T-0153-R1 journal contained only completed read/search activity and no `OUTCOME_UNKNOWN` mutating tool call, so indefinite ownership was not justified by mutation uncertainty.

## Defect and authority model

`delegated_run_cancel` previously treated cancellation as only an in-memory flag plus a durable `CANCEL_REQUESTED` state. Workspace ownership was normally released only when the spawned worker returned through its terminal worker path. A stranded or restarted run could therefore remain `CANCEL_REQUESTED` forever even when no uncertain mutation existed.

The corrected contract is:

- Terminal registry states (`COMPLETED_VERIFIED`, `FAILED`, `CANCELLED`, `NEEDS_SUPERVISOR`) are returned unchanged by a later cancel request. A late cancel cannot overwrite a worker terminal result.
- While an active-run lock still exists in the current daemon, cancel remains `CANCEL_REQUESTED`; CatDesk does not guess that an in-process worker is dead.
- A repeated cancel with no active lock reconciles journal evidence. If there is no `OUTCOME_UNKNOWN` tool call, the durable run becomes `CANCELLED`. If journal evidence is unreadable or contains `OUTCOME_UNKNOWN`, the run becomes `NEEDS_SUPERVISOR` instead of being released.
- On registry rehydration after daemon restart, a prior-process worker cannot still exist. A durable `CANCEL_REQUESTED` run is therefore reconciled from journal evidence: read-only/fully-known history becomes `CANCELLED` and the stale workspace lock is removed; uncertain mutation becomes `NEEDS_SUPERVISOR` and the lock is retained.
- The state machine now explicitly permits `CANCEL_REQUESTED -> NEEDS_SUPERVISOR`, which is required for the fail-closed uncertainty path.

This preserves one-writer-per-workspace and does not use elapsed time, provider prose, or absence of a provider process as mutation authority.

## Task-attributable implementation

### `src/mcp.rs`

1. `delegated_run_cancel` now short-circuits terminal registry states so cancellation cannot overwrite an already committed worker result.
2. A cancel request with an already-absent active lock re-reads the durable tool-call journal. Known-safe history is finalized as `CANCELLED`; `OUTCOME_UNKNOWN` or journal-read ambiguity is finalized as `NEEDS_SUPERVISOR`.
3. `rehydrate_registry_entry` now reconciles durable `CANCEL_REQUESTED` after restart. A safe journal becomes `CANCELLED` and removes the stale `active-run.lock`; uncertain history becomes `NEEDS_SUPERVISOR` and retains ownership evidence.
4. Added production-path restart tests reproducing the T-0153-R1 shape and the mutation-uncertainty negative case.

### `src/delegated/state_machine.rs`

`CANCEL_REQUESTED -> NEEDS_SUPERVISOR` is now a valid fail-closed transition, with deterministic transition coverage.

### `src/delegated/patch_engine.rs`

During the required `-D warnings` verification, an unrelated pre-existing T-0144 working-tree change was exposed by Clippy (`len() != 0`). It was corrected mechanically to `!is_empty()` with no behavior change so the required full verification profile could complete. This is verification cleanup, not part of the T-0156 cancellation design.

## Deterministic regression coverage

- `cancel_requested_rehydration_reaps_read_only_stranded_run_and_stale_lock`
  - creates a durable run through RUNNING -> CANCEL_REQUESTED,
  - leaves a stale workspace lock,
  - records no uncertain mutation,
  - rehydrates as a fresh daemon would,
  - proves durable `CANCELLED` and lock removal.
- `cancel_requested_rehydration_with_unknown_mutation_escalates_and_keeps_lock`
  - creates a mutating `patch.apply` tool record through `OUTCOME_UNKNOWN`,
  - persists CANCEL_REQUESTED and a stale lock,
  - rehydrates,
  - proves durable `NEEDS_SUPERVISOR` and lock retention.
- Existing worker-outcome tests continue to prove:
  - clean cancelled worker -> `CANCELLED` + lock release,
  - clean failed worker -> `FAILED` + lock release,
  - cancelled/failed worker with `OUTCOME_UNKNOWN` -> `NEEDS_SUPERVISOR` + lock retention,
  - unreadable journal -> `NEEDS_SUPERVISOR` + lock retention.
- State-machine coverage now includes cancellation uncertainty escalation.

## Verification

- `cargo test cancel_requested_rehydration -- --nocapture` — PASS (2/2 new restart-reconciliation tests).
- `cargo fmt --check` — PASS.
- `cargo clippy --all-targets --all-features -- -D warnings` — PASS after the mechanical pre-existing Clippy cleanup described above.
- `cargo test` — PASS.
- `git diff --check` — PASS (Windows line-ending warnings only; no whitespace errors).

## Safety / non-actions

No live daemon reload, canonical promotion, browser wake, Secure MCP tunnel restart, Scheduler mutation, external-project mutation, or Git publication was performed while implementing or verifying T-0156. The externally owned official Secure MCP runtime remained attached and `CONNECTED_VERIFIED` during host inspection.

## Host acceptance sequence

1. Independently inspect the narrow T-0156 hunks and this bundle; do not use the repository-wide dirty diff as task attribution.
2. Build an isolated candidate containing T-0156 and load it through the reviewed daemon-reload path without canonical promotion.
3. Require local MCP READY and Secure MCP `CONNECTED_VERIFIED` with no duplicate tunnel runtime.
4. Query/reconcile the real `T-0153-R1`. Because its preserved history has no `OUTCOME_UNKNOWN` mutation, restart rehydration must produce `CANCELLED` and release the stranded workspace ownership.
5. Verify a new bounded delegated run can reserve the workspace. Do not mutate T-0153-R1 again after terminal reconciliation.
6. Only then start T-0154 canonical release identity + one-command recovery self-healing.

## Review conclusion

The source behavior and deterministic tests now cover the observed stranded-cancellation failure and its fail-closed mutation-uncertainty counterpart. T-0156 is ready for isolated-candidate host acceptance; it is not yet live-accepted until the real T-0153-R1 is rehydrated/reaped on the reviewed candidate and workspace reservation is proven available.