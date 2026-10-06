# T-0352 — delegated CANCEL_REQUESTED reconciliation deployment-skew audit

Date: 2026-09-08

## Verdict

NO SOURCE REPAIR REQUIRED. The observed live T-0322 cancellation behavior is inconsistent with the already-present and passing current-source T-0156 reconciliation logic and is therefore classified as deployed-runtime/source skew. Do not delete the active lock, restart the externally owned runtime, or weaken fail-closed cancellation handling to clear it.

## Live observation

- Delegated run `T-0322` remains `CANCEL_REQUESTED` after an idempotent cancellation request.
- Its provider failed on the known Ollama continuation HTTP 500 after only two read operations.
- No mutating tool-call outcome and no `OUTCOME_UNKNOWN` condition was recorded for the run.
- The stale run nevertheless remains workspace-active and blocks creation of the genuinely separate T-0319 Qwen reviewer.

## Current-source audit

Current `src/mcp.rs` already contains the T-0156 cancellation-terminal reconciliation behavior and deterministic regressions for both sides of the safety boundary:

- `cancel_requested_rehydration_releases_stale_active_lock_and_allows_new_run` proves a stranded `CANCEL_REQUESTED` run with no uncertain mutation can be safely terminalized/reaped so the workspace lock is released.
- `cancel_requested_rehydration_with_unknown_mutation_needs_supervisor_and_blocks_new_run` proves uncertain mutation remains fail-closed as `NEEDS_SUPERVISOR` and continues blocking new work.

The focused current-source check `cargo test cancel_requested_rehydration -- --nocapture` passes. Historical T-0156 milestone/review evidence describes the same intended behavior.

## Classification

Because the live T-0322 control plane does not exhibit behavior that current source already implements and tests, adding another reconciliation implementation would duplicate established logic and risk weakening the mutation-uncertainty boundary. The safe conclusion is deployment parity failure: the serving canonical/control-plane generation is older than, or otherwise not exercising, the current T-0156 source behavior. This is consistent with the already-established T-0324 source-versus-canonical skew affecting the Qwen continuation fix.

## Dependency / next action

- Keep T-0319 open; this lineage must not self-accept the cumulative recovery chain.
- Route the stale delegated-run/control-plane parity issue through T-0324's reviewed build/promotion boundary rather than ad-hoc lock deletion or live runtime restart.
- After reviewed promotion/deployment parity is established, verify T-0322 reconciliation is observable in the serving control plane, then create the genuinely separate bounded T-0319 reviewer and run the Qwen continuation canary under T-0324.
- If the promoted generation still strands the same no-mutation `CANCEL_REQUESTED` case, reopen this as a concrete runtime defect with exact promoted-build evidence.

## Safety preserved

No product source/test mutation was made for T-0352. No live recovery, daemon/tunnel restart or migration, release promotion, browser wake, protected wake-target edit, Scheduler mutation, Git publication, active-lock deletion, or dirty-worktree cleanup occurred. The externally owned official Secure MCP runtime remains outside CatDesk ownership.
