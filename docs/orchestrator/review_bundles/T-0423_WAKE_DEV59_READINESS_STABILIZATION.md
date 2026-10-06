# T-0423 — Wake dev.59 ChatGPT readiness stabilization

## Triggering live evidence

Operator-observed ChatGPT browser behavior on canonical Chat33 exposed two pre-submit readiness races:

1. The page can appear loaded and briefly expose a generation Stop/Pause control before the UI settles. Wake could previously proceed as soon as the editor first became detectable.
2. The exact conversation can load into a blank/non-interactive shell with no usable composer or controls. Wake previously waited the full 30-second readiness window before a recovery reload.

These are pre-write browser-readiness defects only. Existing exact-target, login/CAPTCHA, network, submission, receipt, response-completion, timeout-Retry, pause-boundary, and exactly-once protections remain authoritative.

## dev.59 repair

- Added `POST_LOAD_STABLE_SECONDS = 10.0`.
- An exact-target editor-ready page must remain continuously ready for 10 seconds before Wake can proceed to typing.
- `wait_for_idle()` now requires a continuous 10-second period without any visible generation Stop control before returning. A transient disappearance resets safely if the control reappears.
- Added `BLANK_SHELL_RELOAD_SECONDS = 10.0`.
- An exact target whose document is loaded but exposes no editor, send control, or generation control for 10 continuous seconds is classified as a blank shell and receives one normal reload at the existing bounded pre-submit recovery boundary.
- The blank-shell recovery remains strictly pre-write. Once browser typing begins, no reload/session relaunch is introduced.
- Chrome network error behavior remains bounded by the existing three-session pre-write retry model.
- HOME/auth/target-drift/CAPTCHA and non-unique/draft editor conditions remain fail-closed.

## Deterministic coverage

The fixed-runtime Rust->Python regression harness now permanently includes readiness tests proving:

- a slow editor is not accepted until it remains ready for the 10-second stable window, even when that hold extends beyond the original 30-second attempt deadline;
- a transient on-target login/hydration state may settle without a reload;
- an exact loaded blank shell reloads at the 10-second boundary and can subsequently converge;
- a visible generation control must clear and remain absent for the 10-second quiet window before typing;
- existing response-completion and timeout Retry-in-place regressions remain green.

## Verification

- `cargo test --manifest-path wake/Cargo.toml --features test-support`: PASS.
  - Wake library: 33/33.
  - process tree: 3/3.
  - protocol/store: 24/24.
  - Python validation: 3 passed + 1 intentional manual harness ignore.
- `cargo clippy --manifest-path wake/Cargo.toml --lib --all-features -- -D warnings`: PASS.
- Scoped `git diff --check`: PASS (line-ending notices only).

Historical diagnostic examples still emit pre-existing warnings; no test failure is attributable to T-0423.

## Installation boundary

Do not install/restart dev.59 while installed dev.58 owns an active exact USER delivery. Current T-0422 canary has a durable generation-16 `EXACT_USER_MESSAGE_APPENDED` receipt and is in response observation. Allow it to settle first.

After terminal dev.58 state, install dev.59 only through the immutable reviewed Wake package path, preserve exact Chat33 generation-16 authority, then run one fixed MANUAL diagnostic through the installed Python/Selenium path to verify:

1. no rapid page reopen loop;
2. at least 10 seconds of stable ready/idle UI before first typing;
3. blank-shell condition, if observed, waits 10 seconds and reloads once rather than repeatedly opening;
4. exact USER receipt;
5. final SENT / COMPLETE / queueDepth 0 without duplicate USER wake.
