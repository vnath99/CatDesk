# T-0429-R1 — Direct resume continuity independent review

## Verdict

ACCEPT SOURCE LOGIC FOR REVIEWED SERVING HANDOFF.

This review is limited to the T-0429 pause/resume continuity change and its verification evidence. It does not authorize live T-0425 resume until serving/current parity contains the accepted source.

## Independent source review

The defect is real and the repair targets the correct boundary. Direct ChatGPT work is already durable execution ownership, not an unowned queued task. Returning such a PAUSED task to generic QUEUED restart recovery loses the semantic distinction required by `autonomy_session_finalize_direct_work`.

The accepted discriminator is narrow:

- prior session state is `PAUSED`;
- provider route is `WAITING_FOR_CHATGPT`;
- provider turn count is nonzero;
- current task exists;
- provider thread ID is absent;
- provider handle ID is absent; and
- that exact current queue task is `WORKER_RUNNING`.

The route criterion is important: current source sets `AutonomousProviderRouteV1::WaitingForChatgpt` when direct ChatGPT ownership is claimed. Combined with absent provider thread/handle and a durable running task, it distinguishes direct ownership from an interrupted Codex/Qwen provider task.

For that exact shape, restoring `WAITING_FOR_CHATGPT` is correct. It preserves the existing task/output baseline and returns the session to the state required by direct finalization. It does not mint a new claim, provider turn, task, or baseline.

## Fail-closed review

The final T-0429 source does not swallow queue read/mismatch errors.

A paused direct-owned candidate loads queue evidence before `self.mutate`. Missing queue evidence returns the bounded unavailable error; a non-`WORKER_RUNNING` exact task returns the bounded inconsistent error. Therefore neither failure can first rewrite the session to QUEUED.

This is preferable to treating incomplete direct ownership as provider restart evidence. Provider restart reconciliation is retained only for non-direct resume cases.

The internal `expect` on current task is downstream of the same immutable local snapshot's `current_task_id.is_some()` check, so it does not create a caller-controlled panic path between classification and dereference.

## Preserved behavior

The existing planner-gate requeue refusal still executes before resume classification.

The existing interrupted-provider regression continues through `reconcile_queued_restart_task` and passes. No provider-owned resume path is converted into direct ChatGPT ownership.

The repair does not:

- launch a provider;
- change provider turn count;
- create or replace a task-output baseline;
- replace current task identity;
- rewrite queue ownership;
- bypass a planner gate;
- bypass provider restart reconciliation for provider-owned work;
- mutate Wake/browser/target state;
- mutate recovery, reviewed-build, promotion, or LKG authority;
- touch the externally owned Secure MCP runtime; or
- grant Git mutation/publication authority.

## Verification reviewed

Durable evidence reviewed:

- exact positive direct-resume regression: PASS;
- exact inconsistent-queue refusal regression: PASS;
- interrupted-provider restart regression: PASS;
- complete autonomy-supervisor module after final source shape: 29 passed, 0 failed;
- strict `cargo clippy --bin catdesk -- -D warnings`: PASS;
- post-final-source full `cargo test --bin catdesk`: 969 passed, 0 failed, 22 ignored; durable log `.catdesk/logs/1790521263-08c66877-a469-4b1f-8358-baf478af3ee3.log`;
- a second overlapping post-final-source full run also records 969 passed, 0 failed, 22 ignored in `.catdesk/logs/1790521305-e502e2e6-55d0-4359-bd8b-b1c4261c7292.log`.

Global `cargo fmt --check` is not green because of an unrelated pre-existing formatting drift in `src/reviewed_build.rs` at the `legacy_stdio_definitions.lib` fixture. The standard verifier identified that exact unrelated diff. T-0429 does not alter that file and this review does not expand scope to normalize unrelated current-source work.

## Acceptance boundary

T-0429 source logic is accepted for the next reviewed serving handoff.

Before touching live T-0425:

1. finalize this review session through CatDesk verification;
2. obtain the resulting independent final-review record;
3. establish serving/current parity using the existing reviewed release controls;
4. call resume on the SAME T-0425 session;
5. require returned/live state `WAITING_FOR_CHATGPT`, unchanged current task/turn ownership, and no restart-reconciliation result.

Any failure of those conditions leaves T-0425 paused and blocks Wake continuation.
