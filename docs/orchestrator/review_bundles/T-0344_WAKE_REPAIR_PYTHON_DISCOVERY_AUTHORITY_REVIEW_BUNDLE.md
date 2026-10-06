# T-0344 — Wake-repair Python discovery authority review bundle

Date: 2026-09-08
Status: IMPLEMENTATION / REPOSITORY VERIFICATION COMPLETE; WINDOWS RECOVERY HARNESS POLICY-BLOCKED IN THIS RUN; T-0319 INDEPENDENT FINAL REVIEW STILL REQUIRED
Scope: default Python discovery used by wake-runtime repair in one-command recovery. No live daemon/tunnel restart or migration, Secure MCP ownership change, browser wake invocation, protected wake-target mutation, Scheduler mutation, reviewed-build promotion, or Git publication.

## Finding

The T-0340 bounded Python-launcher fix still selected `py.exe` and `python.exe` through `Get-Command` and built fallback paths from caller-controlled `LOCALAPPDATA` / `USERPROFILE`. Because public recovery invokes `repair_wake_bridge_environment.ps1` without an explicit `-PythonExecutable`, caller environment state could influence which interpreter was executed during wake-runtime repair even though launcher execution itself was bounded.

## Change

`Resolve-Python3` now preserves explicit `-PythonExecutable` as intentional operator authority, but default discovery no longer consults `Get-Command`, `PATH`, `LOCALAPPDATA`, or `USERPROFILE` environment variables. It derives LocalAppData and UserProfile from `Environment.GetFolderPath`, derives the Windows root from `Environment.SystemDirectory`, and considers only deterministic Python launcher/interpreter locations.

Every default-discovered launcher/interpreter passes `Get-TrustedPythonExecutableCandidate`, which requires a real leaf file, rejects reparse points, canonicalizes the full path, and requires exact case-insensitive resolved-path identity before execution. A Python path reported by trusted `py.exe` is subjected to the same validator before the existing Python 3.10+ validation.

T-0340's local 5-second launcher deadline, temporary-file output channel, and bounded Python validation remain intact.

## Regression coverage

`scripts/test-start-catdesk-stack.ps1` now guards the recovery resolver against:

- direct launcher execution rather than `Invoke-NativeQuiet`;
- loss of the explicit 5-second launcher timeout;
- `Get-Command py.exe` / `Get-Command python.exe` discovery;
- `$env:PATH`, `$env:LOCALAPPDATA`, or `$env:USERPROFILE` discovery authority;
- loss of trusted special-folder derivation;
- loss of reparse-point rejection or exact resolved-path comparison.

The fixture creates fake `py.exe` and `python.exe` PATH shadows and verifies the resolver source has no mechanism to consult or capture that caller PATH.

## Verification

Sanctioned `verify_project` completed successfully:

- `cargo fmt --check`: PASS
- `cargo test`: PASS
- `cargo build`: PASS

Additional checks:

- `cargo clippy --all-targets --all-features -- -D warnings`: PASS
- `git diff --check`: PASS (existing Windows LF→CRLF warnings only)

The dedicated PowerShell recovery harness could not be launched through `run_command` because CatDesk's allowlist shell mode correctly rejects nested PowerShell/interpreter execution (`SHELL_MODE_BLOCKED`). No unrestricted-shell exception was requested and no policy bypass was used. Therefore this bundle does **not** claim a fresh 2/2 Windows recovery-harness result for T-0344; that fixture remains pending execution through an approved harness/profile or later independent review.

## T-0319 relationship

T-0344 closes a concrete caller-environment executable-authority seam found while continuing the T-0319 one-command recovery audit. It does not self-manufacture final T-0319 acceptance. A genuinely separate reviewer must evaluate the cumulative T-0325 through T-0344 recovery authority/boundedness chain together with existing watchdog/stale-daemon evidence and should execute the Windows recovery harness through an approved path before final acceptance.

## Reviewer decision requested

Accept T-0344 at the implementation/repository-verification boundary if the default Python discovery authority and fail-closed identity validation are correct. Do not close T-0319 from this bundle alone; require the separate independent review and approved Windows harness execution described above.
