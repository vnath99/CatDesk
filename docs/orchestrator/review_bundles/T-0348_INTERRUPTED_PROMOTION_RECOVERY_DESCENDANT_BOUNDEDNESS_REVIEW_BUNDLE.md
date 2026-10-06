# T-0348 — Interrupted-promotion recovery descendant boundedness closure

Status: implementation and repository verification complete; independent acceptance remains under T-0319.

## Finding

T-0342 correctly moved interrupted reviewed-promotion recovery behind a trusted Windows PowerShell child and a 30-second parent deadline, but that bounded child could still call `Invoke-PromotionHandoff`. The promotion handoff invokes `restart_catdesk_daemon.ps1`, which launches a detached restart worker and permits up to 120 seconds for readiness. A timeout of the bounded promotion parent therefore did not prove that all descendant mutation authority had ended: the detached worker could outlive the parent deadline.

This is a public one-command `recover` concern, not a reason to expand ownership of the external Secure MCP runtime. The issue is limited to CatDesk daemon handoff during interrupted reviewed-promotion rollback.

## Correction

T-0348 separates interrupted disk/provenance rollback from daemon reconciliation when the promotion helper is called by public recovery:

- `promote-reviewed-catdesk-build.ps1` adds `-RecoveryRollbackOnly`.
- The switch is accepted only when an interrupted promotion transaction exists; it does not create a new promotion path.
- Existing protected reviewed-promotion authorization remains required. Interrupted transaction state still does not mint rollback authority.
- The prior canonical binary/manifest pair is restored when necessary and revalidated against the transaction's prior hash exactly as before.
- When `-RecoveryRollbackOnly` is present, a live transaction-candidate listener does **not** trigger `Invoke-PromotionHandoff`, so the bounded promotion child cannot launch the detached restart worker.
- The durable promotion transaction is cleared only after the prior canonical pair has been validated.
- `start-catdesk-stack.ps1::Invoke-InterruptedPromotionPairRecovery` passes `-RecoveryRollbackOnly` only on its bounded interrupted-recovery child invocation.
- After that child returns exact `RECOVERED_PRIOR_CANONICAL` proof, the existing recovery parent continues through canonical identity reload, exact listener/process inventory, pinned-process stop semantics where necessary, stale-daemon reconciliation, and final bounded canonical convergence. Ordinary maintainer promotion retains its pre-existing handoff behavior.

The change does not alter Secure MCP runtime ownership, wake authority, release-promotion authorization, or LKG authority.

## Regression evidence

`scripts/test-promote-reviewed-catdesk-build.ps1` now creates an interrupted candidate-over-prior fixture and invokes the promotion helper with `-RecoveryRollbackOnly`. It requires:

1. `RECOVERED_PRIOR_CANONICAL` result;
2. exact restoration of the prior canonical binary hash and manifest;
3. removal of the durable interrupted transaction only after validated rollback; and
4. no recorded `handoff:` invocation.

`scripts/test-start-catdesk-stack.ps1` statically requires `Invoke-InterruptedPromotionPairRecovery` to pass `-RecoveryRollbackOnly`. The T-0342 indefinitely hanging synthetic promotion helper was updated to accept the new switch and continues to prove that the parent deadline returns control rather than hanging.

Verification completed on 2026-09-08:

- `cargo test --test recovery_powershell -- --nocapture`: PASS, 2 passed / 0 failed.
- sanctioned `verify_project`: PASS (`cargo fmt --check`, `cargo test`, `cargo build`).
- `cargo test --all-targets --all-features`: PASS.
- `cargo clippy --all-targets --all-features -- -D warnings`: PASS.
- `git diff --check`: PASS; only pre-existing LF-to-CRLF working-copy warnings were emitted.

No live `recover`, reviewed promotion, CatDesk daemon/tunnel restart or migration, manual browser wake, protected wake-target mutation, Scheduler mutation, Git publication, provider re-probe, Qwen implementation dispatch, or dirty-worktree cleanup was performed for this evidence.

## Independent review boundary

T-0348 is not self-accepted. T-0319 remains open and requires a genuinely separate independent final review of the cumulative T-0325 through T-0348 recovery authority/boundedness chain, including confirmation that recovery-specific rollback-only semantics do not weaken reviewed-promotion provenance and that subsequent parent-owned listener reconciliation is sufficient.

T-0324 remains separately gated on reviewed promotion before its bounded Qwen continuation canary.