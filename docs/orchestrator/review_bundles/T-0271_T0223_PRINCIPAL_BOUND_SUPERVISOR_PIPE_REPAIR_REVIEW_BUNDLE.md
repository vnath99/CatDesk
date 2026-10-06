# T-0271 / T-0223 — Principal-Bound Supervisor Pipe Repair

## Scope and T-0270 rejection

T-0270 closed the functional 3201 front-door plus fixed-pipe wiring gap, but
independent host review rejected its pipe principal boundary.  Its DACL granted
the All Services SID although the actual worker is an ordinary
`catdesk.exe --catdesk-daemon` process, and it read (but did not compare) the
connected token user before decoding registration data.

This corrective source/test slice changes only that Windows principal boundary.
No ProgramData state, supervisor process, port 3201 listener, named pipe,
scheduled task, service, worker, release, browser, Secure MCP/tunnel, Git,
signing, or provenance state was activated or mutated.

## Production principal trace

The supported lifecycle is an ordinary interactive-user model:

- `scripts/test-catdesk-lifecycle.ps1` asserts the CatDesk autostart task uses
  `InteractiveToken`, `RunLevel=Limited`, and the exact current user.
- `scripts/start-catdesk-stack.ps1` starts the canonical current image as
  `catdesk.exe --catdesk-daemon` without `-Credential`, service, or alternate
  principal selection.
- `src/daemon_reload.rs::launch_catdesk` starts the same fixed daemon argument
  with the caller's process token; it owns no supervisor-root authority.
- The zero-argument stable supervisor is separately installed but has no
  service-launch or caller-selected-principal surface in this slice.

The expected model is therefore the supervisor's ordinary interactive user and
nonzero session, with the daemon inheriting that same user/session. Elevation
is not an authority selector: the documented autostart expectation is limited;
if a supervisor is instead launched as LocalSystem or session zero, startup
fails closed rather than inventing a service principal or widening the DACL.

## Before/after DACL and OS evidence

Before T-0271, the pipe used a fixed DACL containing an All Services ACE.
After T-0271, `serve_fixed_control_pipe` snapshots its own Windows token once
at startup:

1. opens the supervisor token with `TOKEN_QUERY`;
2. obtains and bounds `TOKEN_USER`, copies the binary SID, converts the
   OS-supplied SID to bounded SDDL, and obtains `TOKEN_SESSION_ID`;
3. rejects missing/malformed token information, LocalSystem, or session zero;
4. builds only `D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;<OS TokenUser SID>)`.

There is no external SDDL/SID/principal input.  The descriptor has no All
Services, Everyone, Users, or Authenticated Users ACE. System and
Administrators are retained only as OS recovery DACL entries, not protocol
roles.

For every accepted pipe instance, before framed-record read or JSON decode,
the server derives PID using `GetNamedPipeClientProcessId`, opens that real
process/token, derives TokenUser and TokenSessionId, and requires binary SID
and session equality with the startup snapshot. It refuses unqueryable,
malformed, different-user, and different-session clients at this pre-decode
boundary. Only then does it verify the process is live, obtain its actual image
path, and independently SHA-256 hash that image. The existing
`bind_request_to_os_attested_peer` binding still requires declared and expected
worker digest to match that OS-observed digest, overwrites serialized observed
values, and preserves generation CAS/old-backend refusal behavior.

## Request-role matrix

| Connected principal | May decode request | Allowed requests |
| --- | --- | --- |
| Exact TokenUser + TokenSessionId worker | Yes | `Status`, then fixed `REGISTER_BACKEND` only |
| SYSTEM or Administrators recovery identity | No | None; no registration or generic recovery channel |
| Any other/malformed/unqueryable principal | No | None |

The server continues after individual connect, peer-attestation, framing, or
decode refusal, so hostile local clients do not terminate the long-lived pipe
surface. Pipe creation remains required-surface failure and still tears down
the paired 3201 supervisor surface through the existing `try_join!` wiring.

## Deterministic hostile/refusal coverage

Focused Windows/source tests cover:

- exact OS-derived DACL construction and refusal of empty, LocalSystem, and
  session-zero principals;
- source absence of All Services/Everyone/Users/Authenticated Users pipe ACEs;
- same SID plus same session worker admission and different SID/session
  pre-decode recovery refusal;
- recovery-role refusal for every control record, including registration;
- bounded fixed request framing and no generic client/API;
- source evidence that PID, `TOKEN_SESSION_ID`, pre-decode role checking,
  raw pipe security attributes, and remote-client rejection remain present.

Existing `control_plane_supervisor` coverage additionally proves fixed
127.0.0.1:3200 gating, generation CAS/idempotency, invalid peer refusal,
manifest/image mismatch old-backend preservation, and replacement of
worker-supplied observed hash fields with OS-attested image identity. Thus a
fabricated JSON PID/SID/session/hash cannot supply principal evidence or bypass
the wrong-executable digest check.

## Attributable changes

- `src/windows_supervisor_control_pipe.rs`
  - replaces the obsolete service-SID DACL with supervisor-token-derived
    same-user/session authority;
  - adds pre-decode client TokenUser/TokenSessionId comparison;
  - keeps independent image hashing and makes individual hostile-client
    failures nonfatal to the pipe loop;
  - adds focused principal/DACL/source regressions.
- `src/control_plane_supervisor.rs`
  - documents the transport's pre-decode ordinary-principal requirement.
- this review bundle.

The repository was broadly dirty before this task. No unrelated existing
changes were reverted or attributed.

## Verification

Completed during implementation:

- `cargo fmt`
- `cargo test windows_supervisor_control_pipe --no-fail-fast` — passed (4
  focused Windows pipe tests in both relevant binary/module harnesses).
- `cargo test control_plane_supervisor --no-fail-fast` — passed (19 focused
  supervisor tests in both relevant binary/module harnesses).
- `cargo fmt --check` — passed.
- `cargo clippy --all-targets --all-features -- -D warnings` — passed.
- `cargo test --no-fail-fast` — passed: the principal unit suite and full
  workspace test matrix completed with 796 main tests, 775 passed and 21
  expected ignored, plus all binary/integration suites shown by Cargo.
- `cargo build --locked --bins` — passed.
- `cargo build --locked --release --bin catdesk-control-plane-supervisor` —
  passed.
- `git diff --check` — passed.

No repository-local executable `rust_full` command was present; the supplied
profile name appears only in CatDesk contract metadata, so the strict Cargo
checks and supported locked normal/release builds above are the locally
available verification evidence. The recurring Cargo
`could not canonicalize path C:\\Users\\Volap` warning is an environment
warning and did not affect these passes.

## Remaining separately reviewed host acceptance

Independent host review must still provision the already-reviewed stable
supervisor under the documented interactive product user, confirm its nonzero
session matches the canonical daemon, start neither through this provider
work, and observe a normal fixed-pipe registration/refusal path. It must
verify the exact DACL, connected-process token/session, OS image digest,
generation readback, old-backend preservation, and 3201 front-door behavior.
Controller/source green alone does not close T-0223.
