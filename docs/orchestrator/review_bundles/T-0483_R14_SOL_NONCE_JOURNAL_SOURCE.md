# T-0483 R14 — Dormant Sol handoff nonce journal

Date: 2026-10-15. Status: source-only host primitive; no Sol launch or live handoff accepted.

## Source change

`src/delegated/sol_worker_nonce.rs` adds a host-side one-use nonce journal for the verified output contract in `sol_worker_output.rs`. A trusted host supplies a `ProtectedDirectoryGuard` for a ledger object outside worker write authority. Before giving the worker a generated canonical UUID-v4 nonce, `issue_sol_worker_nonce` writes a create-once, bounded, synced issue record and reads back the same object. The record binds the host-held session ID, task ID, host turn ID, contract hash, baseline commit and allowed output paths. The worker's output manifest cannot mint or change this authority.

After trusted host-side native CLI telemetry exists, `bind_sol_worker_native_turn` writes a separate create-once record. It binds the issued nonce and host authority to the expected Codex thread and exact Sol/medium native-turn profile. `verify_and_burn_sol_worker_output` requires that canonical bound record and the exact native turn ID, then checks the pinned output and writes a create-once burn record **before** returning captured bytes. `FILE_CREATE` for the burn name is the concurrent replay serialization point. Missing, torn, duplicate or changed issue/native-turn/burn records fail closed. Failed binding or output checks do not burn a nonce, so the host must decide whether an issued nonce remains active.

All three record types use schema 1 and reject unknown fields. Each serialized record is capped at 256 KiB; the authority limits the path set to 256 entries. This module is registered in `src/delegated/mod.rs` but has no Sol launcher or controller call site. It grants no staging permission.

## Local verification

Seven focused Windows tests cover: one successful handoff followed by rejection after reopening the ledger; two concurrent contenders with at most one returning bytes; coordinated native-turn substitution across host expectation and worker manifest failing; identical native bind replay; torn/missing native binding; wrong model or effort; a torn/existing burn marker remaining spent; and missing/torn issue records. These are fixture tests for the primitive, not evidence of a real restricted worker. The final post-review full root all-features Rust suite exited 0 (main binary: 1,154 passed, 26 ignored; integrations passed); strict all-target/all-feature Clippy, rustfmt and diff check passed. The exact next commit's independent Rust, WakeHost Rust and Python Windows CI remains pending.

## Remaining authority and durability gates

- A separate OS service must establish a restricted worker identity, keep CatDesk host authority outside worker writes, retain the pinned output and ledger objects, prove all worker descendants exited, and prove the worker lost write authority before verification.
- The trusted host must persist which issued nonce belongs to the active task/turn and resolve abandoned unburned issue records. This module cannot choose the active nonce from journal contents on restart.
- File content is synced, but parent-directory metadata persistence across sudden power loss is not proven. A power-loss-safe issue/burn protocol and reviewed recovery behavior remain required.
- A trusted staging transaction must consume the returned bytes with source/review binding. No real Codex Sol/medium deliverable, ChatGPT review, natural `review_ready` event, Wake receipt or recurring loop has been accepted.
- The P0 Wake install broker is unprovisioned. This source primitive does not change that hold.

## Exact CI status at documentation checkpoint

Pushed predecessor `48a62de0ca0cc6398a46855bb111231edb4761b4` has zero GitHub check runs. Explicit workflow dispatch returned HTTP 422, `Actions has been disabled for this repository`, although the repository API reports `enabled=true` and the workflow is active. [GitHub's Actions settings documentation](https://docs.github.com/en/repositories/managing-your-repositorys-settings-and-features/enabling-features-for-your-repository/managing-github-actions-settings-for-a-repository) says a separate GitHub-controlled disabled state may require GitHub Support review. No predecessor CI result can stand in for this source increment's exact commit.
