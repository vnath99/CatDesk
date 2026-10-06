# T-0226 — Stable registration and protected install authority corrective review

## R3 boundary preserved

The accepted R3 proxy remains fixed at `127.0.0.1:3201/mcp` to the fixed,
validated worker endpoint `127.0.0.1:3200/mcp`. Its one-route-per-request
snapshot, body and time bounds, redirect refusal, hop-by-hop filtering,
local-vs-remote health separation, and external Secure MCP non-ownership are
unchanged. `/mcp` remains data-plane only.

## Work completed in this slice

`src/control_plane_supervisor.rs` now defines a closed control record schema:
`REGISTER_BACKEND`, `REPORT_BACKEND_HEALTH`, `ROLLBACK_BACKEND`, `STATUS`, and
bounded remote-route observation. Records contain no executable/path/URL,
command, shell, credential, or tunnel mutation authority. The sole mutation
dispatcher requires independently supplied local peer/process evidence and
checks a process-identity SHA-256, expected registration generation CAS,
manifest equality, fixed worker endpoint, fixed listener readiness and
idempotent replay before delegating to atomic activation. A stale generation,
wrong peer/PID/identity, wrong manifest/listener/backend, or malformed record
is refused before state transition. The data-plane proxy does not call this
dispatcher.

A fixed dry-run startup registration desired-state enum was also added. It is
product-owned and has no caller-selected image/path/arguments. It performs no
Task Scheduler or service operation.

Focused deterministic tests cover readiness-to-registration, CAS/replay,
wrong peer rejection, dry-run state, and static data-plane separation, in
addition to the accepted R3 proxy/installer tests.

## Fail-closed release blocker retained

This ticket's required *production* endpoint is not claimed complete. The
reviewed `ProtectedDirectoryGuard` model in `reviewed_source_snapshot.rs`
provides Windows `OBJECT_ATTRIBUTES.RootDirectory` child opens, stable parent
identity, no-follow/reparse refusal, and no-delete sharing. The separate
supervisor binary currently embeds only `control_plane_supervisor.rs`; making
its ProgramData state/install authority use those primitives requires a shared
Windows handle-relative module and a real named-pipe adapter that derives the
client PID/token/image identity from the pipe handle.

Serializing PID or image-hash text in a control request would be spoofable and
is not an acceptable substitute for OS peer authentication. Likewise, the R3
writer still uses filesystem pathname operations and therefore cannot be
described as protected handle-relative authority. Rather than add an
unauthenticated local HTTP channel or weaken containment, the production
control endpoint and live activation remain unavailable/fail closed.

The next bounded implementation must extract/reuse the reviewed Windows
root-handle primitives for both main and standalone supervisor builds, create
the fixed named-pipe server with an explicit product ACL, obtain the client
PID/token from the pipe, attest its image/listener identity, and move all
state/install staging/current/LKG/lock operations under pinned parents. It
must then add real hostile reparse/replacement/outside-sentinel tests before
R4 activation.

## Verification

- `cargo fmt`
- `cargo test control_plane_supervisor --no-fail-fast` — 15 focused tests
  passed in main and standalone-supervisor harnesses.
- `cargo clippy --all-targets --all-features -- -D warnings` — passed.

No live ProgramData/startup/daemon/release/tunnel/browser/Git mutation, live
supervisor bind, Scheduler mutation, or Secure MCP action occurred.

## Attributable files

- `src/control_plane_supervisor.rs`
- `docs/orchestrator/review_bundles/T-0226_T0223_R3_R1_STABLE_REGISTRATION_PROTECTED_INSTALL_AUTHORITY_REVIEW_BUNDLE.md`

Other dirty worktree changes predate this slice and remain outside this audit.
