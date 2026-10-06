# T-0420R2 Chat33 dev.55 live acceptance canary

## Purpose

This is a bounded, no-source-change continuation of the accepted T-0420 dev.55 Wake repair. Its only purpose is to produce one fresh legitimate independent final-review record created after the explicit Chat33 target rollover so the normal independent WakeHost can exercise the current canonical target naturally.

## Canonical authority at creation

- Project: `catdesk`
- Canonical conversation: `https://chatgpt.com/c/6ab465a8-63a8-83ea-adfc-758e368769e2`
- Target generation: `16`
- Target SHA-256: `d32961ec350c42d039f5514e9171a59574674ecc291b82c0c28f4d9c14336212`
- CatDesk transport: `CONNECTED_VERIFIED / READY`, 89 tools at rollover verification.
- Reviewed Wake package: `1.0.0-dev.55-d38ea4746aa9-57791d5851a3`
- WakeHost SHA-256: `d38ea4746aa91c5834141eecbe305282d5128b19f3519f204d162c2a757285ab`

The prior generation-15 Chat32 canary is historical/stale after the explicit target rollover and is not replayable or valid as Chat33 canonical acceptance evidence.

## Required natural live acceptance

The independent final-review generated from this direct-work session must be left to normal WakeHost discovery and delivery. No manual bridge-run-once, diagnostic test send, old-event replay, direct Wake Store mutation, or browser submission counts as acceptance.

Acceptance requires all of the following on exact Chat33:

1. A real persisted USER wake for the fresh review event.
2. Durable receipt evidence `EXACT_USER_MESSAGE_APPENDED` bound to target generation 16 and the exact target digest above.
3. A live `turnTimers` entry for that same event while the assistant response is active.
4. If ChatGPT completes normally, browser-observed `RESPONSE_COMPLETED`.
5. If ChatGPT instead exposes the exact timeout state, dev.55 must take
   `RESPONSE_TIMEOUT_DETECTED -> RESPONSE_RETRYING -> RESPONSE_RETRY_STARTED`
   by consuming the unique associated assistant Retry in the current page before any reload. The USER wake must never be submitted again.
6. After the response finishes, the same delivery must be final `SENT` and its timer must be frozen `COMPLETE`.

The existing maximum-three response retries, 35-minute per-generation bound, 90-minute total bound, target/sequence/network/ambiguity fail-closed behavior, and passive/advisory timer remain unchanged.

## Scope and preserved boundaries

This canary makes no production source repair and does not alter Wake implementation, browser profile, canonical target, external Secure MCP/tunnel ownership, reviewed-build/recovery/LKG state, or Git publication state. Historical stale/quarantined SUBMITTING evidence remains immutable and non-replayable.

Ordinary non-Wake autonomy remains frozen until the live criteria above are observed. After successful Chat33 acceptance, the next engineering continuation is the already-defined T-0412 V5-equivalent Windows linker diagnostic.
