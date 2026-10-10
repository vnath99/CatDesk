# T-0460 — Guarded canonical target digest recovery (Chat55)

**Date:** 2026-10-09  
**Requested target:** `https://chatgpt.com/c/6ac823ac-91b8-83e9-8996-f639c461de50`  
**State (2026-10-09 22:02 UTC):** Guarded core recovery source PASSED full three-job Windows CI at `b3585ad` (run `37992094331`). The subsequent narrow CLI readback fix at `0b31b05` also PASSED all three Windows jobs in run `37995124770` (Rust, independent WakeHost Rust, Python). Local targeted CLI regression, digest-repair/paired-rollover regression, divergence/no-mutation regression, `cargo fmt --all -- --check`, and `git diff --check` all PASS. The docs-only successor is `7f0b30f`. Source was inspected again, but a distinct protected CatDesk independent-review authority has not been established. Neither serving deployment nor live target binding has been performed.

## Problem evidenced
- CatDesk project registry advertises predecessor URL `https://chatgpt.com/c/6ac6cbe8-6f0c-83ea-9f7d-13489d4d87f5` but stored digest `8eb1e045f231df3214c16d7f97b382511399deef2e62b8ff32b3cdbeea004eb3`.
- Independent WakeHost dev.84 generation 31 advertises the same predecessor URL with valid SHA-256 `3a1d4cc69dfa3d05cb2bc33ef1197ac5a7433a120cfb4214a6996edb5a9d29e9`.
- The normal guarded `DESIGNATED_CHAT_TARGET_URL` route rejects before mutation at `designated_chat_target_readback_locked`, because normal registry loading validates URL/digest parity. The existing CLI `target set` calls the same function; it is not a separate fix.
- Current manual event `manual-wake-mcp-1791459602150` remains ambiguous SUBMITTING on generation 31, and must not be replayed. The target migration must quarantine it on its immutable old generation.

## Candidate approach
- Add a tightly scoped `reconcile_catdesk_digest_with_wake` registry method invoked **only** from the already guarded `operator_update_designated_chat_target` action, and only if its normal readback returns ProtectedStateMismatch.
- Compare caller-supplied expected *old* digest to effective independent Wake URL SHA-256 under the existing global Wake target lock. Refuse stale expected digest, unsupported URL, absent/malformed/oversized project registry, different CatDesk URL, multiple/missing CatDesk project rows, an already correct digest, or corruptions that fail complete standard registry validation.
- Under existing registry registration lock, read no more than 1 MiB of registry bytes, replace **only** the CatDesk digest with the witnessed old Wake digest, validate the entire registry using unchanged normal validation, and commit via the existing validated atomic registry writer.
- Re-read the paired target, then resume the normal quarantine-capable Wake+registry transaction with its CAS guards. Retain all other project data, all historical delivery evidence, and external tunnel ownership.
- This correction is *not* a replacement for reviewed release, independent review, or exact end-to-end Wake acceptance.

## CLI entry-point blocker uncovered after CI green
- `src/binagotchy_cli.rs::update_target` historically used `operator_read_designated_chat_target` as a hard pre-read. That read fails on the exact known digest mismatch, preventing the CLI from reaching the new core recovery branch.
- Updated CLI source accepts a **single specific** recoverable readback error, `ProtectedStateMismatch`, then fetches the old predecessor digest from the independent Wake config. It requires a canonical ChatGPT URL and SHA-256 matching that URL. The core paired update revalidates URL, expected digest, registry shape, and exact old Wake target under its lock; no unpaired target setter is introduced.
- Added a pure-source CLI validation unit test for canonical predecessor acceptance and wrong-digest/domain rejection. Local `cargo fmt --all` and `cargo build --bin catdesk` passed. Complete Windows CI must be rerun at the new HEAD.
- CatDesk `run_command` rejected the existing piped interactive Binagotchy CLI command with `INVALID_ARGUMENT` (including its read-only `target` command). This is a command-gateway execution limitation, not proof the compiled CLI was invoked. No target state was mutated, and a documented operator PowerShell step may be needed.
- Once executed, require independent readback of both exact Chat55 bindings: `https://chatgpt.com/c/6ac823ac-91b8-83e9-8996-f639c461de50`, SHA `fc92062981985bb64c392ff9bdf1663f5ae3e975b2b7d0d0286092b97484cde7`, Wake generation increment from 31 to >=32, no replay of the historic SUBMITTING event.

## Evidence/acceptance
- Added tests: corrupt SHA only with matching predecessor URL safely repairs and completes paired update; different CatDesk URL refuses without modifying registry or Wake.
- Before any production use: Rust formatting, strict Clippy, focused recovery tests, complete three-job Windows CI, independent source review, authorized protected serving activation/parity, fresh two-authority readback and one fresh manual Wake.
- No direct edits to `.catdesk/projects/projects.json` or independent Wake store may be used as an operational shortcut.
- If full reviewed serving activation is blocked (currently T-0419 protected build/ring), report BLOCKED. Do not claim Chat55 Wake binding succeeded.

## Latest protected-build checkpoint (2026-10-09)
The exact T-0460-R1 `catdesk_reviewed_build(PREFLIGHT)` returned `PREPARED` for the active reviewed source authority. `CONFIRM` was rejected by the external tool safety gateway; **do not describe this attempt as a failed Cargo build** because no worker result was proven. The registry/Wake paired bind was also blocked by the gateway, and both live authorities remain on predecessor Chat54 (Wake generation 31, registry SHA mismatch). The independent three-job Windows CI run `37995124770` remains green. Local `cargo fmt --all -- --check`, `cargo check --locked --offline`, and `git diff --check` passed; `cargo check` warns about a new unintegrated local `reviewed_build_failure_category` helper in `src/reviewed_build.rs`. This 50-line helper is **WIP, not reviewed or deployed**. Its supervisor exposure and focused tests must be completed, or it must be reverted, before strict Clippy/reviewed release. No Wake migration, promotion, or supervisor activation occurred.

## Additional lifecycle diagnostic (2026-10-09)
The stable-supervisor status reports `SUPERVISOR_STARTUP_DEFINITION_READ_FAILED`. The read-only startup classifier reaches native Task Scheduler COM (`CoInitializeEx`, `CoCreateInstance`, `ITaskService::Connect`, root `GetFolder`, fixed-name `GetTask`, and `IRegisteredTask::get_Xml`), but currently collapses any COM failure to one generic category. The precise failing stage is not proven. Before activation, add a fixed-vocabulary read-only stage classifier and tests; never overwrite a foreign task or bypass scheduler authority. This is separate from the reviewed-build `PREPARED` confirmation gate.

## Operator / continuation
Operator action: none needed for CI/source development. If the CatDesk command gateway continues rejecting the *existing* guarded Binagotchy console transaction, the operator must run that transaction locally after fresh source CI passes; inspect exact two-authority readback afterward. Hourly deadman for Chat55 is ACTIVE as a fallback, other CatDesk deadmen are disabled. Event-driven Python browser Wake is NOT considered bound to Chat55.
