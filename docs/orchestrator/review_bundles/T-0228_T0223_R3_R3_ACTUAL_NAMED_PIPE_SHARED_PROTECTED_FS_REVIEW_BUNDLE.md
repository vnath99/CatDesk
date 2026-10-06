# T-0228 — actual named-pipe and shared protected filesystem review

## Result: fail closed, not activation-ready

T-0228 inspection reconfirmed that T-0227's source is a non-authoritative
pipe skeleton and that the stable-supervisor writer still holds pathname-based
state/install authority. This bundle does not claim closure.

The accepted R3 data plane remains fixed: loopback supervisor `/mcp` on 3201
can proxy only to the validated worker MCP endpoint on 3200. Route snapshots,
rollback, timeout/body/concurrency limits, redirect and hop-header refusal,
remote-route-detached semantics, fixed startup dry-run planning, and Secure
MCP external ownership remain unchanged.

## Actual pipe requirement retained

`windows_supervisor_control_pipe.rs` uses the compiled pipe identity
`\\.\pipe\CatDeskControlPlaneSupervisorV1`, bounded 64 KiB framing, and
attempts `GetNamedPipeClientProcessId`, `OpenProcess`, and token query before
record parsing. It intentionally refuses before dispatch because it does not
yet derive and compare the connected client token/user/session, executable
file identity/hash, and fixed service identity from OS handles. Its declared
SDDL is not applied by the current Tokio pipe construction. Therefore a pipe
request cannot mutate state, and no serialized PID/hash is trusted.

## Shared filesystem requirement retained

The reviewed `ProtectedDirectoryGuard` in `reviewed_source_snapshot.rs`
already has the correct Windows `RootDirectory`/`NtCreateFile`, no-follow,
identity-pinning, no-delete-sharing, relative file, and relative rename
machinery. It is not shared with the separately compiled supervisor binary.
Migrating stable state, locks, staging, versions, image bytes, current/LKG,
receipts, temp commits, quarantine, and rollback requires extracting that
unsafe handle implementation into a common module and replacing all current
pathname reads/writes/renames/exists operations. No weaker copy or pathname
fallback was introduced.

## Verification

- `cargo fmt`
- `cargo test control_plane_supervisor --no-fail-fast` — 15 focused tests
  passed in both binary harnesses.
- `cargo clippy --all-targets --all-features -- -D warnings` — passed.

No ProgramData, production port 3201, Scheduler/service, daemon/release,
Secure MCP/tunnel, browser, Git, signing, or provenance mutation occurred.

## Attributable files

- `src/windows_supervisor_control_pipe.rs`
- `src/control_plane_supervisor.rs`
- `src/main.rs`
- `src/bin/catdesk-control-plane-supervisor.rs`
- `docs/orchestrator/review_bundles/T-0228_T0223_R3_R3_ACTUAL_NAMED_PIPE_SHARED_PROTECTED_FS_REVIEW_BUNDLE.md`

The exact R4 prerequisite remains a reviewed Windows implementation that
applies the product ACL, performs complete handle-derived peer attestation,
extracts/migrates the shared pinned filesystem authority, and proves hostile
pipe/reparse/replacement/outside-sentinel behavior before any live activation.
