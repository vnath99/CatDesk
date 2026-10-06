# T-0351 — Public lifecycle bootstrap-engine executable identity closure

## Summary

Public `catdesk.ps1 start/recover` previously dot-sourced `scripts/start-catdesk-stack.ps1` after only leaf-existence validation. Because the shared T-0350 helper executable-identity validator is defined inside that lifecycle engine, it could not protect the engine bootstrap itself. A reparse-point replacement of the facade-relative engine leaf could therefore gain PowerShell execution authority before the recovery policy was loaded.

## Change

- Derive the exact facade-relative lifecycle-engine path and normalize it with `System.IO.Path::GetFullPath`.
- Resolve the engine with `Get-Item -Force -ErrorAction Stop` and require a real `System.IO.FileInfo` leaf.
- Reject `System.IO.FileAttributes::ReparsePoint` before execution.
- Normalize the observed `FileInfo.FullName` and require exact `OrdinalIgnoreCase` equality with the expected path.
- Dot-source only the validated observed path.
- Preserve all existing lifecycle/runtime ownership, restart, migration, recovery, promotion, and external Secure MCP boundaries.

## Regression coverage

`tests/recovery_powershell.rs::public_facade_validates_lifecycle_engine_identity_before_dot_source` requires the expected-path normalization, real-file check, reparse rejection, and exact normalized identity comparison to occur before the lifecycle engine is dot-sourced. It also rejects regression to the former leaf-existence-only `Test-Path -PathType Leaf` authority check.

## Verification

- `cargo test --test recovery_powershell -- --nocapture` — PASS, 3/3 tests.
- Sanctioned project verifier — PASS: formatting, default full Cargo tests, and build.
- `cargo test --all-targets --all-features` — PASS.
- `cargo clippy --all-targets --all-features -- -D warnings` — PASS.
- `git diff --check` — PASS before durable-document synchronization; rerun after synchronization is required for final acceptance evidence.

No live `recover`, daemon/tunnel restart or migration, reviewed release promotion, browser wake, protected wake-target mutation, Scheduler mutation, external-project mutation, Git publication, provider re-probe, or Qwen implementation dispatch occurred. The already-running externally owned official Secure MCP runtime and the pre-existing dirty worktree were preserved.

## Acceptance boundary

T-0351 implementation and repository verification are complete. This ChatGPT lineage does not self-accept the cumulative recovery chain. T-0319 remains open for a genuinely separate independent final review of cumulative T-0325 through T-0351 recovery authority/boundedness evidence. T-0324 remains separately gated on reviewed promotion followed by its bounded live Qwen continuation canary.
