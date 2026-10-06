# T-0424 natural Wake observation — 2026-09-26 UTC

## Formal task closure

T-0424 is `COMPLETED_VERIFIED`, inactive, stateVersion 14. Formal verification is `PASSED`. Its completion event is `review-adc-t0424-shared-turn-timer-20260925-13-independent_final_review`; it was generated and discovered naturally. No manual/debug event or browser sender was used. The finalization call timed out with HTTP 504 after dispatch, but durable completion and subsequent authoritative live readback confirmed success; finalization was not retried.

## Natural acceptance result: FAIL, preserve without replay

Installed immutable Wake dev.67, PID 32368, automatically claimed the event against Chat34 generation 17 and digest `8766b100ea0ba654bc4c8531d99c84e34346a231be1ffa0b4deae30df552427e`.

- Event created: Unix 1790387792.
- First attempt: Unix 1790387797.
- Readiness: SAME_CONVERSATION / EDITOR_SELECTOR at 1790387802, then SAME_CONVERSATION / READY at 1790387815.
- Observed stages: CHAT_IDLE, PREWRITE_CONFIRMED, SUBMITTING, then observe-only reconciliation PAGE_READY.
- Exact event-status readback at 02:05 UTC: SUBMITTING, reason SUBMIT_TARGET_DRIFT, updatedUtc 1790388095, receipt null, timer null.
- Event message digest: `405effa18b94fa7a998891d7ea0ee30d985f6723ade1a11a8d66aa7caaa5cc8a`.
- Actionable queue is zero because ambiguous SUBMITTING evidence is quarantined; zero is not delivery success.
- The last receipt remains manual diagnostic 010. It must not be attributed to this natural event.
- Event was not acknowledged, retired, rearmed, or replayed.

Production evidence was read through serving CatDesk, avoiding the Codex MSIX shadow AppData tree. Logs: `.catdesk/logs/1790388334-accc5d89-f630-40be-9395-bccd8cbfe40f.log` (exact event status) and `.catdesk/logs/1790388424-a51dec73-0e3a-4d8e-9719-eed75dad9a71.log` (readiness history). A local absolute AppData read returned the known stopped dev.59 shadow PID54892 and was rejected as production evidence; actual production Wake remains PID32368.

## Chrome correlation

Existing authenticated Chrome extension profile, canonical Chat34 tab 1383641281. Initial rendered page had the editor, voice control, completed response controls, and no active Stop/Pause/Retry. The last USER message requested the Codex handoff. During the attempt the Chrome observation page was refreshed once; immediately after refresh it briefly had no editor/messages, then hydrated back to exact Chat34 with editor and the previous completed handoff. No new USER wake was visible. No credentials, cookies, tokens, unrelated browsing data, or production Wake browser profile were inspected. Chrome was never a sender. The debug-tab refresh is separate from Wake's production behavior.

## Concrete sequence requiring repair review

`wake/adapter.py::attempt` and `scripts/wake_bridge.py::CdpSink.wake` call `smoke_path_submit`, then immediately `durable_receipt_round_trip`. That method reloads the sender document before observing server-persisted receipt. Its tests explicitly require this sequence. This conflicts with the current no-reload-after-first-write requirement and introduces an avoidable chance of aborting a pending submission. It is a plausible contributor to this failure, not a proven explanation of the exact browser/network timing. The current diagnostics preserve only the terminal target-drift reason, so the initial failure's precise cause cannot be reconstructed.

Proposed bounded repair for review: preserve the original sender document, obtain fresh-document receipt proof in one owned same-profile exact-target observation tab, then close only that tab and restore the sender for response observation. No new browser profile, alternate USER sender, arbitrary navigation, receipt weakening, or replay. Verify failure cleanup, exact tab identity, missing/duplicate/delayed receipt, target drift, timeout, and restore failures. If the API cannot establish an independently owned new tab, fail closed. Public SeleniumBase v4.51.5 supports get_active_tab/open_new_tab/switch_to_tab/close_active_tab; implementation semantics were inspected in the pinned upstream source.

T-0424's frozen review bundle remains unchanged. Further Wake repair and its review authority must be separate. Timer production exposure/build promotion and live recovery remain open. OPERATOR ACTION: NONE. Continuation: direct Codex/CatDesk bug fixing; natural Wake acceptance remains failed/open.
