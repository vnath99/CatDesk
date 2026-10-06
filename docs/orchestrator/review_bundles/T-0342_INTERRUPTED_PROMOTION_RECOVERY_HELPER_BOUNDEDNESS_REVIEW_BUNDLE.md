# T-0342 — Interrupted promotion recovery helper boundedness review bundle

Date: 2026-09-07
Status: IMPLEMENTATION / FIXTURE COMPLETE; T-0319 INDEPENDENT FINAL REVIEW STILL REQUIRED
Scope: one-command recovery interrupted reviewed-promotion pair handling only. No live daemon/tunnel restart, Secure MCP ownership change, reviewed release promotion, browser wake, protected wake-target mutation, Scheduler mutation, Git publication, or dirty-worktree cleanup.

## Finding

`Invoke-InterruptedPromotionPairRecovery` in `scripts/start-catdesk-stack.ps1` detected an interrupted reviewed-promotion transaction and then invoked `scripts/promote-reviewed-catdesk-build.ps1` directly in the current PowerShell process using `& $promotion`. That helper is mutation-capable and reachable from the public `recover` path. A wedged helper could therefore block one-command recovery indefinitely even though wake repair, migration, Codex prerequisite probes, and the other recovery-native child boundaries had already received parent-side deadlines.

The helper's authority semantics were otherwise appropriately narrow: interrupted transaction state is not rollback authority by itself. Acceptance still requires the helper to prove `RECOVERED_PRIOR_CANONICAL`; otherwise `Repair-CanonicalReleaseForRecovery` may continue only to independently validated persistent LKG authority.

## Correction

T-0342 keeps that provenance boundary and changes only execution boundedness/interpreter authority:

- `Invoke-InterruptedPromotionPairRecovery` resolves Windows PowerShell through the existing fixed OS resolver, `[Environment]::SystemDirectory\WindowsPowerShell\v1.0\powershell.exe`.
- The promotion helper is launched through `Invoke-BoundedRecoveryHelper`, not direct in-process invocation.
- The production parent deadline is 30 seconds (`MaxInterruptedPromotionRecoveryMilliseconds`).
- Existing bounded native-process semantics apply: finite process deadline, bounded termination, bounded stdout/stderr capture and drain, output-overflow refusal, and nonzero-exit refusal.
- The existing 4 KiB promotion-result proof limit remains, and stdout must still contain exact `RECOVERED_PRIOR_CANONICAL` state before this path is accepted.
- Failure does not mint authority. The caller retains the existing behavior of falling back only to independently validated LKG; missing, damaged, ambiguous, or hash-mismatched LKG continues to fail closed.

## Regression evidence

`scripts/test-start-catdesk-stack.ps1` now:

1. parses `Invoke-InterruptedPromotionPairRecovery` and rejects any return to direct `& $promotion` execution;
2. requires `Resolve-TrustedWindowsPowerShellPath`, `Invoke-BoundedRecoveryHelper`, and the explicit promotion-recovery deadline;
3. creates a synthetic interrupted-transaction workspace whose promotion helper loops indefinitely;
4. reduces only the fixture deadline to 250 ms and proves control returns through the bounded timeout in under 3 seconds.

Verification in this work cycle:

- `cargo test --test recovery_powershell -- --nocapture` — PASS, 2/2.
- `cargo fmt --all -- --check` — PASS.
- `cargo test --bin catdesk` — PASS.
- `cargo build --all-targets --all-features` — PASS.
- `cargo clippy --all-targets --all-features -- -D warnings` — PASS.
- `git diff --check` — PASS; only existing Windows LF→CRLF working-copy warnings.
- A monolithic `cargo test --all-targets --all-features` request reached the connector timeout/HTTP 504 before returning a result. This bundle therefore does not claim a fresh monolithic all-target test result from that invocation; the focused recovery harness and main binary target are green.

## Acceptance boundary

T-0342 is implementation/fixture complete. It does not self-approve T-0319. A genuinely separate independent final reviewer must evaluate the cumulative T-0325 through T-0342 recovery authority/boundedness chain together with the existing watchdog/stale-daemon live evidence.

T-0324 remains separately gated on reviewed build/promotion of the Qwen continuation change followed by its bounded live canary. This ticket neither promotes the dirty worktree nor dispatches Qwen.
