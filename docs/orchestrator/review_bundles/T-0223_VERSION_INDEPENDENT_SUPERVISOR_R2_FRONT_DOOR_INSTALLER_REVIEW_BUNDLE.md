# T-0223 R2 — Fixed front door and installer/stager boundary

## Preserved R1 invariants

R1's separate supervisor binary, fixed `C:\ProgramData\CatDesk\ControlPlaneSupervisor`
policy root, non-secret tunnel identity, replaceable opaque backends, and
three-dimensional health state are preserved. R2 does not start, install, or
activate the supervisor; it does not touch the live CatDesk daemon, ordinary
release/promotion state, browser, Scheduler, or the externally owned OpenAI
Secure MCP runtime.

## Fixed protocol and trust boundary

The stable local control endpoint is exactly `/mcp`. Its R2 Axum router
(`fixed_front_door_router`) accepts only two bounded control methods:

- `catdesk/control-plane-status`
- `catdesk/control-plane-route`

No request field accepts an executable path, worker URL, shell command,
manifest path, tunnel ID, credential, or runtime action. Unsupported methods
return `CONTROL_PLANE_METHOD_UNSUPPORTED`. The status handler remains locally
responsive and returns bounded labels when the state is missing, malformed,
or a worker is unavailable.

A routable backend is bound to all of the following exact fixed assertions:

| Binding | Rule |
| --- | --- |
| Backend ID | bounded opaque ASCII identifier; paths are rejected |
| Endpoint | exactly `http://127.0.0.1:3200/mcp` |
| Manifest | declared SHA-256 equals independently observed SHA-256 |
| Listener/process | expected process-identity SHA-256 equals observed SHA-256 |

Only a fully matching, locally-ready candidate becomes active. A manifest,
process-identity, endpoint, or local-listener failure preserves the old
backend. Exact replay is idempotent. Successful N-to-N+1 registration stores
the prior registration as rollback LKG; `rollback_backend` restores it without
replacing/restarting the supervisor. The route decision itself contains only
the opaque backend ID and compiled loopback endpoint; R2 deliberately does
not bind a socket or forward arbitrary MCP messages before R3's host listener
and authenticated bridge review.

Local readiness and remote Secure MCP attachment remain separate. Remote HTTP
404 sets `REMOTE_ROUTE_DETACHED`; even a locally ready backend therefore has
`CONTROL_PLANE_REMOTE_ROUTE_DETACHED`, never `CONTROL_PLANE_READY`.

## Installer/update/LKG plan (dry-run only)

`reviewed_supervisor_installer_plan` is a fixed-purpose planning seam for the
separately-built supervisor image. It accepts matching declared/observed image
manifest SHA-256 values only and produces a fixed side-by-side layout:

`C:\ProgramData\CatDesk\ControlPlaneSupervisor\versions\<16-hex-prefix>\catdesk-control-plane-supervisor.exe`

with an explicit LKG promotion intent. It has no caller-selected source or
destination path. `validated_fixed_install_child` rejects absolute paths,
traversal, malformed components, and oversize values. State loads reject more
than 64 KiB, malformed state, mismatched root/front-door data, unsafe backend
registrations, conflicting tunnel identity, and unrecognized failure values.

R3 must implement the reviewed host writer using a fixed administrator-owned
installer, handle-relative/no-follow directory operations, side-by-side image
staging, atomic LKG pointer promotion, and reverse rollback. R2 neither calls
the plan's future writer nor creates ProgramData. A static regression test
also verifies `daemon_reload.rs` has no stable-supervisor-root authority, so
ordinary CatDesk worker release/promotion cannot own this install root.

## Secure MCP ownership

The only tunnel datum in supervisor state is a validated non-secret tunnel ID.
It is sufficient after a PowerShell restart when the process environment is
absent; an environment value conflicting with persisted identity fails closed.
No state type, route response, installer plan, or front-door request includes
`CONTROL_PLANE_API_KEY`, credentials, a tunnel client path, or create/stop/
remove/reconfigure action. The existing Secure MCP runtime remains external.

## Deterministic failure matrix

- Missing worker and post-registration crash: status remains available and
  routing returns a bounded local-backend-unavailable result.
- Manifest mismatch or listener/process-identity mismatch: candidate is
  refused; previous active backend remains unchanged.
- Arbitrary/non-loopback endpoint and executable-path-style backend ID: input
  is rejected before state mutation.
- Candidate listener unavailable: failed handoff preserves active worker.
- Exact repeat registration: idempotent; N-to-N+1 registration retains LKG;
  rollback and store reload restore the older worker.
- Local ready plus remote 404: remote detached, not connected.
- Persisted tunnel identity with absent process environment: resolves; a
  conflicting environment identity is rejected.
- Install state: manifest drift and unsafe staging children are rejected;
  plan is fixed side-by-side and cannot select `target\release`.

## Attributable files

- `src/control_plane_supervisor.rs`
- `src/bin/catdesk-control-plane-supervisor.rs` (R1 binary retained; R2 uses
  the same separate build boundary)
- `src/main.rs` (R1 module declaration retained)
- `docs/orchestrator/review_bundles/T-0223_VERSION_INDEPENDENT_SUPERVISOR_R2_FRONT_DOOR_INSTALLER_REVIEW_BUNDLE.md`

All other dirty-worktree paths predated this task and were preserved.

## Verification and R3 remainder

- `cargo test control_plane_supervisor -- --nocapture` — 10 focused R2 tests
  passed in both the main binary and separate-supervisor binary harnesses.
- `cargo fmt --check` — passed.
- `cargo clippy --all-targets --all-features -- -D warnings` — passed.
- `cargo test --bin catdesk` — 708 passed, 21 existing host-specific ignored,
  0 failed.
- `cargo test --bin catdesk-control-plane-supervisor` — 10 passed.
- `cargo test --test recovery_powershell` — 2 passed.
- `cargo test --test t0215_measure` — 2 passed.
- `cargo test --test t0217_release_measure_tmp` — 0 tests, passed.
- `git diff --check` — passed.

No production supervisor binary was installed or invoked as a live host
process, and no ProgramData path was created by this ticket. The listed
binary-test harnesses used only temporary test state roots.

R3 live-host acceptance must install the separately built supervisor through
the reviewed fixed writer, bind its fixed loopback listener, register a real
attested worker, and prove continuity across worker absence/crash, failed
handoff, version replacement, rollback, remote 404, and reattachment—without
owning or mutating the external Secure MCP runtime. Independent final review
is requested before that live activation.
