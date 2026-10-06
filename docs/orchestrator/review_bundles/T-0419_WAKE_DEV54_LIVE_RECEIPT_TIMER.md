# T-0419 Wake dev.54 live receipt + timer acceptance repair

## Scope

This bounded follow-up was opened from the canonical Chat32 dev.53 acceptance
canary, not from a synthetic test. The exact natural USER wake
`review-adc-t0418r1-dev53-direct-20260923-6-independent_final_review` was
visibly delivered in canonical generation 15, but the live dev.53 path exposed
two acceptance defects while that assistant turn was active.

Ordinary non-Wake autonomy remains frozen. This change does not alter target
authority, the external Secure MCP tunnel, reviewed-build/recovery authority,
Git publication, or any historical SUBMITTING record.

## Canonical evidence

The installed dev.53 host initially showed the exact T-0418R1 event as
`SUBMITTING` while the visible USER message was already in Chat32. The
post-submit browser state reached an editor/readiness transient and
`turnTimers` remained empty on the serving status surface.

The USER message was subsequently proven with an exact durable receipt:

- event: `review-adc-t0418r1-dev53-direct-20260923-6-independent_final_review`
- evidence: `EXACT_USER_MESSAGE_APPENDED`
- generation: `15`
- target digest:
  `56333f7656f08b0158c2c115562cf1e64d468ac037846a7f5a6bbb0dca8e9cb7`
- message digest:
  `828ca51b7033c4b7505ae7e81a4b196f0fafd1361d708488c176745eff4c59c9`
- `sentUtc`: `1790202797`

The dev.53 response observer then failed closed with
`RESPONSE_TIMEOUT_STATE_LOST`; the delivery remained SUBMITTING and was not
replayed.

A direct Store-derived Wake status read proved that the passive timer had in
fact been persisted for that same exact receipt, with
`startedUtc=1790202797`, live elapsed/remaining values, and
`responseState=ATTENTION`. The long-running host's serialized
`status.json`, however, still exposed an empty `turnTimers` array.

## Root cause 1 — post-submit receipt used a pre-submit editor predicate

`CdpSink.durable_receipt_round_trip` correctly required a fresh exact-target
document after USER submit before trusting DOM receipt evidence. It then also
required `editor_state == ready`. That final condition was inherited from
pre-submit readiness but is not valid as a post-submit invariant: while the
assistant is actively generating, current ChatGPT surfaces may hide or replace
the normal composer.

That meant a valid USER wake could already be durable and visible while Wake
was prevented from publishing `USER_MESSAGE_APPENDED`, starting the timer,
and entering response observation until the assistant turn had already
finished or reconciliation occurred.

The SUBMITTING reconciliation path had the same conceptual problem because it
entered the ordinary pre-submit `wait_for_page_readiness` path before proving
the existing receipt.

## dev.54 repair 1 — editor-independent post-submit receipt boundary

The fresh-document gate now requires only:

- the exact authorized conversation identity;
- no browser network-error document;
- proof that the pre-reload JavaScript marker disappeared on a newly loaded
  document; and
- document readiness no longer equal to `loading`.

It deliberately does **not** inspect composer/editor presence after USER
submit. This does not weaken receipt identity: the following bounded receipt
poll still requires the exact expected USER-message digest to occur exactly
once and be the final USER message in two separated observations.

SUBMITTING reconciliation now enters the same post-submit fresh-document gate
directly and then runs `reconcile_persisted_receipt`. It remains observe-only:
it never types, presses Enter, clicks USER Send, downgrades SUBMITTING, or
replays the USER wake.

## Root cause 2 — long-running status serialized stale timer snapshots

The existing `runtime::status()` function correctly recomputed
`turnTimers` from durable Store state. The long-running `runtime::run()`
loop, however, initialized its in-memory `live` status once and repeatedly
serialized that snapshot without refreshing `live.turn_timers`.

Therefore a separate direct status command could see the persisted timer while
CatDesk's normal live transport status still showed an empty array.

## dev.54 repair 2 — timer refresh on every host heartbeat

The running WakeHost now reads durable `store.timers()` on every heartbeat,
maps each timer through the same `turn_timer_status` calculation, and writes
the refreshed collection with the current observed UTC timestamp.

This is status-only behavior. It creates no work, sends no message, changes no
target, and has no continuation authority.

## Preserved response policy

The existing dev.53 assistant-response policy is unchanged:

- exact USER receipt is intermediate, never permission to resubmit;
- browser remains owned while the wake-triggered assistant response is active;
- stable completion requires Stop/Pause to clear and the normal editor state to
  return stably;
- the exact `Message delivery timed out. Please try again.` state is scoped to
  the response following the latest exact USER wake;
- recovery reopens only the same canonical conversation and may click exactly
  one associated assistant `Retry`;
- maximum three retries, 35 minutes per generation and 90 minutes total;
- target drift, USER-sequence drift, network ambiguity, missing/ambiguous Retry,
  unproven completion and exhausted budgets fail closed.

The canonical T-0418R1 `RESPONSE_TIMEOUT_STATE_LOST` evidence remains
immutable and is not used as a successful acceptance result.

## Deterministic verification

Fresh dev.54 verification:

- `cargo test --manifest-path wake/Cargo.toml --all-targets --all-features`:
  PASS.
  - Wake library: 27 passed / 0 failed.
  - process-tree: 3 passed / 0 failed.
  - protocol/store: 24 passed / 0 failed.
  - Python bridge validation: the two new ordinary dev.54 tests passed; the two
    older manual harnesses remain intentionally ignored.
- New regression
  `dev54_post_submit_receipt_round_trip_is_editor_independent` invokes the
  fixed independent Python runtime and fails if post-submit receipt readiness
  queries the editor; same-target round-trip succeeds and target drift remains
  fail-closed.
- New regression
  `dev54_adapter_reconciliation_remains_observe_only` verifies reconciliation
  performs fresh-document round trip -> exact receipt proof -> response
  observation without USER typing/submission.
- `cargo build --release --locked --offline --manifest-path wake/Cargo.toml`:
  PASS.
- `cargo clippy --manifest-path wake/Cargo.toml --lib --all-features -- -D warnings`:
  PASS.
- `git diff --check`: PASS; existing LF/CRLF working-copy warnings only.
- `cargo fmt --manifest-path wake/Cargo.toml -- --check` could not be invoked
  through the current bounded command wrapper because that argument form was
  rejected before execution; no fmt failure was observed.

## Classification

`DEV54_LIVE_RECEIPT_AND_TIMER_SOURCE_READY_FOR_FINALIZATION`

The next acceptance stage, after direct-work finalization, is immutable dev.54
installation followed by a fresh natural canonical review event. Success
requires a matching durable USER receipt and a live `turnTimers` entry for
that exact event **while its assistant response is still active**, then final
SENT + frozen COMPLETE timer after the response ends.
