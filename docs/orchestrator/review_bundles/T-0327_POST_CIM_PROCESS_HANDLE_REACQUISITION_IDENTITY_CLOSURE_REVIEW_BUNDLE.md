# T-0327 — Post-CIM process-handle reacquisition identity closure

Date: 2026-09-07
Status: SOURCE / FIXTURE ACCEPTED; T-0319 INDEPENDENT FINAL REVIEW STILL REQUIRED
Scope: repository recovery hardening only. No live CatDesk daemon, external Secure MCP runtime, browser/wake target, Scheduler, or Git mutation.

## Independent-review finding

While performing the required T-0319 review against the strengthened T-0325/T-0326 code, a residual process-instance race was found in both recovery acquisition helpers:

- `Get-CatDeskDaemonProcessInstanceForRecovery`; and
- `Get-CatDeskListenerProcessInstanceForRecovery`.

Each helper correctly re-resolved the selected PID through `Win32_Process` and revalidated CIM creation time, daemon mode, executable path, and SHA-256. It then called `Get-Process -Id` to acquire a `System.Diagnostics.Process` object and pinned its handle. However, the newly acquired `Process` object was checked only for path, not for its own creation/start time.

If the selected process exited after the CIM check and Windows reused the PID before `Get-Process`, a replacement launched from the same canonical path could satisfy the path check and become the pinned mutation target even though it was not the process instance previously validated by creation time.

This means T-0319 could not be accepted on the T-0325/T-0326 evidence alone.

## Correction

A shared `Test-CatDeskPinnedProcessMatchesRecoveryIdentity` boundary now validates the process object *after its OS handle is acquired* and before it can become destructive mutation authority.

The helper:

1. forces acquisition of `Process.Handle` first;
2. reads `Process.StartTime` from that pinned process object;
3. resolves `Process.Path` from that same object;
4. compares pinned-process start time with the previously selected CIM creation time at CIM's microsecond precision; and
5. requires the pinned path to equal the selected canonical/observed path.

Both daemon and listener acquisition helpers use this shared check after their existing CIM creation-time/mode/path/hash validation. A replacement PID therefore cannot become mutation authority merely by using the same executable path.

## Timestamp precision boundary

The real disposable-process fixture showed that the same Windows process can be represented as:

- CIM creation time: `...2394220Z`; and
- `Process.StartTime`: `...2394223Z`.

The 3-tick difference is 300 ns and is below the precision exposed by `Win32_Process.CreationDate`. The implementation therefore normalizes both DateTime tick values by integer truncation to the CIM microsecond boundary (10 .NET ticks) before comparison. It does **not** use a broad time tolerance.

The deterministic regression rejects a same-path process whose start time differs by one full CIM microsecond.

## Regression coverage

`scripts/test-start-catdesk-stack.ps1` now directly exercises the shared pinned-process identity predicate with:

- the same path and same selected creation instant — accepted; and
- the same path but a creation time one CIM microsecond later — rejected.

`scripts/test-stale-canonical-daemon-recovery.ps1` imports the same production helper, so the real disposable stale-daemon and listener stop paths prove that a genuine stable Windows process survives the CIM/Process precision normalization and can still be stopped through the exact pinned process boundary.

Existing T-0325/T-0326 tests continue to cover changed CIM instances, disappeared instances, listener replacement between acquisition and mutation, release repair, migration/restart ordering, foreign/multiple refusal, and no-listener convergence.

## Verification

Post-change verification on 2026-09-07:

- `cargo test --test recovery_powershell lifecycle_and_reviewed_release_recovery_fixtures_pass -- --nocapture` — PASS; the real disposable stale-daemon/listener fixture and bootstrap recovery fixture pass.
- `cargo fmt --check` — PASS.
- full `cargo test` — PASS.
- `cargo build` — PASS.
- `cargo clippy --all-targets --all-features -- -D warnings` — PASS.
- `git diff --check` — PASS; only pre-existing LF→CRLF working-tree notices were emitted.

A direct PowerShell invocation was not used because the CatDesk command allowlist correctly rejects nested interpreter execution. Verification ran through the repository's Rust recovery harness instead; command protections were not weakened.

## Safety / non-claims

- No live destructive recovery cycle was run for T-0327.
- No external Secure MCP/tunnel process was created, stopped, replaced, or reconfigured.
- No browser wake bridge was invoked and no protected wake target was edited.
- No Scheduler/autostart state was mutated.
- No Git publication was performed; the existing dirty worktree was preserved.
- This implementation does not self-manufacture the independent final review required by T-0319. T-0319 remains open for a separate reviewer to evaluate the corrected T-0325/T-0326/T-0327 chain and existing watchdog live evidence.

## Conclusion

**ACCEPT T-0327 at the repository/source-fixture boundary.** The post-CIM, pre-handle PID-reuse gap found during T-0319 review is closed: the exact OS process object must itself prove the selected creation identity and path after handle acquisition before `Kill()`/`WaitForExit()` can use it. T-0319 remains pending independent final review rather than being self-approved by the implementer of this correction.
