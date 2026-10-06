# T-0047E Reload-Handoff Failure Isolation

Status: `COMPLETED_VERIFIED__DISPOSABLE_RELOAD_ACCEPTANCE_PASS`.

## Repair Applied

`run_reload_worker` now advances `.catdesk/restart-handoff/latest.json` for
every post-workspace-validation early failure that previously could return
after a successful reload preflight:

- replacement path validation;
- rollback path validation;
- replacement hashing or hash mismatch;
- old-daemon exit/loopback-port-release wait; and
- replacement daemon launch.

Each record uses fixed, bounded status and stage labels plus the existing
replacement SHA-256 fingerprint. It intentionally excludes operating-system
error text, paths, command lines, endpoints, inherited environment values,
and credentials. The existing replacement-ready and rollback-ready paths are
unchanged.

The one unavoidable precondition is a canonical trusted workspace: if the
worker cannot establish that boundary, it does not write into an untrusted
caller-supplied path merely to manufacture a handoff file.

## Deterministic Evidence

- `early_worker_validation_failure_always_advances_bounded_handoff_state`
  reproduces a preflight-successor with missing replacement, missing rollback,
  and replacement-hash-mismatch failures before any process is launched. It
  verifies a new bounded handoff record for each state.
- Existing atomic-state and Windows listener-inheritance tests continue to
  cover secret-free handoff serialization and native reload safety.
- No CatDesk reload, live listener, Secure MCP tunnel, app-server, or
  credential/config file was accessed by this coding worker.

## Verification

- `cargo fmt --check`: PASS.
- `cargo test daemon_reload::tests`: 7 passed.
- `cargo test autonomy_runtime`: 5 passed, 1 ignored host-only probe.
- `cargo clippy --all-targets --all-features -- -D warnings`: PASS.
- `cargo test --no-fail-fast`: 431 passed, 7 failed, 11 ignored. The failures
  are the known missing advisor executable and Windows process-tree
  access-denied cases; no reload regression failed.
- `git diff --check`: PASS (only existing CRLF warnings on the dirty worktree).

## Independent Host Acceptance

ChatGPT independently ran the ordinary native reload path on disposable port
`33248` using the safe-forward CatDesk binary after the worker completed. The
live CatDesk listener on port `3200` and the external Secure MCP tunnel were
not touched.

Observed sequence:

- disposable old PID `53416` owned the port before reload;
- reload preflight resolved the same old PID and execute was accepted;
- old PID exited and released the listener at approximately `T+1509 ms`;
- replacement PID `55820` owned the disposable port at approximately
  `T+3322 ms`;
- journal status became `REPLACEMENT_READY_PENDING_TRANSPORT_RECONNECT` with
  stage `complete`; and
- harness reported `LIVE_PORT_3200_UNTOUCHED=true`.

The earlier automatic environment-scrubbing migration is intentionally
abandoned as unnecessary. A separate live host probe already proved the
normal current-user Codex path works with recovery/API variables absent
(`OVERRIDE_FREE_CURRENT_USER_CODEX_OK`). `CATDESK_CODEX_HOME` and explicit
Codex executable configuration therefore remain optional advanced overrides;
normal reloads do not need to mutate inherited environment state.

## Go/No-Go

GO for the already-proven ordinary native self-reload path. The failed
T-0047D environment-migration experiment is not part of the production
acceptance path and should not be reintroduced without a separate disposable
acceptance program. Early worker failures are now durably journaled.
