# T-0420 Wake dev.55 in-place timeout Retry

## Live defect

Canonical Chat32 naturally received
`review-adc-t0418r1-dev53-direct-20260923-6-independent_final_review`.
The independent Wake store persisted an exact bound USER receipt:

- evidence: `EXACT_USER_MESSAGE_APPENDED`
- target generation: 15
- target digest: `56333f7656f08b0158c2c115562cf1e64d468ac037846a7f5a6bbb0dca8e9cb7`
- message digest: `828ca51b7033c4b7505ae7e81a4b196f0fafd1361d708488c176745eff4c59c9`
- sentUtc: `1790202797`

The passive timer also started correctly and later reported the same event with
live elapsed/remaining time. The assistant response, however, did not continue.
Wake ended at `RESPONSE_TIMEOUT_STATE_LOST`.

The failure was deterministic. dev.53 detected exactly one
`Message delivery timed out. Please try again.` card and one associated Retry
control, then reopened the conversation *before* clicking Retry. The ChatGPT
timeout card is transient UI; the reload preserved the durable USER wake but
removed the timeout card and Retry control. The observer therefore saw neither
an assistant response nor an actionable timeout after reload and failed closed.

## dev.55 repair

The response observer now treats the currently visible exact timeout card as
the authoritative retry opportunity.

After stable idle, exact latest-USER validation, and exactly one timeout +
exactly one associated Retry:

1. emit `RESPONSE_TIMEOUT_DETECTED`;
2. re-validate the exact expected USER wake is still unique/latest;
3. emit `RESPONSE_RETRYING`;
4. click the unique assistant-response Retry **in the current exact-target
   document before any reload**;
5. prove retry start from Stop/Pause or disappearance of the timeout card;
6. resume the existing bounded generation observer.

The USER wake is never typed or submitted again. The existing bounds remain:
three assistant retries maximum, 35 minutes per generation and 90 minutes
total. Target, network, USER-sequence, Retry-control ambiguity, retry-start
failure and completion ambiguity remain fail-closed.

The same-target reopen helper remains available for other diagnostics but is
not invoked by the normal timeout-Retry path before the transient Retry
control is consumed.

## Regression coverage

The deterministic response suite now explicitly proves:

- timeout Retry occurs in-place before any reload;
- the transient Retry control is not destroyed by a pre-Retry reopen;
- the retry budget remains exactly three;
- sequence drift still prevents Retry;
- ambiguous timeout/Retry controls remain fail-closed;
- normal generation-active -> stable-idle completion remains unchanged.

The fixed-runtime Python response suite is now part of normal Wake verification
when the reviewed Python runtime exists, and skips safely when it is absent.
The independent adapter tests remain part of that same bridge validation
surface.

## Preserved boundaries

- dev.54 editor-independent post-submit durable receipt proof remains intact.
- dev.53 passive timer remains advisory only and still starts from exact USER
  receipt `sentUtc`.
- dev.51 `CHATGPT_NOT_IDLE` pre-submit retry remains unchanged.
- T-0413 target rollover remains unchanged.
- T-0403R2 and the quarantined T-0414/T-0416 SUBMITTING records remain
  non-replayable.
- No live Wake install, browser action, canonical target change, external tunnel
  mutation, recovery/LKG mutation or Git publication is authorized by this
  source-review ticket.

## Classification

`DEV55_TIMEOUT_RETRY_IN_PLACE_SOURCE_READY_FOR_FINALIZATION`
