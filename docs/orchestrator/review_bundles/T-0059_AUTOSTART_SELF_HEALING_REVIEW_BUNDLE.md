# T-0059 Autostart and Self-Healing Review Bundle

## Delivered

- Extended the public root facade with `catdesk.ps1 autostart enable|status|disable`.
- Added the project-local `scripts/catdesk-autostart-supervisor.ps1`.
- Added deterministic scheduler/facade and supervisor fixture coverage.
- Updated the README and canonical architecture with optional autostart usage
  and ownership boundaries.

## Design and security boundaries

The task identity is `CatDesk.Autostart.<workspace-sha-prefix>` in the standard
Task Scheduler root path. It is deterministic for the resolved workspace and its
action contains only an absolute project-local supervisor path and workspace
argument. The exact owned definition requires one logon trigger,
current-user interactive-token + limited principal, start-when-available, and
ignore-new-instance settings. No password, route, endpoint, tunnel identifier,
or secret value is put in task arguments.

Enable registers only an absent exact definition; an exact existing task is
idempotent. Exact ownership includes the current Windows user identity, one
interactive/limited logon trigger, action, persistent settings, and workspace
identity. A same-name action/principal/trigger/settings mismatch returns
`AUTOSTART_CONFLICT` and is never overwritten. Registration intentionally does
not use `-Force`, so a same-name task that appears during the race window is
not replaced. Status is read-only/redacted. Disable removes only an exact owned
task and is idempotent if absent.

The supervisor has a workspace-hashed local mutex, held for its entire
production lifetime. It first converges initial login readiness and then remains
in a low-frequency monitor loop (60 seconds by default). It invokes only public
`catdesk.ps1 status` and `catdesk.ps1 recover`; it does not compile, provision,
launch a browser, inspect/persist secret material, or own the external official
Secure MCP runtime. An unhealthy observation triggers one bounded exponential-
backoff recovery burst. An exhausted burst emits fixed `DEGRADED`, waits a
bounded cooldown, and returns to monitoring so later recovery remains possible.
Production monitoring is unlimited; `MaximumMonitorCycles` is a finite test-only
seam. The task allows start/continue on battery, uses no finite execution-time
limit, starts when available, ignores new instances, and retains restart-on-
crash settings. Delayed external-runtime post-login readiness is treated as a
retry condition, not an instruction to connect/manage a tunnel.

## Deterministic verification

- `scripts/test-catdesk-lifecycle.ps1` verifies positional public syntax from
  arbitrary CWD, workspace-bound identity, enable/disable idempotence, conflict
  refusal, exact owner-only removal, non-mutating/redacted status, and action
  hygiene.
- `scripts/test-catdesk-autostart-supervisor.ps1` verifies singleton behavior
  across the full monitor loop, continuing healthy polls, later unhealthy
  recovery, successful return to monitoring, exhausted-burst cooldown followed
  by later monitoring, finite-cycle termination, and bounded timing values.

Required project checks:

- `powershell -ExecutionPolicy Bypass -File scripts/test-catdesk-lifecycle.ps1`
- `powershell -ExecutionPolicy Bypass -File scripts/test-catdesk-autostart-supervisor.ps1`
- `powershell -ExecutionPolicy Bypass -File scripts/test-start-catdesk-stack.ps1`
- `cargo fmt --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test`
- `git diff --check`

No real Scheduled Task, daemon, browser, external tunnel runtime, credential,
or Git publication was modified while implementing or testing T-0059.
