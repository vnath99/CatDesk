# T-0039 R7 wake page-load retry budget review bundle

## Scope and non-live review

This bundle reviews task T-0086 only. No wake was retried, no browser was launched, and no daemon, scheduler, tunnel, release, protected-state, or Git publication action was performed.

## R6 failure evidence

The durable `.catdesk/wake-bridge/state.json` entry for `review-adc-t0035-r6-canonical-codex-thread-dag-continuity-20260814-7-independent_final_review` is schema 4 and has:

| Field | Durable value |
| --- | --- |
| `status` | `OPERATOR_ATTENTION` |
| `attention` | `EDITOR_SELECTOR` |
| `browser_sent_at_unix` | `null` |
| `message_sha256` | `null` |
| `target_sha256` | `null` |
| `receipt_schema_version` | `null` |

This is a pre-submit failure and has no send receipt. The failed record was not retried by this task.

Before this change, `CdpSink.wake` created one shared `deadline = time.monotonic() + self.ui_timeout` immediately after opening the SeleniumBase context. The config's `ui_ready_timeout_seconds` is 20 seconds by default. Both initial editor readiness and `wait_for_idle` consumed that same deadline. If the editor had not appeared in time, the `with SB(...)` scope exited and SeleniumBase closed the browser/profile context.

The Rust dispatcher classified bridge exit 2 (`OPERATOR_ATTENTION`, including `EDITOR_SELECTOR`) as terminal. Its former two-second outer delay applied only to retryable process outcomes; it was neither a page-ready wait nor a useful retry for R6.

## New readiness state machine

`CdpSink.wait_for_page_readiness` is the sole owner of pre-submit page-load retry. It runs in the one SeleniumBase/CDP/profile context opened by `wake`:

```text
open exact configured target once
  -> attempt 1: fresh monotonic + 30 s window
  -> one same-context reload/navigation boundary
  -> attempt 2: fresh monotonic + 30 s window
  -> one same-context reload/navigation boundary
  -> attempt 3: fresh monotonic + 30 s window
  -> terminal OPERATOR_ATTENTION only if still incomplete
```

There is no whole-event pre-submit deadline. The counter, exactly three attempts, is the only readiness bound. An incomplete transient attempt keeps the context open. At the first two boundaries the bridge performs one bounded same-context action: CDP reload when available, otherwise exact-target CDP activation. It does not open a second browser, and it retains the most specific bounded transient diagnostic (`BROWSER_NETWORK_ERROR`, `EDITOR_SELECTOR`, or `DOCUMENT_LOADING`) for terminal attention.

Readiness uses only browser state, not conversation content: canonical exact target URL, no browser network-error page, no auth redirect/login control, no CAPTCHA/security control, a valid CDP `document.readyState` when CDP can reliably provide it, and exactly one visible expected editor. An unavailable or malformed ready-state result does not use fragile interception or inferred HTTP status; the other signals remain authoritative. Target drift, login, and CAPTCHA/security checks fail closed immediately without spending all three windows.

This means a page that becomes ready after 20 seconds but before 30 seconds continues in the original browser instead of closing at the prior shared 20-second boundary.

## Later safety timers and W13 invariants

Idle waiting is now given a new timer after readiness succeeds. Typing, send-control discovery, and receipt confirmation retain their own bounded timers and never reuse a readiness deadline. Before the first write, the bridge rechecks exact target, idle state, one empty editor, and actionability.

The submission path is unchanged in its W13 safety contract: expected digest must be absent before submit; the durable `SUBMITTING` callback is immediately before one click/Enter; no post-boundary retry is possible; receipt requires an empty composer and one final expected digest stable across two observations; and target identity is rechecked. `SENT` still requires schema-4 outer state, receipt schema 1, exact record/message/target hashes, and a positive browser send timestamp. No message text, profile data, or storage data is persisted.

Initial maximize/foreground presentation remains one-shot and occurs only after readiness but before typing. Retry boundaries only reload/activate the current exact Selenium CDP target; they do not run repeated OS foreground loops or select arbitrary windows.

## Retry ownership

The Rust dispatcher retries exit 3 (`WAKE_BRIDGE_BUSY`), which means the bridge did not enter its singleton/browser operation, and an OS process-launch failure, where no bridge process started. A zero exit lacking the exact durable receipt is terminal for automatic dispatch, as are bridge attention exits. This prevents a completed bridge process from yielding a second three-window browser sequence or crossing a possible submit boundary. The retained 2-second Rust delay is explicitly process/busy coordination, not a 30-second page-load attempt. Thus there is one page-load retry owner and no 3 x 3 readiness expansion.

## Deterministic coverage added

`tests/test_wake_bridge.py` adds clock/CDP seams covering:

- editor readiness after 20 seconds but before the 30-second attempt limit;
- two timed-out attempts followed by third-attempt success in the same CDP context, with reload boundaries at 30 and 60 seconds;
- all three windows failing with terminal attention and only two retries;
- immediate login, CAPTCHA/security, and exact-target-drift fail-closed paths;
- independent fresh deadlines, no cumulative readiness leakage, and one reload per retry boundary; and
- no typing when readiness has not succeeded.

The existing bridge tests continue to cover the durable before-submit boundary and no-post-submit-retry cases. `autonomy_runtime.rs` tests now prove that only busy process coordination is retryable and that invalid/absent receipts cannot relaunch browser readiness.

## Post-review canary plan

After independent review and only through the normal approved orchestration flow, schedule one fresh automatic canary record (not the failed R6 record). Observe its durable state and exact receipt only. Do not manually reuse a message, browser profile, or old record; stop for operator attention on any security, target, or receipt anomaly.
