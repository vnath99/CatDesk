# T-0060F-W9 Smoke Stop-State Review Bundle

## Scope and evidence

W9 is deliberately limited to the standalone SeleniumBase smoke controls:

- `scripts/wake_browser_smoke_test.py`
- `scripts/wake_browser_smoke_test_cdp.py`
- `tests/test_wake_smoke_state.py`

`scripts/wake_bridge.py` was not changed. No production wake state, review
record, dispatcher, browser, configured profile, tunnel, Scheduler, daemon,
or release path was invoked or modified.

The observed composer control has two mutually exclusive operational states:
the fixed Send selector set when ready to submit, and the fixed Stop
answering/generating selector set while ChatGPT is generating. The smoke implementations now model those
states directly without inspecting conversation content.

## Smoke state machine

Before typing, both smoke paths:

1. wait boundedly until fixed Stop/generating controls disappear;
2. re-check the exact configured conversation URL;
3. require one visible empty `#prompt-textarea`; and
4. re-check the idle/absence-of-Stop state immediately before typing.

After typing, both paths wait only for an unambiguous, visible, enabled Send
control. The CDP control de-duplicates fixed selectors referring to the same
element and fails closed for an ambiguous control; disabled controls wait until
the bounded timeout and then fail. The WebDriver control likewise rejects
multiple visible controls and never treats a visible disabled control as an
Enter fallback. Native Enter remains available only when there is no safe send
control at all.

The actual click or native Enter is performed once. No branch after that
submission boundary calls a send primitive again. Both paths then observe the
fixed Stop state while confirming the editor clears. A seen Stop state keeps
the browser open for at least two seconds from the first observation. A quick
successful response may make Stop unobservable, so clear confirmation without
a seen Stop is still success rather than a false failure.

## Deterministic coverage

`tests/test_wake_smoke_state.py` uses fake clocks and fixed fake controls to
cover:

- Stop visible before typing, then disappearance and exact idle/editor
  revalidation;
- bounded Stop timeout;
- disabled Send becoming ready after a poll;
- disabled and ambiguous Send failure paths;
- one direct CDP click only, with no post-boundary re-click;
- observed Stop with an accumulated two-second hold;
- transient/missed Stop plus successful clear; and
- disabled WebDriver Send never degrading to the Enter fallback.
- CDP smoke context construction omitting SeleniumBase `test=True`, so bounded
  stop/send failures cannot be context-manager-suppressed.

## Verification

- `cargo fmt -- --check`, `cargo clippy --all-targets --all-features -- -D
  warnings`, `cargo test`, and `git diff --check` passed in this worker.
- The project wake Python interpreter may be used only if its existing
  environment is operational. This worker does not repair, replace, or bypass
  it.

## Host-only smoke acceptance

After independent review, the CatDesk host/operator may run each standalone
smoke script separately against the already configured exact conversation and
dedicated profile using a non-sensitive unique marker. This is not a bridge or
dispatcher wake.

For each run, accept only:

1. the fixed target and a single empty editor before typing;
2. any prior Stop state disappearing before typing;
3. one enabled Send click, or one native Enter only if no safe Send control is
   available;
4. editor-clear confirmation; and
5. either an observed `STOP_STATE_OBSERVED` followed by at least two seconds
   before browser close, or clear confirmation when Stop was too transient to
   observe.

Any Stop timeout, changed target, non-unique editor/send control, disabled
send timeout, or failed clear must stop without a second submit. Record only
bounded status and timestamps; never collect conversation text, cookies,
local storage, profile data, credentials, or network/tunnel details.
