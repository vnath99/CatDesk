# T-0359 — Independent-review repair review bundle

Date: 2026-09-08
Status: IMPLEMENTED / VERIFIED / FRESH INDEPENDENT RE-REVIEW REQUIRED

## Trigger

The genuinely separate Codex/Terra-high T-0319 review session `adc-t0319-r1-independent-recovery-final-review-20260908` completed with the explicit verdict `REJECT_WITH_CONCRETE_DEFECTS`.

It identified two concrete acceptance blockers:

1. `scripts/restart_catdesk_daemon.ps1` selected `restart_catdesk_daemon_worker.ps1` for the trusted Windows PowerShell `-File` handoff using only workspace-relative construction plus leaf existence. The trusted interpreter was pinned, but the project-owned worker script itself was not required to be the exact regular non-reparse path before execution.
2. That reviewer observed one failure of the stale-canonical-daemon recovery fixture during its verification turn, at the candidate-enumeration assertion.

This ticket repairs only evidence-backed blockers. It does not reopen the broad recovery-hardening sweep.

## Implementation

### Restart worker executable-script identity

`restart_catdesk_daemon.ps1` now defines `Resolve-TrustedRestartWorkerScriptPath` and requires the detached worker script to satisfy the same narrow executable-authority properties used by the other hardened project-owned recovery helpers before the trusted PowerShell `-File` handoff:

- derive the exact expected path from `$PSScriptRoot`;
- normalize it through `IO.Path.GetFullPath`;
- resolve with `Get-Item -Force`;
- require a real `System.IO.FileInfo`;
- reject `FileAttributes.ReparsePoint`;
- normalize the observed `FullName`;
- require exact `OrdinalIgnoreCase` expected/observed identity;
- pass only that validated path to the existing trusted Windows PowerShell detached handoff.

No restart ownership, old-process identity, process termination, readiness, reviewed-promotion, or Secure MCP behavior was widened.

### Regression coverage

`scripts/test-restart-catdesk-daemon-process-identity.ps1` now requires the worker resolver and statically guards `Get-Item`, `FileInfo`, `ReparsePoint`, `GetFullPath`, and `OrdinalIgnoreCase`. It also requires the launcher to assign `$worker = Resolve-TrustedRestartWorkerScriptPath` and rejects a regression to the earlier `Join-Path` + `Test-Path -PathType Leaf` trust shape.

## Stale-daemon fixture review finding

No production process-selection or ambiguity semantics were changed for the reviewer's second finding because the failure is not reproducible against the current candidate.

Before the T-0359 source change, `cargo test --test recovery_powershell -- --nocapture` passed 3/3, including `windows::lifecycle_and_reviewed_release_recovery_fixtures_pass`. After the T-0359 code/test change, the same sanctioned Rust-to-PowerShell target again passed 3/3. The stale fixture already proves the real `Get-CatDeskDaemonProcessCandidates` path sees its exact temporary canonical process before installing its isolated process-set seam. A direct nested PowerShell rerun was correctly rejected by CatDesk shell policy and was not bypassed.

Therefore this ticket does not weaken foreign-row ambiguity refusal, exact canonical path/hash matching, process-instance selection, or stale-daemon termination authority merely to chase a single non-reproducing review-time failure. A fresh independent reviewer must decide whether the repeated current passes are sufficient evidence.

## Verification

Current candidate verification is green:

- `cargo test --test recovery_powershell -- --nocapture` — PASS, 3/3 (repeated before and after the repair).
- `cargo fmt --all -- --check` — PASS.
- `cargo test --all-targets --all-features` — PASS.
- `cargo build --all-targets --all-features` — PASS.
- `cargo clippy --all-targets --all-features -- -D warnings` — PASS.
- `git diff --check` — PASS; only the existing LF-to-CRLF working-copy warnings were emitted.

## Authority / non-actions

No live `recover`, daemon/tunnel restart or migration, reviewed release promotion, raw reload, browser wake, protected wake-target edit, Scheduler/service mutation, Git publication, provider-authentication mutation, or dirty-worktree cleanup was performed. The externally owned official Secure MCP runtime remains outside CatDesk mutation authority.

## Required independent decision

Run a fresh genuinely separate Codex/Terra-high T-0319 review. The reviewer must independently verify both prior blockers rather than inheriting this bundle's conclusion:

1. confirm the detached restart worker cannot be redirected through a reparse or path-identity substitution before the trusted PowerShell `-File` handoff;
2. rerun the sanctioned recovery PowerShell integration target and determine whether stale-daemon candidate enumeration is currently reliable enough for T-0319 acceptance;
3. re-check the cumulative T-0325-through-T-0359 recovery authority/boundedness boundary relevant to T-0319.

Allowed verdicts remain exactly `ACCEPT`, `REJECT_WITH_CONCRETE_DEFECTS`, or `INSUFFICIENT_EVIDENCE`. T-0319 must not be self-accepted by the implementation lineage.
