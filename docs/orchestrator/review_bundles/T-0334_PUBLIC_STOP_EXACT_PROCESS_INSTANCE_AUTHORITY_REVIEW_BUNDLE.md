# T-0334 — Public Stop Exact Process-Instance Authority

## Scope

Close the residual PID-reuse mutation boundary in the public `catdesk.ps1 stop` lifecycle path without changing Secure MCP ownership, recovery authority, wake state, Scheduler state, Git state, or the intentionally dirty worktree.

## Finding

The public stop path first obtained a loopback CatDesk listener and required `MatchesCanonical`, but then discarded the listener's creation/path/hash identity and invoked `Stop-Process -Id <pid>`. A process exit plus PID reuse between those two operations could therefore redirect destructive authority to a different process instance. This was inconsistent with the exact-process-instance mutation boundary established by T-0325 through T-0327 and was directly relevant to T-0319 because watchdog acceptance cycles use the public stop surface.

## Implementation

`catdesk.ps1` now passes the complete local endpoint, canonical identity, and selected listener candidate through the lifecycle stop seam. The production default delegates to the already-reviewed `Stop-CatDeskListenerProcessForRecovery` primitive from `scripts/start-catdesk-stack.ps1`.

That primitive:

- pins the selected listener as a `System.Diagnostics.Process` instance after creation-time/path/hash/daemon-mode validation;
- re-reads the loopback listener immediately before mutation and requires PID + creation identity + path + SHA-256 continuity;
- calls `Kill()` only on the pinned process object rather than reacquiring mutation authority by PID; and
- bounds `WaitForExit` to 30 seconds, failing closed if the exact process does not terminate.

If the exact selected process already disappeared before acquisition, public stop reports the daemon already exited. Identity ambiguity/replacement causes `STOP_REFUSED`.

No external Secure MCP runtime stop/restart/remove route was added.

## Regression coverage

`scripts/test-catdesk-lifecycle.ps1` now asserts that the facade contains no PID-only `Stop-Process -Id $processId` mutation path and that production stop delegates to `Stop-CatDeskListenerProcessForRecovery`. The lifecycle fixture also models exact-process-instance rejection after canonical listener selection and proves the public stop returns `STOP_REFUSED` with zero mutation.

The lower-level exact-instance helper remains covered by the T-0326 deterministic replacement-listener tests in `scripts/test-start-catdesk-stack.ps1`, including a replacement that appears after the original process is pinned.

## Verification

- `cargo test --test recovery_powershell -- --nocapture` — PASS, 2/2.
- `cargo fmt --check` — PASS.
- `cargo test` — PASS, exit code 0. The first invocation encountered a transient MCP/upstream 504; the immediate rerun completed successfully.
- `cargo build` — PASS.
- `cargo clippy --all-targets --all-features -- -D warnings` — PASS.
- `git diff --check` — PASS; only the pre-existing Windows LF→CRLF warnings remain.

## Review status

Implementation/test complete. This ticket does not self-approve T-0319. T-0319 must receive a genuinely separate independent review of the cumulative T-0325 through T-0334 recovery-authority/boundedness chain before final recovery acceptance.
