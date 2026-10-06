# T-0223 R1 — Version-independent supervisor foundation

## Scope and diagnosis

The existing `scripts/catdesk-autostart-supervisor.ps1` is not a stable
control-plane boundary: it calls the version-coupled `catdesk.ps1` public
lifecycle facade, whose recovery decision depends on the current release,
manifest, and daemon. A failed or half-promoted worker can therefore strand
the same control path that would be needed to recover it.

T-0223 R1 adds a deliberately small Rust foundation in
`src/control_plane_supervisor.rs` plus the separately buildable,
zero-argument `catdesk-control-plane-supervisor` binary. Its current status
surface is read-only; it does not replace the externally-owned OpenAI Secure
MCP runtime, start a tunnel, access credentials, launch a worker, reload the
daemon, or promote a release.

## R1 architecture and invariants

The later separately-installed supervisor has fixed locations, not worker or
workspace-selected locations:

| Item | Fixed policy |
| --- | --- |
| Windows supervisor state/install root | `C:\ProgramData\CatDesk\ControlPlaneSupervisor` |
| Local front door | `/mcp` |
| Backend name | `catdesk-local-backend` |
| Worker identity | opaque bounded backend ID plus dual SHA-256 manifest assertions; never an executable path |

`ControlPlaneSupervisorStoreV1` owns the supervisor state file and models
versioned CatDesk binaries only as replaceable backend registrations. The
state stays valid when the active worker reports `LOCAL_BACKEND_MISSING` or
`LOCAL_BACKEND_CRASHED`; those worker failures do not delete, corrupt, or
replace the supervisor's own front-door/tunnel state.

The fixed registration contract accepts a worker only when its declared and
independently observed manifest hashes match and the local listener was
observed ready. A failed candidate handoff or manifest mismatch leaves the
prior active backend intact. Exact repeat registration is idempotent. A
successful handoff retains the previous backend for explicit rollback.

Health is intentionally three-dimensional:

- local backend: `LOCAL_BACKEND_READY`, `LOCAL_BACKEND_MISSING`,
  `LOCAL_BACKEND_CRASHED`, or unknown;
- remote routing: `REMOTE_ROUTE_ATTACHED`, `REMOTE_ROUTE_DETACHED`, or
  unknown; and
- overall: `CONTROL_PLANE_READY` only when a local backend is ready **and** a
  remote route has an observed 2xx attachment.

In particular, a local listener plus a remote HTTP 404 yields
`CONTROL_PLANE_REMOTE_ROUTE_DETACHED`; it is never represented as connected.

The persistent tunnel field has only a validated non-secret tunnel ID. It has
no credential field and no `CONTROL_PLANE_API_KEY` path. Once an existing
configured tunnel ID is recorded, resolution succeeds even when a restarted
PowerShell process supplies no `CATDESK_OPENAI_TUNNEL_ID`; a conflicting
environment ID fails closed. R1 does not call runtime connect, stop, remove,
or reconfigure APIs.

## Deterministic test evidence

`control_plane_supervisor` unit tests use temporary state roots only and
cover:

- backend missing and crash observations while the supervisor survives;
- manifest mismatch and failed candidate handoff preserving the old backend;
- idempotent registration, successful replacement, durable rollback, and
  restart/reload state;
- local readiness plus remote 404 as route-detached rather than ready;
- route attachment only after an observed 2xx result;
- persisted tunnel identity resolution with no process environment and no
  credential serialization; and
- rejection of a `target/release`-style path passed as a backend identifier.

No test starts a real Secure MCP runtime, browser, daemon, or listener.

## Changed files and attribution

- `src/control_plane_supervisor.rs` — new R1 durable state, registration,
  rollback, health, and non-secret tunnel-identity model with tests.
- `src/bin/catdesk-control-plane-supervisor.rs` — separately buildable,
  zero-argument read-only status executable. It reports only bounded health
  labels and does not create its fixed ProgramData root when absent.
- `src/main.rs` — declares the R1 module only. The temporary
  `allow(dead_code)` documents that R2 owns host installer/front-door bridge
  activation; no runtime route was activated by this ticket.
- `docs/orchestrator/review_bundles/T-0223_VERSION_INDEPENDENT_SUPERVISOR_R1_REVIEW_BUNDLE.md`

The worktree contains extensive pre-existing dirty paths. They were preserved
and are not part of the T-0223 attribution boundary.

## Local verification

- `cargo test control_plane_supervisor -- --nocapture` — five supervisor
  tests passed in the main binary and five matching tests passed in the
  separately-buildable supervisor binary test harness.
- `cargo build --bin catdesk-control-plane-supervisor` — passed; the binary
  was built only and never executed or installed.
- `cargo fmt --check` — passed.
- `cargo clippy --all-targets --all-features -- -D warnings` — passed.
- `cargo test` under the bounded 60-second verifier window completed 703 unit
  tests with 21 existing host-specific ignores, then timed out while entering
  integration tests. The separately run integration suites all passed:
  `cargo test --test recovery_powershell -- --nocapture` (2 passed),
  `cargo test --test t0215_measure` (2 passed), and
  `cargo test --test t0217_release_measure_tmp` (0 tests, passed).
- `git diff --check` — passed.

## Migration/install plan and remaining R2 work

R2 must independently review and implement the host installer and the actual
fixed local MCP front-door process outside `target\release`. It must bind a
worker registration to the current reviewed manifest, atomically route only
the registered loopback backend, and read existing persisted OpenAI tunnel
configuration without taking runtime ownership. Host acceptance must prove
that the supervisor remains reachable across missing worker, worker crash,
failed promotion, rollback, and remote-route reattachment. It must not treat
R1's model-only unit tests as a live tunnel or daemon acceptance.

No live daemon reload, worker promotion, generated-binary execution, Secure
MCP runtime action, browser wake, Scheduler/ProgramData mutation, signing,
provenance work, or Git publication was performed. Independent review is
requested.
