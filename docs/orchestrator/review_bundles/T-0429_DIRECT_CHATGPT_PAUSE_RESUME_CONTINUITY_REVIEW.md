# T-0429 — Direct ChatGPT pause/resume continuity review bundle

## Review scope

T-0429 repairs one control-plane continuity defect discovered while resuming the already-owned T-0425 direct ChatGPT task. The implementation is limited to `src/delegated/autonomy_supervisor.rs` plus tests in the same module. It does not modify Wake, target authority, reviewed-build/LKG state, the external Secure MCP runtime, or T-0425's declared Wake outputs.

The pre-fix live diagnostic is preserved as evidence: T-0425 was PAUSED with `providerTurnCount=1`, no provider thread/handle, and a `WORKER_RUNNING` current task. Generic `autonomy_session_resume` changed it to QUEUED and returned `PENDING_RECOVERY`. The session was immediately re-paused and remains preserved.

## Implementation

`AutonomySupervisor::set_state` now distinguishes an exact paused direct-ChatGPT ownership shape before the generic QUEUED/restart-reconciliation path:

- prior state `PAUSED`;
- provider route `WAITING_FOR_CHATGPT`;
- provider turn count greater than zero;
- current task present;
- no provider thread ID;
- no provider handle ID; and
- the exact current queue task remains `WORKER_RUNNING`.

Only that exact shape resumes to `WAITING_FOR_CHATGPT` without provider restart reconciliation.

The queue check is fail closed before mutation. If the direct-owned candidate's queue cannot be read, resume returns `paused direct ChatGPT ownership queue evidence is unavailable`. If the current queue task is not exactly `WORKER_RUNNING`, resume returns `paused direct ChatGPT ownership queue evidence is inconsistent`. In either case no state mutation occurs.

All other resume cases retain the existing QUEUED plus `reconcile_queued_restart_task` behavior. The existing planner-gate refusal remains before either path.

## Regression coverage

Two T-0429 regressions are present:

1. `resume_preserves_paused_direct_chatgpt_ownership_for_finalization`
   - resumes to `WAITING_FOR_CHATGPT`;
   - no `restartReconciliation` output;
   - current task unchanged;
   - provider turn count unchanged;
   - provider thread/handle remain absent;
   - provider route remains `WAITING_FOR_CHATGPT`;
   - queue task remains `WORKER_RUNNING`; and
   - `autonomy_session_finalize_direct_work` remains accepted.

2. `resume_refuses_inconsistent_paused_direct_chatgpt_queue_without_mutation`
   - constructs the same durable direct-ownership markers but leaves the queue task non-running;
   - resume is rejected with the exact bounded inconsistency error;
   - state remains `PAUSED`;
   - state/event version is unchanged; and
   - queue state is unchanged.

The pre-existing `start_and_resume_share_interrupted_worker_reconciliation_without_launching` regression continues to prove provider-owned paused work still uses restart reconciliation.

## Verification evidence

- New direct-resume regression: PASS, 1 passed / 0 failed. Durable log: `.catdesk/logs/1790520817-fdd12401-baf1-4470-b5d7-d8e6a1ebd8a5.log`.
- Existing interrupted-provider resume regression: PASS, 1 passed / 0 failed. Durable log: `.catdesk/logs/1790520864-08eef1fc-a52f-4d95-b58d-88ee64bedea1.log`.
- Supervisor module before fail-closed tightening: PASS, 28 passed / 0 failed.
- New fail-closed queue regression: PASS, 1 passed / 0 failed. Durable log: `.catdesk/logs/1790521155-d95f307c-fd43-469e-9e39-1582be27e61d.log`.
- Supervisor module after final tightening: PASS, 29 passed / 0 failed. Durable log: `.catdesk/logs/1790521159-324d38df-203e-4668-b649-0fc7436def21.log`.
- Strict Clippy after final tightening: PASS, `cargo clippy --bin catdesk -- -D warnings`. Durable log: `.catdesk/logs/1790521172-e0ff510e-9c42-4ee9-846d-d697665adf66.log`.
- Full CatDesk binary suite before final tightening: PASS, 968 passed / 0 failed / 22 ignored. Durable log: `.catdesk/logs/1790520955-0e8f28d3-4dcb-4e48-bd0e-2a5ced14179e.log`.
- Full CatDesk binary suite after final tightening: PASS, 969 passed / 0 failed / 22 ignored. Durable log: `.catdesk/logs/1790521263-08c66877-a469-4b1f-8358-baf478af3ee3.log`. A second overlapping post-tightening run also completed PASS, 969/0/22, at `.catdesk/logs/1790521305-e502e2e6-55d0-4359-bd8b-b1c4261c7292.log`.
- `cargo build`: PASS after the final source shape.
- Standard `cargo fmt --check` remains globally red only because of a pre-existing formatting drift in `src/reviewed_build.rs` around the `legacy_stdio_definitions.lib` test fixture. The verifier reports that exact unrelated line; T-0429 does not edit `src/reviewed_build.rs`. Do not fold that unrelated source into this ticket merely to obtain a global green formatter.

## Security / authority review

The repair does not create new authority. Direct resume is narrower than generic resume because it requires a durable route/ownership/queue conjunction and rejects inconsistent evidence before mutation. It does not:

- launch or select a provider;
- recapture a task-output baseline;
- alter the current task ID;
- rewrite queue ownership;
- bypass planner gates;
- bypass restart reconciliation for provider-owned work;
- create a replacement T-0425 session;
- alter Wake or browser state;
- change project/Wake target authority;
- mutate reviewed-build, promotion, or LKG state;
- touch the external Secure MCP runtime; or
- grant Git publication authority.

## Review disposition

Implementation verification is complete for T-0429. The exact source shape is suitable for independent final review.

Do not use the source-only repair against live T-0425 until the reviewed serving generation contains it. After independent acceptance and serving/current parity, resume the SAME T-0425 and require `WAITING_FOR_CHATGPT` with no restart-reconciliation result before continuing T-0425 finalization.
