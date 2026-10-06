# T-0337 Recovery helper trusted PowerShell authority review bundle

## Scope

Close the remaining one-command recovery interpreter-search boundary for the two mutating project-owned PowerShell helpers: wake-runtime repair and ordered `external_foreground` -> official Secure MCP runtime migration. Preserve the externally owned Secure MCP runtime, dirty worktree, bounded helper execution, and existing recovery authority.

## Defect

`scripts/start-catdesk-stack.ps1` launched both helpers with `Join-Path $PSHOME 'powershell.exe'`. That made helper interpreter selection dependent on the caller's PowerShell installation context rather than the fixed Windows OS interpreter identity already required by the reviewed restart handoff in T-0335. A recovery mutation boundary should not inherit caller-selected interpreter authority.

## Change

Added `Resolve-TrustedWindowsPowerShellPath` to resolve only `[Environment]::SystemDirectory\WindowsPowerShell\v1.0\powershell.exe`. The resolver requires the target to be a real `FileInfo`, rejects reparse points, and requires the resolved full path to equal the fixed expected OS path. `Invoke-WakeBridgeRuntimeRepair` and `Invoke-OrderedOfficialRuntimeMigration` now use only this resolver before passing the absolute interpreter path into the existing bounded recovery-helper runner. No tunnel ownership, restart, browser, Scheduler, provider, or release authority was added.

## Regression evidence

`scripts/test-start-catdesk-stack.ps1` now imports the resolver and prepends a fake `powershell.exe` to `PATH`, proving the resolver still returns the fixed SystemDirectory interpreter and cannot select the fake image. Source-scoped assertions require both mutating helper functions to call the trusted resolver and reject `$PSHOME`/`Get-Command` interpreter selection.

Focused Windows recovery verification:

- `cargo test --test recovery_powershell -- --nocapture`: PASS, 2 passed / 0 failed.
- `cargo fmt --all -- --check`: PASS.
- `cargo build --all-targets --all-features`: PASS.
- `cargo clippy --all-targets --all-features -- -D warnings`: PASS.
- `cargo test --all-targets --all-features`: two attempts ended at the CatDesk connector with HTTP 504 before a test result was returned; this is recorded as inconclusive transport evidence, not a repository test failure or success.

## Safety/acceptance disposition

No live helper invocation, daemon/tunnel restart, browser wake, protected wake-target edit, Scheduler mutation, release promotion, Git publication, or worktree cleanup occurred. T-0319 remains open for a genuinely separate independent review of the cumulative recovery hardening chain, now including this T-0337 change. The task queue allocated the new semantic work under a duplicated/misaligned `T-0336` label despite the durable next-id marker; that protected queue bookkeeping defect was not bypassed by direct edit, so this review bundle uses the unambiguous next semantic identifier T-0337.
