# T-0326 — Remaining recovery process-instance stop authority

Date: 2026-09-07
Status: SOURCE / FIXTURE ACCEPTED
Scope: repository recovery hardening only; no live daemon, tunnel, browser, Scheduler, protected wake-target, or Git mutation.

## Problem reviewed

T-0325 removed the PID-reuse mutation race from stale/no-listener daemon retirement, but two destructive recovery sites in `scripts/start-catdesk-stack.ps1` still used `Stop-Process -Id` followed by PID polling:

1. `Stop-CanonicalPathCatDeskForReleaseRepair`; and
2. the existing-listener `mustRestart` / `external_foreground` migration path in `Invoke-CanonicalStackBootstrap`.

Both sites first gathered useful listener/path evidence, but the final mutation was still addressed by PID. If the selected process exited and Windows reused that PID before the stop, the process receiving the mutation was no longer strongly bound to the instance that passed recovery attribution.

## Implemented boundary

### Observed listener identity

`Get-LoopbackCatDeskListener` now returns a bounded, process-instance-qualified listener identity:

- PID;
- UTC creation time from `Win32_Process.CreationDate`;
- executable path;
- SHA-256 of that path;
- fixed `--catdesk-daemon` command-line mode; and
- whether path+hash match the supplied canonical identity.

The listener still preserves the prior loopback/dual-stack ownership rules: at most the expected IPv4/IPv6 pair, one unique owner, no third row, and no foreign/non-daemon process acceptance.

### Exact process acquisition

`Get-CatDeskListenerProcessInstanceForRecovery` re-resolves the selected PID immediately before mutation and requires the same creation time, daemon mode, executable path, and SHA-256. It then obtains a `System.Diagnostics.Process` object and forces acquisition of its underlying process handle.

A changed or ambiguous process instance fails closed. If the selected process has already exited, the helper returns no process rather than targeting a replacement PID.

### Final mutation

`Stop-CatDeskListenerProcessForRecovery`:

1. acquires the exact process instance above;
2. rechecks that any still-present configured listener is the same PID + creation time + path + hash candidate;
3. calls `Kill()` on the pinned process object, never `Stop-Process -Id`;
4. uses bounded `WaitForExit(30000)` on that same process object; and
5. treats `InvalidOperationException` after pinning as the selected process already having exited, without redirecting to a replacement PID.

No PID polling is used for completion.

## Release-repair application

`Stop-CanonicalPathCatDeskForReleaseRepair` now derives an observed identity from the current on-disk canonical path and its current SHA-256, verifies that the configured MCP listener owns exactly those observed bytes, and delegates the stop to the pinned listener-process boundary.

This is intentionally different from requiring the *reviewed target* hash: release repair exists precisely because current canonical bytes may be damaged or stale. The mutation is nevertheless bound to the exact observed process instance and exact current canonical path/bytes before replacement from reviewed LKG authority.

## Existing-listener / migration application

The `mustRestart` / `external_foreground` path no longer calls `Stop-Process`. It stops only the exact listener instance returned by the verified listener observation. After the stop, it checks that no replacement listener appeared before migration/start proceeds; a replacement race fails closed instead of being killed by reused PID authority.

External Secure MCP ownership is unchanged. The recovery path still does not create, replace, or terminate the externally owned tunnel runtime.

## Adversarial fixture coverage

`scripts/test-start-catdesk-stack.ps1` now covers:

- the ordinary noncanonical-listener reload path still produces one exact stop followed by one canonical daemon start;
- a selected listener PID that resolves to a changed process instance fails before any kill/start/migration action;
- a different listener introduced after process acquisition but before kill fails before the pinned process can be killed;
- release repair rejects a changed selected process instance with zero destructive actions;
- release repair successfully stops the exact pinned observed listener when identity remains stable; and
- the pre-existing external-foreground migration ordering remains stop → migrate → one daemon start.

The raw listener fixture was strengthened to model `Win32_Process` creation time, executable path, SHA-256, and daemon mode rather than only process name/path.

## Real disposable-process proof

`scripts/test-stale-canonical-daemon-recovery.ps1` now also exercises the T-0326 listener-process boundary using the real disposable `catdesk_recovery_fixture.exe` copied to the canonical `catdesk.exe` path:

- a deliberately wrong creation-time candidate is rejected and the real process remains alive; and
- the genuine verified listener candidate is then stopped and waited through the exact pinned process object.

This remains an isolated disposable-process fixture. It does not stop the live CatDesk daemon or mutate the external tunnel runtime.

## Verification

Post-change verification on 2026-09-07:

- `cargo test --test recovery_powershell lifecycle_and_reviewed_release_recovery_fixtures_pass -- --nocapture` — PASS, including both `test-start-catdesk-stack.ps1` and the real stale/listener disposable-process fixture;
- project verification surface: `cargo fmt --check` — PASS;
- project verification surface: full `cargo test` — PASS;
- project verification surface: `cargo build` — PASS;
- `cargo clippy --all-targets --all-features -- -D warnings` — PASS;
- search for `Stop-Process -Id` in `scripts/start-catdesk-stack.ps1` — zero remaining matches;
- `git diff --check` — PASS; output contains only existing LF→CRLF working-tree notices and no whitespace errors.

## Safety / non-claims

- No live destructive recovery cycle was run for this ticket.
- No browser/profile/target mutation was performed.
- No Scheduler/autostart mutation was performed.
- No Secure MCP/tunnel ownership change was performed.
- No Git publication was performed.
- This source/fixture acceptance does not by itself close T-0319; T-0319 still requires its explicitly requested independent final review using the strengthened T-0325/T-0326 evidence.

## Conclusion

**ACCEPT T-0326 at the repository/source-fixture boundary.** The two remaining recovery PID-only mutation sites have been removed. Recovery now binds destructive listener retirement to PID + creation time + daemon mode + path + SHA-256, pins the process handle before mutation, and waits on that same process instance. Deterministic PID-reuse/replacement races and a real disposable Windows process prove that an unrelated replacement cannot be terminated through stale PID authority.
