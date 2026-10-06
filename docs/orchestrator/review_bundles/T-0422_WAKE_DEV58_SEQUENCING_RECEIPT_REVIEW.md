# T-0422 — Wake dev.58 sequencing and receipt-reconciliation independent review

## Scope

Independent review of the combined dev.58 repair produced from the canonical Chat33 sequential-soak failures. Installed Wake remains dev.57 and intentionally PAUSED during this review. This review does not authorize target mutation, external Secure MCP changes, recovery/LKG changes, Git publication, or replay of any quarantined dev.57 event.

## Live failures that motivated dev.58

Two distinct defects were exposed while exercising installed dev.57:

1. `manual-wake-dev57-restart-repeatability-002` delivered successfully after a WakeHost restart, but its assistant response contained tool-use pauses. dev.57 could treat an intermediate pause as terminal merely because assistant DOM existed and the editor/Stop surface looked idle. That allowed a subsequent queued diagnostic to begin while the prior assistant turn was still executing.
2. `manual-wake-dev57-queue-sequencing-003` became visibly present in canonical Chat33, but Wake retained the delivery as `SUBMITTING / SUBMIT_CLEARED_NO_APPEND` with the previous event still in `lastReceipt`. Source inspection showed a SUBMITTING record received only one receipt-reconciliation attempt per WakeHost process, after which the exact record was quarantined forever for that process even when the durable USER message later became observable.
3. A separate diagnostic `manual-wake-dev57-queued-sequential-003` was interrupted at the old pause/submit boundary before a visible USER append. It also remains quarantined SUBMITTING. Both records are evidence and must never be replayed or downgraded.

## Reviewed dev.58 changes

### 1. Response completion is terminal-turn based

`scripts/wake_bridge.py::response_snapshot` now returns four bounded signals: timeout count, associated Retry count, whether an assistant turn exists after the latest expected USER turn, and whether the latest assistant turn exposes the visible `button[data-testid='copy-turn-action-button']` terminal action.

`wait_for_response_completion` emits `RESPONSE_COMPLETED` only when both a post-USER assistant and that terminal action are present. Merely seeing assistant DOM during a tool-call pause is insufficient. Timeout-card handling remains exact-text scoped to the latest expected USER turn and still consumes only the assistant-response Retry control in place.

Regression coverage explicitly exercises `assistant present / completion action absent` followed by `completion action present` before success.

### 2. Pause cannot abandon an already-dispatched submit

`submission_control_reason` distinguishes three boundaries:

- Pause/Stop before Rust dispatches `{"submit":true}` => `HOST_PAUSED_BEFORE_SUBMIT`; no USER submission crosses the boundary.
- Pause/Stop after submit dispatch but before exact receipt => the bounded adapter transaction continues draining until exact receipt or fixed-vocabulary error. This prevents creating an ambiguous delivery merely because the operator/host paused after authorization.
- Pause/Stop after exact receipt => response observation may stop with `HOST_PAUSED_AFTER_RECEIPT`; the exact USER receipt remains durable and cannot be replayed.

Unit tests cover all three states.

### 3. SUBMITTING reconciliation is receipt-only, bounded, and delayed

The reconciliation adapter is structurally observe-only. `reconcile_submission` opens the exact stored target, performs the durable document round-trip, calls `reconcile_persisted_receipt`, and then observes the assistant response. It contains no editor typing, Enter press, USER send-button click, or USER submit authorization path.

Rust now permits transient receipt-proof failures to receive bounded later read-only reconciliation:

- maximum four reconciliation attempts per WakeHost process;
- retry delays: 0s, 30s, 120s, 300s;
- global event-age retry window: 30 minutes, so restarts cannot cause indefinite reconciliation churn;
- delayed retries are limited to transient/ambiguous receipt-proof reasons such as `SUBMIT_CLEARED_NO_APPEND`, receipt query/round-trip/unproven failures, ambiguous submit-control outcomes, bounded reconciliation timeout, browser network failure, or adapter exit;
- target drift, sequence drift, digest mismatch and other contradictory authority signals are not retryable.

The Store boundary remains unchanged: `reconcile_submitting_sent` only allows `SUBMITTING -> SENT` when an exact bound `EXACT_USER_MESSAGE_APPENDED` receipt matches event ID, target generation/digest and message digest. `claim()` refuses an existing SUBMITTING delivery, and `retry_pre_submit()` refuses SUBMITTING. Therefore dev.58 cannot type or submit either quarantined dev.57 event again.

Regression `submitting_reconciliation_retries_are_bounded_delayed_and_never_replay` proves delay, attempt budget, global age expiry, refusal of contradictory target drift, and Store-level replay/downgrade refusal.

### 4. Telemetry

Historical stale records remain durable forensic evidence. Binagotchy source labels this surface as historical/forensic rather than current queue health; no evidence is deleted to improve appearance.

## Verification

Final combined-source verification after all dev.58 changes:

- `cargo test --manifest-path wake/Cargo.toml --features test-support` — PASS.
  - library: 33/33;
  - manual startup diagnostic: 1 intentionally ignored;
  - process tree: 3/3;
  - protocol/store: 24/24;
  - Python validation: 3 passed, 1 intentional manual harness ignore.
- `cargo clippy --manifest-path wake/Cargo.toml --lib --features test-support -- -D warnings` — PASS.
- scoped `git diff --check` over Wake/runtime/bridge/test/Binagotchy surfaces — PASS; only existing LF/CRLF working-copy notices.
- Historical diagnostic examples still emit pre-existing warnings; they do not fail the test suite and are outside this bounded repair.

## Independent conclusion

The combined dev.58 repair is coherent and preserves the exactly-once USER boundary. It closes the observed false-completion race, closes the pause-after-dispatch ambiguity, and improves recovery of a legitimately appended but temporarily unprovable USER message without introducing a USER replay path. Receipt reconciliation remains exact-target/exact-digest bound, bounded by attempts and event age, and fail-closed on authority contradictions.

The source is suitable for reviewed immutable Wake packaging and live acceptance.

## Live acceptance after reviewed install

1. Keep both dev.57 sequential SUBMITTING records immutable; never call pre-submit retry or construct a replay.
2. On dev.58 startup, allow only its receipt-read reconciliation logic to inspect those records. If the visibly delivered `manual-wake-dev57-queue-sequencing-003` can be proven, it may advance to SENT from exact receipt only. The non-visible `manual-wake-dev57-queued-sequential-003` must remain quarantined if no exact receipt exists.
3. Create a fresh uniquely named diagnostic only after dev.58 is active.
4. Require one exact generation-16 USER append, live OBSERVING timer, no early COMPLETE during tool-use pauses, no second event claim while the assistant turn is active, terminal SENT/CLOSED_AFTER_SUCCESS/COMPLETE, and no duplicate USER message.
5. Continue later maturity work with network/browser retry, timeout Retry-in-place, profile/restart soak, and reboot/recovery once this sequencing repair is accepted.

Operator action is not required for this review/install path.
