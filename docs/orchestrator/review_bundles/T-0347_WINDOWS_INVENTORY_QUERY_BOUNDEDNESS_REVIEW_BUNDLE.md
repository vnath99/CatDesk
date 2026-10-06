# T-0347 Windows inventory query boundedness review bundle

Date: 2026-09-08
Status: IMPLEMENTATION / REPOSITORY VERIFICATION COMPLETE; INDEPENDENT REVIEW REQUIRED

## Problem

The public `plan` / `recover` path had accumulated finite deadlines for native children, helper subprocesses, runtime status, wake repair, promotion recovery, and output drains, but Windows listener/process inventory was still performed directly in the recovery PowerShell process. `Get-NetTCPConnection` and three recovery-facing `Get-CimInstance Win32_Process` observations therefore remained able to block indefinitely if the Windows networking or CIM provider wedged. That could defeat the one-command recovery boundedness contract before any destructive process decision was reached.

## Implementation

- Added `scripts/query-catdesk-windows-inventory.ps1`, a read-only inventory helper with three fixed modes: `Listener`, `Process`, and `Daemons`.
- Added `Invoke-BoundedWindowsInventoryProbe` in `scripts/start-catdesk-stack.ps1`.
- The parent resolves only the fixed OS Windows PowerShell identity through `Resolve-TrustedWindowsPowerShellPath`.
- The checked-in helper must be a real non-reparse `FileInfo` whose normalized full path retains exact expected identity.
- Each inventory child has a 5,000 ms parent deadline and 64 KiB output cap through `CatDesk.BoundedNativeProcess`; timeout, termination failure, output-drain timeout, overflow, nonzero exit, empty output, or malformed JSON fail closed.
- `Get-LoopbackCatDeskListener` now obtains listener rows and owning-process CIM data only through the bounded probe.
- `Get-CatDeskListenerProcessInstanceForRecovery`, `Get-CatDeskDaemonProcessCandidates`, and `Get-CatDeskDaemonProcessInstanceForRecovery` now obtain Win32 process observations only through the bounded probe.
- Canonical executable path resolution, SHA-256 comparison, selected creation-time comparison, pinned `System.Diagnostics.Process` acquisition, and all `Kill()` / `WaitForExit()` mutation authority remain in the recovery parent. The helper has no daemon, tunnel, Scheduler, wake, promotion, or Git mutation authority.

## Regression coverage

`scripts/test-start-catdesk-stack.ps1` now:

1. requires the bounded inventory function to use trusted Windows PowerShell, `CatDesk.BoundedNativeProcess`, and the explicit inventory timeout;
2. rejects direct recovery-parent `Get-NetTCPConnection` / `Get-CimInstance` inventory assignment paths;
3. verifies the read-only helper retains the required networking and CIM observations;
4. executes an indefinitely hanging synthetic inventory helper with only the fixture deadline reduced to 250 ms and requires bounded failure in under 3 seconds;
5. retains deterministic listener ambiguity, dual-stack coalescing, process identity, and daemon-attribution coverage by injecting inventory rows into the parent logic rather than querying the live host.

`scripts/test-stale-canonical-daemon-recovery.ps1` imports the bounded inventory dependencies and continues to exercise the real production inventory path against its disposable stale-daemon fixture.

## Verification

- `cargo test --test recovery_powershell -- --nocapture` — PASS: 2 passed / 0 failed.
- sanctioned `verify_project` — PASS:
  - `cargo fmt --check` — PASS;
  - `cargo test` — PASS;
  - `cargo build` — PASS.
- `cargo clippy --all-targets --all-features -- -D warnings` — PASS.
- `git diff --check` — PASS; only pre-existing LF-to-CRLF working-copy warnings were emitted.

The first focused test attempt correctly exposed that the stale-daemon extraction fixture had not imported the new production inventory helper. The fixture dependency list was corrected. A second attempt exposed that extracted functions do not retain the production script's `$PSScriptRoot`; helper identity was therefore made an initialization-time script constant and the checked-in helper path supplied explicitly to extracted fixtures. The final focused suite then passed 2/2. These were test-integration corrections, not relaxed production behavior.

## Preserved boundaries

- No live `catdesk.ps1 recover` invocation was performed.
- No live daemon was restarted, migrated, or killed for this ticket beyond the existing disposable stale-daemon regression fixture.
- The externally owned already-running official Secure MCP runtime was not restarted, migrated, replaced, or claimed.
- No browser wake was invoked.
- No protected wake-target state or Scheduler state was edited.
- No reviewed release was promoted.
- No Git commit/push/publication occurred.
- The pre-existing dirty worktree was preserved.

## T-0319 relationship

T-0347 closes a distinct recovery-reachable boundedness gap that remained after T-0325 through T-0345. It does not self-accept cumulative recovery. T-0319 remains open for a genuinely separate independent review of the cumulative T-0325 through T-0347 authority/boundedness chain together with the existing watchdog, stale-daemon, and one-command host evidence.
