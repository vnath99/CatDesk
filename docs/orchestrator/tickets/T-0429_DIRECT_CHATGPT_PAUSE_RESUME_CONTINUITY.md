# T-0429 — Direct ChatGPT pause/resume continuity

## Status

INDEPENDENTLY_ACCEPTED_PENDING_SERVING_PARITY_AND_LIVE_T0425_ACCEPTANCE

## Trigger

Canonical Chat37 resumed the existing T-0425 direct-ChatGPT-owned session through `autonomy_session_resume`. T-0425 was PAUSED with a current task, `providerTurnCount = 1`, no provider thread/handle, `provider_route = WAITING_FOR_CHATGPT`, and its execution queue task still `WORKER_RUNNING`.

The generic resume path forced the session to `QUEUED` and invoked provider restart reconciliation, which returned `PENDING_RECOVERY`. That state cannot be re-claimed as new direct work and cannot be direct-finalized because finalization requires `WAITING_FOR_CHATGPT`.

The live diagnostic was immediately reversed with `autonomy_session_pause`; T-0425 is safely PAUSED at stateVersion 7.

## Scope

Repair only pause/resume state continuity for an already-owned direct ChatGPT task.

The direct-work marker is deliberately strict:
- prior state is `PAUSED`;
- `provider_route == WAITING_FOR_CHATGPT`;
- `providerTurnCount > 0`;
- `current_task_id` exists;
- provider thread and provider handle are absent; and
- the exact current queue task remains `WORKER_RUNNING`.

When all conditions hold, resume restores `WAITING_FOR_CHATGPT` and does not invoke provider restart reconciliation. All other resume cases retain the existing `QUEUED` plus restart-reconciliation behavior.

## Safety invariants

- Do not recapture or replace the task-output baseline.
- Do not create a second task/session.
- Do not launch a provider.
- Do not rewrite queue ownership.
- Do not weaken provider restart reconciliation.
- Do not alter Wake, target authority, external Secure MCP ownership, recovery/LKG authority, or Git publication state.
- T-0425 remains a separate Wake task and must not claim this control-plane diff as its own.

## Regression

A focused supervisor regression now constructs a paused direct-owned session, resumes it, and requires:
- state `WAITING_FOR_CHATGPT`;
- no `restartReconciliation` result;
- same current task;
- unchanged `providerTurnCount`;
- no provider thread/handle;
- `provider_route == WAITING_FOR_CHATGPT`;
- queue task remains `WORKER_RUNNING`; and
- `autonomy_session_finalize_direct_work` is still accepted afterward.

The existing interrupted-provider resume regression remains in place and must continue to prove provider-owned work uses restart reconciliation.

## Verification state

Verified on 2026-09-27 against the current dirty worktree:
- focused direct-resume regression passed;
- existing interrupted-provider resume/reconciliation regression passed;
- full `delegated::autonomy_supervisor::tests` module passed 28/28 before the additional refusal regression was added;
- strict `cargo clippy --bin catdesk -- -D warnings` passed;
- broad `cargo test --bin catdesk` passed 968, failed 0, ignored 22;
- `cargo build` passed.

A fail-closed refusal regression was then added for inconsistent paused-direct queue evidence. Current source refuses before mutation when the queue cannot prove the exact current task remains `WORKER_RUNNING`. A subsequent connector-level verification attempt timed out rather than returning contradictory test evidence.

Whole-worktree `cargo fmt --check` is not green because of unrelated pre-existing formatting drift in `src/reviewed_build.rs` around the `legacy_stdio_definitions.lib` diagnostic. That file is outside T-0429's attributable source change and must not be folded into this narrow repair merely to manufacture a global formatting pass.

## Acceptance

1. Focused supervisor tests including the new regression pass.
2. Existing provider restart/reconciliation tests remain green.
3. Formatting and strict Clippy pass.
4. Broader project verification passes or any unrelated/pre-existing failure is explicitly separated.
5. Independent review accepts the exact attributable diff.
6. Serving/current parity is established through existing reviewed controls.
7. Only then resume the SAME T-0425 and prove it returns from PAUSED to WAITING_FOR_CHATGPT without restart reconciliation.
