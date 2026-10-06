# T-0349 — Rollback-only recovery observation independence

## Scope

T-0349 closes a residual dependency in the recovery-specific `-RecoveryRollbackOnly` path introduced by T-0348. The change is intentionally narrow and does not expand CatDesk ownership of the externally owned Secure MCP runtime.

## Finding

`Invoke-InterruptedPromotionRecovery` restored and revalidated the exact prior canonical binary/manifest pair before any daemon reconciliation, but it still resolved the transaction candidate and invoked `Get-PromotionListener` before checking `-RecoveryRollbackOnly`. `Get-PromotionListener` observes Windows TCP listener/process state. That observation is required for ordinary maintainer interrupted recovery, but it has no authority or correctness role in public rollback-only disk/provenance recovery and could unnecessarily consume the bounded promotion-helper deadline if Windows observation wedged.

## Implementation

After exact prior-pair validation, `Invoke-InterruptedPromotionRecovery` now handles `-RecoveryRollbackOnly` immediately: it clears the durable promotion transaction and returns the proven prior canonical evidence. Candidate-path resolution, candidate listener proof, `Invoke-PromotionHandoff`, and final canonical-listener proof remain unchanged for non-rollback-only maintainer recovery.

This preserves the existing protected reviewed-promotion authorization and transaction provenance checks performed before `Invoke-InterruptedPromotionRecovery`. It also preserves the public recovery parent's responsibility for subsequent bounded Windows inventory, listener/stale-daemon reconciliation, pinned process handling, and final convergence.

## Deterministic regression

`scripts/test-promote-reviewed-catdesk-build.ps1` now overrides the `ListenerForBuild` seam with a script block that throws if invoked during `-RecoveryRollbackOnly`. The rollback-only fixture must still return `RECOVERED_PRIOR_CANONICAL`, restore the exact prior canonical hash and manifest, clear the durable transaction, and record no handoff. Any regression that reintroduces listener/process observation into this path therefore fails deterministically before it can be mistaken for successful rollback.

Existing ordinary interrupted-recovery fixtures remain unchanged and continue to exercise the listener/handoff behavior, guarding against accidentally weakening maintainer recovery semantics.

## Verification performed

- CatDesk sanctioned verifier: `cargo fmt --check` passed.
- CatDesk sanctioned verifier: default full `cargo test` passed.
- CatDesk sanctioned verifier: `cargo build` passed.
- `cargo test --all-targets --all-features` passed.
- `cargo clippy --all-targets --all-features -- -D warnings` passed.
- `git diff --check` exited 0; only the repository's existing Windows LF-to-CRLF warnings were emitted.
- The dedicated PowerShell promotion fixture was not re-run in this session because CatDesk's protected allowlist shell rejects nested interpreter execution. No unrestricted-shell exception or bypass was requested. The fixture source contains the deterministic T-0349 seam assertion and remains part of the independent acceptance boundary.

## Runtime / operator safety

No live `recover`, reviewed promotion, CatDesk daemon/tunnel restart or migration, manual browser wake, protected wake-target mutation, Scheduler mutation, Git publication, provider re-probe, Qwen implementation dispatch, or dirty-worktree cleanup was performed. CatDesk remains attached to the already-running externally owned official Secure MCP runtime without creating a duplicate.

## Independent review boundary

T-0349 is implementation/repository-verification complete but is not self-accepted. T-0319 remains open for genuinely separate independent final review of cumulative T-0325 through T-0349. The reviewer should confirm that the rollback-only early return occurs only after exact prior-pair validation and the pre-existing protected authorization/provenance checks, that transaction cleanup occurs only after proven restoration, that rollback-only performs no listener/process observation or handoff, and that ordinary maintainer interrupted recovery still retains its listener/handoff proof path.

T-0324 remains separately gated on reviewed promotion before its bounded Qwen continuation canary.
