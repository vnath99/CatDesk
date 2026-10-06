# T-0368 — T-0324 Signed Main-Image Rotation Operator Boundary

Status: **PREPARED; WINDOWS UAC ELEVATION REQUIRED FOR LIVE ROTATION**

## Purpose

T-0368 reduces the remaining T-0324 host mutation to one fixed-purpose administrator action. The product-root signature has already been independently verified and persisted as the canonical signed T-0366 envelope. This helper does not create or choose release authority; it consumes only the exact ChatGPT-designated T-0366 candidate and exact signed envelope.

## Fixed authority inputs

- candidate: `.catdesk/candidates/t0366-build/release/catdesk.exe`
- candidate SHA-256: `d09c677f8ce5f14b3600a5435929aff31b4128d9a041cdaceae3213b5f0af36f`
- candidate length: `26302464`
- signed envelope: `docs/orchestrator/review_bundles/T-0366_T0324_ROTATION_SIGNED_ENVELOPE.v1`
- signed-envelope SHA-256: `42ababb6dcd47ec20db97775d7d441eb9fcadc806d3f304b7f221809bfda892c`
- signed-envelope length: `507`
- fixed rotation command: `--catdesk-reviewed-main-image-rotate-fixed-policy`
- accepted result: `REVIEWED_BUILD_MAIN_IMAGE_ROTATED_PENDING_T0212_ACCEPTANCE`

## Helper

`scripts/t0368_apply_signed_main_image_rotation.ps1`

The helper is intentionally closed over the exact T-0366 hashes and lengths. It:

1. refuses a missing or changed candidate/envelope;
2. requests Windows elevation if and only if the current token is not administrator;
3. creates only the compiled fixed incoming ProgramData directory;
4. refuses conflicting pre-existing fixed incoming files rather than overwriting them;
5. stages through a fixed `.t0368.next` sibling and re-measures before/after the rename;
6. revalidates both fixed incoming objects immediately before execution;
7. invokes the exact measured T-0366 candidate with only the zero-parameter reviewed rotation flag;
8. requires the accepted rotation receipt;
9. re-measures `C:\Program Files\CatDesk\CatDesk.exe` and the installed rotation envelope against the designated values.

The helper does not alter Secure MCP, browser/wake state, Scheduler, services, Git, the dirty worktree, candidate designation, signing material, epoch, purpose, policy, or arbitrary Program Files/ProgramData paths.

## Why operator action is now genuinely required

The currently serving pre-T-0299 MCP deliberately does not expose signed main-image rotation, and CatDesk's MCP shell policy rejects nested/unrestricted PowerShell. The reviewed rotation implementation itself deliberately requires an administrator token. Therefore ChatGPT can prepare and audit the exact fixed-purpose action but cannot supply the Windows UAC consent through the current safe control surface.

The remaining operator action is limited to running the prepared helper from the CatDesk project and approving its one Windows UAC prompt. No release decision, hash, path, epoch, purpose, payload, or signature choice is delegated to the operator.

## Post-rotation continuation

After successful elevation/rotation, ChatGPT should independently revalidate Secure MCP without taking ownership, prove the serving generation/tool catalog changed to the T-0366 image, reconcile stranded T-0322 cancellation state, run the bounded Qwen 3.8 continuation canary, and resume T-0223 host-live acceptance. Wake-target rebinding remains forbidden until the reviewed paired project-target+wake-target authority is live.
