# T-0341 — Recovery tunnel-client discovery authority hardening

Date: 2026-09-07
Status: IMPLEMENTATION / FIXTURE COMPLETE; T-0319 INDEPENDENT FINAL REVIEW STILL REQUIRED
Scope: one-command `plan`/`recover` tunnel-client executable discovery only. No live daemon, externally owned Secure MCP runtime, browser wake, protected wake target, Scheduler, reviewed-build promotion, Git publication, or worktree cleanup.

## Defect

`Find-TunnelClient` in `scripts/start-catdesk-stack.ps1` accepted a `Get-Command tunnel-client` result from caller `PATH` before CatDesk's pinned user-local installation paths. The selected client is subsequently used by one-command recovery to query official-runtime status. This meant caller environment search order could influence executable identity at a recovery verification boundary.

## Correction

Recovery discovery now accepts only:

1. an explicit operator-supplied `-TunnelClientPath`, when present and a file; otherwise
2. `%USERPROFILE%\.catdesk\tools\tunnel-client\current\tunnel-client.exe`; then
3. the legacy pinned `%USERPROFILE%\.catdesk\tools\tunnel-client\tunnel-client.exe` location.

Ordinary caller `PATH` is no longer consulted by the recovery bootstrap. Existing explicit-path behavior and durable current/legacy CatDesk installation fallback are preserved. This change does not stop, restart, migrate, recreate, or otherwise acquire ownership of the externally owned Secure MCP runtime.

`setup-secure-mcp.ps1` is intentionally not broadened by this ticket: the recovery migration mode does not perform tunnel-client discovery or invocation, and T-0341 is scoped to the supported `start-catdesk-stack.ps1` plan/recover executable-selection boundary.

## Regression evidence

`scripts/test-start-catdesk-stack.ps1` now:

- prepends a fake `tunnel-client.exe` directory to `PATH`;
- proves an explicit operator path still wins;
- proves a missing explicit path cannot fall through to the fake PATH client;
- statically rejects `Get-Command` inside recovery `Find-TunnelClient`;
- preserves coverage for current pinned user-local selection, legacy pinned fallback, and fail-closed missing-client behavior.

## Verification

- `cargo test --test recovery_powershell -- --nocapture`: PASS, 2/2.
- `cargo fmt --all -- --check`: PASS.
- `cargo test --all-targets --all-features`: PASS.
- `cargo build --all-targets --all-features`: PASS.
- `cargo clippy --all-targets --all-features -- -D warnings`: PASS.
- Final `git diff --check`: PASS (exit 0; existing Windows LF→CRLF working-copy warnings only).

## Authority / operational boundaries

- No live daemon or tunnel process was stopped, restarted, replaced, or migrated.
- No browser wake bridge was invoked.
- No protected wake target or Scheduler state was edited.
- No provider was re-probed or substantive Qwen work dispatched.
- No release bytes were promoted and no Git state was published.
- Existing dirty worktree is preserved.

## T-0319 relationship

T-0341 is implementation/test complete at the repository/source-fixture boundary. T-0319 remains open for a genuinely separate independent final review of the cumulative T-0325 through T-0341 recovery authority/boundedness chain and existing watchdog/stale-daemon evidence. This implementation lineage does not self-approve that gate.
