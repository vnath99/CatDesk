# T-0227 — named-pipe and handle-relative supervisor authority review

## Preserved R3/R3-R1 boundary

The fixed supervisor data plane remains `/mcp` on loopback port 3201 with the
fixed worker endpoint on 3200. It is not a registration endpoint. Secure MCP
remains external, and remote-route-detached is distinct from local backend
health. No live ProgramData, supervisor listener, Scheduler/service, daemon,
tunnel, browser, or Git mutation occurred.

## Exact Windows requirement

The fixed intended control endpoint is
`\\.\pipe\CatDeskControlPlaneSupervisorV1`. A valid production server must
create it with an explicit least-privilege product ACL and derive, from the
accepted pipe instance, the client PID (`GetNamedPipeClientProcessId`), token
(`OpenProcessToken`/`GetTokenInformation`), process identity, and executable
file identity/hash from OS handles. It must reject a failure at any of those
steps before decoding an authority-bearing operation.

The only records remain REGISTER_BACKEND, REPORT_BACKEND_HEALTH,
ROLLBACK_BACKEND, STATUS and bounded remote-route observation. Each record is
bounded, has no executable/path/URL/command/tunnel/credential field, and
requires fixed endpoint, manifest equality, listener readiness, OS-derived
process identity, generation CAS, and idempotency. N→N+1 keeps prior N as LKG;
an in-flight proxy request keeps its R3 route snapshot.

## Fail-closed state

T-0226 supplied a dispatcher accepting an externally-attested peer. T-0227
adds `SupervisorControlTransportV1` to make the missing boundary explicit:
only an OS-attested transport may invoke the state mutation dispatcher. A
decoded worker record cannot construct its own peer evidence. This is not
claimed as a completed Windows named-pipe implementation.

`src/windows_supervisor_control_pipe.rs` now contains the fixed Windows pipe
endpoint skeleton with the compiled pipe name, fixed SDDL policy, 64 KiB
framing, `GetNamedPipeClientProcessId`, `OpenProcess`, and
`OpenProcessToken`/`GetTokenInformation` calls before record parsing. The
current peer routine intentionally returns a bounded refusal after proving it
can query the token: it does **not** yet complete the required handle-derived
process-image hash and service-SID/token comparison, so it cannot dispatch a
record. This is safer than trusting the record's process hash. The binary
includes the module but does not activate the pipe in this provider turn.

The current standalone supervisor is compiled separately from the main binary,
while `ProtectedDirectoryGuard` and its `OBJECT_ATTRIBUTES.RootDirectory`
child primitives remain in `reviewed_source_snapshot.rs`. Extracting those
Windows handles without duplicating their unsafe implementation requires a
shared module compiled by both binaries. The existing R3 supervisor writer
still has pathname operations, so it must not be treated as protected stable
authority. No pathname fallback, synthetic peer PID/hash, or unauthenticated
HTTP control endpoint was added.

## Required next implementation boundary

Before R4, extract the reviewed Windows no-follow/root-handle code to one
shared module, migrate stable state/lock/staging/version/image/current/LKG
operations to pinned relative handles, and implement the actual pipe server
and worker client with a real pipe ACL plus OS token/image/process evidence.
Add Windows hostile reparse/dangling/special/replacement/outside-sentinel and
pipe-peer/replay/concurrency tests. This ticket deliberately leaves R4 blocked
rather than manufacturing those proofs in provider-only tests.

## Attributable files

- `src/control_plane_supervisor.rs`
- `src/windows_supervisor_control_pipe.rs`
- `src/main.rs`
- `src/bin/catdesk-control-plane-supervisor.rs`
- `docs/orchestrator/review_bundles/T-0227_T0223_R3_R2_NAMED_PIPE_HANDLE_RELATIVE_SUPERVISOR_AUTHORITY_REVIEW_BUNDLE.md`

Independent review is required; no live activation or mutation is claimed.
