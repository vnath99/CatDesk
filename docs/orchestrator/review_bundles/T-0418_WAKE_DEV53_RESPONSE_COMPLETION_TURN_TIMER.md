# T-0418 Wake dev.53 response completion + passive turn timer

## Scope

This review closes the two Wake follow-ups requested after the natural Chat32
T-0415/T-0417 wake deliveries exposed a separate ChatGPT assistant-response
timeout. The inherited provider session exhausted cloud credits and was
cancelled after leaving a coherent dev.53 candidate; ChatGPT-direct work
claimed `adc-t0418r1-dev53-direct-20260923` and independently reviewed the
candidate without changing live Wake, target authority, the Secure MCP tunnel,
recovery/LKG state, or Git publication state.

## Feature 1 — deterministic assistant-response completion

Production `wake/adapter.py` now treats the exact USER receipt as an
intermediate durable boundary rather than final browser success. It emits
`USER_MESSAGE_APPENDED` with the bound
`EXACT_USER_MESSAGE_APPENDED` receipt, keeps the owned browser open, and calls
the bounded `CdpSink.wait_for_response_completion` path before emitting final
`SENT`.

The response observer in `scripts/wake_bridge.py`:

- watches the generation Stop/Pause control and waits for the normal
  editor/Send-ready state to return stably for two polls;
- verifies the expected wake digest is still present exactly once and is the
  latest USER message before any completion or retry decision;
- scopes the exact text `Message delivery timed out. Please try again.` to the
  response following that latest USER message;
- if the timeout is present, reopens only the same canonical conversation,
  re-verifies the exact USER sequence, and clicks exactly one associated
  assistant-response `Retry` control;
- never types or submits another USER wake after the durable receipt;
- permits at most three assistant-response retries, 35 minutes per generation,
  and 90 minutes total; target drift, network error, sequence drift, ambiguous
  timeout/retry controls, retry-start failure, unproven completion, and budget
  exhaustion all fail closed.

The SUBMITTING reconciliation path has the same post-receipt observer, but its
USER-side authority remains observe-only: it can prove the existing exact
receipt and resume assistant-response observation/retry, never type, press
Enter, or click USER Send.

## Feature 2 — passive CatDesk turn timer

The Rust host persists one passive `TurnTimer` per exact received Wake. The
timer starts from the browser receipt's original `sentUtc`, not browser claim
time. A `USER_MESSAGE_APPENDED` adapter stage lets the host persist this timer
while the browser is still waiting for the assistant response.

Independent Wake status exposes additive timer readback:

- exact event ID and `startedUtc`;
- `elapsedSeconds` and `remainingSeconds` against a 20-minute target;
- `softCheckpointReached` at 18 minutes;
- `deadlineReached` at 20 minutes;
- response state (`OBSERVING`, `RETRYING`, `ATTENTION`, `COMPLETE`).

The timer is advisory only. It cannot interrupt ChatGPT, publish a continuation
event, replay a wake, or consume assistant COMPLETE/CONTINUE flags. The
superseded T-0417 `.catdesk/wake-turn-control.json` and automatic continuation
state machine are absent from the dev.53 Wake source. Receipt reconciliation is
idempotent and preserves the original timer start; browser-proven completion
freezes the terminal elapsed duration.

## Safety boundaries preserved

- dev.51 receiptless pre-submit `CHATGPT_NOT_IDLE` retry semantics remain
  unchanged.
- T-0413 designated-chat rollover semantics remain unchanged.
- Historical T-0403R2 generation-14 SUBMITTING evidence remains immutable and
  non-replayable.
- Canonical Chat32 generation-15 target authority is not changed.
- The externally owned official Secure MCP tunnel runtime is not touched.

## Verification

Fresh direct review verification on the inherited dev.53 candidate:

- `cargo test --manifest-path wake/Cargo.toml --all-targets --all-features`:
  PASS. Wake lib 27/27, process-tree 3/3, protocol-store 24/24; manual
  diagnostic/Python harnesses remain intentionally ignored by the ordinary
  all-target test command. Existing diagnostic examples emit inherited
  warnings only; exit code is 0.
- `cargo build --release --locked --offline --manifest-path wake/Cargo.toml`:
  PASS.
- `cargo clippy --manifest-path wake/Cargo.toml --lib --all-features -- -D warnings`:
  PASS.
- `git diff --check`: PASS (line-ending warnings only).
- Existing deterministic Python response tests cover generation-active ->
  stable idle, exact timeout reopen + assistant Retry, completion-after-reopen,
  latest-USER sequence drift refusal, ambiguous Retry refusal, and three-retry
  exhaustion. The fixed-runtime harness remains a separately invoked manual
  regression surface; live browser acceptance is intentionally deferred until
  reviewed dev.53 activation.

## Classification

`DEV53_RESPONSE_COMPLETION_AND_PASSIVE_TIMER_SOURCE_READY_FOR_FINALIZATION`

Live installation and Test-chat/canonical acceptance are explicitly outside
this source-review session and require successful direct-work finalization
first.
