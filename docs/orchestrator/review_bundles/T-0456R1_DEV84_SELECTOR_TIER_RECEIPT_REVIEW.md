# T-0456 R1 — dev.84 selector-tier receipt repair

## Scope

Session: `adc-t0456r1-dev84-selector-tier-receipt-20261004`  
Task: `t0456r1`

Wake-only repair. No Wake install/promotion, historical event replay/retirement, Git/GitHub mutation, protected-build action, or ordinary autonomy is authorized by this session.

## Why R1 supersedes the initial T-0456 candidate

Independent review caught a pre-assistant race in the first candidate: semantic USER units and their broader paired wrapper could coexist in one combined list. Before the assistant mounts, both could match the exact USER message; after the assistant mounts, the broad wrapper becomes both-role and disappears. That could transiently produce two matches before later producing one.

R1 removes mixed-tier receipt ordering entirely.

## Final repair

`CdpSink.trusted_turn_anchor_state()` now evaluates mutually exclusive selector tiers in confidence order:

1. `[data-content-search-unit-key]`
2. explicit `[data-message-author-role]` USER/assistant nodes
3. explicit `[data-turn]` USER/assistant nodes
4. USER message bubble/class fallback
5. conventional conversation-turn wrappers
6. broad `[data-content-search-turn-key]` compatibility fallback

Within each tier, any container that simultaneously classifies as USER and assistant is discarded. A tier is selected only if it contains at least one unambiguous USER turn. Once selected, lower-confidence tiers are ignored.

The exact wake match remains restricted to `isUserTurn && !isAssistantTurn`, preserving dev.83's assistant-quotation protection. The broad paired wrapper can therefore neither double-count a semantic USER turn before assistant generation nor erase it after assistant generation.

## Regression coverage

`tests/test_wake_bridge.py` now requires:

- semantic-unit tier precedes the broad paired wrapper,
- one selector tier is selected rather than mixed,
- both-role containers are excluded through XOR role classification,
- selected tier must contain a USER,
- no `rawTurns` mixed-tier accumulator remains,
- exact matching remains USER-only and assistant-excluding.

## Verification so far

`cargo test --test stable_wake_adapter_python`: PASS 2/2. The fixed installed Wake Python harness explicitly includes `WakeBridgeTests.test_turn_anchor_and_response_queries_accept_role_only_turn_dom` plus bounded active-generation, submission-acceptance, target-drift, response-retry, and browser-cleanup regressions.

Contract finalization must additionally pass the approved project-test, strict Clippy, and Git-diff profiles.

## Acceptance

The initial T-0456 review must not be ACKed as final; R1 supersedes it.

After R1 independent review, use only the reviewed immutable Wake package/install path to produce dev.84. Preserve canonical Chat45 generation 28. Then send exactly one fresh diagnostic/canary and require generation-28 `EXACT_USER_MESSAGE_APPENDED`, no duplicate USER, response completion, terminal `SENT`, timer `COMPLETE`, and queue zero before lifting the current Wake freeze.
