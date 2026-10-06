# T-0394 — Generation-13 Natural Wake Canary

Date: 2026-09-20

## Purpose

This bounded documentation-only task exists to generate one fresh legitimate CatDesk `independent_final_review` **after** the current independent Wake generation-13 target cutoff, so the standalone WakeHost can exercise normal discovery and browser delivery without a manual test event or bridge-run-once invocation.

This task is not itself Wake acceptance. Acceptance requires the resulting fresh review to be naturally discovered and delivered as a real USER message into the canonical ChatGPT conversation, followed by a durable receipt whose evidence is `EXACT_USER_MESSAGE_APPENDED`.

## Authoritative live pre-canary state

- Canonical CatDesk conversation:
  `https://chatgpt.com/c/6ab06a56-56c0-83e9-9697-9bc7c380c154`
- CatDesk project target SHA-256:
  `1ffdbc8aa68d7b0d3b3ec695a87fb87d8c43f04c64f0b52f3d9c8880a3aa5f43`
- Independent Wake target SHA-256:
  `1ffdbc8aa68d7b0d3b3ec695a87fb87d8c43f04c64f0b52f3d9c8880a3aa5f43`
- Independent Wake target generation: `13`
- Installed Wake package: `1.0.0-dev.46`
- WakeHost state at canary preparation: `RUNNING`
- Wake submission state at canary preparation: `IDLE`
- Wake queue depth at canary preparation: `0`
- Secure MCP transport: `CONNECTED_VERIFIED`
- Local MCP self-check: `READY`, 88 tools
- Dedicated Wake browser profile was independently verified by a visible non-sending probe to open an authenticated ChatGPT conversation with the prompt editor present.

The older T-0393 independent review was created before generation 13 and is therefore intentionally ineligible for this acceptance canary.

## Allowed change

This exact file only:

`docs/orchestrator/review_bundles/T-0394_GENERATION13_NATURAL_WAKE_CANARY.md`

No product source, Wake config/state/profile, CatDesk daemon/release state, Secure MCP runtime, external project, Git history, or protected authority mutation is part of this task.

## Natural-delivery acceptance requirements

The resulting T-0394 completion is valid only if all of the following occur naturally:

1. CatDesk reaches `COMPLETED_VERIFIED`.
2. CatDesk appends a fresh ordinary `independent_final_review` record created after the generation-13 target cutoff.
3. Standalone WakeHost discovers that review from the normal durable review inbox.
4. No manual Wake diagnostic event, stale replay, `catdesk_wake_bridge_run_once`, direct Store publication, or manual browser send is used.
5. The browser delivery appends a real USER message to the canonical conversation above.
6. Durable Wake delivery reaches `SENT`.
7. The durable receipt is non-null and records `EXACT_USER_MESSAGE_APPENDED`.
8. Record ID, message digest, target digest, and target generation remain correlated to this fresh review and generation 13.

Any pre-submit browser/auth/network/target/readiness failure remains preserved as failure evidence and does not count as acceptance.

## Verification intent

This canary is documentation-only. Verification is limited to the contract-approved authoritative diff check and CatDesk's normal completion/final-review machinery. The dirty pre-existing worktree must remain intact and unrelated changes must not be attributed to this task.

## Expected next step

After CatDesk emits the fresh independent review, do not manually wake it. Allow dev.46 to discover and deliver it naturally. Then inspect the durable Wake delivery/receipt and confirm the actual user message appears in the canonical conversation before declaring Wake accepted.
