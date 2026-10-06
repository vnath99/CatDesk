# Wake maturity investigation — 2026-09-24

Status: IN PROGRESS; not accepted as mature.

## Verified source changes

- Blank-shell recovery is limited to one reload, followed by a bounded observation window.
- Missing response editors obey the 35-minute generation deadline.
- Receipt polling continues sleeping after the Stop hold expires instead of busy-spinning.
- Updated two stale full-suite fixtures and added deterministic regressions for these failures and transient editor readiness.
- Full Python bridge suite: 93 passed (306.821 seconds). Final combined Wake verification: 33 library, 26 protocol/store, 3 process-tree, 3 Python harness, and 1 CLI integration test passed; 1 manual harness ignored. Strict Wake library Clippy passed. Whole-crate formatting check encounters pre-existing formatting debt in diagnostic examples; touched Rust files were scoped-formatted.
- Manual CLI now requires `test-event <id> <expected-generation> <expected-digest>`. Store checks expected authority and publication in one transaction, with mismatch, idempotency, and concurrent rollover regression coverage.

## Observation browser

Per the operator's explicit instruction, use the existing authenticated Chrome profile through the Codex Chrome extension, exclusively for observation. Canonical conversation: https://chatgpt.com/c/6ab465a8-63a8-83ea-adfc-758e368769e2 . No built-in browser and no production Wake profile attachment.

Chrome inspection confirmed an available composer and completed assistant controls with data-testid=copy-turn-action-button. This verifies selector presence only, not completion correctness during tool pauses.

## Critical live boundary discrepancy

Connector transport status reports Wake dev.59 PID 16392, generation 16, digest d32961ec350c42d039f5514e9171a59574674ecc291b82c0c28f4d9c14336212, three historical SUBMITTING records.

The local installed CLI resolved a generation-1 config dated September 13 under the same apparent LOCALAPPDATA CatDeskWake root. The single `manual-wake-dev59-readiness-001` test-event invocation started PID 54892 against that old target. It was paused before submission, durably ATTENTION / HOST_PAUSED_BEFORE_SUBMIT, receipt null, then stopped through the supported CLI. Do not replay this event or count it as canonical acceptance. The canonical connector still reports PID 16392 and generation 16 unchanged.

Cause established by open-handle file identity plus `fsutil file queryfilenamebyid`: the local path resolves to Codex MSIX package storage at `AppData/Local/Packages/OpenAI.Codex_2p2nqsd0c76g0/LocalCache/Local/CatDeskWake`, while production PID 16392 holds the real AppData lock. Never operate production Wake through Codex-local AppData paths. The shadow diagnostic is stopped and must remain forensic only.

Supported control restored through the serving CatDesk environment: `run_command` permits Cargo, so invoke the existing bounded WakeHost CLI using `cargo run --locked --offline --manifest-path wake/Cargo.toml --bin CatDeskWakeHost -- ...`. Read-only `status` proved PID 16392 and generation 16 before publication. This is the product CLI, not direct Store mutation or browser sending. `catdesk_binagotchy_command` is still absent from this connector catalog; full catalog parity remains open.

At Unix 1790277560, one fresh `manual-wake-dev59-canonical-readiness-002` diagnostic was published through that CLI with explicit generation 16 and exact target digest. Installed immutable dev.59 remains the sender. Initial observation is pre-write CLAIMED / READINESS_HOME_TARGET_DRIFT; no receipt yet. Do not queue another event until this one settles.

## Remaining maturity gates

Three consecutive canonical installed-path passes, exact receipt and live timer correlation, network retry, timeout Retry-in-place, restart/recovery, production browser cleanup, catalog parity, and historical/current status separation remain OPEN. No natural acceptance has been claimed in this run.
