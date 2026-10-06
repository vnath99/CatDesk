# T-0274 / T-0223-R4B — Host-Live Stable-Supervisor Activation and Continuity Acceptance

## Disposition

**`SUPERVISOR_ACTIVATION_SURFACE_UNAVAILABLE`**

T-0275 is recorded as independently accepted in the current plan. This fresh
T-0274 audit nevertheless stopped before any host mutation because the current
reviewed product exposes no first-class, fixed host install/activation
operation. This is a fail-closed source-surface blocker, not an elevation
request and not a host acceptance result.

## Required evidence read

The audit read the current plan and the accepted T-0270, T-0271, and T-0275
review bundles, then inspected only the stable-supervisor entrypoint,
protected installer/state APIs, fixed control-pipe server, and canonical worker
registration seam.

Accepted source properties remain visible:

- T-0270 supplies the fixed `127.0.0.1:3201` front door and the fixed private
  control pipe, with worker registration limited to an already-bound canonical
  `127.0.0.1:3200` listener.
- T-0271 derives the expected pipe client TokenUser and TokenSessionId from the
  supervisor token, checks both before decoding authority-bearing records, and
  retains independent executable/reviewed-manifest validation and generation
  CAS.
- T-0275 supplies protected no-follow state/install readiness checks, unique
  crash-resilient temporary/staging identities, and source-linked runtime
  non-ownership of externally managed Secure MCP/tunnel.

## Closed activation-surface audit

| Candidate | Current authority | Result |
| --- | --- | --- |
| `assess_fixed_supervisor_activation_readiness()` | Fixed, read-only readiness report | Not an installer or activator. |
| `ControlPlaneSupervisorStoreV1::fixed_read_only()` | Fixed, read-only protected-root open | Not an installer or activator. |
| `reviewed_supervisor_installer_plan(...)` | Plan validation only; takes digest values | Not a host mutation surface. |
| `SupervisorInstallerWriterV1::install_exact_reviewed_image(...)` | `pub(crate)` writer taking image bytes and digest | Deliberately unavailable to operator/CLI/MCP callers; all visible call sites are tests. |
| `catdesk-control-plane-supervisor` | Zero arguments only; reads an already-provisioned root and then serves 3201 plus pipe | A listener runner, not installation/activation; invoking it would not establish reviewed install authority. |
| Fixed worker pipe registration | Crate-private registration after canonical worker listener readiness | Registers a worker with an already-running supervisor; cannot install or activate one. |

The source search found the installer writer declaration plus test call sites,
but no public binary, lifecycle facade, CLI, or host operation that binds
accepted reviewed artifact evidence to the fixed protected install root and
then starts/activates the stable supervisor without caller-selected authority.

## Host activity deliberately not performed

Because the required reviewed activation surface is absent, this provider turn
did **not** inspect or mutate live ProgramData install/state, start a
supervisor, bind/listen on 3201 or 3200, connect to the pipe, restart a worker,
exercise registration, perform negative injection, or exercise rollback.
Consequently there is no host PID, port, generation, receipt, registration, or
continuity claim in this bundle.

The stable-supervisor runtime boundary was source-audited only: the entrypoint
is limited to fixed local 3201, fixed private control pipe, and an existing
protected local root. Its source explicitly says it never starts or
reconfigures a tunnel. No Secure MCP/tunnel route, identifier, configuration,
credential, or health detail was accessed or changed.

## Narrow required repair

Before another host-live attempt, add and independently review exactly one
first-class fixed host supervisor activation/install/status surface. It must
have no caller-selected ProgramData path, service/Scheduler identity, pipe,
listener, executable, backend, hash, SID/session, tunnel, or arbitrary command
authority. It must consume the accepted reviewed-artifact trust evidence,
perform protected fixed-root preflight, provide a bounded status/result, and
be the sole reviewed lifecycle route for install/activation and any supported
rollback/restart. The current crate-private byte writer is intentionally not a
substitute.

## Attribution and verification

Only this required review bundle was changed in this provider turn. No source
was edited. The repository is broadly dirty/untracked, including the
supervisor source files, so whole-worktree status is not task attribution.

Completed bounded checks:

- current-plan and exact predecessor-bundle review;
- closed-surface/source call-site audit of the installer, entrypoint, pipe, and
  worker registration seam;
- `git diff --check` — exit code 0. Git emitted pre-existing LF-to-CRLF
  warnings for broad unrelated worktree files.

No test/build command was required or represented as host proof because source
was not changed and the activation hard gate stopped the task before host work.

## Prohibited actions not performed

No direct ProgramData/SCM/Scheduler manipulation; no live supervisor/worker
start, stop, restart, or duplicate; no browser or wake action; no ChatGPT
target/profile change; no Secure MCP/tunnel mutation; no release, signing,
provenance, dedicated-producer, or Git publication action occurred.

## Independent review request

Please independently review the exact source-surface finding and this bundle.
T-0223 host-live acceptance remains open. The next action is the single narrow
source repair above, not a host activation workaround.
