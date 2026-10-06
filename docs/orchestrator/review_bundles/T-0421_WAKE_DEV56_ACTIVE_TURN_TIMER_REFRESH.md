# T-0421R1 — Wake dev.56 active-turn timer refresh independent review

## Scope

Independent direct review of the already-implemented Wake `1.0.0-dev.56` repair for the Chat33 T-0420R2 live observability defect. Installed Wake remains dev.55 during this review. This review does not authorize USER resubmission, target changes, tunnel changes, recovery, Git publication, or unrelated work.

## Defect and repair

The T-0420R2 natural Chat33 delivery proved exact generation-16 USER receipt and eventual `SENT / COMPLETE`, but while the assistant response was active the current event was absent from live `turnTimers`. Durable timer creation was already correct. The blocking `attempt()` / `reconcile_submitting()` loops serialized a stale in-memory `Status` instead of re-reading durable timers while they owned the browser.

dev.56 introduces `persist_live_status(store, status)`, which refreshes `updatedUtc` and re-reads durable `Store::timers()` immediately before `status.json` serialization. The reviewed source uses this helper at the outer heartbeat, pre-submit browser-session retry write, `reconcile_submitting()` loop, and `attempt()` loop. The deterministic Rust regression `active_status_persistence_refreshes_new_exact_receipt_timer` starts from a stale Status snapshot, persists an exact-receipt timer afterward, then proves the current timer is visible both in memory and in serialized `status.json`.

No reviewed dev.56 production change alters USER submission, exact receipt binding, assistant-response Retry behavior, the 3-retry / 35-minute-generation / 90-minute-total limits, target generation/digest checks, sequence/ambiguity fail-closed behavior, passive timer authority, or external Secure MCP ownership.

## Python regression classification

The fixed-runtime Python suite initially failed only `WakeBridgeTests.test_response_completion_waits_for_generation_to_finish_before_success` with:

`AttributeError: 'object' object has no attribute 'get_current_url'`

The production response observer has required `cdp.get_current_url()` for exact-target validation since the prior response-completion hardening. The failing normal-completion fixture still passed a bare `object()`, while the retry-path fixtures already supplied a minimal `Cdp.get_current_url()` implementation. This is a stale test-fixture defect, not a dev.56 runtime failure.

The bounded fixture repair supplies the same exact-target `Cdp` shape and passes that object to `wait_for_response_completion()`. No production Python behavior changed.

## Verification

- Focused fixed-runtime regression:
  - `cargo test --manifest-path wake/Cargo.toml --test python_bridge_validation adapter_and_response_python_regression_suite -- --nocapture`
  - PASS: 1 passed, 0 failed.
- Full Wake all-target/all-feature test:
  - `cargo test --manifest-path wake/Cargo.toml --all-targets --all-features`
  - PASS.
  - Library: 28/28.
  - Process tree: 3/3.
  - Protocol/store: 24/24.
  - Python validation: 3 passed, 1 intentionally ignored manual harness.
  - Historical diagnostic examples emit pre-existing warnings but no test failures.
- Strict production Wake library Clippy:
  - `cargo clippy --manifest-path wake/Cargo.toml --lib --all-features -- -D warnings`
  - PASS.
- Scoped whitespace/diff gate:
  - `git diff --check -- wake/src/runtime.rs wake/Cargo.toml wake/Cargo.lock Cargo.lock tests/test_wake_bridge.py`
  - PASS; only existing line-ending notice for root Cargo.lock.

## Review conclusion

The dev.56 active-turn status refresh is bounded to observability and closes the live stale-timer snapshot mechanism identified by T-0420R2. The only failing Python result was independently classified and repaired as a stale test fixture. The reviewed source is ready for immutable Wake package installation and one fresh natural Chat33 acceptance canary.

Acceptance after installation must require all of the following for the same fresh event:

1. Normal independent WakeHost discovery on exact Chat33 generation 16.
2. One real USER wake with `EXACT_USER_MESSAGE_APPENDED`; no duplicate USER submission.
3. The current event visible in live `turnTimers` while response observation is active.
4. Normal `RESPONSE_COMPLETED`, or if the platform timeout occurs, `RESPONSE_TIMEOUT_DETECTED -> RESPONSE_RETRYING -> RESPONSE_RETRY_STARTED` with Retry consumed in place before any reload.
5. Final delivery `SENT` and the same timer frozen `COMPLETE`.

Ordinary non-Wake autonomy remains frozen until that live acceptance completes.
