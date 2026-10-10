# T-0460 R1 — Source-Current Guarded Paired Wake-Target Recovery Review

**Date:** 2026-10-09
**Review session:** `adc-t0460r1-chat55-paired-target-source-review-20261009`
**Task:** `T-0460-R1-PAIRED-TARGET-SOURCE-REVIEW`
**Exact committed source checkpoint:** `7f0b30f9b1b1f1bc4fc221a1166f25f9a55e4e00`
**Source decision:** **PASSED for source-level fail-closed design and formal independent final-review verification.** This document neither approves nor performs live installation, protected build promotion, daemon reload, unpaired target mutation, or Wake submission.

## Problem and evidence

The older serving controller reports `CONNECTED_VERIFIED/READY` with 93 tools, but its CatDesk project registry still stores the predecessor URL `https://chatgpt.com/c/6ac6cbe8-6f0c-83ea-9f7d-13489d4d87f5` and invalid digest `8eb1e045f231df3214c16d7f97b382511399deef2e62b8ff32b3cdbeea004eb3`. The independent installed WakeHost `1.0.0-dev.84` at generation 31 stores the **same** predecessor URL with valid SHA-256 `3a1d4cc69dfa3d05cb2bc33ef1197ac5a7433a120cfb4214a6996edb5a9d29e9` and the historical `manual-wake-mcp-1791459602150` remains ambiguous `SUBMITTING`.

The assistant verified October 8's successful generation-30→31 rollover used the **existing** `autonomy_project_registry_bind` `DESIGNATED_CHAT_TARGET_URL=...` path. Repeating this exact guarded control on October 9 with the independent old SHA failed `INVALID_ARGUMENT`; current `catdesk_daemon_reload RESULT` also returned `INVALID_ARGUMENT`. Do not infer these calls changed the live target.

## Source review

1. **Existing paired authority retained:** `src/mcp.rs::operator_update_designated_chat_target` canonicalizes new ChatGPT URL, validates the supplied 64-hex old SHA, takes the global `WAKE_TARGET_SET_LOCK`, compares against independent Wake old URL digest, and follows the existing quarantine-capable Wake CAS → project CAS → compensation and readback transaction.
2. **Specific pre-existing mismatch recovery only:** `src/delegated/autonomy_projects.rs::reconcile_catdesk_digest_with_wake` is reachable only when normal readback returns `ProtectedStateMismatch` and the independent Wake old URL SHA equals caller-supplied old CAS digest. It checks canonical URL, project identity and canonical workspace, exact old URL equality, a syntactically valid but unequal stored digest, unique CatDesk project record, and a 1 MiB registry size limit. It validates the *entire* registry after changing only the existing CatDesk digest, under registration lock, and persists through existing validated store writer. It is not a generic write primitive.
3. **CLI reaches the guarded code:** `src/binagotchy_cli.rs::update_target` now handles exactly `ProtectedStateMismatch` by reading the independent Wake predecessor URL + SHA and validating their integrity via `validated_wake_predecessor_digest`, then calls the same `operator_update_designated_chat_target`. No other readback error is converted into permission to mutate.
4. **Quarantine and failure handling:** A prior event in `SUBMITTING` is quarantined on its historical immutable generation by the existing designated rollover. If Wake CAS succeeds but registry CAS fails, the existing compensation attempts restoration; ambiguous results are reported as `SynchronizationFailure`, never success. If the initial normalization succeeds but subsequent paired mutation fails, the old URL with corrected digest can remain; that is coherent but **not** success for Chat55. Independent post-mutation readback is mandatory.
5. **Safety/ownership preserved:** No browser scripting, auth profile, process launcher, tunnel takeover, target-only registry setter, external MCP runtime change, force-push, or old Wake event replay is introduced.

## Tests and evidence

- `src/mcp.rs::designated_chat_update_repairs_only_corrupt_digest_matching_independent_wake` exercises digest-only corruption, guarded repair, paired rollover and readback.
- `src/mcp.rs::designated_chat_digest_recovery_rejects_other_corruption_without_mutation` exercises divergent old target URL rejection with no registry or Wake change.
- `src/binagotchy_cli.rs::rollover_fallback_only_accepts_canonical_integrity_verified_wake_identity` exercises valid independent predecessor and invalid digest/host rejection.
- GitHub Windows CI for `b3585ad`, `0b31b05`, and current `7f0b30f`: three jobs completed successfully including root Rust format/Clippy/tests, independent WakeHost strict Clippy/tests and full offline Python suite. Runs: `37992094331`, `37995124770`, `37995569502`.
- Local source-current `cargo build --bin catdesk`, `cargo fmt`, and staged diff validations passed previously. A development image is **not** a reviewed serving-release image.

## Decision and next authorized actions

**Source review: PASS. Protected deployment/operational acceptance: NOT YET PROVEN.**

1. Complete first-class direct-work independent final-review verification of this exact source snapshot and review artifact; obtain the exact `COMPLETED_VERIFIED` record and ACK it before any protected reviewed build.
2. Use only the matched review record to `catdesk_reviewed_build PREPARE/CONFIRM`; require `BUILD_ATTESTED`, then reviewed promotion, canonical parity, reviewed daemon-reload and independent serving-image identity readback. The historical T-0419 protected `ring/cc-rs` build failures are a separate unresolved gate; do not reuse failed attestations.
3. Once the new source actually serves CatDesk, use `autonomy_project_registry_bind` with `projectId=catdesk`, `decision=DESIGNATED_CHAT_TARGET_URL=https://chatgpt.com/c/6ac823ac-91b8-83e9-8996-f639c461de50`, `expectedSha256=3a1d4cc69dfa3d05cb2bc33ef1197ac5a7433a120cfb4214a6996edb5a9d29e9`, rather than the unpaired setters.
4. Require project registry and independent WakeHost separately report URL `https://chatgpt.com/c/6ac823ac-91b8-83e9-8996-f639c461de50` with exact SHA-256 `fc92062981985bb64c392ff9bdf1663f5ae3e975b2b7d0d0286092b97484cde7` and Wake generation >=32. Only afterward queue one fresh manual diagnostic and verify its own exact receipt; manual debug is not natural acceptance.
5. The current hourly deadman remains an independent fallback and cannot substitute for verified event-driven Python browser Wake. Do not hand the user a direct registry-edit workaround or claim already-bound success.
