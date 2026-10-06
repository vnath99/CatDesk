# T-0328 — Restart handoff exact-process identity closure review bundle

Date: 2026-09-07
Status: SOURCE / FIXTURE ACCEPTED; T-0319 INDEPENDENT FINAL REVIEW STILL REQUIRED
Scope: detached CatDesk daemon restart handoff only. No live daemon replacement, external Secure MCP/tunnel mutation, browser wake invocation, protected wake-target edit, Scheduler mutation, or Git publication.

## Review finding

A bounded recovery audit found a residual PID-reuse authority gap outside the T-0325/T-0326/T-0327 `start-catdesk-stack.ps1` closures. `scripts/restart_catdesk_daemon.ps1` selected an `OldPid`, but the detached `scripts/restart_catdesk_daemon_worker.ps1` slept before reacquiring that numeric PID and then used PID-only `Stop-Process` / `Wait-Process` mutation. If the selected CatDesk exited during the detached delay and Windows reused the PID for another same-name process, the worker could terminate a process instance that the launcher had never selected.

The replacement side also reacquired `newProcess.Id` by PID during readiness polling. A sufficiently fast child exit plus PID reuse could therefore make readiness observations refer to a different process than the one just launched.

## Correction

The restart handoff now carries process-instance authority across the detached boundary rather than relying on PID alone:

1. The launcher acquires the selected CatDesk `Process` handle before recording identity, requires its executable path, and captures the exact UTC `StartTime` plus resolved path.
2. Those immutable observations are passed to the detached worker as `ExpectedOldProcessStartedAtUtc` and `ExpectedOldProcessPath` alongside the PID used only for reacquisition.
3. After the startup delay, the worker performs one PID lookup, immediately acquires that `Process` object's OS handle, and requires the pinned object itself to match CatDesk name, exact start-time identity, and expected resolved path before collecting old listener ports or mutating anything.
4. Termination and waiting use the same exact `Process` object (`Kill()` / `WaitForExit()`). The worker no longer uses `Stop-Process -Id $OldPid`, `Wait-Process -Id $OldPid`, or force-reacquires a PID after timeout. If the exact target has disappeared, it never redirects authority to a replacement PID.
5. The newly launched replacement is likewise handle-pinned immediately. Readiness observes that exact object's `HasExited` / `StartTime`; it does not reacquire `newProcess.Id` through `Get-Process`.

The external tunnel remains explicitly outside the restart handoff and is neither discovered nor controlled by these changes.

## Files changed

- `scripts/restart_catdesk_daemon.ps1`
- `scripts/restart_catdesk_daemon_worker.ps1`
- `scripts/test-restart-catdesk-daemon-process-identity.ps1`
- `tests/recovery_powershell.rs`

## Regression evidence

The new `scripts/test-restart-catdesk-daemon-process-identity.ps1` fixture is included in the normal Rust recovery PowerShell harness. It verifies:

- both restart scripts parse successfully;
- the exact selected process identity is accepted;
- a same-path replacement whose start time differs by one tick is rejected;
- a wrong-path process with the same start time is rejected;
- the launcher pins the old process before exporting start/path identity;
- the worker has no destructive PID-only stop/wait path;
- mutation uses the exact pinned old-process object; and
- replacement readiness does not reacquire the new child by PID.

The first integrated test run correctly caught a PowerShell parser typo introduced during the edit. That typo was corrected before acceptance; no failing run is counted as evidence.

## Verification

```text
cargo test --test recovery_powershell
2 passed; 0 failed

CatDesk verify_project
pass

cargo clippy --all-targets --all-features -- -D warnings
pass

git diff --check -- scripts/restart_catdesk_daemon.ps1 scripts/restart_catdesk_daemon_worker.ps1 scripts/test-restart-catdesk-daemon-process-identity.ps1
pass (only existing Windows line-ending warnings where emitted)
```

## T-0319 relationship

This correction was discovered while reviewing the broader one-command/watchdog recovery authority required by T-0319. It does not self-manufacture independent final acceptance. A genuinely separate T-0319 reviewer must evaluate the corrected T-0325/T-0326/T-0327/T-0328 chain together with the existing watchdog/stale-daemon live evidence.

T-0324 remains a separate runtime/source-parity remediation: its diagnosis is complete, but reviewed build/promotion plus a bounded live Qwen continuation canary must occur through the existing authority path rather than by promoting the mutable dirty worktree.

## Conclusion

**ACCEPT T-0328 at the repository/source-fixture boundary.** The detached restart handoff no longer converts a previously selected PID into later destructive authority. Old-process termination and replacement readiness are bound to exact pinned process objects, while T-0319 remains open for independent final review.
