# T0478 R3B4-B2 — Fixed GitHub branch/CI witness foundation

**Date:** 2026-10-11. **Canonical chat:** https://chatgpt.com/c/6acba3bf-e740-8331-8896-8a25fefdbf16.

## Purpose and scope

The CatDesk GitHub mirror/publication milestone is already complete. This separate T0478 source-only feature aims to obtain *current remote source authority* before copying candidate Git object bytes into an isolated disposable developer fixture. Production recovery remains incomplete (stable supervisor root unavailable, startup definition/policy unproven, frontDoor3201 not probed); no production startup, pending signed T0366 epoch2 rotation or official externally owned Secure MCP changes are part of this work.

## New source-only module

Added dormant `src/isolated_fixture_github_authority.rs`, using the existing `reqwest` dependency. It exposes a single fixed HTTPS live verification entry point and a privately constructed typed `LiveGithubSourceWitness` with only a commit SHA and CI run ID.

- Server endpoint fixed in source to `https://api.github.com/repos/vnath99/CatDesk`, no caller-controlled base URL, token, credentials or redirect. HTTPS certificate verification uses reqwest's default TLS trust. No general network or REST tool exposed through MCP.
- Branch fixed to `orchestrator/chatgpt-codex-autonomous-loop`, workflow fixed to `.github/workflows/ci.yml`. Require exactly matching 40-character lowercase hex commit and remote branch head before and *after* CI check.
- Query recent branch Actions runs. Select highest run ID among exact SHA, branch, workflow and push-event matches; reject if latest matching run not `completed/success`, even when an older successful run exists.
- Read the exact selected run's job inventory and require `total_count == 3` and one each of `Rust`, `Python Wake and advisor tests`, `Independent WakeHost Rust`, all `completed/success` with matching run IDs.
- Reject non-200 status, malformed responses, unsupported/unknown data, request timeout, HTTP redirect, missing jobs and responses above a hard 256-KiB cap. The HTTPS path takes no user-provided JSON to mint a witness; pure JSON validation functions are private and exist for deterministic unit tests.
- The existing dormant `src/isolated_fixture_git_objects.rs` receives a new `read_witnessed_candidate_git_objects(workspace, &witness, snapshot_id)` facade. It passes the opaque witness commit into the existing fixed two-path local trusted Git-object reader, but **does not** yet produce a protected snapshot, confer read/write permission to MCP, or start an autonomous worker.
- This verifies GitHub's currently reported branch and passing CI, **not a cryptographic Git signature**, independent code-review approval, or a privileged production release. A repository with inaccessible GitHub public REST endpoints fails closed; authorization for private repos is a separately reviewed capability.

## Local verification and limits

Five deterministic focused tests in `isolated_fixture_github_authority.rs` exercise exact branch-SHA match, wrong branch/commit, missing/not-green/PR-run CI, rejection of newer failed/in-progress run masking an older passing run, the exact 3-job inventory, mismatch/failed individual jobs and invalid commit shape.

Local Windows: `cargo fmt --check` PASS; `cargo test --locked --offline --bin catdesk isolated_fixture_github_authority` 5/5 PASS; `cargo test --locked --offline --bin catdesk isolated_fixture_git_objects` 4/4 PASS; strict all-target/features Clippy `-D warnings` PASS; full main Rust tests still to be confirmed for this increment. Actual outbound Rust HTTPS calls have not been exercised in these tests. The response shapes were read independently via the connected GitHub API and match the validators for branch, workflow and jobs; this is contract inspection, **not** an end-to-end in-process live proof. Independent GitHub Windows CI for the new commit must be terminal SUCCESS on Rust, Python Wake/advisor and WakeHost before R3B4-B2 can be accepted.

## Next milestones

1. Verify publication/CI for this B2 source, then add a bounded operator-owned live HTTPS canary (no credentials/production access) that proves `fetch_live_witness` on the exact newest verified CI commit. If the source repository is private and unauthenticated read fails, stop and design a separately reviewed access method rather than silently substituting a user JSON assertion.
2. T0478 R3B4-B3: fixed leaf source snapshot with trusted Git-object bytes and audited no-follow Windows `ProtectedDirectoryGuard` handle-bound file creation, exact length/SHA-256 remeasurement, deterministic provenance manifest and owned temp root.
3. T0478 R3B4-C: only a proven in-process immutable manifest permits fixture read/search. Reject new files, modified bytes, hardlink/reparse and changed root identity from the single no-follow handle. No generic shell, new MCP tool or source/workspace mutation.
4. T0478 R3B4-D: bounded owned child PID/port readiness, stop/restart and crash recovery. After this, progress to T0479 versioned worker + LKG release and production recovery (supervisor root/startup/port), rather than redoing GitHub mirroring.
