# T-0060F-W8 Swallowed Exception / Receipt Shape Review Bundle

## Root cause

The R3 wake artifact at `latest_logs/wake_bridge.line_392/basic_test_info.txt`
is timestamped 2026-08-13 15:36:21 EDT, matching the R3 delivery window. It
records `Attention("USER_MESSAGE_RECEIPT")` from the initial fixed
author-role receipt query at `CdpSink.wake()` before typing and before
`before_submit()`.

Two independent defects then made that pre-submit failure appear to be a
post-submit ambiguity:

1. Production constructed SeleniumBase as `SB(uc=True, test=True, ...)`.
   The installed SeleniumBase 4.51.5 `SB` context-manager source catches an
   exception from its body and returns when `test=True`; therefore a raised
   `Attention` could cause `wake()` to return `None`.
2. `main()` classified every returned malformed receipt as post-submit without
   knowing whether `before_submit()` had executed. The swallowed `None` was
   consequently persisted as `SUBMITTING` with
   `SUBMIT_RECEIPT_UNPROVEN`, even though no type or submit occurred.

The initial receipt probe also supplied an uninvoked JavaScript arrow function
to `cdp.evaluate`. Native SeleniumBase CDP evaluation returns the evaluated
value, not an implicit invocation of that function, so the resulting object
could not satisfy the required bounded string-list shape.

## Repair

Changed paths:

- `scripts/wake_bridge.py`
- `tests/test_wake_bridge.py`
- this review bundle

The production sink now uses ordinary `SB(uc=True, user_data_dir=...)` rather
than SeleniumBase test mode, so `Attention` and `PostSubmitUnknown` propagate
to the bridge boundary. The author-role query is an invoked IIFE and its
result goes through a small normalization helper that accepts only a direct
bounded `list[str]` or known bounded Runtime.evaluate result/value envelopes.
Unexpected dictionaries, scalar values, malformed envelopes, oversized lists,
and oversized strings fail closed. The helper immediately hashes accepted
values; it neither persists nor emits message text.

`main()` now records `submission_started` only after its `before_submit()`
callback durably writes `SUBMITTING`. That callback is the sole transition
over the submission boundary:

- a missing/malformed result before it is `OPERATOR_ATTENTION` with
  `PRE_SUBMIT_NO_RECEIPT`, and is not reclassified as `SUBMITTING`;
- another unexpected pre-submit exception is likewise redacted into
  `PRE_SUBMIT_UNEXPECTED` operator attention;
- a `PostSubmitUnknown`, malformed result, or receipt-query failure after it
  remains `SUBMITTING` and blocks every blind retry;
- a complete exact append is still the only path to `SENT` with the state
  schema-4, exact hash-bearing receipt fields.

## Deterministic regression coverage

The wake bridge tests cover:

- direct, nested, and raw Runtime.evaluate value envelopes for bounded message
  lists, plus malformed/non-list/oversized rejection;
- the invoked author-role query form;
- a synthetic SeleniumBase context that would suppress an exception only in
  test mode, proving the production sink omits `test=True` and the pre-submit
  `Attention` propagates;
- missing receipts and nominally post-submit errors before the callback staying
  `OPERATOR_ATTENTION`;
- unexpected pre-submit and post-submit exceptions retaining their respective
  pre-submit/operator-attention and post-submit/submitting classifications;
- post-boundary malformed receipts and receipt-query failures remaining
  `SUBMITTING`, with no second submission attempt;
- direct CDP click, constrained native-Enter fallback, exact append receipt,
  and the existing fixed conversation/profile safeguards.

## Verification

- `cargo fmt -- --check`: passed.
- `cargo clippy --all-targets --all-features -- -D warnings`: passed.
- `cargo test`: 476 passed, 18 ignored.
- Project-local Python wake tests were attempted with
  `.catdesk/wake-bridge/venv/Scripts/python.exe`, but that pre-existing venv
  references a missing Python 3.12 base interpreter. It was not repaired,
  replaced, or bypassed.
- `git diff --check`: pending final review after this bundle.

No live browser, bridge, wake, Secure MCP tunnel, Scheduler, daemon/release,
protected wake target, browser profile storage, credential, or Git remote was
accessed or changed by this task.

## Exact host-only A/B acceptance after independent review

These are post-review CatDesk-host/operator steps, not worker actions. Use the
already configured exact conversation and dedicated profile; do not expose
their values in logs or review artifacts.

1. Confirm the reviewed project-local bridge hash is the one bound by the
   Rust host and that the target review record is fresh, unread, actionable,
   and has no existing delivery state.
2. **Control:** run the existing standalone
   `scripts/wake_browser_smoke_test.py` once against that already configured
   exact target/profile using a unique non-sensitive canary marker. Require
   its `SMOKE_TEST_SENT` outcome and an empty composer confirmation. This
   control is intentionally not a CatDesk dispatcher invocation.
3. Record only the control exit/status and timestamp. Do not retain page
   content, conversation text, cookies, local storage, or profile files.
4. **Treatment:** create one fresh actionable review record and allow only the
   normal CatDesk automatic dispatcher to launch the reviewed production
   bridge. Do not call the bridge directly and do not retry it manually.
5. Require treatment success to be `WOKE` plus a complete exact `SENT` state:
   schema version 4, positive browser timestamp, exact message and target
   hashes, and the Rust host's exact receipt validation. Confirm the record is
   bound to the configured conversation only.
6. If the pre-submit probe fails, require `OPERATOR_ATTENTION` rather than
   `SUBMITTING`; if uncertainty happens after the durable callback, require
   `SUBMITTING` and no automatic/manual duplicate submission. Preserve the
   bounded diagnostic only and stop for review.

The A/B sequence is accepted only when both the standalone control and the
automatic treatment satisfy their respective criteria without credentials,
content capture, browser-profile inspection, tunnel changes, or fallback
providers.
