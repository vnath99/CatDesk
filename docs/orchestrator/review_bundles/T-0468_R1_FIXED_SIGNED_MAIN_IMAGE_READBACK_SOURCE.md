# T-0468 R1 — Fixed read-only signed main-image status source

**Date:** 2026-10-10. **Canonical:** CatDesk_chat50, `https://chatgpt.com/c/6aca50a3-0da0-83ea-8358-dbc128c4f9ad`.

**Status:** SOURCE IMPLEMENTED; FOCUSED TESTS PASS; LIVE HOST EXECUTION, INDEPENDENT REVIEW AND DEPLOYMENT NOT AUTHORIZED.

## Problem and narrow scope

T-0467 `COMPLETED_VERIFIED` (final review `review-adc-t0467r1-signed-bootstrap-readiness-20261010-9-independent_final_review` ACKed) classified signed rotation as BLOCKED on one factual gap: the current accepted signed epoch and exact installed main-image identity are not observable through the old CatDesk MCP runtime. Historical T-0216 epoch-1 installation and T-0217 unsigned epoch-2 signing payload cannot establish current anti-rollback state. The reviewed T-0464 isolated Cargo bootstrap and T-0465 candidate-specific reload approval are not signed product-root installed-image authority.

This source patch adds only a future **local, zero-parameter, read-only** fixed-policy command to answer that question from the existing product verifier. It creates no new signing, elevation, trust-root, review, release, or installation capability and does not bypass T-0436 reviewed reload.

## Source changes and authority boundary

- `src/reviewed_build.rs`: fixed `--catdesk-reviewed-main-image-status-fixed-policy` flag and exact-only parser (any extra argument, second flag, caller-provided path, epoch, output or token is refused).
- Windows command `run_reviewed_main_image_status_command` reuses only `production_reviewed_main_image_trust_root`, `read_accepted_reviewed_main_image_envelope`, `read_optional_reviewed_main_image_rotation_envelope` and the existing no-follow fixed destination opener, `evidence_from_open_regular`, and `verify_reviewed_main_image_payload_binding`. All envelopes are authenticated by the compiled Ed25519 public root and fixed purpose/policy checks before their epochs can participate in status output. **No private key, raw signature, envelope contents, variable path, or host-wide inventory is accepted or returned.**
- `select_signed_main_image_readback` chooses only the installed rotation receipt (if any), otherwise the accepted first-image receipt; never treats a merely pending higher epoch as installed. It rejects installed/pending epochs not strictly above first bootstrap epoch and conflicting/stale same-epoch or older pending receipt combinations. `SIGNED_MAIN_IMAGE_READBACK state=VERIFIED` reports only first-image, installed rotation, pending rotation epochs (0 means no receipt), installed payload SHA-256 and byte length. Non-Windows execution refuses with the existing transport-unavailable code.
- The actual installed image is opened **read-only, with no-follow path checks and no write/delete sharing**; its measured bytes must match the selected signed receipt's SHA and length. The receipt records are re-read and compared and the image remeasured with the same retained handle before success, to fail closed if concurrent on-disk rotation changes the observed state.
- `src/main.rs` dispatches the exact fixed flag before mutation-oriented bootstrap/rotation flags. The new path returns text or a bounded existing failure code and does not call any mutator, start a daemon, contact the browser, change WakeHost, alter Secure MCP, or write protected records.

## Verification and status

- `cargo fmt --all`: PASS.
- `cargo test --locked --offline --bin catdesk signed_main_image_readback -- --nocapture`: **2/2 PASS**, covering exact fixed flag/no caller-controlled extras, source non-mutating wiring, predecessor/current/pending selection, stale/conflicting receipt refusal.
- `cargo clippy --locked --offline --all-targets --all-features -- -D warnings`: PASS.
- `git diff --check`: PASS.
- **No host command invoked.** The old installed CatDesk controller does not support this new flag, and the current workspace binary is not a product-root signed installed image. The diagnostic must first pass independent source review and then be invoked through an explicitly approved read-only host/administrator context that does not replace or start a daemon. Never launch an unreviewed scratch executable simply to read protected receipts.
- This patch does **not** resolve the one-time signed image/bootstrap transition. It only prepares a future safe observation mechanism. It is not proof that the protected host's accepted receipt is epoch 1, that epoch 2 is unused, or that any signed T-0299+ rotation payload exists.

## Next gates

1. Complete full Windows CI for this source commit and obtain independently reviewed, task-attributable source verification for T-0468. Do not claim acceptance from this self-authored report.
2. Review how to run this exact status mode via an authorized host source/image and the existing protected product public root, preserving Program Files/ProgramData read controls. If unavailable to CatDesk, request narrowly scoped operator readback rather than a generic shell/legacy reload.
3. Only once signed epoch + installed-image receipt/provenance have been verified, select an immutable reviewed new main-image candidate, produce the exact canonical unsigned payload, obtain *external offline* product-root signing and separately reviewed/authorized administrator fixed-policy rotation. No private signing key in CatDesk, ChatGPT, GitHub or MCP.
4. After serving parity, resume T-0463 short Cargo-home worker under source-current controller for a **fresh** reviewed protected build and require `BUILD_ATTESTED`, promotion, nine-layer recovery acceptance and guarded paired Chat50 URL/SHA target generation >=32 before WakeHost restart.

**Non-actions:** no live signing/readback, private-key handling, Program Files writes/reads, image installation/reload, protected reviewed build, LKG/promotion, official tunnel change, Wake send/start, or historical file deletion.
