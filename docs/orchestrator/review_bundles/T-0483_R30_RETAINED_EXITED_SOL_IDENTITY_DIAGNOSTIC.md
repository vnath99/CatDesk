# T-0483 R30 — Retained exited Sol identity diagnostic (source only)

## Change

The inert `StoppedBoundFixedWorkerCandidateV1` can now diagnose the original exited process and the primary token handle retained before resume. It checks the retained process handle's PID, creation time and signaled exit, then rereads the retained token and compares its complete snapshot with the pre-exit observation and restricted-token policy. Fixed `CatDeskSolWorker` account preflight brackets that readback. The diagnostic does not reopen a PID or call `OpenProcessToken` on an exited process. It returns only `Result<(), String>` and grants no nonce burn, stage, repository use, review or Wake authority.

## Verification

- A real short-lived same-user child fixture confirms that the original retained process and token can be read after exit. Negative cases cover a live child, wrong PID, wrong creation time, changed expected token snapshot and the old live observer's rejection after exit. The token suite passed 10 tests with one ignored helper; the focused `sol_worker_` suite passed 83 tests with two ignored helpers.
- The isolated root shell/cwd test and remaining all-features workspace Rust suite passed locally after this source, including nested Wake tests. Strict root all-targets/all-features Clippy, formatting and all-bin build passed. Independent source review found no concrete blocker.
- Source commit `de89d796fa49172ae8be94a696d421fa25486a82` was pushed. Its immediate read-only GitHub readback showed zero workflow runs and zero check-runs; there is no remote CI acceptance for this SHA.

## Remaining gate

The fixture uses the host user; `CatDeskSolWorker` is absent here, so no positive distinct-user fixed-account run occurred. A retained original process and token do not exclude escaped descendants or duplicated writable handles, establish ancestor ACL exclusion, or prove all worker write authority ended. The separately provisioned restricted primary token, creation-time fixed-user launch, durable burn-to-stage recovery, real Sol/medium deliverable, natural `review_ready` and exact Wake receipt remain required. The candidate remains inert.
