# T-0035 R4 planner-reply continuation dispatch review bundle

## Live failure evidence and root cause

Durable live evidence is preserved in `.catdesk/current_plan.md` and the
failed T-0081 session `adc-t0035-live-dag-acceptance-20260814`:

- the session reached `WAITING_FOR_CHATGPT` at event 9;
- the supported `autonomy_session_reply` was accepted at state version 9;
- durable state records `planner_reply:9`, event 10 `planner_reply`, the
  matching gate satisfaction, and `QUEUED` state at version 10.

The source path explained why work did not then continue. In
`autonomy_supervisor::reply`, the accepted reply transaction writes the
bounded reply, satisfies its approved gate, changes the session to `QUEUED`,
and appends `planner_reply`. `ensure_reviewer_wakeup` exits when its ticker
observes `WAITING_FOR_CHATGPT` and removes the live wakeup key. Meanwhile
`handle_autonomy_supervisor_mcp_tool` previously invoked the runtime only for
`autonomy_session_start`; reply and resume returned after durable mutation.

No failed T-0081 canary was resumed or otherwise mutated by this task.

## R4 continuation behavior

`rearm_accepted_queued_session` is a CatDesk-local runtime primitive. It
accepts only workspace/session identity, reloads durable state, allows only
`QUEUED`, `RATE_LIMITED`, or already-live `RUNNING` sessions, and delegates to
the existing serialized `start_or_tick` controller path. It accepts no MCP
executable, authentication, environment, model, sandbox, or provider inputs.
The established operator-local executable discovery, exact captured Codex
thread preflight, Terra/High gate, same-thread recovery, and confirmed-credit-
exhaustion-only Qwen route remain authoritative.

The MCP reply/resume branch is deliberately durable-first:

1. `handle_autonomy_mcp_tool` completes its locked validation and persistence.
2. Only a successful result calls the local continuation primitive.
3. The response reports `continuation.status = TICKED` with bounded controller
   state/events, or `PENDING_RECOVERY` with a bounded recoverable reason.

Rejected stale-version, escalation-ID mismatch, state/gate/graph mismatch, or
persistence failure never reaches runtime/provider initialization. If local
continuation initialization fails after acceptance, the reply/gate/event stay
durable and the structured `PENDING_RECOVERY` result makes recovery explicit;
the control-plane action is never presented as rolled back.

Normal `autonomy_session_resume` now uses the same continuation path after its
successful `QUEUED` transition. Its existing guard still rejects a resume that
would bypass a planned ChatGPT gate.

## Idempotency and wake-loop safety

The runtime registry continues to serialize `run_once`, so a replay or
concurrent accepted re-arm cannot concurrently own a provider turn. The
reviewer wakeup registry now stores a generation per live loop. Re-arm bumps
that generation before the controller tick. A loop that had just observed
`WAITING_FOR_CHATGPT` detects the changed generation and continues instead of
removing the wakeup registration. If it has already removed the key, the new
tick installs a fresh loop. Either ordering leaves one serialized runtime
owner, not duplicate provider/task execution.

Restart/recovery remains in `start_or_tick`: a rehydrated controller runs the
existing interrupted-session recovery and preserves the established durable
queued recovery semantics before task selection.

## Changed files

- `src/mcp.rs` — durable-first reply/resume dispatch and bounded recoverable
  continuation result.
- `src/delegated/autonomy_runtime.rs` — shared local continuation primitive
  plus reviewer-loop generation latch.
- `src/delegated/autonomy_supervisor.rs` — deterministic stale/mismatch and
  replay durability regression coverage.
- `src/delegated/autonomous_controller.rs` — planner-gated task continuation
  regression coverage.
- this review bundle.

## Tests and verification to rerun

Focused local tests added/executed during this task:

- `delegated::autonomy_supervisor::tests::planner_reply_*` (durable reply and
  replay);
- `delegated::autonomous_controller::tests::accepted_planner_reply_rearms_the_gated_task_once`;
- `delegated::autonomy_runtime::tests::accepted_rearm_latches_a_live_reviewer_loop_generation`.

Existing controller coverage retains graph materialization fail-closed
regressions, waiting-state no-launch behavior, active-turn no-duplicate
polling, and queued/restart recovery. CatDesk must independently run the
required focused controller/supervisor/runtime/MCP suite, `cargo fmt --
--check`, `cargo clippy --all-targets --all-features -- -D warnings`, full
`cargo test`, and `git diff --check` against the reviewed candidate.

After reviewed candidate reload, rerun T-0035 from a **fresh DAG canary**
(starting at A). Require the planned B gate reply to produce automatic next
provider/task progression without a second `autonomy_session_start`. Do not
manually resume the failed T-0081 canary.
