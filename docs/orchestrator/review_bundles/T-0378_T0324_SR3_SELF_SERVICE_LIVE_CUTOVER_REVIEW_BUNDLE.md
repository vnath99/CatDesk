# T-0378 — T-0324 SR3 self-service live cutover

## Status: NOT EXECUTED — fail closed

No live cutover was attempted. The required preconditions are not established
by the permitted workspace surfaces, so this ticket does not claim a serving
generation, supervisor/daemon identity, release activation, readiness, or
rollback result.

## Go/no-go evidence

The durable state is not sufficient to treat T-0377 as independently accepted:

- `.catdesk/current_plan.md` still names T-0377 the active preflight slice and
  says a separate activation ticket follows only after T-0377 acceptance.
- `CATDESK_MILESTONES.md` records its session as started/running and likewise
  conditions the activation canary on independent T-0377 acceptance.
- The T-0377 bundle says that fresh independent review is required before any
  separate activation uses lifecycle/control-pipe authority.

Even if that review becomes green, the exact live pre-state requested by this
ticket is protected-host evidence: the selected serving generation, stable
supervisor/daemon process identity, immutable release readback, exact rollback
predecessor, and official externally-owned Secure MCP health. None is present
as authoritative workspace evidence, and this worker has no approved
protected-host read/control surface. Workspace-visible absence is therefore
`UNOBSERVABLE_FROM_CURRENT_SAFE_SURFACE`, not a claim that the protected state
is absent.

## Authority audit

`src/user_worker_release.rs::preflight_current_user_worker_host` has no caller
inputs and only reaches `SupervisorPeerAttestationRequired` after protected,
fixed-current-user release readback. It deliberately does not connect to the
supervisor or mutate release/runtime state. Its later authority boundary is
the server-side OS-attested peer check.

`src/windows_supervisor_control_pipe.rs::register_fixed_ready_worker_listener`
is the mutating fixed-pipe route. It registers only through the fixed endpoint
and expected supervisor generation, while
`src/control_plane_supervisor.rs::bind_request_to_os_attested_peer` replaces
worker-supplied observations with OS-attested peer image identity. Calling
either without the missing independent-review decision and exact host pre-state
would violate this ticket's required fail-closed preconditions.

The release reader uses the fixed Windows current-user root and
`ProtectedDirectoryGuard` no-follow reopen path; it accepts no workspace root,
caller-selected executable, hash, role, endpoint, or trust material. The
official Secure MCP route remains external and was neither observed through an
authorized health surface nor started, stopped, adopted, or reconfigured.

## Actions not taken

No release state, Program Files/ProgramData, lifecycle/control pipe, daemon,
supervisor, target/registry, protected wake state, browser, Secure MCP/tunnel,
Scheduler/service, signing/UAC, external project, or Git state was mutated.
`DESIGNATED_CHAT_TARGET_URL` and `CHAT_TARGET_URL` were not invoked.

## Repository verification

- `cargo fmt --all -- --check` — PASS (the environment emitted only its known
  workspace canonicalization warning).
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` — PASS.
- `cargo test --workspace --all-targets --all-features` — PASS: primary suite
  889 passed, 0 failed, 21 ignored; supervisor, recovery, and integration
  targets passed.
- `git diff --check` — PASS; existing CRLF conversion notices only.

These source checks do not substitute for live pre-state/readiness/rollback
evidence and do not accept serving parity.

## Required next bounded action

First obtain and durably record independent T-0377 final-review acceptance.
Then authorize a separate fixed protected-host preflight/readback that can
prove the exact selected release, prior rollback state, stable supervisor and
daemon identity, and externally owned official Secure MCP health without using
raw shell paths or changing any runtime state. Only a positive, current,
unambiguous readback may authorize a subsequent reviewed activation canary;
paired target migration remains after independently accepted modern serving
parity.

## Attribution

This bundle is the sole T-0378 change. All other dirty-worktree files predate
this ticket and are not attributed to it.
