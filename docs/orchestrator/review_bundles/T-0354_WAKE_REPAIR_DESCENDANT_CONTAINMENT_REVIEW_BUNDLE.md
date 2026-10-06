# T-0354 — Wake-repair descendant containment review bundle

Status: IMPLEMENTATION / REPOSITORY VERIFICATION COMPLETE; T-0319 INDEPENDENT FINAL REVIEW STILL REQUIRED

## Objective

Close the remaining descendant-boundedness gap in public one-command recovery without changing ownership or lifecycle semantics of the externally owned Secure MCP runtime.

## Defect found

`scripts/repair_wake_bridge_environment.ps1` already imposed finite deadlines on its Python launcher, `venv`, `pip`, import-check, and deterministic-test children. On timeout, however, `Invoke-NativeQuiet` killed only the immediate `System.Diagnostics.Process`. Python `venv`/`ensurepip`/`pip` can spawn descendants, so recovery could regain control while a repair-owned descendant continued filesystem or network mutation after the parent had been killed.

This is distinct from T-0348's interrupted-promotion detached-handoff closure. The defect was inside wake-runtime repair's own native process tree.

## Implementation

The repair script now defines `CatDesk.WakeRepairContainedProcess`, a Windows Job Object based native runner dedicated to wake-repair-owned children:

- creates a job with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` before repair work is allowed to proceed;
- starts the selected repair child and assigns that exact process handle to the job;
- fails closed if the process cannot be assigned to the containment job;
- on timeout, closes the job so the parent and repair-owned descendants are terminated together, then bounds the final parent wait to 2 seconds;
- on ordinary successful child exit, closes the job as well, ensuring a supposedly completed Python/venv/pip operation cannot leave detached descendants behind;
- preserves the existing per-operation deadlines and exit-code contract.

The containment primitive is deliberately local to `repair_wake_bridge_environment.ps1`. `CatDesk.BoundedNativeProcess`, official tunnel-client probes, Secure MCP runtime checks, and the externally owned serving runtime retain their existing semantics; no generic kill-tree behavior was introduced across recovery.

## Deterministic regression

`scripts/test-start-catdesk-stack.ps1` now:

1. requires the wake-repair runner to retain `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, `AssignProcessToJobObject`, and the job-close boundary;
2. requires `Invoke-NativeQuiet` to route through `WakeRepairContainedProcess`;
3. executes a behavioral fixture through the sanctioned Rust PowerShell harness: a parent PowerShell process launches a delayed descendant that would write a marker file, then the parent sleeps long enough to exceed a 250 ms wake-repair timeout;
4. asserts the timeout is reported as a contained-process-tree termination and, after the descendant's would-be write time, asserts that the marker file does not exist.

The first focused run exposed only a fixture dependency omission (`ConvertTo-NativeArgument` had not been extracted into the test scope). After correcting that fixture dependency, the focused target passed.

## Verification

- `cargo test --test recovery_powershell lifecycle_and_reviewed_release_recovery_fixtures_pass -- --nocapture` — PASS (1/1; includes behavioral descendant-escape fixture).
- sanctioned `verify_project` — PASS: `cargo fmt --check`, default full `cargo test`, and `cargo build`.
- `cargo test --all-targets --all-features` — PASS.
- `cargo clippy --all-targets --all-features -- -D warnings` — PASS.
- `git diff --check` — PASS; only pre-existing LF→CRLF working-copy warnings were reported.

## Safety / non-actions

No live recovery was invoked. No serving daemon or Secure MCP tunnel/runtime was restarted, stopped, migrated, duplicated, or re-owned. No release was promoted. No browser wake was invoked. No protected wake target, Scheduler state, delegated lock/state, Git publication state, or unrelated dirty worktree content was modified.

## T-0319 relationship

T-0354 is implementation/test complete but is not an independent acceptance of T-0319. The genuinely separate T-0319 reviewer must now evaluate the cumulative T-0325 through T-0354 recovery authority/boundedness chain, including this proof that wake-runtime repair cannot return from a timed-out Python/venv/pip operation while a repair-owned descendant continues mutating.
