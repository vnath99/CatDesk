# T-0159 / T-0154-R3 - Reviewed LKG Authority Binding

## Finding corrected

T-0158 allowed a legacy LKG escrow migration from a native daemon-reload
receipt after path/hash/PID/listener continuity checks. That receipt proves a
replacement process identity and pending transport reconnection only. It has
no independent reviewed-promotion authorization and therefore cannot mint
rollback authority. Local MCP health, a listener, and the presence of a review
bundle are likewise not release approval evidence.

## Authority contract

The only migration inputs are:

1. an already-valid alternating-slot LKG escrow; or
2. `.catdesk/promotion-recovery/reviewed-promotion.json`, written by the
   reviewed promotion success boundary.

The latter has an exact bounded schema: schema version 1, a 32-hex promotion
transaction ID, `CANONICAL_HANDOFF_PROVEN`, the exact candidate SHA-256, the
fixed relative canonical path `target\release\catdesk.exe`, and the same exact
canonical SHA-256. It is accepted only when the current canonical binary and
sidecar independently validate to that hash. Missing authority reports
`LKG_AUTHORITY_MISSING`; malformed, unsafe, or ambiguous authority reports
`LKG_AUTHORITY_AMBIGUOUS_OR_DAMAGED`. Neither outcome writes an escrow slot or
pointer.

Promotion now writes a schema-2 durable transaction with an immutable
transaction ID. Only after the candidate is the exact canonical pair and the
final canonical listener handback is proven does it write and validate the
reviewed-promotion authority record, then persist the alternating-slot LKG
escrow, and finally clear the active transaction. A crash before that boundary
cannot make the candidate LKG. A crash after authority persistence remains
bound to that one canonical hash and can complete through the reviewed
transaction/recovery rules.

Healthy canonical lifecycle repair remains distinct: a valid on-disk
canonical pair can restart/reattach a mismatched daemon without LKG escrow;
that restart does not create rollback authority. Broken canonical pairs still
require an existing LKG or exact interrupted reviewed-promotion evidence.
Canonical-recovery attempt generation, pending-worker ownership, and terminal
rearm semantics from T-0158 are unchanged.

## Changed files

- `scripts/catdesk-release-recovery.ps1`: replaces native-convergence migration
  with strict reviewed-promotion-record validation.
- `scripts/start-catdesk-stack.ps1`: attempts optional LKG migration only from
  reviewed-promotion authority and keeps healthy lifecycle restart independent.
- `scripts/promote-reviewed-catdesk-build.ps1`: writes the transaction ID and
  authoritative completion record only after exact canonical handback, then
  saves LKG; it no longer snapshots an unreviewed pre-swap canonical pair.
- `scripts/test-start-catdesk-stack.ps1`: covers native receipt and review-file
  rejection, exact reviewed-record acceptance, and wrong-hash rejection.
- `scripts/test-promote-reviewed-catdesk-build.ps1`: asserts the durable
  reviewed-promotion record produced by the successful production path.

## Deterministic evidence

Executed locally:

- `powershell -NoProfile -ExecutionPolicy Bypass -File scripts/test-promote-reviewed-catdesk-build.ps1` — passed.
- `powershell -NoProfile -ExecutionPolicy Bypass -File scripts/test-start-catdesk-stack.ps1` — passed.
- `powershell -NoProfile -ExecutionPolicy Bypass -File scripts/test-catdesk-lifecycle.ps1` — passed.

The fixtures cover rejected native receipt-only and review-bundle-only legacy
migration, exact record/hash migration, wrong-record rejection, bounded
escrow persistence after exact promotion, and retained native/legacy receipt
continuity behavior for Phase-1 promotion handoff only.

## Host acceptance sequence

For a legacy installation with no LKG escrow, first establish it through the
supported reviewed-promotion control plane; do not use daemon reload evidence
as a migration substitute. Then use the supported recovery acceptance surface,
verify the bounded local-MCP and official-runtime result, and inspect only the
fixed redacted durable recovery status. No live promotion, reload, tunnel,
browser, Scheduler, external-project, or Git-publication action was performed
for this implementation and test work.
