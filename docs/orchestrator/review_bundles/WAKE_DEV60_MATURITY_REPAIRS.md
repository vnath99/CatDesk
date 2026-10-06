# Wake dev.60 — bounded recovery and truthful diagnostics

## Problem and resulting behavior

The maturity audit found three Python boundary defects: blank shells consumed two reloads, an absent response composer could bypass the 35-minute generation limit, and receipt polling busy-spun after its Stop hold expired. dev.60 allows one blank-shell reload, enforces the generation limit while the composer is missing, and retains positive polling delays.

The dev.59 installed canonical diagnostic `manual-wake-dev59-canonical-readiness-002` exhausted pre-write recovery as ATTENTION/TARGET_DRIFT, with no receipt or timer. Old quarantined SUBMITTING events then overwrote its displayed status. dev.60 retains each historical record but reconstructs the last terminal attempt's state from its own durable delivery, and places active response timers ahead of forensic/completed timers.

Rust's two readiness handlers had divergent vocabularies and rejected Python's BLANK_SHELL_RECOVERY. Both now use one bounded validator accepting loading/blank-shell transitions and rejecting arbitrary content.

Codex MSIX AppData virtualization caused a local CLI invocation to resolve a historical shadow store. No USER submission occurred: its event was paused before submit and host stopped. Manual CLI publication now requires expected generation and digest, checked inside the same Store transaction as publication; rollover and conflicting old event IDs fail closed. Production control must run through serving CatDesk, never Codex-local AppData.

## Interfaces

- `test-event <id> <expected-generation> <expected-digest>` replaces the unbound ID-only form.
- `event-status <id>` returns validated durable delivery and timer state without modifying either.
- No target change, Store edit, history deletion, credential access, or production profile attachment is introduced.
- Existing SUBMITTING/SENT anti-replay, receipt binding, retry maximums, and immutable package activation remain in place.

## Validation

- Full Python bridge suite: 93 passed, 306.821 seconds.
- Final dev.60 scoped suite: 35 library + 26 protocol/store + 3 process-tree + 3 Python harness + 1 CLI integration passed; 1 intentional manual harness ignore.
- Strict Wake library Clippy `-D warnings`: passed.
- Added tests cover single blank-shell reload, transient editor readiness reset, missing-editor deadline, post-Stop polling, readiness protocol vocabulary, current-event status, expected target mismatch/idempotency, concurrent rollover, and CLI refusal before queue/start.
- Whole-crate formatting includes pre-existing diagnostic-example debt. No broad formatting cleanup was performed.

## Installation and live acceptance

Candidate source is ready for the existing immutable installer in the serving CatDesk environment. The current dev.59 canonical diagnostic is terminal pre-submit ATTENTION with no receipt, so there is no active response to interrupt. Preserve Chat33 generation 16 and existing forensic records across activation. Retry only this receiptless canonical diagnostic via the supported retry-pre-submit operation after package/target readback. Installation is not maturity acceptance; three consecutive live passes, network recovery, timeout Retry-in-place, restart/recovery and cleanup evidence remain open.
