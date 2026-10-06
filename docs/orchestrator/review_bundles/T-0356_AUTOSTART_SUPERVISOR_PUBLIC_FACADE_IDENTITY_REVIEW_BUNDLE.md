# T-0356 — Autostart supervisor public facade executable identity review bundle

Date: 2026-09-08
Status: IMPLEMENTATION / REPOSITORY VERIFICATION COMPLETE; T-0319 INDEPENDENT FINAL REVIEW STILL REQUIRED
Scope: persistent autostart supervisor lifecycle-entry identity only. No live CatDesk recovery, daemon/tunnel restart or migration, release promotion, browser wake, protected wake-target mutation, Scheduler mutation, Git publication, or dirty-worktree cleanup.

## Problem

The persistent unattended recovery supervisor in `scripts/catdesk-autostart-supervisor.ps1` invokes the public `catdesk.ps1` lifecycle facade for both `status` and `recover`. T-0351 already hardens the recovery engine imported *inside* `catdesk.ps1`, but that check occurs only after the public facade itself has begun executing. The supervisor therefore still had a pre-T-0351 executable-authority seam if its workspace-relative `catdesk.ps1` leaf were replaced by a reparse point or otherwise resolved to a different path.

Because the autostart supervisor is part of automatic one-command recovery, its public facade crossing must not rely on leaf existence alone.

## Implementation

`Resolve-TrustedLifecycleFacadePath` now:

- derives the exact expected `catdesk.ps1` path from the already-resolved workspace root;
- normalizes that expected path using `System.IO.Path.GetFullPath`;
- resolves the exact literal candidate with `Get-Item -Force -ErrorAction Stop`;
- requires the observed item to be a real `System.IO.FileInfo`;
- rejects `FileAttributes.ReparsePoint`;
- normalizes the observed `FullName`; and
- requires exact expected/observed identity using `StringComparison.OrdinalIgnoreCase`.

`Invoke-PublicLifecycle` invokes only the path returned by this validator. The existing supervisor state machine, retry/cooldown behavior, single-instance mutex, external-runtime-pending semantics, and public-only `status`/`recover` boundary remain unchanged.

## Regression coverage

`scripts/test-catdesk-autostart-supervisor.ps1` now requires:

- the trusted facade resolver to be defined before public facade invocation;
- exact `Get-Item` inspection of the expected facade path;
- real `FileInfo` validation;
- explicit reparse-point rejection;
- observed full-path normalization;
- exact Windows path comparison; and
- rejection of regression to `Test-Path -PathType Leaf` as executable authority.

Existing positive supervisor behavioral fixtures remain in place, including healthy monitoring, degraded/backoff behavior, external-runtime-pending handling, duplicate-supervisor refusal, and bounded output/state surfaces.

## Verification

- sanctioned project verifier: PASS (`cargo fmt --check`, default full Cargo tests, `cargo build`);
- `cargo test --all-targets --all-features`: PASS;
- `cargo test --test recovery_powershell lifecycle_and_reviewed_release_recovery_fixtures_pass -- --nocapture`: PASS;
- `cargo clippy --all-targets --all-features -- -D warnings`: PASS;
- `git diff --check`: PASS (exit 0; only the existing LF-to-CRLF working-copy warnings).

## T-0319 relationship

T-0356 is a bounded source/fixture hardening change and does not self-manufacture T-0319 acceptance. A genuinely separate final reviewer must now evaluate cumulative T-0325 through T-0356 together with the existing watchdog, stale-daemon, literal one-command host, deployment-parity, and recovery-authority evidence.

## Safety / non-actions

- The already-running externally owned official Secure MCP runtime was preserved.
- No duplicate Secure MCP runtime was created.
- No live `recover` was invoked.
- No daemon or tunnel was stopped, restarted, or migrated.
- No reviewed release was promoted.
- The browser wake bridge was not invoked.
- Protected wake target state was not edited.
- Scheduler state was not mutated.
- Git state was not published and the pre-existing dirty worktree was not cleaned or reset.
