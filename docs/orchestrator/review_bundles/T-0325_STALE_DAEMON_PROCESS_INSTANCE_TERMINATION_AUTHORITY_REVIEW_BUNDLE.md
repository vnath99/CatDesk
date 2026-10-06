# T-0325 — Stale-daemon process-instance termination authority hardening

Date: 2026-09-07
Status: COMPLETE — source/test accepted; no live destructive host mutation performed

## Trigger

The T-0323 acceptance sweep found a narrow destructive-process authority race in `Stop-StaleCanonicalCatDeskDaemonForRecovery`.

Before T-0325, recovery correctly discovered daemon candidates from `Win32_Process`, required the `--catdesk-daemon` mode, validated exact canonical path and SHA-256, rejected foreign/multiple candidates, and rechecked that no canonical loopback listener had appeared. However, the final mutation was still:

`Stop-Process -Id <validated PID>`

followed by PID-based polling.

A selected stale canonical process could exit after validation and its PID could theoretically be reused before the final stop. The mutation target could therefore differ from the process instance whose path/hash/mode had been reviewed.

## Correction

### Candidate identity now includes creation time

`Get-CatDeskDaemonProcessCandidates` now records `CreationTimeUtc` from the authoritative `Win32_Process.CreationDate` together with PID and exact canonical path/hash classification. Missing or invalid creation-time identity is treated as ambiguous and fails closed.

### Exact process instance is reacquired before mutation

A new helper, `Get-CatDeskDaemonProcessInstanceForRecovery`, receives the canonical identity and the selected candidate and then:

1. performs a fresh bounded `Win32_Process` query for the selected PID;
2. returns no process if the selected instance has already exited;
3. requires exactly one process row;
4. requires the `--catdesk-daemon` command-line token;
5. requires exact creation-time equality with the selected candidate;
6. resolves the executable path and SHA-256 and requires an exact match to the canonical binary identity;
7. reacquires the corresponding `System.Diagnostics.Process` object and verifies its path;
8. forces acquisition of the underlying process handle before the destructive boundary.

A process replacement/PID reuse therefore produces the fixed fail-closed error `CatDesk daemon process instance changed before recovery mutation` rather than reaching termination.

### Kill and wait use the same pinned process object

`Stop-StaleCanonicalCatDeskDaemonForRecovery` now:

- performs the original no-listener check;
- performs bounded candidate discovery and foreign/multiple refusal;
- obtains the exact pinned process instance;
- performs the listener check again **after** the process instance has been pinned;
- calls `Kill()` on that exact process object;
- waits using the same process object/handle with `WaitForExit(30000)`.

It no longer issues a PID-only `Stop-Process` or PID-based exit polling for this stale-daemon path.

If the exact selected instance exits before acquisition, no kill occurs and recovery may start the one canonical replacement. If the pinned exact instance exits between pinning and `Kill()`, `InvalidOperationException` is treated as safe convergence because the reviewed stale process is already gone; a replacement PID cannot be redirected into the mutation.

## Tests

`tests/start-catdesk-stack` PowerShell fixtures were extended to include exact process-instance semantics.

Deterministic cases now prove:

- the normal stale canonical daemon/no-listener path still performs one bounded stop and launches one canonical replacement;
- if the selected PID now represents a changed process instance, recovery fails closed with **zero mutation**;
- if the exact selected stale process has simply disappeared before acquisition, recovery performs no PID-directed stop and launches only the canonical replacement;
- candidate discovery requires valid creation-time identity;
- the real disposable stale canonical daemon fixture continues to pass using the new production helper.

`test-stale-canonical-daemon-recovery.ps1` was updated to extract the new helper into the bounded fixture.

## Verification

Post-correction verification:

- `cargo test --test recovery_powershell -- --nocapture` — PASS
  - `lifecycle_and_reviewed_release_recovery_fixtures_pass`
  - `stale_canonical_daemon_recovery_real_process_fixture_pass`
- full project verification — PASS
  - `cargo fmt --check`
  - `cargo test`
  - `cargo build`
- `cargo clippy --all-targets --all-features -- -D warnings` — PASS
- `git diff --check` — PASS; only existing CRLF normalization warnings were emitted

The full Rust test run reported 377 passing tests in the main suite with no failures, and all integration suites passed.

## T-0319 relationship

T-0319's prior watchdog/stale-daemon evidence remains useful, but its destructive stale-daemon mutation boundary is now materially stronger. Any independent final review of T-0319 should incorporate the T-0325 process-instance proof rather than relying on the older PID-only stop behavior.

T-0325 does **not** by itself convert T-0319 into live watchdog-only final acceptance; it removes a concrete source/test authority race that was discovered during independent review.

## Residual related scope

A bounded search after T-0325 found two other PID-only stop boundaries in `scripts/start-catdesk-stack.ps1` outside this ticket's stale-daemon function:

- `Stop-CanonicalPathCatDeskForReleaseRepair`
- the existing-listener `mustRestart` / migration path

They were not silently broadened into T-0325. T-0326 tracks exact process-instance hardening for those remaining recovery mutation sites with the same no-PID-reuse objective.

## Safety invariants preserved

- No live canonical daemon was deliberately stopped for acceptance.
- No Secure MCP/tunnel process or configuration was changed.
- No browser/wake target or profile state was touched.
- No Scheduler/service state was changed.
- No canonical release bytes were promoted or rebuilt by recovery.
- No Git publication occurred.
- Foreign/multiple daemon candidates still fail closed.
- Exact canonical path/hash/mode and no-listener proof remain required.
- The public one-command recovery behavior remains intact while destructive authority is narrowed to the exact selected process instance.

## Independent conclusion

**ACCEPT T-0325.** The prior PID-only final mutation could, in principle, terminate a replacement process after PID reuse. Recovery now binds stale-daemon termination and exit waiting to the exact canonical process instance validated for recovery, and deterministic replacement/disappearance cases plus the real disposable-process fixture pass.
