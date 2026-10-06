# T-0060F-W6 Trusted Click Diagnostics Review Bundle

## Remaining live evidence

The fresh W5 review entry again records `SUBMITTING` with
`SUBMIT_RECEIPT_UNPROVEN` and null timestamp/message/target receipt fields.
This preserves the required post-boundary fail-closed state but proves that W5
did not establish a receipt. It does not prove the click submitted anything:
the former bridge emitted one undifferentiated attention code and did not retain
safe stage evidence.

The installed project-local SeleniumBase source documents that `cdp.click()`
finds the selected element and first invokes its simulated mouse-click path.
This is the narrow browser-automation primitive available without inspecting
the active browser, profile, storage, or conversation.

## Repair

- The bridge no longer invokes `HTMLElement.click()` in page script.
- Before the irreversible boundary, a fixed DOM evaluation accepts exactly one
  visible, enabled send control from the existing fixed selector set. It rejects
  no match, duplicates, disabled controls, and any pre-existing CatDesk marker.
- Only that element receives the temporary `data-catdesk-wake-send='1'`
  marker. After the boundary, SeleniumBase `cdp.click()` receives the fixed
  unique marker selector, then the marker is removed in `finally`. Click or
  cleanup ambiguity remains post-boundary attention and is never retried.
- The durable `attention` field receives only these fixed non-secret stage
  codes:

  - `SUBMIT_COMPOSER_RETAINED`
  - `SUBMIT_COMPOSER_CHANGED`
  - `SUBMIT_CLEARED_NO_APPEND`
  - `SUBMIT_APPEND_DIGEST_MISMATCH`
  - `SUBMIT_RECEIPT_SEQUENCE_DRIFT`
  - `SUBMIT_RECEIPT_QUERY_FAILED`
  - `SUBMIT_CLICK_UNKNOWN`, `SUBMIT_MARKER_CLEANUP_UNKNOWN`, or the existing
    `SUBMIT_RECEIPT_UNPROVEN` fallback.

No wake text, message digest details, DOM content, selectors beyond fixed source
constants, URLs, credentials, or browser state are written as diagnostics.

## Receipt integrity and coverage

The exact schema-4 receipt gate is unchanged: only one exact appended normalized
wake-message digest after the unchanged prior author-message sequence can set
`SENT`. Tests cover unique-marker automation click and cleanup, marker/control
ambiguity, click/cleanup uncertainty, current content-node receipt extraction,
all requested append-stage classifications, and a full simulated CDP valid
receipt path. Existing tests retain typed-unsent, no-message, duplicate,
post-submit crash, and no-resubmit coverage.

## Verification

- `cargo fmt -- --check`: passed.
- `cargo clippy --all-targets --all-features -- -D warnings`: passed.
- `cargo test`: 476 passed, 18 ignored.
- The project-local wake Python test command was attempted but cannot start:
  its venv executable references a missing Python 3.12 base interpreter. It was
  not repaired, replaced, or bypassed.
- `git diff --check`: passed.

## Residual acceptance

After independent review and candidate loading, let only the ordinary CatDesk
automatic dispatcher create one fresh read-only canary for the already exact
configured conversation. A complete schema-4 receipt is required for success.
Any enum attention result requires operator review and must not trigger a manual
or blind retry.

No live wake, browser action, daemon reload/promotion, tunnel or Scheduler
action, credential/browser-storage inspection, or Git publication was performed.
