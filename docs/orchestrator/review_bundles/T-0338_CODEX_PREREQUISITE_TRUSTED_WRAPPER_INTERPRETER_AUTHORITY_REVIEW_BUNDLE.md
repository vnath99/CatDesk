# T-0338 — Codex prerequisite trusted wrapper interpreter authority

## Scope

Harden the supported `catdesk.ps1 plan` / `recover` Codex prerequisite path so discovered Codex wrapper files cannot delegate interpreter authority to caller-controlled environment state.

## Defect

T-0332 bounded `codex --version` and `codex login status`, but `Invoke-BoundedCodexProbe` still executed `.cmd` / `.bat` wrappers through `$env:ComSpec` and `.ps1` wrappers through `$PSHOME\powershell.exe`. A caller-controlled `ComSpec`, or a non-canonical invoking PowerShell installation reflected through `$PSHOME`, could therefore select the interpreter at a recovery prerequisite boundary. This was inconsistent with the fixed-OS interpreter authority established by T-0335/T-0337.

## Implementation

- Added `Resolve-TrustedWindowsCommandPromptPath` using `[Environment]::SystemDirectory\cmd.exe`.
- The command processor must resolve to a regular `FileInfo`, must not be a reparse point, and its resolved full path must exactly match the fixed OS-derived candidate.
- `.cmd` / `.bat` Codex wrappers now use that resolver rather than `$env:ComSpec`.
- `.ps1` Codex wrappers now use the existing `Resolve-TrustedWindowsPowerShellPath` rather than `$PSHOME`.
- Direct executable Codex discovery remains direct.
- T-0332's bounded process timeout, output cap, post-exit drain bound, and fail-closed behavior are unchanged.
- No serving daemon/tunnel ownership or browser wake authority is added.

## Regression evidence

`script/test-start-catdesk-stack.ps1` now:

- injects a fake caller-controlled `ComSpec` and PATH entry while exercising the real `.cmd` Codex shim;
- proves wrapper execution still succeeds through fixed System32 `cmd.exe`;
- proves the trusted command resolver cannot return the injected executable;
- statically rejects `$env:ComSpec`, `$PSHOME`, or `Get-Command cmd/powershell` inside `Invoke-BoundedCodexProbe`;
- requires both trusted interpreter resolvers to remain present in that probe.

## Verification

- `cargo test --test recovery_powershell -- --nocapture` — PASS, 2/2.
- `cargo fmt --all -- --check` — PASS.
- `cargo test --all-targets --all-features` — PASS, exit 0.
- `cargo build --all-targets --all-features` — PASS.
- `cargo clippy --all-targets --all-features -- -D warnings` — PASS.
- `git diff --check` — PASS after final documentation synchronization; exit 0 with only the pre-existing Windows LF→CRLF warnings.

## Acceptance impact

T-0338 is implementation/test complete but does not close T-0319. T-0319 still requires a genuinely separate independent final review of the cumulative T-0325 through T-0338 recovery-authority/boundedness chain. T-0324 remains separately gated on reviewed build/promotion and a bounded live Qwen continuation canary.

## Safety / operator boundary

No live daemon or tunnel restart, Secure MCP ownership change, release promotion, Scheduler mutation, browser wake invocation, protected wake-target edit, Git publication, or dirty-worktree cleanup is part of this ticket.
