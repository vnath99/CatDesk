# T-0419 R4 — Read-only supervisor Task Scheduler COM-stage diagnostics

Date: 2026-10-09
Scope: source-only diagnostic enhancement; no installation, startup registration, daemon reload, WakeHost mutation, or protected-build promotion.

## Live observation and bounded diagnosis

The installed stable supervisor status currently reports:
- `startupDefinition=SUPERVISOR_STARTUP_DEFINITION_READ_FAILED`
- `startupPolicy=SUPERVISOR_STARTUP_POLICY_UNPROVEN`
- `activationBlocked=SUPERVISOR_STARTUP_POLICY_UNPROVEN`

These existing categories collapse every native COM failure into one status. The exact failing API call is **not established**. No evidence supports treating an absent, foreign, or malformed scheduler task as owned.

## Source change

`src/windows_supervisor_startup.rs` introduces fixed, non-sensitive read-only failure categories for COM initialization (including the fixed `RPC_E_CHANGED_MODE` apartment-conflict category), Task Scheduler activation, COM Connect, root-folder read, fixed-task lookup, and task XML retrieval. Access-denied HRESULTs retain a distinct elevation-required category. No raw HRESULT, XML, SID, task action path, credential, or caller-selected scheduler identity is returned.

`src/supervisor_lifecycle.rs` exposes only the fixed category through the existing read-only `startupDefinition` status field. The activation preflight remains fail-closed and continues to return `SUPERVISOR_STARTUP_POLICY_UNPROVEN` when ownership cannot be proven.

The `reviewed_build_failure_category` helper is separate and not yet wired into the large supervisor dispatch file; its integration must be reviewed independently. It is not live and must not be counted as deployed diagnostics.

## Verification on local development source

- `cargo fmt --all -- --check`: PASS.
- `cargo clippy --locked --offline --all-targets --all-features -- -D warnings`: PASS.
- `cargo test --locked --offline windows_supervisor_startup`: PASS, 11/11 focused tests.
- `cargo test --locked --offline supervisor_lifecycle`: PASS, 15/15 focused tests.
- `git diff --check`: PASS (line-ending warnings only).

## Remaining gates

This source change is not a serving-image repair. It must be committed, independently reviewed, tested in Windows CI, built under the protected reviewed-build authority, promoted and verified as the serving image before the new fixed failure stage can be observed on the user's machine.

The T-0460 protected build `CONFIRM` call was rejected by the command safety gateway. The old independent WakeHost remains on generation 31 and the prior Chat54 URL. Do not claim the new Chat55 target is bound until both authorities independently attest the exact new URL and digest.
