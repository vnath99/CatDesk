# T-0371 — Exact stale rotation transport reconciliation

Status: **PREPARED; UAC EXECUTION PENDING**  
Date: 2026-09-10

## Trigger

T-0370 crossed the waited UAC boundary correctly but failed closed before changing installed authority:

`Candidate existing fixed input length mismatch: expected 26302464, found 25172480`

The observed fixed incoming candidate length `25172480` exactly matches the historical T-0217 unsigned rotation candidate. Accepted T-0217 evidence binds that candidate to SHA-256:

`552cb917998d283e7d901593ffed5a9ceaa108223654aa69349fa7e567c21482`

The installed Program Files image previously measured `25134592`, exactly the accepted T-0215 epoch-1 image length; T-0215 binds that predecessor to SHA-256:

`2421a90aeb9ad7775ec9927f8dfbe294faa3cbfe11ec7a071a1ed554eee28459`

This distinguishes a stale ProgramData incoming transport object from accepted installed rollback authority.

## Security disposition

The T-0215/T-0217 threat model explicitly defines the fixed ProgramData incoming directory as transport only. T-0371 therefore permits reconciliation only for exact independently known historical transport bytes and never for unknown content.

The new fixed-purpose helper is:

`scripts/t0371_reconcile_stale_rotation_transport_and_apply.ps1`

Before any transport mutation it requires the installed image to be either:

- exact designated T-0366 candidate `d09c677f8ce5f14b3600a5435929aff31b4128d9a041cdaceae3213b5f0af36f`, length `26302464`; or
- exact accepted T-0215 predecessor `2421a90aeb9ad7775ec9927f8dfbe294faa3cbfe11ec7a071a1ed554eee28459`, length `25134592`.

For the fixed incoming candidate, a conflicting existing file may be superseded only if both SHA-256 and length match the historical T-0217 candidate above. For the fixed incoming envelope, a conflicting existing file may be superseded only if it exactly matches the historical unsigned T-0217 422-byte signing payload. That payload's SHA-256, recomputed from the exact repository bytes, is:

`eaad9c085257a9fa459d2f4771f69c360dda6930b120778c2efeaaa5d2e8a308`

Recognized stale inputs are renamed to timestamped `.superseded-t0217-*` evidence siblings instead of being deleted. Unknown or differently measured bytes fail closed before mutation.

The helper does not delete or manually rewrite accepted/pending/installed Program Files receipts. After exact transport reconciliation it stages only the already signed T-0366 candidate/envelope, remeasures both, and invokes only the reviewed zero-parameter command:

`--catdesk-reviewed-main-image-rotate-fixed-policy`

The Rust rotation state machine remains the authority for signature verification, predecessor binding, monotonic epoch, pending-recovery state, staging, atomic replacement, installed receipt persistence and refusal behavior.

## Result evidence

T-0371 waits for its elevated child using `Start-Process -Verb RunAs -Wait -PassThru`, writes `.catdesk/t0371-rotation-result.json`, and does not report success unless the installed Program Files image and installed rotation envelope exactly match the designated T-0366 hashes/lengths. Terminal success marker:

`T0371_ROTATION_VERIFIED_SUCCESS`

## Safety

No raw reload, service/Scheduler change, Secure MCP/tunnel replacement, browser wake, protected wake-target edit, Git publication, private-key access, candidate regeneration, or worktree cleanup is part of T-0371. The live legacy 77-tool serving daemon remains untouched until the signed Program Files image is proven installed.
