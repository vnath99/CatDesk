# T-0330 — Promotion-to-restart exact listener identity handoff

Date: 2026-09-07
Status: IMPLEMENTED_AND_VERIFIED_PENDING_T0319_INDEPENDENT_ACCEPTANCE

## Scope

Close the residual PID-reuse authority seam between `Get-PromotionListener` in `scripts/promote-reviewed-catdesk-build.ps1` and the detached restart launcher `scripts/restart_catdesk_daemon.ps1` without changing Secure MCP ownership, reviewed promotion authority, wake state, or the one-command recovery contract.

T-0328 already binds restart launcher -> worker mutation/readiness to an exact process instance. T-0330 closes the earlier promotion preflight -> restart launcher boundary so a PID that is reused after promotion validates a listener cannot silently become the restart target.

## Defect

Before T-0330, `Get-PromotionListener` proved one listener by PID/path/hash but returned only `Pid`. `Invoke-PromotionHandoff` later invoked `restart_catdesk_daemon.ps1` with only that numeric PID. The launcher then reacquired the PID and established a fresh process identity. If the reviewed listener exited and the PID was reused in that interval, the newly reacquired process could become restart authority even though it was not the process instance promotion had reviewed. T-0328 did not cover this earlier handoff seam.

## Implementation

### Promotion listener proof

`Get-PromotionListener` now:

- acquires/pins the selected `System.Diagnostics.Process` handle before recording identity;
- reuses that pinned process object for process-name/path/start-time evidence;
- retains the already-required executable SHA-256 proof; and
- returns `Pid`, exact `ProcessStartedAtUtc`, resolved `BuildPath`, and verified `BuildHash`.

### Promotion -> restart handoff

`Invoke-PromotionHandoff` now requires identity-complete listener evidence and passes:

- `-ExpectedOldProcessStartedAtUtc`,
- `-ExpectedOldProcessPath`, and
- `-ExpectedOldProcessSha256`

to `restart_catdesk_daemon.ps1` together with the PID.

### Restart launcher fail-closed proof

`restart_catdesk_daemon.ps1` adds optional expectation parameters so ordinary direct callers remain backward compatible. When any promotion expectation is supplied, `Test-ExpectedOldProcessIdentity` pins the reacquired process object and requires the exact creation tick, resolved executable path, and expected SHA-256 to match before restart intent is written or the detached worker is launched. A same-path process replacement with a different creation identity is rejected.

The launcher continues to pass its freshly pinned exact start/path identity to the T-0328 worker, preserving the existing launcher -> worker destructive-process guarantee.

## Regression coverage

`scripts/test-restart-catdesk-daemon-process-identity.ps1` now extracts and executes the launcher identity helper and proves:

1. the exact reviewed listener instance is accepted;
2. a same-path/same-hash but different-start-time process instance is rejected;
3. an incorrect reviewed executable hash is rejected;
4. promotion source pins the listener and records start/path/hash; and
5. promotion transfers all three identity fields into the restart launcher.

`tests/recovery_powershell.rs` explicitly executes both `scripts/test-promote-reviewed-catdesk-build.ps1` and `scripts/test-restart-catdesk-daemon-process-identity.ps1`.

During the first bounded harness run, an unrelated test-only clock-skew assertion proved flaky: the fixture used `UtcNow + 31s` against a production tolerance of 30 seconds while native evidence is stored at whole-Unix-second precision. The fixture margin was changed from +31s to +60s in both future-evidence cases. Production timing/tolerance semantics were not changed.

## Verification

- `cargo test --test recovery_powershell -- --nocapture` — PASS, 2/2 after the deterministic fixture-margin repair.
- `cargo fmt --check` — PASS.
- full `cargo test` — PASS.
- `cargo build` — PASS.
- `cargo clippy --all-targets --all-features -- -D warnings` — PASS.
- `git diff --check` — PASS; only pre-existing LF -> CRLF warnings were emitted.

## Safety / non-mutation evidence

This ticket did not:

- restart, replace, or take ownership of the externally owned Secure MCP runtime;
- promote or replace canonical CatDesk bytes;
- invoke the browser wake bridge;
- edit protected wake-target state;
- mutate Scheduler state;
- publish Git state; or
- clean/reset the intentionally dirty worktree.

The work is repository-only hardening plus disposable fixture execution.

## Acceptance boundary

T-0330 is implementation/test complete, but it does not self-close T-0319. T-0319 must receive a genuinely separate independent final review of the cumulative destructive-process/recovery authority chain, now T-0325/T-0326/T-0327/T-0328/T-0329/T-0330, together with the existing watchdog/stale-daemon live evidence.

No independent Terra-high endpoint is currently exposed and Codex remains operator-configuration-blocked, so this implementation lineage must not manufacture that final acceptance.
