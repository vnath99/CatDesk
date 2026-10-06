# T-0157 / T-0154-R1 first-class recovery acceptance surface

## Scope and implementation

This change adds the closed `catdesk_release_recovery` MCP operation.  It accepts an empty object only.  Its static-schema bridge is the exact existing-tool request:

```json
{"decision":"CATDESK_CANONICAL_RECOVERY"}
```

to `catdesk_daemon_reload`.  The bridge rejects every other field, and ordinary daemon-reload behavior is unchanged when `decision` is absent.  A decision-bearing request is never eligible for the server's native reload-exit scheduling predicate.

The operation first blocks active autonomous mutation/verification, then validates and schedules a detached fixed-purpose CatDesk helper.  The helper waits for the JSON-RPC acknowledgement window, revalidates the canonical workspace/facade and trusted absolute Windows PowerShell, and invokes only:

```text
<trusted SystemRoot>\\System32\\WindowsPowerShell\\v1.0\\powershell.exe
  -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass
  -File <canonical workspace>/catdesk.ps1 recover
```

There is no MCP or helper input for a program, script, command, path, tunnel identity, route, or credential.  The helper remains outside the HTTP-server response path and persists only a bounded fixed-vocabulary record: `RECOVERY_SCHEDULED`, `RECOVERY_COMPLETED`, `RESTORED_KNOWN_GOOD`, `INTERRUPTED_TRANSACTION_COMPLETED`, or `RECOVERY_AUTHORITY_REQUIRED`.

## Authority boundaries

`catdesk.ps1 recover` and `scripts/start-catdesk-stack.ps1` remain the reviewed recovery authority.  They retain T-0154's canonical binary plus fingerprint-pair validation, interrupted reviewed-promotion completion, last-known-good restoration, exact canonical process checks, and official-runtime verification/reattachment.  The adapter cannot calculate a sidecar for the currently-present binary, create a competing tunnel runtime, or kill an arbitrary process.  It captures no raw PowerShell output; only a strict bounded JSON lifecycle result with a fixed `recover` command and allowed state is recorded.

The lifecycle engine now emits only a bounded recovery source (`INTERRUPTED_PROMOTION` or `LAST_KNOWN_GOOD`) alongside a successful redacted status when a release repair actually occurred.  The public facade maps that source to the two corresponding fixed recovery terminal states.  Normal `CONNECTED_VERIFIED` behavior remains unchanged.

## Changed files

- `src/delegated/autonomy_supervisor.rs` — first-class empty-schema MCP operation and exact cached bridge.
- `src/daemon_reload.rs` — bounded recovery worker parser, delayed detached scheduling, and durable redacted result record.
- `src/operator_facade.rs` — fixed canonical recovery facade invocation using the reviewed trusted-PowerShell resolver.
- `src/main.rs` — exact recovery-worker entrypoint.
- `src/mcp.rs` — deterministic full tool-list expectation for the new operation.
- `src/server.rs` — explicit exclusion of decision-bearing compatibility calls from reload exit scheduling.
- `catdesk.ps1` and `scripts/start-catdesk-stack.ps1` — bounded repaired-source propagation only.

## Deterministic evidence

Added Rust tests:

- `daemon_reload::tests::canonical_recovery_worker_arguments_and_result_are_closed_and_redacted`
- `operator_facade::tests::canonical_recovery_invocation_is_fixed_and_never_accepts_caller_command_inputs`
- `delegated::autonomy_supervisor::tests::canonical_recovery_surface_and_cached_bridge_are_closed_before_execution`
- extended `server::tests::daemon_reload_exit_is_scheduled_only_for_execute_calls`

Executed locally:

- `cargo fmt --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test` — 585 passed, 18 expected ignores; the two Windows recovery integration tests passed.
- `git diff --check`
- focused recovery/control-plane tests above
- `powershell -NoProfile -ExecutionPolicy Bypass -File scripts/test-start-catdesk-stack.ps1` — passed (`canonical bootstrap behavioral tests passed`)
- `powershell -NoProfile -ExecutionPolicy Bypass -File scripts/test-catdesk-lifecycle.ps1` — passed (`consumer lifecycle fixture tests passed`)

No live recovery, fault injection, daemon reload, promotion, tunnel action, browser/wake/Scheduler action, external-project action, or Git publication occurred.

## Host acceptance sequence

1. Build the reviewed candidate in isolation and reload only through separately approved release controls.
2. Prove `CONNECTED_VERIFIED` before invoking recovery.
3. Invoke `catdesk_release_recovery` with `{}` (or the exact cached bridge form above); do not send any extra field.
4. Observe only the bounded result record/state.  Accept terminal success only as `RECOVERY_COMPLETED`, `RESTORED_KNOWN_GOOD`, or `INTERRUPTED_TRANSACTION_COMPLETED`; treat `RECOVERY_AUTHORITY_REQUIRED` as fail-closed operator attention.
5. Re-prove canonical pair identity and non-duplicated verified official runtime using the reviewed status/acceptance procedure.

Remaining limitation: this control plane intentionally does not expose raw lifecycle diagnostics or a retry-with-parameters surface.  Ambiguous release, process, or transport evidence remains fail-closed.
