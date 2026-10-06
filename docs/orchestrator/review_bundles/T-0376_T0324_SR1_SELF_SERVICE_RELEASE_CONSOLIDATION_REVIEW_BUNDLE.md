# T-0376 — T-0324 SR1 self-service release consolidation

## Classification

SR1_REPOSITORY_READY_FOR_BOUNDED_LIVE_PREFLIGHT

This repository/source-test classification does not establish a deployed worker, host readiness, current-chat binding, event-driven wake acceptance, reviewed-image promotion, or authority for an ad-hoc reload/promotion path.

## Lineage and preserved history

The original T-0374 autonomous session `adc-t0374-t0324-sr1-self-service-user-release-foundation-20260910` remains `CANCELLED` after repair-budget exhaustion and is not resumed. Its later SR1 repair superseded split `current.json` / `previous.json` transition with the single atomic `active-state.json` authority described in `T-0374_R1_T0324_SR1_ATOMIC_USER_RELEASE_STATE_REVIEW_BUNDLE.md`. T-0375 separately corrected only the stale 12-versus-13 registry-schema test.

## Authority-flow audit

| Boundary | Source/evidence | Result |
| --- | --- | --- |
| Candidate identity | `UserWorkerReleaseManifestV1` binds fixed ordinary-worker role, generation, image SHA-256/length, reviewed-source identity, review session/record/authority digest, and attestation identity/digest. | No caller `accepted` flag, path, role, or image hash is authority. |
| Durable transition | `user_worker_release.rs` prepares immutable generation-plus-manifest-digest directories; `active-state.json` atomically carries current plus at most one prior pointer. | Replay is idempotent; stale CAS, lower generation, malformed relation, tamper, reparse, and interrupted staging fail closed. |
| Production reopen | `read_current_user_worker_release()` gets the Windows current-user profile through `SHGetKnownFolderPath`, then uses `ProtectedDirectoryGuard` descent under fixed `AppData/Local/CatDesk/WorkerReleases`. | No PATH, environment, workspace, caller root, caller filename, or raw `target/release` selection. |
| Supervisor eligibility | `fixed_current_user_worker_registration()` delegates to `fixed_current_worker_registration()`, requiring the running image to equal the validated manifest digest and producing only compiled fixed endpoint/backend identity. | Representation only; it does not start or activate a worker. |
| Peer attestation | `windows_supervisor_control_pipe::handle_connection` calls `bind_request_to_os_attested_peer`, replacing worker-supplied observed image fields before `RegisterBackend` mutates supervisor state. | Worker claims alone cannot replace the active backend. |
| Registry/schema parity | `AutonomousCommandProfileV1::catalog()` derives runtime capabilities including `CARGO_BUILD_RELEASE_ISOLATED`; registry bind has exactly 13 closed-world forms. T-0324 paired mode is exact `DESIGNATED_CHAT_TARGET_URL=<URL>` plus project/digest CAS. | Source-ready; legacy serving daemon cannot consume it. |
| External runtime | The release module has no tunnel, Secure MCP, browser, wake, Scheduler, service, or external-route operation. | Official Secure MCP remains externally owned and observation-only. |

## Privilege and host boundary

The per-user SR1 store removes routine per-version product-root signing/UAC once a stable host consumes this reviewed release model. Exceptional product-root bootstrap/rotation remains separate. The old pre-T-0299 daemon is not a valid consumer. The exact next action after independent final review is a separate fixed host-live preflight that only reads trusted current-user release state, validates reviewed candidate/supervisor eligibility, and reports readiness or rollback attention. It must not activate a worker or mutate target, wake, tunnel, service, Scheduler, Program Files, or ProgramData.

## Source-change decision and attribution

No T-0376 product-source defect was proven, so no product source changed. This bundle is the sole T-0376 edit. The audit confirmed the SR1 atomic state repair, protected current-user reopen, and existing server-side peer attestation rather than a second supervisor or trust root.

## Verification

- `cargo fmt --all -- --check` — PASS.
- `cargo clippy --all-targets --all-features -- -D warnings` — PASS.
- `cargo test --all-targets --all-features` — PASS (909 unit tests plus
  integration targets).
- `git diff --check` — PASS; existing CRLF notices are not source failures.

The separately operated isolated release-equivalent verification remains outside this connected controller because it does not deserialize that newer profile. Independent final review is required; this bundle does not self-accept SR1 or authorize host action.

## Prohibited-action audit

No daemon cutover/reload, release activation, Program Files/ProgramData write, UAC, signing, browser wake, project/wake target change, Secure MCP/tunnel change, Scheduler/service action, Git publication, external-project mutation, or dirty-worktree cleanup occurred.
