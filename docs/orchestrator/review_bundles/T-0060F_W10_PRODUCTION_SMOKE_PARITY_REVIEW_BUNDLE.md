# T-0060F-W10 Production / Smoke Parity Review Bundle

## Scope

This change ports the reviewed W9 standalone interaction state machine into
`scripts/wake_bridge.py`. It does not create a worker-owned browser route or
change the CatDesk dispatcher, tunnel, Scheduler, daemon, release, or Git
state. Only CatDesk's existing fixed-path automatic dispatcher may invoke the
production bridge for a durable actionable review record.

## Differential and convergence

The manually proven W9 smoke flow supplied these interaction requirements that
the production bridge had not yet made explicit:

| Phase | W9 smoke | W10 production bridge |
| --- | --- | --- |
| Before typing | bounded Stop wait, then exact target and unique empty editor recheck | same, with a bounded fixed CDP composer-state result |
| After typing | wait for one enabled Send; Enter only after bounded no-control observation | same, including deduplication, disabled wait, and ambiguous fail-closed path |
| Submission | one click/Enter | existing durable `SUBMITTING` callback immediately before the same single action |
| After submit | observe Stop where visible; retain browser for two seconds; clear still succeeds if Stop is transient | same, then require the existing exact appended-message receipt |

The production Stop selector set now includes the observed `Stop answering`
variant. The bridge rechecks the exact canonical conversation, absence of Stop,
and exactly one empty visible composer immediately before typing. It polls for
a safe Send control until its bounded UI deadline; a disabled control never
degrades into Enter, and a distinct ambiguous control remains operator
attention. A completely absent safe control through that bounded window is the
only Enter fallback.

After `before_submit()` persists `SUBMITTING`, no code path invokes click or
Enter again. The receipt loop observes a Stop/generating state if available
and extends its observation through two seconds from the first sighting. A
fast response which clears the composer and proves the exact receipt before a
Stop can be observed is still valid; lack of Stop is not a false failure.

## Preserved production guarantees

- exact current conversation and project-local dedicated-profile validation;
- one fresh actionable CatDesk review-record claim and singleton protection;
- durable `SUBMITTING` boundary before the single browser submit action;
- exact normalized message and target hashes plus schema-4 durable receipt
  fields before `SENT`;
- fixed user-author sequence requiring exactly one expected appended digest;
- post-boundary click/Enter/query uncertainty stays non-retryable
  `SUBMITTING` operator attention;
- legacy malformed `SENT`, login, CAPTCHA, network, draft, malformed CDP,
  non-unique editor/control, and target drift all fail closed.

No message contents are written to logs or durable state; author-role values
are accepted only as bounded strings and immediately hashed for receipt
comparison.

## Regression coverage

`tests/test_wake_bridge.py` covers production-only seams for:

- initial Stop then idle plus exact/unique/empty editor recheck;
- Stop timeout and non-unique editor failure before typing;
- disabled Send becoming safely ready and disabled timeout never becoming
  Enter;
- direct one-click/one-Enter submission, durable boundary, and no retry after
  post-boundary uncertainty;
- observed Stop with an accumulated at-least-two-second hold;
- transient/missed Stop plus exact receipt success;
- exact append success, mismatch, sequence drift, and receipt-query failure.

The established Rust controller regression
`transient_codex_429_does_not_activate_local_fallback` retains the required
provider-routing boundary: routine/smoke work and transient 429 conditions do
not activate Qwen/Ollama; local Qwen is eligible only after confirmed Codex
plan/credit exhaustion. This task does not start or exercise Qwen.

## Verification and residual acceptance

`cargo fmt -- --check`, `cargo clippy --all-targets --all-features -- -D
warnings`, `cargo test` (476 passed, 18 ignored), and `git diff --check`
passed in this worker. The project Python wake suite was attempted with the
existing dedicated venv, but its configured Python 3.12 base interpreter is
missing. The environment was not repaired, replaced, or bypassed.

After independent review and approved candidate loading, CatDesk—not Codex or
ChatGPT Web—may create one fresh actionable canary and allow its normal
automatic dispatcher to invoke the exact bridge once. Accept only a complete
schema-4 `SENT` receipt with the exact record/message/target hashes. The host
may record bounded status/timestamps. If pre-submit validation fails, require
operator attention without a submit; if post-boundary uncertainty occurs,
require `SUBMITTING` and no manual or automatic duplicate submission. Do not
launch a browser manually, inspect profile data, or alter the tunnel/Scheduler
as part of this acceptance.
