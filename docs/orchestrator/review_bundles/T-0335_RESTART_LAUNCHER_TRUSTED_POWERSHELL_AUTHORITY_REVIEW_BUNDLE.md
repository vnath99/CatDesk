# T-0335 — Restart launcher trusted PowerShell authority review bundle

Date: 2026-09-07
Status: IMPLEMENTATION / FIXTURE COMPLETE; T-0319 INDEPENDENT FINAL REVIEW STILL REQUIRED
Scope: detached restart launcher executable authority only. No live daemon/tunnel replacement, external Secure MCP ownership change, browser wake invocation, protected wake-target edit, Scheduler mutation, reviewed-build promotion, Git publication, or dirty-worktree cleanup.

## Defect found

The reviewed restart launcher in `scripts/restart_catdesk_daemon.ps1` still invoked the detached worker with `Start-Process -FilePath "powershell.exe"`. Although T-0328/T-0330 hardened the CatDesk process identity crossing this handoff, a bare executable name delegates interpreter selection to Windows executable search semantics. A hostile or accidental `powershell.exe` earlier in the effective search path could therefore become executable authority at a restart mutation boundary.

## Correction

T-0335 adds `Resolve-TrustedWindowsPowerShellPath` and requires the detached handoff to launch through its returned absolute path. The resolver:

- derives the candidate only from `[Environment]::SystemDirectory` plus `WindowsPowerShell\v1.0\powershell.exe`;
- requires the candidate to resolve to a file;
- rejects a `ReparsePoint` candidate;
- requires the resolved full path to equal the OS-derived expected full path using Windows case-insensitive comparison; and
- fails closed if any of those checks fail.

The launcher then calls `Start-Process -FilePath $trustedPowerShell`. It no longer launches `powershell.exe` by basename. This is deliberately narrow and does not weaken or replace the existing exact-process old-listener identity checks, worker handoff identity, externally owned Secure MCP boundary, or reviewed-build promotion authority.

## Regression coverage

`scripts/test-restart-catdesk-daemon-process-identity.ps1` now extracts and executes the trusted PowerShell resolver in the existing Windows recovery fixture. The fixture prepends a fake `powershell.exe` directory to `PATH` and verifies the resolver still returns the OS-derived Windows PowerShell path. It also verifies that the selected system PowerShell is not a reparse point and adds source invariants requiring the trusted resolver/absolute-path launch while rejecting a basename `Start-Process -FilePath "powershell.exe"` regression.

Approved focused harness:

- `cargo test --test recovery_powershell -- --nocapture` — PASS, 2 passed / 0 failed.

Repository gates completed in this run:

- `cargo fmt --check` — PASS.
- `cargo build` — PASS.
- `cargo test --bin catdesk --quiet` — process exit 0. The harness output contains internally spawned negative-fixture failure text, but the top-level Cargo command completed successfully.
- `cargo clippy --all-targets --all-features -- -D warnings` — PASS.

A monolithic `cargo test` request was attempted repeatedly, but the CatDesk connector returned transient HTTP 504 timeouts before a result could be retrieved. This was not recorded as a test failure. The same transport remained responsive for the focused recovery suite, build, binary test suite, and Clippy. No shell-policy bypass was used: a direct nested `powershell.exe` test attempt was rejected by CatDesk with `SHELL_MODE_BLOCKED`, and verification continued through the approved Rust recovery harness.

## Authority / non-ownership invariants

- No serving CatDesk daemon was stopped or replaced.
- No Secure MCP/tunnel process was restarted, duplicated, or claimed by CatDesk.
- No browser wake bridge was manually invoked.
- No protected wake-target state was edited.
- No reviewed build was promoted and no release bytes were blessed.
- No Git publication or worktree cleanup was performed.

## T-0319 relationship

T-0335 is implementation/test complete at the repository/fixture boundary. It closes a concrete executable-search authority seam discovered during the continuing one-command recovery audit, but it does not self-approve T-0319. A genuinely separate independent reviewer must now evaluate the cumulative T-0325 through T-0335 recovery authority/boundedness chain together with the existing watchdog/stale-daemon evidence.
