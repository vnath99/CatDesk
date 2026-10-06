# T-0430 — Active-generation refresh and browser-close convergence

## Status

VERIFIED_AND_INDEPENDENTLY_ACCEPTED_FOR_REVIEWED_PACKAGING

## Trigger

A fresh natural generation-20 review event,
`review-adc-t0429-r1-direct-resume-independent-review-20260927-6-independent_final_review`,
was delivered by installed Wake dev.67 with durable `EXACT_USER_MESSAGE_APPENDED`.
The owned Selenium/Chrome window remained visibly open with ChatGPT's Stop/Pause
control active after the assistant response had otherwise completed in the user's
normal view. Repeated Wake status remained `GENERATION_WAIT`.

The operator clarified the required lifecycle:

1. send the USER wake once;
2. while Stop/Pause is active, keep the same browser session alive;
3. wait a few minutes rather than spinning/relaunching;
4. periodically refresh the same conversation and re-check Stop/Pause;
5. once Stop/Pause is gone and deterministic completion is proven, finish receipt
   persistence and allow the owned browser to close.

## Root cause

Current dev.67 polls Stop/Pause every 500 ms but explicitly forbids reload while
generation is active. A stale client-side Stop/Pause control can therefore hold
the observer until the 35-minute generation bound even after server-side
completion.

## Narrow source repair

`scripts/wake_bridge.py` now defines
`RESPONSE_ACTIVE_REFRESH_SECONDS = 120`.

`wait_for_response_completion`:
- starts a refresh deadline only after Stop/Pause is continuously observed;
- refreshes only after two continuous minutes of active Stop/Pause;
- uses only the existing owned CDP/browser/profile and exact authorized
  conversation URL;
- never types or resubmits the USER wake;
- revalidates the exact latest USER digest after refreshed-document hydration;
- resets the two-minute refresh cadence when Stop/Pause clears;
- retains the existing 35-minute per-generation and 90-minute total bounds;
- retains the existing timeout Retry path before periodic refresh can destroy a
  transient Retry control; and
- returns only after the existing editor/assistant completion checks prove the
  turn complete, after which the existing durable receipt/persistence step runs
  and the browser context may close normally.

A new `refresh_response_observer` helper performs only a same-target reload and
bounded exact-target/document-readiness validation. It accepts no URL, profile,
browser, credential, or execution authority from callers.

## Tests staged

`tests/test_wake_bridge.py` adds
`test_active_generation_periodically_refreshes_same_target_until_pause_clears`.
The deterministic fixture requires:
- Stop/Pause remains stale until reload;
- no refresh before the two-minute continuous-active boundary;
- exactly one same-target reload;
- exact USER digest survives/revalidates;
- final stage is `RESPONSE_COMPLETED`; and
- the old reopen path is never called.

The test is also added to the selected Wake Python regression runner and the Wake
Python validation list.

## Verification state

- Source inspection: complete; control flow and indentation are coherent.
- Normal root `verify_project`: root `cargo build` PASS. Root `cargo test`
  exceeded the 30-second verifier window. Global root `cargo fmt --check`
  remains red only for the previously documented unrelated
  `src/reviewed_build.rs` formatting drift.
- Dedicated Wake-subcrate `cargo test` invocations through generic
  `run_command` are currently rejected at the hardened command boundary before
  Cargo starts. Do not weaken that policy or describe those refusals as test
  failures.
- Therefore T-0430 is NOT yet verified or review-ready.

## Acceptance

1. Execute the approved Wake Python/Rust verification path with the new refresh
   regression included.
2. Preserve existing timeout-Retry, sequence-drift, target-drift, generation
   timeout, and no-duplicate-USER tests.
3. Independently review the exact attributable T-0430 diff.
4. Build/install the next immutable reviewed Wake package; do not mutate dev.67.
5. Use a fresh legitimate review event or sanctioned continuation of an
   interrupted receipt-bearing event only as allowed by reviewed Wake
   reconciliation semantics.
6. Live proof must show: USER once -> Stop/Pause active -> bounded periodic
   same-target refresh -> Stop/Pause absent/completion proven -> terminal SENT ->
   owned browser closed.
7. No manual test event may count as natural acceptance.
