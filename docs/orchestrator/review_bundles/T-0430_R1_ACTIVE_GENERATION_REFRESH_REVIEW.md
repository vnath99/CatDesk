# T-0430 R1 — Independent active-generation refresh review

## Verdict

ACCEPT T-0430 SOURCE LOGIC FOR THE NEXT IMMUTABLE REVIEWED WAKE PACKAGE.

This review accepts only the T-0430 source/test behavior. It does not itself
publish, install, activate, or live-accept a new Wake package, and it does not
authorize replay of the historical dev.67 receipt-bearing event.

## Independent source findings

The observed dev.67 defect is credible and the repair is targeted at the correct
boundary. The pre-T-0430 response observer kept polling ChatGPT's Stop/Pause
control but deliberately did not refresh while generation was active. A stale
client-side Stop/Pause control could therefore hold the owned browser open until
the generation bound even when the server-side response had settled.

T-0430 adds one bounded behavior to that post-submit observer:

- `RESPONSE_ACTIVE_REFRESH_SECONDS` is fixed at 120 seconds.
- A refresh deadline begins only while Stop/Pause is continuously visible.
- At the deadline, `refresh_response_observer` calls only the existing owned
  CDP object's reload operation. It accepts no caller URL, profile, executable,
  browser, credential, or alternate authority.
- The refreshed document must remain the exact authorized conversation; target
  drift and browser/network errors fail closed.
- After refresh, the observer allows bounded document hydration and requires the
  exact submitted USER digest to occur exactly once and remain the latest USER
  turn. Duplicate, reordered, or missing durable USER ownership cannot be
  converted into success.
- If Stop/Pause remains active, the next refresh is scheduled another fixed
  120 seconds later. If Stop/Pause clears, the active-refresh cadence resets and
  ordinary completion checks resume.
- Existing per-generation and total response bounds remain 35 and 90 minutes.
- The response-timeout Retry path remains the only path that retries a failed
  assistant turn. It revalidates the exact USER turn and never types or
  resubmits the wake USER message.
- Successful response observation still returns to the existing durable receipt
  round trip. The adapter remains one browser attempt per process; returning
  after SENT/successful reconciliation exits the SeleniumBase context, which
  closes the session-owned Chrome rather than leaving it for another event.

The repair therefore changes observation/revalidation behavior, not USER
submission ownership or Wake routing authority.

## Verification evidence reviewed

The dedicated T-0430 verification session
`adc-t0430-wake-refresh-verification-20260927` completed
`COMPLETED_VERIFIED`.

CatDesk's contract verifier reported PASSED for its approved profiles. The root
Cargo suite completed 991 tests successfully, and strict Clippy passed. The
root integration harness now invokes a fixed installed Wake Python interpreter
against fixed source tests with no caller-controlled command, path, or test
arguments. Its T-0430 regression exercises:

- active Stop/Pause persisting until the bounded refresh boundary;
- one same-target refresh followed by completion;
- timeout Retry remaining in-place;
- no reload before the transient timeout Retry control is consumed;
- sequence-drift rejection;
- refusal to treat a same-document append as sufficient submission proof; and
- post-submit target-drift failure.

The Wake-local Python validation list and selected runner also include the new
active-generation refresh regression for the next immutable Wake package's own
verification path.

The workspace remains intentionally broadly dirty. This review does not assign
unrelated historical changes to T-0430.

## Supplemental Codex packaging review

The parallel Codex session
`adc-t0430-codex-packaging-review-20260927` also completed
`COMPLETED_VERIFIED` and wrote
`T-0430_CODEX_PARALLEL_PACKAGING_REVIEW.md`.

Its packaging review is consistent with the existing immutable Wake boundary:
`wake/install.ps1` stages and hashes WakeHost, Binagotchy, `adapter.py`, and
`wake_bridge.py`; Rust publication/activation revalidates artifact identity and
owns the current/previous pointer handoff. Installed dev.67 is historical serving
evidence only and must not be overwritten or relabeled.

## Preserved boundaries

This review does not authorize:

- modifying installed dev.67 in place;
- resubmitting or replaying the existing T-0429 USER wake;
- treating a manual test event as natural acceptance;
- changing the canonical Chat37 generation-20 target;
- resuming T-0425 before accepted T-0429 source is present in serving/current;
- using legacy `catdesk_release_recovery`, direct `current.json` edits,
  direct executable replacement, or stale staging directories;
- restarting or replacing the externally owned official Secure MCP runtime; or
- treating package publication as live browser acceptance.

## Next reviewed boundary

1. Allocate the next immutable Wake package version. With current source still
   declaring dev.67, the expected next version is dev.68 unless authoritative
   state has advanced before packaging.
2. Update the Wake package version as a reviewed source change and rerun the
   required locked/offline build and package verification.
3. Publish only through the existing reviewed immutable install path and require
   exact manifest/artifact readback.
4. Activate only through the reviewed install handoff so prior desired state,
   singleton ownership, `previous.json`, `current.json`, and conditional
   restart remain transactional.
5. Establish T-0429 serving/current parity before resuming the same T-0425.
6. Run a fresh natural live acceptance proving:
   USER once -> Stop/Pause active -> bounded same-target periodic refresh ->
   Stop/Pause absent/completion proven -> terminal SENT -> owned browser closed.

T-0430 source logic is accepted for that next reviewed packaging boundary.
