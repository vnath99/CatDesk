# Independent CatDesk Wake convergence

## Current authority (2026-09-14 UTC)

The operator's direct Codex handoff supersedes older T-0324 continuation and
hourly-deadman instructions. Ordinary autonomy remains frozen. The sole desired
browser target is `https://chatgpt.com/c/6aa564cc-920c-83e9-a42e-4c0f6d5b944d`.
Remote Git publication is not authorized. Preserve the dirty worktree, historical
receipts, existing release authority, and externally owned Secure MCP runtime.

## Observed baseline

- Actual populated worktree: `<USER_PROFILE>\OneDrive\Desktop\Projects\CatDesk-codex-loop`.
  The desktop thread's `CatDesk_v2` directory contains only Git metadata.
- Git HEAD: `b958eb9fff4522168ebb1ae4a726209896a27451`; substantial pre-existing
  tracked and untracked modifications. HEAD alone does not identify current source.
- Daemon PID 47192 serves `.catdesk/verification-targets/autonomy-release/release/catdesk.exe`;
  measured SHA-256 `e4f8561c3eb42f5da93ad9f1f0c623976ee21f4a97c2adde9886e4c720a21183`.
- No separate stable-wake host/owner or GUI process was observed.
- Python PIDs 43152/9472 still run the legacy one-shot bridge for historical
  record `review-adc-t0389-t0324-sr3k-first-pair-evidence-bootstrap-closure-20260911-7-independent_final_review`.
  The legacy lock is held. Their command targets an older conversation; current
  config independently targets the obsolete WEB conversation. Neither is correct.
- Canonical inbox contains 411 records (354 COMPLETED_VERIFIED, 57
  WAITING_FOR_CHATGPT). These counts are state totals, not pending/stale classifications.
- Existing host executable is a read-only one-shot readiness evaluator, not a
  resident service. Existing GUI is a separate mode of catdesk.exe launched by
  CatDesk start/recover, explaining why daemon operation does not ensure visibility.
- T-0302 explicitly records source readiness only and does not claim visible GUI
  acceptance. Existing Rust delivery and browser primitives contain valuable
  claim, exact-target, submission-boundary and receipt checks to preserve.

## Bounded implementation sequence

1. Introduce a separately versioned Wake package and small protocol-v1 client:
   bounded events, durable atomic spool, immutable event IDs and target generation.
2. Implement Wake-owned config with atomic CAS, delivery journal and restart-safe
   claims. Unknown submission state must never trigger automatic replay.
3. Add independent resident host, native GUI, lifecycle controls and diagnostics;
   install under `%LOCALAPPDATA%\CatDeskWake`, outside build/source directories.
4. Reuse reviewed browser primitives with the independent profile/runtime and
   explicit durable before-submit handshake. Preserve narrower attention reasons.
5. Integrate CatDesk event publishing and a supported legacy-owner quiescence
   transaction. Preserve historical state read-only; never activate dual owners.
6. Verify crash, target-change, GUI and daemon independence; conduct visible
   Windows acceptance and a legitimate natural review-event delivery.
7. Promote only after exact current-chat receipt evidence; record hashes, review,
   tests, migration and rollback. Source/unit-test completion is not live acceptance.

## Acceptance state

NOT ACCEPTED. No new runtime is installed or activated, no target was changed,
and no browser submission was invoked during this inventory. Operator login,
CAPTCHA or other attention must be established from direct browser evidence,
not inferred from generic provider status.
