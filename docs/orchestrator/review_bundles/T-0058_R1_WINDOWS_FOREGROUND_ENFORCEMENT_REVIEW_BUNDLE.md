# T-0058-R1 Windows foreground enforcement review bundle

## Repair

T-0058's current-CDP maximize() and bring_active_window_to_front() remain
unchanged and are still invoked first. R1 adds one bounded Windows top-level
window attempt immediately afterward.

The PID is sourced only from the already active SeleniumBase CDP object:
cdp.driver.cdp_base._process_pid when that browser object exists, otherwise the
equivalent current driver's _process_pid or browser_pid. It must be a positive
non-boolean integer. It remains local to the helper and is never logged,
returned, or persisted.

On Windows, stdlib ctypes enumerates only top-level candidates. A candidate
must be a positive valid HWND, a root unowned non-tool top-level window,
visible, and owned by that exact browser PID according to
GetWindowThreadProcessId. The helper acts only if there is exactly one
candidate. Zero, multiple, mismatched, invalid, or enumeration failure is a
no-op; title, executable, command-line, profile, and generic process discovery
are never used.

## One-shot presentation sequence

For an eligible W13 wake before typing:

1. cdp.maximize();
2. cdp.bring_active_window_to_front() for the exact CDP target;
3. on Windows with one proven owned HWND, ShowWindow(SW_MAXIMIZE),
   BringWindowToTop, and SetForegroundWindow, once each;
4. existing pre_typing_ready() revalidation before the first write.

All native exceptions and false returns are swallowed independently. They do
not change durable delivery/receipt state, create operator attention, or retry
a focus action. Target/editor drift still fails closed before typing. No
presentation call occurs after typing, the durable before_submit() boundary,
or receipt polling.

## Deterministic coverage

tests/test_wake_bridge.py adds injected Win32 seams proving:

- PID provenance from the current Selenium-owned CDP Browser;
- invalid/missing PID, zero/mismatched/multiple windows cause no activation;
- the unique exact-PID visible top-level HWND receives maximize, top, then
  foreground once in order;
- false returns/exceptions remain non-fatal, and non-Windows is a no-op;
- native presentation follows CDP maximize/activation and precedes typing;
- stale, draft, pre-existing digest, login, CAPTCHA, and network paths retain
  zero presentation; and
- existing W13 post-presentation revalidation and receipt ordering remain in
  place.

## Compatibility and limitations

Changed files are scripts/wake_bridge.py, tests/test_wake_bridge.py, and this
bundle. W13 schema-4/schema-1 receipt semantics, T-0054 policy, T-0055
accounting, T-0056 observability, Rust dispatch, tunnel, scheduler, daemon
lifecycle, and release behavior are unchanged.

Windows foreground-lock policy may deny SetForegroundWindow; success is not
inferred from a receipt. The implementation proves only that it makes a
bounded exact-window request. CatDesk must run the specified fresh normal
automatic wake canary after independent source review. No live browser was
launched by this worker.

## Local verification

Requested commands include the project venv focused Python suite, cargo fmt,
clippy, full cargo test, and git diff --check.

The project-local wake venv remains unavailable because its configured Python
3.12 base interpreter is absent. This ticket does not replace or repair that
runtime; CatDesk must run the focused Python suite in its approved runnable
environment and capture authoritative verification/diff evidence. Locally,
cargo fmt, clippy, the Rust suite (496 passed, 18 ignored, 0 failed), and git
diff --check passed.
