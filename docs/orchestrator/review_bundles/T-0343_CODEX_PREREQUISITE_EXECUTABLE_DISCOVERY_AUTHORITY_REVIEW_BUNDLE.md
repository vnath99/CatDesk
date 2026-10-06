# T-0343 — Codex prerequisite executable discovery authority review bundle

Date: 2026-09-08
Status: IMPLEMENTATION / FIXTURE COMPLETE; T-0319 INDEPENDENT FINAL REVIEW STILL REQUIRED
Scope: one-command recovery Codex prerequisite executable discovery only. No live daemon/tunnel restart or migration, Secure MCP ownership change, reviewed release promotion, browser wake, protected wake-target mutation, Scheduler mutation, Git publication, provider re-probe, Qwen dispatch, or dirty-worktree cleanup.

## Finding

T-0338 fixed interpreter authority after a Codex wrapper had already been selected, but `Test-CodexOnDemandPrerequisites` in `scripts/start-catdesk-stack.ps1` still selected `codex` with `Get-Command "codex"`. That made caller-controlled `PATH` part of recovery prerequisite authority: an earlier shadow `codex.cmd`/`codex.ps1` could be selected and, if it returned plausible success for the bounded `--version` and `login status` probes, falsely clear public `plan`/`recover` preflight.

The existing bounded probe behavior was otherwise correct: process deadline, bounded termination, bounded output capture/drain, output cap, nonzero-exit refusal, and the T-0338 fixed-OS wrapper interpreter selection remain unchanged.

## Correction

T-0343 adds `Resolve-TrustedCurrentUserCodexCommand` and removes caller-PATH/npm-command discovery from this recovery boundary:

- Existing explicit inherited `CATDESK_CODEX_CLI_EXECUTABLE` retains precedence, preserving the established recovery override contract.
- Without that override, recovery considers only the deterministic current-user npm command directory derived from `[Environment]::GetFolderPath('ApplicationData')`, with candidates `npm\codex.exe`, `npm\codex.cmd`, and `npm\codex.ps1`.
- A selected candidate must exist as an actual `FileInfo`, must not be a reparse point, and its resolved full path must exactly match the expected candidate path.
- `Test-CodexOnDemandPrerequisites` now calls that resolver rather than `Get-Command codex`.
- `Invoke-BoundedCodexProbe` and T-0338's fixed System32 `cmd.exe` / fixed OS Windows PowerShell interpreter authority remain intact.
- Missing or ambiguous trusted discovery fails closed as `Codex CLI is unavailable`; no fallback to caller `PATH`, `npm root`, `npm prefix`, or `npm config` is allowed.

## Regression evidence

`scripts/test-start-catdesk-stack.ps1` now:

1. loads and requires `Resolve-TrustedCurrentUserCodexCommand` in the recovery fixture;
2. uses the explicit `CATDESK_CODEX_CLI_EXECUTABLE` contract for the existing synthetic Codex prerequisite probe;
3. creates a trusted synthetic current-user npm command root and a different fake `codex.cmd` in a directory prepended to caller `PATH`;
4. removes the explicit override, resolves against the trusted root, and proves the trusted candidate is selected while the PATH shadow is rejected;
5. statically rejects reintroduction of `Get-Command codex`, `$env:PATH`, or `npm root`/`npm prefix`/`npm config` discovery inside the trusted resolver;
6. restores prior PATH and prior `CATDESK_CODEX_CLI_EXECUTABLE` state after the fixture.

Verification in this work cycle:

- `cargo test --test recovery_powershell -- --nocapture` — PASS, 2/2.
- `cargo fmt --all -- --check` — PASS.
- `cargo test --all-targets --all-features` — PASS; main suite 876 passed / 0 failed / 21 ignored and remaining integration targets completed successfully.
- `cargo build --all-targets --all-features` — PASS.
- `cargo clippy --all-targets --all-features -- -D warnings` — PASS.
- `git diff --check` — PASS; only existing Windows LF→CRLF working-copy warnings.

## Acceptance boundary

T-0343 is implementation/fixture complete. It does not self-approve T-0319. A genuinely separate independent final reviewer must evaluate the cumulative T-0325 through T-0343 recovery authority/boundedness chain together with the existing watchdog/stale-daemon live evidence.

T-0324 remains separately gated on reviewed build/promotion of the Qwen continuation change followed by its bounded live canary. This ticket neither promotes the dirty worktree nor dispatches Qwen.
