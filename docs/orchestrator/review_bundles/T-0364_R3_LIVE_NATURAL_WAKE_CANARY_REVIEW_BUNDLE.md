# T-0364 R3 Live Natural Wake Canary Review Bundle

## Classification

`LIVE_NATURAL_WAKE_CANARY_REVIEW_RECORD_READY`

This is a bounded no-product-source-change canary. It creates a normal review
record only; it neither attempts nor claims live wake delivery.

## Exact target convergence

The approved durable plan, convergence review, project-registry readback, and
effective wake configuration agree on the current canonical control
conversation:

| Binding | Exact value |
| --- | --- |
| Canonical chat URL | `https://chatgpt.com/c/6aa87d1f-7190-83ea-9a9c-dc4b8d658204` |
| SHA-256 | `15dcac440d1b523f9c6535c42f5ec4e2365ce90f7aff429ce7d3fba1c4798cf9` |

Read-only evidence was limited to `.catdesk/current_plan.md`, the accepted
T-0364 Wake/Binagotchy convergence bundle, bounded project registry and
`.catdesk/wake-bridge/config.json` target readback, and this task's approved
contract. No target, wake configuration, or protected state was written.

## Existing verified baseline

The accepted convergence record supplies the current source/test baseline:

- one normal visible `CatDesk Binagotchy` GUI with persisted designated-target
  editing/readback and WakeHost controls/status;
- independent WakeHost backend lifetime; closing or crashing the GUI does not
  own CatDesk or WakeHost;
- interactive launch/recovery opens or focuses the singleton, while
  headless/session-zero paths remain GUI-free;
- empty configured target returns before WakeHost/browser startup, event claim,
  or delivery;
- prior deterministic results: `windows_gui` 19/19, wake process-tree 3,
  wake protocol/store 16, recovery harness 3/3, formatting, strict Clippy,
  and full workspace tests all passed.

This canary makes no product-source or test change and does not rerun those
profiles. It does not reinterpret the prior baseline as live browser proof.

## Natural-delivery boundary

Natural event-driven delivery to the exact canonical chat remains **PENDING**.
The current plan records diagnostic backlog counts of 124 pending and 287
stale; those counts are not a receipt, claim, delivery, or browser proof and
were not replayed, cleared, or otherwise changed.

After this task reaches ordinary `COMPLETED_VERIFIED`, only CatDesk's normal
automatic dispatcher may discover the resulting legitimate review event and
attempt natural delivery. Final acceptance requires that exact event's durable
claim, target, message, and `SENT` receipt binding plus browser-side
exact-message evidence. No manual dispatch or browser route can substitute for
that evidence.

## Prohibited-action audit

No product source, wake policy/config/state, browser profile/storage, Secure
MCP/tunnel runtime, Scheduler, daemon/release state, Git history, or external
project was changed. No `catdesk_wake_bridge_run_once`, SeleniumBase, Python
wake script, browser tool, target setter, tunnel operation, daemon/reload,
release action, Git publication, signing, or elevation was invoked.

## Verification and attribution

The contract permits only `GIT_STATUS` and `GIT_DIFF` capture for this
documentation-only canary. The sole task-attributable workspace mutation is
this bundle; all pre-existing dirty worktree content is preserved. Independent
final review is requested before treating the normal completion event as a
natural-wake acceptance candidate.

`git diff --check` completed without output. `git status --short` reported 175
existing/current dirty entries; this bundle is the only canary-attributable
entry and is untracked pending normal CatDesk completion handling.

## 2026-09-15 post-canary observation

The automatic dispatcher later recorded this exact R3 event as `SENT`, with the
canonical target digest and exact wake-message digest. The operator then
reported that no corresponding wake message is visible in the canonical
conversation. Therefore that `SENT` record is **not** accepted as end-to-end
natural-delivery proof.

Source inspection found a concrete false-positive receipt path in the Python CDP
adapter: `confirm_exact_receipt` could accept two separated observations of the
same post-submit document without any server-backed navigation round trip. A
client-side optimistic user-message append could therefore satisfy the old
receipt condition even if the message did not persist. R4a now requires a
same-profile reload of the exact canonical conversation, proof that a fresh JS
document replaced the submitting document, and only then exact-message receipt
confirmation. A fresh legitimate natural event remains required after that
repair is reviewed and promoted; hourly fallback is never acceptance evidence.
