# T-0266 / T-0224-R1B-R3G-R2 — Child-lifetime-safe retained launch lease

## Disposition

T-0265 correctly retained reviewed runtime handles across spawn/import, but its
`try_wait(...)?` branch could drop the lease after a successfully spawned child
without proving that child had exited. Rust `Child` drop does not kill or reap.

T-0266 preserves T-0265’s `FILE_SHARE_READ` reviewed interpreter/adapter/
primitive lease and adds `RetainedAdapterChild`, which owns both `Child` and
`RuntimeLaunchLease` after a successful spawn.

## Post-spawn terminal invariant

- Normal poll observes terminal state, then `wait_with_output` reaps before
  releasing the lease; output bounds and result parsing happen afterwards.
- Timeout kills and waits before release, then returns the existing fail-closed
  timeout result.
- Poll/wait error does not use `?`: it kills and reaps before returning process
  uncertainty.
- If finalization cannot confirm a reap, `Drop` deliberately leaks the child
  handle and reviewed lease rather than detach a possibly-live child and release
  pathname authority. This is fail-closed; no retry is authorized.
- Spawn failure happens before guard construction, so no child exists and the
  lease may safely drop.

The platform guarantee used is Windows share semantics retained by T-0265:
read sharing allows Python path reads but denies write/delete access; rename
requires delete access. The guard makes release conditional on terminal child
state rather than local-variable drop order.

## Tests and evidence

`injected_poll_error_terminates_reaps_before_releasing_launch_lease` creates an
isolated reviewed runtime, starts a sleeping child, injects a synthetic poll
error, verifies interpreter/adapter/primitive writes remain denied during
finalization, then verifies replacement succeeds only after kill/reap and lease
release. Existing T-0265 process replacement and reviewed-digest tests remain
in place. Existing parser tests cover invalid/oversized output after terminal
output collection; all process uncertainty remains operator-attention/nonretry
at the delivery boundary.

## Verification truth

| Command | Result |
| --- | --- |
| `cargo test stable_wake_owner -- --nocapture` | PASS — 16 focused owner tests, including injected post-spawn poll error. |
| `cargo fmt --check` | PASS. |
| `cargo clippy --all-targets --all-features -- -D warnings` | PASS. |
| `cargo test` | PASS — 784 unit tests plus integration suites. |
| `rust_full` | No separately runnable project command discovered. |

## Attribution and boundary

The workspace was broadly dirty before this task. T-0266 attributable files:

- `src/stable_wake_owner.rs`
- this bundle

No real `.catdesk`, runtime provisioning, target/profile, owner selector,
browser, release, daemon, tunnel, Scheduler/service, or Git state was mutated.
Legacy Python remains the sole live submit owner. The separately gated host
runtime provision plus expected-old CAS, fresh W13 canary, receipt/readback,
and restart suppression remain out of scope.
