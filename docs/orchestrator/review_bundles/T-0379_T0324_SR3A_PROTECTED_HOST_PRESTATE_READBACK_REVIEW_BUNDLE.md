# T-0379 — T-0324 SR3A protected-host prestate readback

## Scope and authority

T-0379 implements a zero-choice, read-only host-prestate projection needed
before any later reviewed activation canary. It does not resume T-0378, invoke
the lifecycle/control pipe, or claim a live serving result.

The production reader has no root, path, generation, digest, role, filename,
endpoint, process-ID, or trust-material input. It derives the per-user release
root only from `SHGetKnownFolderPath` and fixed descendants under
`AppData/Local/CatDesk/WorkerReleases`; it derives the supervisor state only
from `ControlPlaneSupervisorStoreV1::fixed_read_only()` at the compiled fixed
root.

## Readback model

`read_current_user_worker_release_prestate()` reads canonical
`active-state.json`, revalidates the immutable current pointer and optional
previous pointer independently through protected no-follow reads, then rereads
the active-state bytes and rejects a changed relation. Each projected release
contains only generation, immutable manifest digest, and the already required
worker-image/source-snapshot/review/attestation identities. Manifest and image
digest/length bindings are checked for both current and prior releases.

`read_fixed_supervisor_prestate()` applies the existing fixed activation
readiness assessment, reopens only the fixed supervisor protected store, and
projects registration generation, local backend health, and active/rollback
backend digest assertions. It omits endpoint, pathname, process ID, tunnel
identity, credentials, and all mutation operations.

`read_fixed_protected_host_prestate()` composes those sources into bounded JSON
(`schema: 1`) with only these classifications:

- `READY_FOR_REVIEWED_ACTIVATION` requires an exact current and prior release,
  fixed supervisor activation readiness, local backend ready state, nonzero
  registration generation, and all active/rollback manifest/process identity
  assertions matching the respective worker **image** digest. The separate
  immutable manifest-document digest remains reported but is not confused with
  the legacy supervisor image-digest field.
- `NO_USER_RELEASE`, `NO_ROLLBACK_AUTHORITY`, `USER_RELEASE_INVALID`,
  `SUPERVISOR_STATE_UNAVAILABLE`, `SUPERVISOR_NOT_READY`, and
  `IDENTITY_DIVERGENCE` are fail-closed outcomes.

The dedicated `catdesk-user-worker-host-prestate` binary accepts exactly zero
arguments and prints only this bounded JSON. It is read-only; it opens no
mutating pipe client and contains no launch, registration, release write,
process, wake, browser, tunnel, or Secure-MCP operation.

## Host-execution boundary

No measured zero-argument host readback was collected in this ticket. Although
the new binary is read-only, this workspace worker has no separately approved
protected-host observation surface. Its absence is
`UNOBSERVABLE_FROM_CURRENT_SAFE_SURFACE`, not evidence that a user release or
supervisor state is absent. The externally owned official Secure MCP runtime
remains outside this reader and must continue to be observed separately through
`catdesk_transport_status`.

## Changed files and attribution

- `src/user_worker_release.rs`: paired immutable release projection, bounded
  prestate classifier/JSON, and hostile read-only regressions.
- `src/control_plane_supervisor.rs`: validated fixed supervisor projection and
  projection regression.
- `src/bin/catdesk-user-worker-host-prestate.rs`: zero-argument inspector.
- This review bundle.

These files exist in a preserved untracked/dirty worktree with extensive prior
CatDesk material. Attribution is limited to the T-0379 additions above; no
unrelated dirty content is claimed.

## Verification

- `cargo test user_worker_release -- --nocapture` — PASS: 15 focused tests in
  the main binary and the standalone inspector compilation context.
- `cargo test fixed_supervisor_prestate_projects_only_validated_backend_identities -- --nocapture` — PASS.
- `cargo fmt --all -- --check` — PASS.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` — PASS.
- `cargo test --workspace --all-targets --all-features` — PASS: primary suite
  893 passed, 0 failed, 21 ignored (914 total); all binary and integration
  targets passed.
- `git diff --check` — PASS; existing CRLF conversion notices only.

## Later boundary

Fresh independent review must validate the projection before any host readback
or activation. A future reviewed cutover may use a current positive prestate
plus independently observed external Secure MCP continuity to make a fixed
supervisor/control-pipe CAS decision. It must still require server-side
OS-attested peer/image evidence, bounded post-activation readback, and exact
rollback on failure. This ticket grants no target migration, wake action,
browser action, signing/UAC, Program Files/ProgramData write, Scheduler/service
action, external-project mutation, or Git publication authority.
