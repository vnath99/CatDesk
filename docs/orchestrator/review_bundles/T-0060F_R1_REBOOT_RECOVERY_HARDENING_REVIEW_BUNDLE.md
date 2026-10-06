# T-0060F-R1 Reboot Recovery Hardening Review Bundle

## Delivered boundary

The canonical PowerShell bootstrap now runs official `tunnel-client` status
through a native `ProcessStartInfo` runner with fixed argument encoding,
redirected bounded stdout/stderr capture, and a hard process kill when its
deadline expires. Every status attempt is capped by the single bootstrap
`ReadyTimeoutSeconds` deadline; it cannot extend readiness by repeatedly
waiting five seconds after that deadline. The setup helper uses the same
bounded native runner for its fixed status and connect command forms.

The bootstrap’s local MCP and runtime `/healthz` and `/readyz` requests use a
scoped silent HTTP helper. It restores the caller’s `ProgressPreference` in a
`finally` block.

The official-runtime monitor may now reconnect an existing runtime when it is
`DEGRADED` only if that runtime is still running, local MCP is ready,
`auto_recover` is enabled, both required environment references are present,
and the existing cooldown/attempt-window gate permits a recovery. Recovery
continues to invoke only `runtimes connect`; it does not stop, remove, or
create an additional runtime. A `CONNECTED_VERIFIED` transition removes only
the stale existing-runtime-not-ready warning and retains unrelated warnings.

## Regression coverage

- `scripts/test-start-catdesk-stack.ps1` uses a fake hanging native client to
  prove timeout kill behavior, checks readiness-deadline capping, and asserts
  silent HTTP preference restoration.
- `scripts/test-secure-mcp-route-validation.ps1` confirms the setup connect
  path retains validated local-route construction and invokes the bounded
  client helper rather than an unbounded direct command.
- `src/ngrok.rs` unit tests cover the degraded existing-runtime gates,
  cooldown budget, fixed reconnect command shape with no stop/remove/create,
  and targeted warning cleanup.

## Verification record

Completed in this workspace:

- PowerShell bootstrap and secure-MCP fixtures passed.
- `cargo fmt --check` passed.
- `cargo clippy --all-targets -- -D warnings` passed.
- `cargo test ngrok::tests -- --nocapture` passed (3/3).
- `git diff --check` passed.

The follow-up verifier repair made the managed-child fixture poll its owned
child to a bounded two-second deadline instead of assuming its Windows `.cmd`
wrapper exits within 200 ms. `cargo test` now completes: 457 passed, 18
ignored, and zero failed. Seven Windows host-dependent tests are explicitly
ignored with reasons: four use Python-only fake advisor sidecars when this host
has no Python interpreter, and three require process-tree termination that the
host denies. Production cancellation remains fail-closed when termination is
denied; no production behavior was weakened to accommodate the host policy.

## Ticket boundary

No CatDesk MCP call, live daemon/lifecycle action, tunnel operation, reboot,
browser/wake action, credential inspection, Scheduled Task mutation, release
promotion, or Git publication was performed. CatDesk host retains independent
verification and authoritative diff capture.
