# T-0422 — Wake dev.58 post-submit pause receipt-drain repair

## Live defect

During the dev.57 serialized-queue soak, canonical Chat33 visibly received:

`MANUAL WAKE DEBUG — CatDesk diagnostic test-event manual-wake-dev57-queued-sequential-003 — NOT natural acceptance.`

However durable Wake state remained `Submitting` with reason `HOST_PAUSED_BEFORE_SUBMIT`, no `EXACT_USER_MESSAGE_APPENDED` receipt, and no turn timer. A separate queued event `manual-wake-dev57-queue-sequencing-003` remained `Submitting / SUBMIT_CLEARED_NO_APPEND`.

A read-only Store probe proved the contradiction. The visible USER append crossed the browser write boundary, but Rust processed a PAUSED control value before draining the adapter's pending `USER_MESSAGE_APPENDED` stage.

## Root cause

`attempt()` transitioned the delivery to `Submitting` and sent `{"submit":true}` to the browser adapter. On the next loop iteration it unconditionally evaluated `desired(store) != RUNNING` before reading adapter output. A pause/stop arriving after dispatch but before Rust consumed the receipt therefore returned `HOST_PAUSED_BEFORE_SUBMIT`, even though the browser could already have appended the USER message.

## dev.58 repair

dev.58 tracks two local boundaries:

- `submit_dispatched`
- `receipt_recorded`

Control semantics are now:

1. Before submit dispatch, PAUSED/STOPPED returns `HOST_PAUSED_BEFORE_SUBMIT` exactly as before.
2. After submit dispatch but before a durable exact receipt, PAUSED/STOPPED does **not** abort. The bounded adapter attempt continues until it yields the exact receipt or a post-submit error/timeout. This prevents loss of exactly-once evidence.
3. Once the receipt is durably recorded, a pending PAUSED/STOPPED control may stop response observation as `HOST_PAUSED_AFTER_RECEIPT`; the timer is marked Attention and the `Submitting` event remains quarantined for normal reconciliation after resume.
4. RUNNING behavior, USER submission semantics, target authority, browser retry bounds, response Retry behavior, and receipt validation are unchanged.

This deliberately does not make package STOP/PAUSE drain the entire assistant-response window; reviewed package handoff retains its bounded stop behavior.

## Verification

Focused boundary regressions all pass:

- `pause_before_submit_aborts_without_crossing_write_boundary`
- `pause_after_submit_dispatch_waits_for_exact_receipt_or_error`
- `pause_after_exact_receipt_can_stop_observer_without_losing_evidence`
- `running_never_requests_pause_abort`

Full Wake verification log shows:

- library: 33 passed / 0 failed
- manual startup diagnostic: 1 intentionally ignored
- process tree: 3 passed / 0 failed
- protocol/store: 24 passed / 0 failed
- Python validation: 3 passed / 0 failed / 1 intentional manual ignore
- strict Wake library Clippy with `-D warnings`: PASS
- scoped `git diff --check`: PASS
- `cargo check`: PASS after removing an orphaned unused `BTreeSet` import

Historical diagnostic examples still emit their pre-existing warnings during all-target compilation; they do not fail verification.

## Live migration rule

The two dev.57 generation-16 `Submitting` records are evidence and must never be replayed as USER submissions:

- `manual-wake-dev57-queue-sequencing-003` — `SUBMIT_CLEARED_NO_APPEND`
- `manual-wake-dev57-queued-sequential-003` — `HOST_PAUSED_BEFORE_SUBMIT`, but the USER message is visibly present in Chat33

Install dev.58 while Wake remains paused. After dev.58 owns the installed path, resume/start only the installed host and allow its existing reconciliation path to inspect those immutable `Submitting` records. A fresh diagnostic may be created only after reconciliation state is understood.
