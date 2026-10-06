# T-0424 — Wake dev.62 backward-compatible readiness diagnostics

## Scope

dev.62 resolves the dev.61 status-schema compatibility hazard while preserving Codex's useful readiness diagnostics. Installed dev.60 remains the production baseline until this review/install step completes.

## Problem

dev.61 added `readinessHistory` directly to the serialized Wake `Status` struct. Wake Status uses `deny_unknown_fields`, and CatDesk transport parses that exact type from `status.json`. A serving CatDesk binary built before the additive field could therefore reject a newer Wake status as `INDEPENDENT_WAKE_STATUS_INVALID_JSON`, the same compatibility class previously observed when `turnTimers` was introduced.

## Repair

- Bumped Wake source to `1.0.0-dev.62`.
- Removed readiness history from shared `status.json`.
- Added separate bounded/versioned `readiness-history.json` diagnostics.
- History entries use the same fixed validated route/reason vocabulary as the adapter protocol.
- Consecutive duplicates are suppressed.
- History is capped at 64 entries.
- History is observability-only and cannot alter target, delivery, receipt, timer, retry, or submission authority.
- Added read-only CLI command `readiness-history`.
- Existing generation/digest-bound `test-event` behavior remains unchanged.
- No production CatDesk parser change or daemon reload is required for this Wake package.

## Verification

- `cargo test --locked --offline --manifest-path wake/Cargo.toml --features test-support`: PASS.
  - library 36/36
  - manual CLI integration 1/1
  - process tree 3/3
  - protocol/store 26/26
  - focused Python validation 3 passed + 1 intentional manual harness ignore
- New regression `readiness_history_is_bounded_validated_and_separate_from_status_contract` proves:
  - bounded 64-entry history;
  - fixed-vocabulary validation;
  - duplicate suppression;
  - event binding;
  - `status.json` contains no `readinessHistory` field;
  - delivery/timer authority remains untouched.
- `cargo clippy --locked --offline --manifest-path wake/Cargo.toml --all-features --lib -- -D warnings`: PASS.
- Scoped `git diff --check`: PASS; only existing LF/CRLF notice for `scripts/wake_bridge.py`.
- Read-only current-source CLI probe `readiness-history` against the production Store returned `[]` without mutation.
- Current-source `status` readback successfully parsed the installed dev.60 status and serialized the backward-compatible status shape.

## Installation / live acceptance

The current installed dev.60 host has no active USER submission for the latest diagnostic: `manual-wake-dev59-canonical-readiness-002` is terminal pre-submit `ATTENTION / TARGET_DRIFT`, receipt null, timer null. Historical ambiguous records remain preserved.

Activate dev.62 only through the immutable reviewed Wake installer. Preserve canonical Chat33 generation 16 / digest `d32961ec350c42d039f5514e9171a59574674ecc291b82c0c28f4d9c14336212`.

After install:
1. confirm CatDesk transport still parses Wake status;
2. confirm exact generation-16 authority unchanged;
3. run exactly one fresh generation/digest-bound MANUAL diagnostic;
4. inspect separate readiness-history diagnostics;
5. require no duplicate USER wake and terminal exact receipt/SENT/COMPLETE on success, or truthful receiptless ATTENTION on pre-write failure.

Wake maturity remains open after this package; repeated passes, timeout Retry, restart, network/recovery, queue/backlog truthfulness, and bounded MCP Binagotchy control remain outstanding.
