# T-0367 — T-0324 ChatGPT main-image designation

Date: 2026-09-09

## Decision

ChatGPT independently designates the T-0366 frozen/locked candidate as the next CatDesk product-root signing candidate for T-0324. This decision does not rely on the autonomous controller's `COMPLETED_VERIFIED` state as release authority.

Designated source evidence:

- frozen source root: `.catdesk/candidates/t0366-source`
- source domain: exact `Cargo.toml`, `Cargo.lock`, and recursive `src/**`; `build.rs` was absent at freeze time
- source file count: 90
- canonical frozen-source manifest SHA-256: `8ac4015fb238c09cc1444f7867b280a6ba90f7529fa75feb1e1af75907e2b562`
- manifest: `.catdesk/candidates/t0366-source-manifest.txt`
- freeze-time workspace-to-copy comparison: exact for all 90 files
- post-build workspace-to-copy comparison and manifest recomputation: exact

Designated executable candidate:

- path for offline evidence only: `.catdesk/candidates/t0366-build/release/catdesk.exe`
- SHA-256: `d09c677f8ce5f14b3600a5435929aff31b4128d9a041cdaceae3213b5f0af36f`
- length: `26302464`
- build command: locked release build from the frozen source root, not from mutable `target/release`

This candidate is not yet an installed/reviewed main image. Product-root signature verification and the fixed administrator rotation consumer remain mandatory.

## Purpose and epoch designation

The required purpose is `reviewed-main-image-rotation`, not first-image bootstrap. Durable accepted project history establishes that T-0216 successfully installed the signed epoch-1 bootstrap image at the fixed Program Files destination. Current production source also structurally refuses first-image bootstrap when that destination exists and provides a distinct monotonic signed rotation state machine for replacement.

ChatGPT designates **epoch 2** for this candidate. The durable project history contains one accepted signed epoch-1 image and only an unsigned historical T-0217 epoch-2 signing request; no accepted epoch-2 rotation is recorded. The legacy serving generation cannot expose a safe read-only protected receipt query, so protected-host state cannot be freshly observed through the present control surface. This uncertainty does not weaken rollback safety: the fixed rotation consumer independently derives the authenticated predecessor from protected signed receipts and refuses any incoming epoch less than or equal to a newer predecessor. Therefore, if unobserved protected state contradicts the durable history, this epoch-2 candidate fails closed before replacement rather than rolling back the installed image.

Current rotation policy SHA-256, recomputed from the exact current compiled policy constants, is:

`c477d5bc486002e2034324b3a40b07d8e87c149cf5f2dab9b12aafee92816705`

It matches the reviewed T-0217 policy digest.

## Canonical unsigned signing payload

The exact canonical bytes designated for product-root signing are persisted at:

`docs/orchestrator/review_bundles/T-0366_T0324_ROTATION_SIGNING_PAYLOAD.txt`

Binding:

- product: `CatDesk`
- purpose: `reviewed-main-image-rotation`
- root id/version: `catdesk-main-image-root` / `1`
- epoch: `2`
- policy SHA-256: `c477d5bc486002e2034324b3a40b07d8e87c149cf5f2dab9b12aafee92816705`
- payload SHA-256: `d09c677f8ce5f14b3600a5435929aff31b4128d9a041cdaceae3213b5f0af36f`
- payload length: `26302464`
- review id: `review-t0366-t0324-main-image-20260909`
- build id: `build-t0366-d09c677f`

The payload has LF-only canonical lines, one trailing LF, and intentionally contains no `signature=` line.

## Independent source/policy review

ChatGPT independently reviewed the current production rotation implementation around `reviewed_main_image_rotation_predecessor`, `classify_reviewed_main_image_rotation`, and `execute_reviewed_main_image_rotation_as_administrator`. The consumer authenticates the bootstrap receipt and any installed rotation receipt against the compiled product root, requires a strictly newer epoch for replacement, binds the incoming payload by exact SHA-256/length from an opened non-reparse object, persists pending state before replacement, and validates the installed object before advancing the installed rotation receipt.

The focused current-source regression `cargo test t0217_rotation_envelope_is_distinct_from_first_image_bootstrap -- --nocapture` passed 1/1 on 2026-09-09, confirming that bootstrap and rotation retain distinct purpose/policy bindings.

## Governance

ChatGPT remains the main-image/version designation authority. Codex/Terra-high may prepare implementation/build/review evidence but does not select release bytes, purpose, epoch, or signing payload. Future image rotation is required only when an accepted executable/control-plane delta must cross into the deployed generation; documentation-only changes do not trigger rotation.

The production private Ed25519 signing key remains outside CatDesk, MCP, ChatGPT, Codex, Qwen, and the repository. Nothing in this designation authorizes reading, exporting, copying, displaying, or importing that key. Only a closed non-exporting signer operation may be used if such a reviewed capability exists. Otherwise the detached-signature operation remains the sole external cryptographic boundary.

No ProgramData/Program Files staging, bootstrap, rotation, promotion, reload, recovery, Secure MCP/tunnel mutation, browser wake, protected target mutation, Scheduler/service mutation, Git publication, reset, or worktree cleanup was performed by this designation.
