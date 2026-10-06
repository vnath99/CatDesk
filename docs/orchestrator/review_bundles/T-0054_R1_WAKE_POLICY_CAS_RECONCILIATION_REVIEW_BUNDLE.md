# T-0054-R1 Wake-Policy CAS Reconciliation Review Bundle

## Root cause

The T-0054 pre-launch gate evaluated one wake-policy generation. On a terminal
`THROUGH_TASK` or `UNTIL_PROVIDER_EXHAUSTED` condition it attempted a
generation-CAS transition to `MANUAL_OFF`, but returned suppression even when
that transition failed. A concurrent operator update could therefore be newer
than the evaluated policy while its pending, not-yet-launched wake was still
incorrectly suppressed.

## Reconciled decision semantics

`wake_policy_allows_dispatch` now uses a fixed four-generation reconciliation
loop. For each iteration it reads and validates the durable policy, evaluates
the exact task/provider state required by that policy generation, then:

- permits immediately for `INDEFINITE` or a non-terminal policy condition;
- suppresses immediately for `MANUAL_OFF`;
- on a terminal stop condition, suppresses only after the matching
  generation-CAS stop commits; or
- after a failed stop CAS, re-reads and re-evaluates the newer generation.

Thus a newer `MANUAL_OFF` suppresses, a newer `INDEFINITE` permits the pending
pre-launch wake, a changed `THROUGH_TASK` evaluates its new exact ID, and a
fresh `UNTIL_PROVIDER_EXHAUSTED` evaluates fresh provider/session state.
Unread review records are still untouched whenever suppression occurs. Invalid,
unreadable, or never-stabilizing policy state fails closed before review claim
or bridge launch.

The reconciliation boundary is deliberately before `wake_bridge.py` starts.
Once the existing W13 durable browser submit boundary has been crossed, this
gate has no retroactive effect; later policy updates apply only to later
dispatches.

## Changed files and deterministic coverage

- `src/delegated/autonomy_runtime.rs`
  - adds the bounded reconciliation primitive and fresh-policy evaluation;
  - covers stale terminal `THROUGH_TASK` changes to `INDEFINITE`, `MANUAL_OFF`,
    and a different exact task ID;
  - covers provider-exhaustion changes to `INDEFINITE`, a fresh
    `UNTIL_PROVIDER_EXHAUSTED` provider state, and bounded repeated churn.

W13 browser, bridge invocation, receipt schema 1, outer wake schema 4, exact
record/message/target binding, two-observation final-message proof, single
durable submit boundary, and no post-submit retry were not modified.

## Verification and host handoff

The worker ran `cargo fmt -- --check`,
`cargo clippy --all-targets --all-features -- -D warnings`, `cargo test`
(482 passed; 18 operator-local tests ignored), and `git diff --check`. No
browser, wake bridge, tunnel, Scheduler, daemon, release, provider fallback,
or Git publication was invoked by this worker. After independent review, only
CatDesk may build/reload the reviewed candidate, inspect the policy surface,
and perform the fresh W13 acceptance canary.
