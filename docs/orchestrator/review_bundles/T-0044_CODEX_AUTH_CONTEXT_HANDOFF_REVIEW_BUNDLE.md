# T-0044 Codex Auth Context Handoff Review Bundle

Status: `PARTIAL_OPERATOR_LOCAL_RELAUNCH_AND_RUNTIME_WRITE_GATE`.

## Outcome

T-0044 safely identified an already-authorized Codex config root without
opening, copying, parsing, hashing, or exposing any authentication files. The
direct CatDesk executable reports `Logged in using ChatGPT` when the supported
`CODEX_HOME` environment variable is set to that existing directory. Its
default inherited context reports `Not logged in` instead. This establishes a
config-root/context mismatch, not an executable-version mismatch.

The current worker sandbox cannot complete the live app-server acceptance:
with the safe explicit home, `codex app-server --stdio` fails before
`initialize` while creating its local SQLite state runtime because that
operator-local directory is not writable from this worker context. The
failure is independent of the successful login-status proof. No CatDesk
daemon was listening on the expected loopback MCP port during this run, so
there was no safe existing daemon to replace via the PID-scoped relaunch
helper.

No credential material, API key, cookie, auth-file contents, cloud fallback,
billing action, thread mutation, registry mutation, or Git publication was
performed.

## Bounded Supported Evidence

| Probe | Result |
| --- | --- |
| PATH executable classification | PowerShell external-script shim; fingerprint `sha256:d9ec03d07ae9` |
| Direct executable classification | existing direct executable; fingerprint `sha256:805ba775e70b` |
| Versions | both `codex-cli 0.146.1` |
| `CATDESK_CODEX_HOME`, `CODEX_HOME`, `XDG_CONFIG_HOME` in worker | absent |
| Supported help semantics | `codex --help` and `codex login --help` identify `~/.codex/config.toml`; direct `app-server --help` identifies `--stdio` |
| Operator's normal PATH `codex login status` (task input) | `Logged in using ChatGPT` |
| This worker's inherited PATH `codex login status` | `Not logged in` |
| Default inherited direct `codex login status` | `Not logged in` |
| Direct login status with safe explicit config root | `Logged in using ChatGPT` |
| Explicit-home app-server | startup blocked before `initialize`; redacted diagnostic fingerprint `sha256:81428f77dc36` classified as local SQLite-state-runtime initialization / access denied |
| Loopback CatDesk listener on port 3200 | absent |
| Existing local relaunch helper | path-only preflight accepts the opaque `CATDESK_CODEX_HOME`; it inspected/stopped/replaced no process without explicit launch |

The approved root was identified by presence-only directory metadata and the
documented config-home semantics. Its path and contents are intentionally not
recorded; the path remains an opaque operator-local value. The stale `arg0`
cleanup warning was also classified only as `Access denied` with its path
redacted. It was not treated as authentication evidence.

## Root Cause And Safe Handoff

The worker's default Codex context is not the same effective home as the
operator's already-authorized Codex context. This also explains why the
operator's normal PATH session is authenticated while the task worker's
inherited PATH/default and direct contexts are not. Explicitly handing the
existing root to the direct executable through `CODEX_HOME` changes the
supported login-status result from unauthenticated to ChatGPT-authenticated.
The direct binary and the PATH shim are the same reported CLI version, so
changing the binary cannot resolve the issue.

CatDesk already implements the corresponding opaque handoff:

- `CATDESK_CODEX_HOME` is validated only as an existing directory.
- Direct CLI/app-server children receive it as `CODEX_HOME`.
- API-key variables are removed from those children.
- The PID-scoped relaunch helper preserves the external secure tunnel and
  refuses to replace a non-CatDesk process.

This worker cannot persist an operator-session environment value or start an
app-server that writes into the operator-local root. The required safe next
action is therefore operator-local:

1. In the desktop/session that launches CatDesk, set `CATDESK_CODEX_HOME` to
   the already-authorized config directory identified above (path only; do
   not provide the path or its contents through MCP).
2. Use `scripts/restart_catdesk_daemon.ps1` with the current CatDesk PID and
   its explicit `-Execute` mode, or start a new CatDesk instance from that
   same environment if no daemon is running.
3. Reconnect the existing secure tunnel, then rerun the direct app-server
   `initialize`, `account/rateLimits/read`, and exact-CWD thread probe.

## Thread, Continuity, And Registry Safety

Canonical workspace:

`C:\\Users\\Volap\\OneDrive\\Desktop\\Projects\\CatDesk-codex-loop`

The app-server could not initialize in this sandbox, so an authenticated
fresh `thread/list` / `thread/read` cannot be proved here. Consequently no
existing thread was selected, no canonical replacement was created, no
continuity turn was sent, and no `project -> workspace -> thread` mapping was
persisted. The one-replacement budget remains unused.

After the operator-local relaunch, perform one fresh exact-CWD list for
`Integrate Codex MCP for ChatGPT`; bind only an exact, unowned,
direct-input-capable candidate. If none exists, create at most one canonical
replacement and prove reconnect/list/read/turn continuity before using the
bounded registry bind/read surface.

## Remaining Work

- Prove direct app-server authentication in the writable operator CatDesk
  process using `initialize` and `account/rateLimits/read`.
- Fresh-list and bind or create one eligible canonical thread, then prove
  reconnect/read/direct-input continuity.
- Persist and read back the mapping through the bounded CatDesk registry
  surface after the daemon is locally available.
- Run CatDesk independent verification after those live proofs.

## Verification

The source-level handoff implementation was inspected without modifying the
pre-existing T-0030--T-0043 work.

- `cargo fmt --check`: PASS.
- `cargo test codex_app_server -- --nocapture`: PASS (6 tests).
- `cargo test codex_cli -- --nocapture`: PASS (12 tests).
- `cargo test autonomy_supervisor::tests::project_registry_binding_is_canonical_evidence_gated_and_durable -- --nocapture`:
  PASS.
- `cargo test autonomy_runtime::tests::app_server_binding_requires_exact_workspace_thread_and_persists_bounded_telemetry -- --nocapture`:
  PASS.
- `cargo clippy --all-targets --all-features -- -D warnings`: PASS.
- `git diff --check`: PASS; Git emitted only existing line-ending warnings.
- `cargo test --no-fail-fast`: 399 passed, 7 failed, 9 ignored. The failures
  match the previously recorded environment-sensitive baseline: three advisor
  tests cannot start their configured advisor program, the related advisor
  cancellation expectation fails, and three Windows process-tree cancellation
  tests receive `ERROR: Access denied`.

No independent CatDesk verification is claimed by this bundle.
