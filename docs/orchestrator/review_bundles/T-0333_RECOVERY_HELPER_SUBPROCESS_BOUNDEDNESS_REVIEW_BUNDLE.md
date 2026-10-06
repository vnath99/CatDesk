# T-0333 — Recovery helper subprocess boundedness review bundle

Date: 2026-09-07
Status: IMPLEMENTATION / FIXTURE COMPLETE; T-0319 INDEPENDENT FINAL REVIEW STILL REQUIRED
Scope: one-command recovery helper boundedness only. No live daemon replacement, external Secure MCP runtime ownership change, browser wake invocation, protected wake-target mutation, Scheduler mutation, reviewed-build promotion, or Git publication.

## Defect

The supported `plan`/`recover` path still contained synchronous helper boundaries that could wait forever even after the earlier recovery-hardening work:

- `Test-WakeBridgeRuntime` directly invoked the wake venv Python interpreter with no timeout.
- `Invoke-WakeBridgeRuntimeRepair` synchronously invoked `repair_wake_bridge_environment.ps1` with no parent-side deadline.
- `Invoke-OrderedOfficialRuntimeMigration` synchronously invoked `setup-secure-mcp.ps1 -Mode migrate` with no parent-side deadline.
- The wake repair helper's Python/venv/pip/test children were themselves unbounded, so merely killing a timed-out parent PowerShell process could strand a mutating child.
- `setup-secure-mcp.ps1` still drained redirected stdout/stderr after a timed-out child had been killed; a descendant retaining inherited pipe handles could therefore convert the nominal timeout into an unbounded drain.

These are boundedness defects rather than new authority requirements. A one-command recovery path must return a finite success/failure result even when an auxiliary executable wedges.

## Correction

`scripts/start-catdesk-stack.ps1` now adds `Invoke-BoundedRecoveryHelper`, reusing the existing `CatDesk.BoundedNativeProcess` primitive introduced and hardened by the earlier recovery tickets. It applies bounded output capture and parent-side deadlines to:

- wake-runtime Python probing (5 seconds),
- wake-runtime repair helper execution (25 minutes, intentionally larger than the bounded child budgets), and
- ordered official-runtime migration (30 seconds).

`scripts/repair_wake_bridge_environment.ps1` now runs mutating native children through a bounded `System.Diagnostics.Process` wrapper. Ordinary Python/venv/import/test operations have a finite 120-second default; the pinned dependency installation receives a finite 600-second budget. Timeout termination itself is bounded to an additional 2 seconds. This closes the dangerous case where the parent repair deadline could otherwise leave an indefinitely mutating venv/pip child.

`scripts/setup-secure-mcp.ps1` now mirrors T-0332's timeout-drain rule: after a timed-out child exits following termination, it returns the timeout result immediately instead of calling unbounded `Task.WaitAll(stdout, stderr)`. This prevents inherited pipe handles from defeating the timeout.

The migration path continues to preserve externally owned Secure MCP runtime authority: `-Mode migrate` changes the legacy configuration shape only and does not stop/recreate the serving official runtime.

## Regression evidence

`cargo test --test recovery_powershell -- --nocapture` passes 2/2 after the change. `scripts/test-start-catdesk-stack.ps1` now imports `Invoke-BoundedRecoveryHelper` and deterministically launches a sleeping child PowerShell with a 250 ms helper deadline; the fixture requires bounded timeout classification and a hard return within 3 seconds.

Project gates completed in this implementation context:

- `cargo fmt --check` — PASS
- `cargo test` — exit 0; main suite 876 passed / 21 ignored, with all integration suites passing. The existing AppContainer helper subprocess fixture emitted `Access is denied` diagnostics under the current token profile but did not fail the top-level suite.
- `cargo build` — PASS
- `cargo clippy --all-targets --all-features -- -D warnings` — PASS
- `git diff --check` — PASS, with only pre-existing Windows LF→CRLF warnings

A separate nested-PowerShell parser-only command was rejected by CatDesk's shell allowlist (`SHELL_MODE_BLOCKED`); no bypass was attempted. The approved recovery PowerShell harness itself parses `start-catdesk-stack.ps1` and passed.

## Authority / acceptance boundary

T-0333 did not invoke the browser wake bridge, edit protected wake state, modify Scheduler/autostart state, promote release bytes, restart the serving daemon, take ownership of the external Secure MCP runtime, clean the dirty worktree, or publish Git state.

T-0333 is implementation/test complete. T-0319 remains open for a genuinely separate independent final review of the cumulative T-0325 through T-0333 recovery authority/boundedness chain and existing watchdog/stale-daemon evidence. This implementation lineage must not self-manufacture that acceptance verdict.
