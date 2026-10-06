# T-0270 / T-0223 — Functional Supervisor Migration Convergence

## Scope and diagnosis

This is the bounded source/test convergence slice for the version-independent
control-plane supervisor.  Before this change, the zero-argument supervisor
bound only its fixed `127.0.0.1:3201` MCP front door; it imported, but did not
run, the fixed Windows control-pipe server.  Separately, the CatDesk worker
marked its MCP listener ready without a closed fixed-pipe registration client.

No live ProgramData supervisor root, port 3201 listener, named pipe, service,
Scheduler task, daemon, release, browser, Secure MCP/tunnel, Git, signing, or
provenance state was activated or mutated during this source/test work.

## Runtime wiring

`catdesk-control-plane-supervisor` remains zero-argument.  On Windows it now
uses `tokio::try_join!` to run both required, long-lived surfaces:

1. the compiled fixed 3201 `/mcp` front door; and
2. `serve_fixed_control_pipe` on the compiled fixed pipe name.

Either required-surface failure drops the peer future and terminates the
supervisor instead of leaving a half-functional front door.

The worker-side client has one crate-private production entry:
`register_fixed_ready_worker_listener`. It accepts only the already-bound
listener and the digest returned by the fixed reviewed-image bridge; it offers
no caller-selected path, pipe, URL, command, manifest, endpoint, or
process-identity authority. It first checks the listener is exactly
`127.0.0.1:3200`, after the listener has been made non-inheritable. It reads
the current supervisor generation through the same fixed pipe and sends one
bounded, closed `REGISTER_BACKEND` request with that generation. A missing or
refusing supervisor logs a bounded worker warning and leaves the worker
listener and its local state untouched.

Headless/test ports and non-loopback/alternate production binds cannot reach
this registration seam.  The native daemon now fails closed if its configured
listener is not the canonical fixed `127.0.0.1:3200` backend.

## Evidence and trust boundary

The control pipe is created with the fixed SDDL
`D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;S-1-5-80-0)`, installed using
`ConvertStringSecurityDescriptorToSecurityDescriptorW` and Tokio's
`create_with_security_attributes_raw`; remote clients are rejected.  There is
no default-ACL fallback.

Before pipe connection, the worker uses the existing signed
reviewed-main-image trust root and accepted envelope to open the fixed reviewed
main image and prove its payload digest.  It then proves its current executable
has exactly that digest.  The worker request's declared manifest/process
expectations therefore carry the accepted reviewed digest, not a free-standing
pathname claim.  At the supervisor pipe boundary, the connected client PID and
process image are obtained from the pipe/OS handle.  Before any
dispatcher/state operation, the server requires the reviewed declared manifest
and expected identity to equal that OS-observed image digest and overwrites
both request `observed_*` fields with the independently observed digest. Thus
two equal worker-supplied hashes cannot masquerade as independent
manifest/process observation. Generation remains a CAS at the supervisor state
dispatcher; a stale registration is refused before it can replace the active
backend.

Only `Status` and fixed `RegisterBackend` are accepted by this worker startup
pipe seam.  Health, rollback, and route records do not become generic worker
state-control authority.  The fixed client uses bounded retries only across
the server's one-request-instance recreation gap; it never falls back to a
different pipe or endpoint.

## Deterministic coverage

Focused source/behavior tests cover:

- dual-surface zero-argument supervisor wiring and required-surface failure;
- fixed-only length-prefixed request framing and no generic pipe client;
- applied SDDL and remote-client rejection source boundary;
- accepted signed reviewed-image envelope/payload evidence before registration;
- OS peer evidence replacing worker-declared observed values;
- invalid peer evidence preserving the prior active backend;
- registration generation CAS/idempotence and stale-generation refusal;
- fixed `127.0.0.1:3200` listener-only registration; and
- source assertions that no MCP front-door or direct worker state-write path
  invokes the control dispatcher.

The existing supervisor tests retain idempotent registration, mismatch/old
backend preservation, and unsupported endpoint refusal.  The added client is
not exercised against a real host pipe: fixtures/source tests use no ProgramData
or production surface, by design.

## Verification performed

All commands below completed with exit code zero:

- `cargo fmt --check`
- `cargo test "fixed_pipe" -- --nocapture`
- `cargo test "fixed_supervisor_registration" -- --nocapture`
- `cargo test "rejected_pipe_evidence" -- --nocapture`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test` (794 unit tests plus integration suites; expected opt-in Python
  advisor tests remained ignored)
- `cargo build --bins`
- `cargo build --release --bin catdesk-control-plane-supervisor`
- `git diff --check`

Cargo emitted its pre-existing `could not canonicalize path C:\\Users\\Volap`
warning during commands; it did not cause a test, clippy, or build failure.

## Attributable files and narrow review

The T-0270 changes are confined to:

- `src/bin/catdesk-control-plane-supervisor.rs`
- `src/windows_supervisor_control_pipe.rs`
- `src/control_plane_supervisor.rs`
- `src/reviewed_build.rs` (one read-only, bounded accepted-image digest bridge)
- `src/main.rs` (only the fixed native listener/registration gate and its
  regression test)
- this review bundle

The workspace was already broadly dirty and the three supervisor source files
were already untracked in the inherited worktree, so Git's ordinary tracked
diff cannot by itself isolate their prior contents.  Narrow source review was
performed against the task-specific functions above; no unrelated dirty files
were changed, reverted, or attributed.

## Remaining bounded host acceptance

Independent host acceptance remains required.  An authorized operator must
provision the separately reviewed supervisor root and service identity, start
the stable supervisor, start the canonical production worker, and observe one
normal closed registration through the fixed pipe.  That host activity must
verify the supervisor's OS peer evidence, generation/readback, 3201 front-door
route, and old-backend preservation on refusal.  It was intentionally not
performed here.
