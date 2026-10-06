# T-0456 — dev.84 paired-turn receipt repair

## Scope

Session: `adc-t0456-dev84-paired-turn-receipt-20261004`  
Task: `t0456`

Wake-only source repair. This session does not install/promote Wake, replay or retire historical events, retry a reviewed build, mutate Git/GitHub/publication authority, or resume ordinary non-Wake autonomy.

## Live failure evidence

On canonical Chat45 generation 28, both of these exact messages visibly arrived as USER turns:

- `review-adc-t0415-r4-short-protected-target-20261004-6-independent_final_review`
- `manual-wake-mcp-1791165680221`

dev.83 initially reached `SUBMISSION_ACCEPTED / GENERATION_ACTIVE`, but later persisted `SUBMIT_CLEARED_NO_APPEND` and never created a new durable `EXACT_USER_MESSAGE_APPENDED` receipt. The R4 Wake timer remained `ATTENTION / SUBMITTING`.

Source inspection isolated the receipt regression in `CdpSink.trusted_turn_anchor_state()`. Candidate nodes were always promoted first to the nearest `[data-content-search-turn-key]`. On the current ChatGPT DOM that wrapper may contain the complete USER + assistant exchange. Before the assistant is present it can look like a USER turn; after the assistant appears the same wrapper contains both roles, so the dev.83 role gate correctly rejects it and the previously visible USER anchor disappears from the detector.

This is a container-selection bug, not permission to weaken the assistant-quotation guard or infer delivery from composer clearing.

## Repair

`scripts/wake_bridge.py` now:

1. Includes `[data-content-search-unit-key]` as a semantic message-unit candidate.
2. Chooses containers in this order:
   - semantic content-search unit,
   - explicit role/user-message container,
   - conventional conversation-turn container,
   - broad paired `data-content-search-turn-key` fallback.
3. Classifies USER/assistant role once through a shared fixed `classifyTurn` function.
4. Removes any container containing both USER and assistant roles from receipt ordering before exact-match and `turns_after` calculations.
5. Retains the dev.83 rule that only `isUserTurn && !isAssistantTurn` may satisfy the exact wake message.

Therefore an assistant quotation cannot become USER receipt evidence, while an outer paired wrapper can no longer erase a semantic USER unit after the assistant response mounts.

## Regression coverage

`tests/test_wake_bridge.py` now requires:

- semantic `data-content-search-unit-key` support,
- semantic-unit / role-container precedence ahead of the broad paired wrapper,
- explicit filtering of both-role containers,
- preservation of the USER-only exact-message gate.

The existing assistant/response selectors and post-submit fail-closed tests remain intact.

## Verification so far

- `cargo test --test stable_wake_adapter_python`: PASS, 2/2 Rust bridge tests.
- That fixed installed-Wake-Python harness explicitly ran and passed `WakeBridgeTests.test_turn_anchor_and_response_queries_accept_role_only_turn_dom` along with the bounded active-generation, response-retry, submission-acceptance, target-drift, and browser-cleanup regression set.
- Contract finalization must additionally pass its approved project-test, strict Clippy, and Git-diff profiles before independent review.

## Acceptance boundary

Do not count the already-visible R4/manual events as newly receipted and do not fabricate historical `SENT`. After independent review, build/install dev.84 only through the reviewed immutable Wake package path. Preserve the current generation-28 target. Then use exactly one fresh post-install diagnostic/canary and require:

- exact USER message,
- generation-28 `EXACT_USER_MESSAGE_APPENDED`,
- no duplicate USER submission,
- response completion,
- terminal `SENT`,
- event timer `COMPLETE`,
- queue depth zero.

Only after that receipt acceptance may the R4 Wake freeze be considered resolved and ordinary GitHub/protected-build work resume.
