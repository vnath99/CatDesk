# T-0428 — Wake dev.66 HOME navigation settle verification

## Trigger

Installed dev.65 diagnostic `manual-wake-dev65-idle-window-005` did not reach Chat33. It failed safely before browser write as `ATTENTION / TARGET_DRIFT`, with receipt null, timer null, and queueDepth 0.

The bounded readiness journal for that exact event showed:
- HOME / TARGET_DRIFT
- RECOVERY_BEGIN
- immediate RECOVERY_RETURN still on HOME
- second RECOVERY_BEGIN
- immediate RECOVERY_RETURN still on HOME

The same behavior occurred in the preceding dev.65 diagnostic. This ruled out active Chat33 response contention: the idle-window attempt still remained on HOME.

## Root cause

Production `retry_page_readiness()` invoked `cdp.get(exact_target)` and then immediately sampled `get_current_url()`. SeleniumBase/CDP navigation may return before ChatGPT's router has settled. The older smoke-test path deliberately sleeps/polls after navigation before judging the resulting route.

The existing deterministic test incorrectly modeled `cdp.get()` as a synchronous URL mutation, so it could not reproduce the live timing.

## dev.66 repair

- Added `HOME_RECOVERY_SETTLE_SECONDS = 10.0`.
- `retry_page_readiness()` now accepts the existing deterministic monotonic/sleeper hooks.
- After an exact-target in-place `cdp.get()`, Wake polls the same active tab for up to 10 seconds.
- HOME is treated as a transient only inside that bounded settle interval.
- `RECOVERY_RETURN` is emitted from the verified post-navigation route, not merely because the navigation API returned.
- No new browser session, alternate URL, write path, USER replay path, or post-write reload was added.
- Existing three-window readiness budget, 10-second stable-ready/idle gate, blank-shell recovery, exact-target/login/CAPTCHA/network guards, receipt logic, response completion, timeout Retry-in-place, and exactly-once boundaries remain unchanged.

## Regression coverage

Added deterministic tests proving:
1. a navigation call may return while the browser remains HOME, then transition to the exact conversation two seconds later; recovery waits and reports `SAME_CONVERSATION / RECOVERY_RETURN`;
2. persistent HOME consumes the full 10-second settle interval and reports `HOME / RECOVERY_RETURN`;
3. persistent-HOME page-readiness timing now accounts for the settle intervals between the existing readiness windows.

The fixed-runtime Rust->Python bounded readiness/response suite including the new tests passes.

## Verification

PASS:
- `cargo test --locked --offline --manifest-path wake/Cargo.toml --test python_bridge_validation adapter_and_response_python_regression_suite -- --nocapture`
- `cargo check --manifest-path wake/Cargo.toml --features test-support`
- `cargo clippy --manifest-path wake/Cargo.toml --lib --all-features -- -D warnings`
- scoped `git diff --check`

The generic CatDesk command gate repeatedly rejected the broad `cargo test --manifest-path wake/Cargo.toml --features test-support` invocation before execution. This is recorded as a verification-surface limitation, not a passing or failing full-suite result. dev.65 had the full Wake suite green before this narrow Python-only production change.

## Install / live acceptance

Install only through the immutable Wake package path while the dev.65 host is PAUSED. Preserve exact Chat33 generation 16 authority.

After installation, queue exactly one fresh fixed diagnostic and give it a genuinely idle Chat33 window. Require:
- readiness history to show either verified SAME_CONVERSATION recovery or a precise bounded failure;
- no USER write unless exact target + stable ready/idle gates pass;
- on delivery, one exact USER append, exact durable receipt, live timer, final SENT/COMPLETE, and no duplicate.

Do not replay `manual-wake-dev65-idle-window-005`.
