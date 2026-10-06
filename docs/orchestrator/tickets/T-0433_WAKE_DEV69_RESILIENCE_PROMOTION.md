# T-0433 — Wake dev.69 resilience promotion

## Status

DEV69_LIVE_RESILIENCE_AND_NATURAL_END_TO_END_ACCEPTED

## Parent authority

T-0432 WakeHost crash-observability and resilient-loop source is accepted by:

- `adc-t0432-resilience-verification-r7-20260927` — COMPLETED_VERIFIED;
- `docs/orchestrator/review_bundles/T-0432_R7_RESILIENCE_SOURCE_REVIEW.md`.

R6 remains valid superseded evidence for the pre-review candidate.

## Version allocation

The next immutable Wake version is `1.0.0-dev.69`.

Authoritative identity entries:

- `wake/Cargo.toml`
- `wake/Cargo.lock`
- root `Cargo.lock` path dependency

all identify `catdesk-wake 1.0.0-dev.69`.

Installed dev.68 remains untouched and accepted.

## Included maturity repair

dev.69 candidate adds:

- one singleton-held resilient WakeHost loop;
- exact current immutable package/hash/browser-artifact revalidation before retry;
- desired=RUNNING and no reviewed-install-handoff requirements;
- narrow transient-error retry allowlist;
- fail-closed integrity/activation/binding/package-authority errors;
- bounded backoff: 1,2,4,8,16,32,60 seconds;
- finite maximum of 8 consecutive recovery attempts inside the five-minute reset window;
- deferred in-memory recovery telemetry merge when immediate status persistence fails;
- status/MCP/Binagotchy fields:
  `hostRecoveryCount`, `lastHostError`, `lastHostErrorUtc`;
- permanent root Wake-local verification harness.

Exactly-once delivery semantics remain the existing durable
CLAIMED/ATTENTION/SUBMITTING/SENT state machine.

## Required package verification

Before publication/activation:

1. strict root Clippy;
2. full root Cargo suite, including the permanent Wake-local harness;
3. locked/offline WakeHost release build;
4. locked/offline Binagotchy release build;
5. exact SHA-256 + lengths for WakeHost, Binagotchy, adapter.py, wake_bridge.py;
6. independent review of exact dev.69 package candidate;
7. zero-argument immutable installer transaction only;
8. exact installed pointer/hash/desired-state/target readback.

## R6 historical sequencing resolution

Fresh live readback on 2026-09-28 proved the retained R6 review delivery is historical `SUBMITTING` with no receipt and an expired `OBSERVING` Wake timer. Current dev.68 state is browser `NOT_OBSERVED`, submission `IDLE`, attention null, actionable queue depth 0, and canonical authority has advanced from R6 generation 20 to generation 21.

`docs/orchestrator/review_bundles/T-0433_R2_R6_HISTORICAL_SUBMITTING_SEQUENCING_REVIEW.md` supersedes only the earlier activation-sequencing hold. R6 must remain immutable forensic evidence: never replay, retire, force-complete, or synthesize a receipt. Because the active browser/submission ownership that the original hold protected no longer exists, the already-reviewed zero-argument dev.69 installer may now run if the R2 preconditions still hold immediately before activation.

## 2026-09-28 activation and live recovery evidence

The reviewed zero-argument installer completed successfully for immutable dev.69:
`1.0.0-dev.69-336d8867524e-5d5de7a0d31e`.
Activation preserved priorDesired `RUNNING` and returned WakeHost `RUNNING` PID
`57716`. Fresh transport/Binagotchy readback agrees on dev.69, exact generation-21
canonical target, queueDepth 0, submission IDLE, browser NOT_OBSERVED, and no
attention.

Exact artifact SHA-256:
- WakeHost: `336d8867524ee34ef149f6cec21869bb6d8424cfc98df989494101e80beb3fcf`
- Binagotchy: `5d5de7a0d31e9f6138ccad6f185103581e5c2b1ffa0d6ba3cb3488d6f00270c0`
- wake_bridge.py: `02646440f25941aead9140b62b453ef6e0b46b3869e28ffc119c6fa9531331a5`
- adapter.py: `d7afbf5bc44144756e41f7d06454027965616531580b88bb2fc3db08be4dafb3`

A live recoverable-failure probe then temporarily removed only the CatDesk review
inbox from its expected path and restored the exact file. No Wake Store, target,
USER message, receipt, or browser state was mutated. The same dev.69 process/PID
survived. Fresh status proved `hostRecoveryCount=3`,
`lastHostError=REVIEW_SOURCE_INBOX_UNAVAILABLE`,
`lastHostErrorUtc=1790639762`, host RUNNING, queueDepth 0 and submission IDLE.
This is live evidence of same-process/same-singleton in-process recovery.
Deterministic source tests remain the authority for the finite exhaustion
threshold/backoff matrix.

The fresh ordinary post-recovery review event
`review-adc-t0432-dev69-postrecovery-natural-canary-20260928-6-independent_final_review`
has now reached canonical generation-21 Chat38 visibly through the natural dev.69
browser path. While the assistant response is active, exact live readback is host
RUNNING / PID 57716, browser GENERATION_ACTIVE, login READY, queueDepth 0,
delivery `SUBMITTING`, submission `SUBMISSION_ACCEPTED`, and Wake-event timer
`OBSERVING`. Durable `EXACT_USER_MESSAGE_APPENDED` receipt is not yet present,
so acceptance remains open. Do not ACK this review record and do not create a
second canary. Final acceptance still requires this same event to converge to
exact receipt -> terminal SENT -> timer COMPLETE -> browser close with no USER
replay.

## Live acceptance

After dev.69 activation, live T-0432 acceptance must safely induce a recoverable
host-loop error with no ambiguous USER submission and prove:

- same immutable package and same singleton authority remain in force;
- host recovery telemetry increments with exact error evidence;
- bounded retry occurs;
- persistent recovery cannot loop forever;
- normal event delivery still succeeds afterward;
- no USER replay or target drift occurs.

T-0432/dev.69 acceptance is now complete. Exact post-recovery canary
`review-adc-t0432-dev69-postrecovery-natural-canary-20260928-6-independent_final_review`
finished with generation-21 `EXACT_USER_MESSAGE_APPENDED`, terminal `SENT`, timer
`COMPLETE` at 102 seconds, browser `CLOSED_AFTER_SUCCESS`, queueDepth 0,
submission `SENT`/subsequent Binagotchy `IDLE`, same dev.69 PID 57716, and no
USER replay. The review record is acknowledged. Historical R6 remains quarantined
unchanged.

Do not resume T-0425 until root T-0429 serving parity is separately established.
