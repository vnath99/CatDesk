# T-0035 R5 reviewer-loop re-arm ownership-race review bundle

## Scope and source reproduction

This R5 candidate fixes the remaining R4 reviewer-wakeup ownership race. No
live browser, daemon reload, release action, tunnel/Scheduler change, Git
publication, or failed-session resume was performed. In particular, the
failed T-0081 canary was not manually resumed.

The source before this change had two independent interleavings:

1. **Normal WAITING/terminal exit:** a ticker owning generation `g` observed
   `WAITING_FOR_CHATGPT` (or terminal), dispatched its wake, read that the
   registration still held `g`, and decided to break. Before the later final
   cleanup lock, accepted reply/resume processing bumped the key to `g + 1`
   and `start_or_tick` found that existing key, so it did not install another
   reviewer loop. Final cleanup then noticed `g + 1 != g`, preserved the key,
   and the old loop exited. The `g + 1` registration was left ownerless.
2. **Initial-wake exit:** an immediately actionable initial outcome owned `g`
   while it completed dispatch/retry work. A concurrent accepted re-arm bumped
   the map entry to `g + 1`; `start_or_tick` again observed an existing key and
   did not install another loop. The initial-wake branch then unconditionally
   removed the key and returned, clobbering the newer registration/owner
   signal.

Both windows occurred because observation, exit decision, and removal were
separate transitions.

## Atomic ownership state machine

`src/delegated/autonomy_runtime.rs` now makes owner retirement one state
transition under the `reviewer_wakeups` mutex:

| Locked entry for key | Observed owner generation | Result | Map effect |
| --- | --- | --- | --- |
| `g` | `g` | `RETIRED` | remove exactly `g` |
| `g + n` (`n > 0`) | `g` | `CONTINUE(g + n)` | preserve newer entry |
| missing or `< g` | `g` | `OWNERSHIP_LOST` | preserve state; delete nothing |

`retire_or_adopt_reviewer_wakeup_locked` is the sole retire-or-adopt
primitive. The async wrapper acquires the same reviewer-wakeup lock; callers
cannot read generation, make an exit decision, and remove it later. Re-arm
generation bumps and fresh-loop installation are also lock-held operations.

The initial-wake path now invokes this transition after its bounded dispatch:
`CONTINUE` adopts the newer generation and falls through into the normal
ticker loop. `RETIRED` makes a subsequent re-arm see no key, allowing its
`start_or_tick` completion to install one fresh loop. Normal terminal,
`WAITING_FOR_CHATGPT`, `WAITING_FOR_USER`, missing-controller, and error exits
all use the same transition and return only after retirement or safe ownership
loss. There is no final detached cleanup operation.

## Safety boundaries retained

- The runtime registry still serializes `run_once`, keeping one provider-turn
  owner even if reply/resume re-arm signals are concurrent or replayed.
- Reply/resume remains durable-first and only accepted queued/rate-limited/live
  sessions reach local continuation dispatch. Rejected or stale replies do not
  launch a provider.
- Existing captured-thread, Terra/High routing, Qwen fallback,
  graph/gate/materialization, accounting, recovery, and W13 wake receipt
  boundaries are unchanged.
- A stale/missing reviewer owner fails safe: it cannot delete a registration
  it no longer owns. An unexpected lower generation is treated the same way.

## Changed files

- `src/delegated/autonomy_runtime.rs` — atomic reviewer owner retirement or
  adoption, all exit-path integration, and deterministic race tests.
- `docs/orchestrator/review_bundles/T-0035_R5_REVIEWER_LOOP_REARM_OWNERSHIP_RACE_REVIEW_BUNDLE.md`
  — this R5 review record.

## Deterministic coverage

The runtime unit tests cover:

- initial-wake re-arm before retirement adopts the newer generation;
- normal terminal re-arm before retirement adopts the newer generation;
- re-arm after atomic retirement permits precisely one fresh registration;
- unchanged generation retires/removes atomically;
- newer generation continues without deletion;
- replayed re-arms keep one logical reviewer owner (with the runtime registry
  remaining the provider-launch serialization authority);
- stale, lower, and missing ownership fail safe without deleting a key.

Existing focused controller/supervisor tests additionally cover accepted
planner reply requeue once, replay durability, rejected reply no launch,
restart/resume continuity, graph materialization fail-closed behavior, and
provider routing/no-launch safety.

## Local verification record

These commands completed locally for this candidate; CatDesk must independently
verify the reviewed candidate and is the authority for acceptance:

- `cargo test autonomy_runtime::tests --bin catdesk` — 24 passed, 1
  operator-local probe ignored.
- Focused `planner_reply`, `resume`, `graph_materialization`, and `provider`
  test selections — passed (two operator-local tests ignored).
- `cargo fmt -- --check` — passed.
- `cargo clippy --all-targets --all-features -- -D warnings` — passed.
- `cargo test` — 514 passed, 18 environment-gated tests ignored, plus the
  T-0035 DAG marker integration test passed.

`git diff --check` also passed with only pre-existing dirty-workspace
line-ending warnings and no whitespace errors. These are local worker results,
not an assertion of independent CatDesk acceptance.

## Required follow-up

After the reviewed candidate is reloaded, rerun T-0035 as a **fresh live DAG
from A**. Require `WAITING_FOR_CHATGPT` -> successful planner reply ->
automatic next-task progression without a second start. Also recheck resume,
stale/rejected reply, structured recovery, rehydration, graph/accounting,
provider routing, and wake behavior. Do **not** manually resume failed T-0081.
