# T-0058 Wake browser focus/maximize review bundle

## Change

`scripts/wake_bridge.py` now calls
`CdpSink.best_effort_present_browser()` exactly once for an eligible wake. The
helper uses the installed SeleniumBase 4.51.5 active-CDP methods in this order:

1. `cdp.maximize()`;
2. `cdp.bring_active_window_to_front()`.

Both operations are bound to SeleniumBase's current CDP page after
`activate_cdp_mode()` has loaded the configured conversation. No Win32 handle,
process list, window title, profile data, browser storage, command line, shell
helper, or new dependency is used. Consequently there is no native ownership
selection to persist or audit. A platform without either current-CDP capability
is a no-op; a platform with them uses only the already validated target.

## W13 ordering and safety

Presentation occurs only after the bridge has loaded the exact configured
conversation/editor, observed idle, passed `pre_typing_ready()`, confirmed the
review remains actionable, and proved the expected wake digest absent from the
pre-submit snapshot. It is followed immediately by a second
`pre_typing_ready()` before the first `press_keys()` write.

This preserves the schema-4/schema-1 W13 boundary: maximize/foreground does
not call `before_submit()`, does not persist state, and cannot cause a submit
retry. There is no presentation path after typing, after `SUBMITTING`, or in
receipt polling. Presentation-induced target/editor drift therefore fails at
the existing pre-submit attention boundary before typing.

## Failure model

Each supported operation is attempted once. Exceptions and false-like results
are intentionally ignored: Windows foreground lock may reject activation, but
that must not turn a safe delivery into operator attention or mutate receipt
state. The later validation remains fail-closed for page/editor safety.

## Deterministic coverage

`tests/test_wake_bridge.py` covers:

- maximize then foreground exactly once before typing and the durable submit
  boundary;
- non-fatal exception/false outcomes;
- presentation-induced target drift before typing/boundary;
- zero presentation for draft, stale/non-actionable, pre-existing digest,
  login, CAPTCHA, and terminal network failures;
- a recovery reload followed by exactly one presentation; and
- no post-typing/boundary/receipt-polling focus call through exact event order.

Existing W13 receipt, idempotency, state, and submit tests remain unchanged.

## Scope and compatibility

Changed files are `scripts/wake_bridge.py`, `tests/test_wake_bridge.py`, and
this bundle. T-0054 wake policy, T-0055 accounting, T-0056 observability,
Rust dispatch/receipt validation, tunnel, scheduler, daemon lifecycle, and
canonical release behavior are not changed.

Windows may still deny foreground activation under OS foreground-lock policy;
the intentional product behavior is a bounded best-effort request, not a
guarantee or focus-stealing retry loop. Unit tests do not launch a browser.
CatDesk must perform the separate post-review live canary before treating the
visible presentation behavior as production-proven.

## Local verification

Requested commands:

```text
.catdesk\wake-bridge\venv\Scripts\python.exe -m unittest tests/test_wake_bridge.py -v
cargo fmt -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
git diff --check
```

The repository-local wake venv currently cannot start because its recorded
base interpreter is absent. This ticket does not repair or replace that
runtime, so the focused Python suite did not reach test discovery. `cargo fmt
-- --check`, clippy, the Rust suite (496 passed, 18 ignored, 0 failed), and
`git diff --check` completed locally. CatDesk's independent verifier must run
the Python suite in its approved runnable environment and capture the
authoritative diff.
