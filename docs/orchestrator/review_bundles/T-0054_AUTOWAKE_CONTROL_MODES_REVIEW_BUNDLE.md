# T-0054 Autowake Control Modes Review Bundle

## Policy model

CatDesk now owns a versioned, non-secret durable wake policy at
`.catdesk/autonomy/wake-policy.json`. A missing policy defaults to
`INDEFINITE`, preserving the accepted W13 behavior. Corrupt policy fails
closed before a bridge launch.

Supported modes are `MANUAL_OFF`, `INDEFINITE`, `THROUGH_TASK`, and
`UNTIL_PROVIDER_EXHAUSTED`. The record has a monotonic generation, set time,
optional exact terminal task ID, and bounded stopped reason. Updates are
atomic and compare the caller's expected generation; stale writers are
rejected. `MANUAL_OFF` is always allowed. Enabling any automatic mode first
performs a non-launching fixed bridge/config readiness check.

## Dispatch semantics

The Rust dispatcher re-reads policy immediately before every bridge launch,
including retry attempts. `MANUAL_OFF` returns before review-record claim or
send, preserving the unread actionable record. `INDEFINITE` is the prior
automatic behavior.

`THROUGH_TASK` matches only the exact task ID from the session's durable
contract. Earlier work and waiting/error states continue to wake. Once that
exact task reaches `COMPLETED_VERIFIED`, the policy atomically becomes manual
off before the completion record is claimed, leaving the final review unread.

`UNTIL_PROVIDER_EXHAUSTED` does not stop on transient 429/rate limits. It
stops only when durable state reports the allowed provider chain terminally
exhausted/unavailable (`CREDIT_BUDGET_EXHAUSTED` or `QWEN_UNAVAILABLE`). It
does not change the existing no-premature-Qwen policy.

## Interface and boundaries

MCP exposes `autonomy_wake_policy_get` and CAS-safe
`autonomy_wake_policy_set`. The status is bounded and contains only policy
metadata plus fixed readiness state. No credential, profile, browser, target,
or transcript data is returned.

The supported consumer facade uses only closed shapes and the already
verified loopback CatDesk MCP listener:

- `./catdesk.ps1 wake status`
- `./catdesk.ps1 wake off`
- `./catdesk.ps1 wake on`
- `./catdesk.ps1 wake through <exact-task-id>`
- `./catdesk.ps1 wake until-exhausted`

The facade first reads the current generation, then sends the corresponding
CAS update. It rejects extra terminal IDs, non-slug task IDs, unverified
listeners, malformed/oversized MCP replies, and any MCP error without
displaying endpoints or diagnostics. A concurrent update is therefore
reported unavailable rather than overwritten.

The dispatcher gate is before `wake_bridge.py`; the accepted W13 browser and
receipt contract is unchanged: schema 4 state, schema 1 receipt, exact
record/message/target hashes, positive send timestamp, two final-message
observations, durable one-submit boundary, and no post-boundary retry.

## Changed files and tests

- `src/delegated/autonomy_state.rs`: policy schema, atomic CAS persistence,
  and stop transition.
- `src/delegated/autonomy_runtime.rs`: pre-launch policy gate and non-launching
  enablement readiness check.
- `src/delegated/autonomy_supervisor.rs`, `src/mcp.rs`: control-plane
  discovery and get/set operations.
- `catdesk.ps1`, `scripts/test-catdesk-lifecycle.ps1`: closed consumer wake
  commands plus deterministic local-MCP fixture coverage.
- `src/main.rs`: macro recursion limit needed by the expanded static schema.

Focused tests cover default `INDEFINITE`, OFF without readiness, stale CAS
rejection, enablement fail-closed behavior, closed facade parsing and bounded
read/CAS-set calls, and existing runtime receipt/dispatch regressions. No live
browser, wake, tunnel, Scheduler, daemon, or provider fallback was invoked.

## Verification and handoff

The worker ran `cargo fmt -- --check`,
`cargo clippy --all-targets --all-features -- -D warnings`, `cargo test`
(479 passed; 18 pre-existing operator-local tests ignored), `git diff --check`,
and `scripts/test-catdesk-lifecycle.ps1`. After independent review, CatDesk
may set `INDEFINITE` through the control plane and run one fresh normal
automatic canary; only CatDesk owns that action and authoritative diff capture.
