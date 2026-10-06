# T-0370 — T-0324 waited signed main-image rotation retry

Status: **IMPLEMENTED; HOST UAC EXECUTION PENDING**  
Date: 2026-09-10

## Trigger

The operator ran the read-only T-0369 verifier after the first T-0368 UAC attempt. It failed closed because the installed fixed image `C:\Program Files\CatDesk\CatDesk.exe` measured `25134592` bytes instead of the independently designated T-0366 length `26302464`. Therefore T-0368 did not establish installation of the signed T-0366 image and no serving restart/recovery may rely on that attempt.

## Root cause

T-0368's non-administrator path used `Start-Process -Verb RunAs` without `-Wait` and without a durable child-result receipt. The parent shell could return immediately after the UAC launch, leaving the elevated child's success/failure unobserved. This is an evidence/control defect in the helper, not a reason to weaken the reviewed rotation state machine or change the designated image.

## Corrected fixed-purpose helper

`scripts/t0370_apply_signed_main_image_rotation_waited.ps1`

The helper is closed over the already-designated authority:

- candidate SHA-256: `d09c677f8ce5f14b3600a5435929aff31b4128d9a041cdaceae3213b5f0af36f`
- candidate length: `26302464`
- signed envelope SHA-256: `42ababb6dcd47ec20db97775d7d441eb9fcadc806d3f304b7f221809bfda892c`
- signed envelope length: `507`
- rotation flag: `--catdesk-reviewed-main-image-rotate-fixed-policy`
- required success receipt: `REVIEWED_BUILD_MAIN_IMAGE_ROTATED_PENDING_T0212_ACCEPTANCE`

Behavior:

1. Revalidates exact project-side candidate and signed envelope before requesting elevation.
2. Requests one UAC elevation and waits for the elevated child with `-Wait -PassThru`.
3. Removes only its own stale workspace result receipt before launch; it does not delete protected host state.
4. Reuses a fixed incoming candidate/envelope only when their exact hash and length already match; conflicting bytes fail closed.
5. Invokes only the exact designated T-0366 candidate with the zero-parameter reviewed rotation flag.
6. Requires the accepted rotation result, then re-measures the installed Program Files image and installed signed envelope.
7. Writes `.catdesk/t0370-rotation-result.json` with bounded success/failure evidence and returns `T0370_ROTATION_VERIFIED_SUCCESS` only after exact installed verification.

The reviewed rotation implementation remains responsible for authenticated predecessor/epoch/pending-state reconciliation. T-0370 does not bypass, rewrite, or delete those controls.

## Live state before retry

- Secure MCP: `CONNECTED_VERIFIED`
- local MCP: `READY`
- externally owned official runtime preserved
- serving catalog: legacy 77 tools
- delegated-run inventory: empty on current serving surface
- installed fixed image observed by T-0369: `25134592` bytes, not T-0366

## Remaining sequence

1. Operator runs the corrected helper and approves the single Windows UAC prompt.
2. Require `T0370_ROTATION_VERIFIED_SUCCESS` or consume the exact durable failure message; do not infer success from window behavior.
3. Re-run exact read-only installed image/envelope verification.
4. Only after exact installation proof, transition/recover serving authority while preserving the external Secure MCP runtime and prove new catalog/generation parity.
5. Reconcile T-0322, run the Qwen 3.8 continuation canary, and continue T-0223 and the remaining acceptance queue.

No raw reload, manual binary copy, direct promotion-script authority, protected-state deletion, browser wake, wake-target edit, Secure MCP replacement, Git publication, private-key access, or dirty-worktree cleanup is authorized by this ticket.
