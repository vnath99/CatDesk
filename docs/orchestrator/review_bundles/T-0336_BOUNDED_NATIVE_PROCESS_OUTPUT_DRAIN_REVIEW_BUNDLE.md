# T-0336 — Bounded native-process successful-exit output-drain closure

Status: implementation/test complete; pending the separate independent T-0319 final recovery review.

## Defect

The recovery subprocess runners already bounded the lifetime of the selected wrapper process, but their normal-success path still called an unbounded `Task.WaitAll(stdout, stderr)`. On Windows, a wrapper can exit within its deadline while a descendant retains inherited redirected stdout/stderr handles. In that shape `WaitForExit` succeeds, yet the stream-reader tasks do not complete until the descendant closes the inherited handles. A nominally bounded `catdesk.ps1 recover` helper call could therefore hang indefinitely after the selected parent exited successfully.

The defect existed in both recovery-relevant bounded runners:

- `scripts/start-catdesk-stack.ps1`, which backs official-runtime commands, Codex prerequisite probes, and the bounded recovery helper path used for wake/runtime repair and Secure MCP migration helpers.
- `scripts/setup-secure-mcp.ps1`, which bounds one-shot tunnel-client operations used by the supported Secure MCP migration workflow.

This is distinct from the already-closed timeout path: T-0332/T-0333 prevented a timed-out wrapper from re-entering an unbounded drain. T-0336 closes the successful-parent-exit case.

## Implementation

Both bounded native-process result types now carry an explicit `OutputDrainTimedOut` state. After the selected process exits within its process deadline, stdout/stderr completion is allowed a fixed 2-second drain window. If both stream tasks do not complete within that window, the invocation returns a bounded fail-closed result rather than blocking indefinitely or accepting partial output.

The PowerShell callers convert that state to fixed failure diagnostics before any output/exit-code result can be treated as success:

- `official runtime command output drain exceeded bounded timeout`
- `Codex CLI probe output drain exceeded bounded timeout`
- `<helper failure>; bounded helper output drain timed out`
- `tunnel-client command output drain exceeded bounded timeout`

No incomplete stdout/stderr is accepted as authoritative output after a drain timeout.

## Regression evidence

`scripts/test-start-catdesk-stack.ps1` now models the exact failure shape: an outer PowerShell wrapper starts a descendant with `-NoNewWindow`, allowing the descendant to inherit redirected handles, and then exits successfully. The parent process deadline is not what recovers control; the test requires the bounded output-drain classification and requires control to return within 3.5 seconds even though the descendant sleeps for 5 seconds.

`scripts/test-secure-mcp-route-validation.ps1` also source-gates the Secure MCP runner: it requires the 2-second `Task.WaitAll(new Task[] { stdout, stderr }, 2000)` form plus `OutputDrainTimedOut`, and rejects reintroduction of unbounded `Task.WaitAll(stdout, stderr)`.

## Verification

- `cargo test --test recovery_powershell -- --nocapture`: PASS, 2 passed / 0 failed.
- `cargo fmt --check`: PASS.
- `cargo test`: PASS; main suite 876 passed / 0 failed / 21 ignored, with all other test binaries passing.
- `cargo build`: PASS.
- `cargo clippy --all-targets --all-features -- -D warnings`: PASS.
- `git diff --check`: PASS after durable-state synchronization; exit code 0 with only the existing Windows LF→CRLF working-copy warnings.

## Authority and ownership invariants

T-0336 does not grant new process, tunnel, browser, release, wake-target, Scheduler, or Git authority. It only bounds completion of helper subprocess output capture. The externally owned official Secure MCP runtime remains external and must not be stopped/recreated merely because a helper invocation fails. The dirty worktree remains preserved. No live daemon/tunnel restart, browser wake, protected wake edit, release promotion, Scheduler mutation, or Git publication is part of this ticket.

## Acceptance disposition

T-0336 is implementation/test complete. It does not independently close T-0319. A genuinely separate reviewer must evaluate the cumulative T-0325 through T-0336 recovery authority/boundedness chain together with the existing watchdog/stale-daemon live evidence before T-0319 can be accepted. This implementation lineage must not self-approve that gate.
