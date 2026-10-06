# T-0246 / T-0224-R1A-R4 — Stable Wake Implementation Review Bundle

## Independent review request

Independent final review is requested.  Controller terminal state is advisory;
review this bundle, the exact source call path, and the bounded verification
evidence below.

## Why T-0244 and T-0245 were rejected

- T-0244 retained a compressed incomplete implementation, lacked a real
  production caller, and left dead-code/fmt problems.
- T-0245 still had no exact required bundle, only two narrow tests, and only a
  `mod stable_wake_bootstrap;` declaration rather than a non-test discovery
  caller.

## Attributable files for this corrective slice

- `src/stable_wake_bootstrap.rs` — reformatted and hardened read-only stable
  review-event discovery, classification, bounds, containment, and tests.
- `src/state.rs` — production transport-status payload invokes the read-only
  stable-wake readiness wrapper and exposes only bounded counts/availability.
- `src/server.rs` — existing transport-status integration test asserts the
  bounded stable-wake result through the real production call path.
- `docs/orchestrator/review_bundles/T-0246_T0224_R1A_R4_STABLE_WAKE_IMPLEMENTATION_REVIEW_BUNDLE.md` — this evidence.

The worktree was already broadly dirty before this slice.  In particular,
`src/state.rs` contains unrelated pre-existing edits; the attributable hunk is
the `workspace_readiness(Path::new(&self.workspace_root))` call and its bounded
`stableWake` payload.  `src/stable_wake_bootstrap.rs` was an untracked prior
attempt and was substantively replaced in this slice.  No broad worktree diff
is offered as task attribution.

## Concrete production call path

`server::post_mcp` handles the existing read-only `catdesk_transport_status`
tool, calls `AppState::transport_status_payload`, which calls
`stable_wake_bootstrap::workspace_readiness`, which calls `discover`.

The wrapper uses the fixed workspace-relative stable root
`<workspace>/.catdesk/stable-wake/review-events` and fixed project binding
`catdesk`; callers cannot provide event paths, browser targets, executable
paths, daemon inputs, or credentials.  The core has no daemon/MCP/release or
browser dependency.  The status caller only observes results and reports:
`discoveryAvailable`, `pendingCount`, and `staleCount`.

## Record contract and read-only behavior

The accepted durable-record-compatible camelCase schema is:

`schemaVersion`, `recordId`, `projectId`, `sessionId`, `state`, `nextAction`,
`reference`, `createdAtUnix`, and `unread`.

- Only schema version `1` and exact project `catdesk` are accepted.
- Maximums: 64 directory entries, 16 KiB per record file, 256 bytes per
  identity/reference field.
- `recordId`, `projectId`, `sessionId`, `state`, and `nextAction` must be
  non-empty ASCII alphanumeric, `_`, or `-` identities.
- `reference` must be a non-empty, single normal relative component; rooted,
  prefixed, empty, and parent-traversal forms are rejected.
- Root and spool are canonical directories; entry names are relative normal
  components; entries must be non-symlink regular files; canonical candidates
  must remain beneath the canonical spool.
- Discovery never writes, renames, claims, acknowledges, deletes, receipts, or
  submits an event.

## Deterministic semantics

- Pending means exactly `unread=true`, `state=COMPLETED_VERIFIED`, and
  `nextAction=independent_final_review`.
- Every other schema-valid record is stale/non-actionable.
- Byte/semantic exact duplicates collapse by `recordId` in deterministic
  `BTreeMap` ordering.
- A shared `recordId` with any semantic difference fails closed with
  `stable wake event conflict`.

## Focused evidence

`cargo test stable_wake_bootstrap` passed: 8 tests, including canonical absolute
root/camelCase pending/read-only preservation, stale/duplicate collapse,
conflict/project/schema/malformed rejection, bounds/count/traversal rejection,
identity/non-regular rejection, rooted/prefixed reference rejection, wrapper
determinism, and a source regression proving the production status caller.

The tests directly retain read-only before/after bytes.  Symlink escape testing
is compiled where deterministically supported (`cfg(unix)`); Windows production
also rejects `symlink_metadata` symlink entries and canonical containment escape.
`cargo test transport_status_tool_returns_redacted_status` also passed, proving
the existing non-test server/status path invokes the wrapper and returns a
bounded unavailable result when the fixed stable spool is absent.

## Verification evidence

| Check | Result |
| --- | --- |
| `cargo fmt --check` | passed |
| `cargo clippy --all-targets --all-features -- -D warnings` | passed |
| `cargo test stable_wake_bootstrap` | passed (8 tests) |
| `cargo build` | passed |
| main test binary | passed: 723 passed, 21 ignored |
| supervisor test binary | passed: 15 passed |
| `recovery_powershell` integration | passed: 2 passed |
| `t0215_measure` integration | passed: 2 passed |
| `t0217_release_measure_tmp` integration | passed: 0 tests |

The initial monolithic `cargo test` invocation exceeded the 60-second command
window after the main and supervisor binaries had reported success.  Remaining
integration binaries were rerun individually and passed as recorded above.
The repository `rust_full` Rust plan is `cargo fmt --check`, `cargo test`, and
`cargo build`; its components are evidenced above without relaxing a gate.

## Prohibited mutations not performed

No browser launch/submission, durable claim/receipt, `.catdesk` mutation,
conversation-target mutation, host install/reload/promotion, Secure MCP/tunnel
mutation, Scheduler/service/ProgramData mutation, signing/provenance work, or
Git publication occurred.

## Residual R1A work

Stable target-config integrity and the complete runtime-independence matrix
remain for a later bounded slice.  Durable claim/receipt ownership, browser or
interactive-desktop execution, and stable host installation remain R1B work.
