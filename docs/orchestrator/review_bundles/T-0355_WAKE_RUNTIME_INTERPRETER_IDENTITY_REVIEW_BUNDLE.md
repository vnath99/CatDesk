# T-0355 — Wake runtime interpreter executable identity closure

Date: 2026-09-08
Status: IMPLEMENTATION / REPOSITORY VERIFICATION COMPLETE; T-0319 INDEPENDENT FINAL REVIEW STILL REQUIRED
Scope: public one-command recovery wake-runtime health probe only. No live recovery, browser wake, target mutation, serving daemon/tunnel mutation, release promotion, Scheduler mutation, Git publication, or dirty-worktree cleanup.

## Finding

`Test-WakeBridgeRuntime` in `scripts/start-catdesk-stack.ps1` treated the provisioned wake virtual-environment interpreter `.catdesk\wake-bridge\venv\Scripts\python.exe` as executable recovery authority after only `Test-Path -PathType Leaf`. Other recovery-reachable executable boundaries had already converged on `Resolve-TrustedBootstrapHelperPath`, which requires a real `FileInfo` leaf, rejects reparse points, normalizes the expected and observed full paths, and requires exact path identity before execution. The wake venv interpreter therefore remained a policy-drift seam: a reparse-point replacement could redirect the bounded health probe even though the subprocess itself was deadline-bounded.

## Correction

`Test-WakeBridgeRuntime` now resolves the exact workspace-relative wake venv interpreter through `Resolve-TrustedBootstrapHelperPath` before `Invoke-BoundedRecoveryHelper` can execute it. Invalid identity remains a fail-closed unhealthy-runtime result through the existing `try/catch`; the existing bounded probe and repair/convergence behavior are unchanged.

This change does not broaden interpreter discovery, grant PATH authority, alter browser ownership, or change the externally owned Secure MCP runtime. It only applies the already-reviewed shared exact non-reparse executable identity boundary to the fixed wake-runtime interpreter.

## Regression coverage

`scripts/test-start-catdesk-stack.ps1` now statically requires the production `Test-WakeBridgeRuntime` function to:

- invoke `Resolve-TrustedBootstrapHelperPath` for the interpreter;
- reject regression to the former `Test-Path -LiteralPath $python -PathType Leaf` existence-only authority check; and
- continue routing execution through `Invoke-BoundedRecoveryHelper -FileName $python` after identity validation.

The existing focused Rust→PowerShell recovery harness exercises the updated production script and remains green.

## Verification

The following completed successfully after the source change:

- `cargo test --test recovery_powershell lifecycle_and_reviewed_release_recovery_fixtures_pass -- --nocapture`
- sanctioned `verify_project` repository verification
- `cargo test --all-targets --all-features`
- `cargo clippy --all-targets --all-features -- -D warnings`

Final post-documentation `git diff --check` exits 0; only the pre-existing LF→CRLF working-copy warnings remain.

## T-0319 relationship

T-0355 is implementation/repository-verification complete but is not an independent acceptance verdict. T-0319 remains open for a genuinely separate final review of the cumulative T-0325 through T-0355 recovery authority/boundedness chain and existing watchdog/stale-daemon/one-command host evidence. This implementation lineage must not self-accept that boundary.

## Preserved boundaries

- externally owned official Secure MCP runtime remains outside CatDesk recovery ownership;
- no duplicate tunnel/runtime creation;
- dirty worktree is preserved;
- no live daemon/tunnel restart or migration;
- no release promotion or canonical blessing;
- no manual browser wake and no protected wake-target mutation;
- no Scheduler mutation;
- no Git publication.
