# Routine source review authorization

The October 25 standing user instruction permits bounded, read-only source
reviews for previously registered and verified project conversations. Use public
source or explicitly authorized private source, preserve native project work,
and never publish secrets. This instruction is not a platform permission bypass.

The first implementation uses exact frozen message batches in the **existing
project WebWork ledger**, `.catdesk/web-work/<project>.json`. There is no second
project registry. Each grant records the action category, exact paired URL,
generation and digest, approval-evidence digest, expiration, message-count and
execution limits. A message records its request ID, event ID, wire digest and
`reviewKey`: a digest of the material review scope (repository, base/head commits
and fixed review template). Do not include timestamps or generated request IDs
in that material scope. A new C-number must not evade an existing review key.

## Supported local operator commands

Use a qualified controller containing this change. Existing installed/selected
versions do not gain these commands merely because source tests pass.

```text
<controller> operator web review authorize --project-id <id> --input-file <grant.json>
<controller> operator web review preflight --project-id <id> --input-file <review.json>
<controller> operator web review run --project-id <id> --input-file <review.json>
<controller> operator web review resume --project-id <id> --input-file <review.json>
<controller> operator web review revoke --project-id <id> --input-file <revoke.json>
<controller> operator web review reconcile --project-id <id> --event-id <existing-submitting-id>
```

Grant input is `{projectId, authorization}`. `authorization` contains `grantId`,
`category: "READ_ONLY_SOURCE_REVIEW"`, `target: {generation,url,digest}`,
`expiresUtc` (Unix seconds), `maxMessages` (1–32), `maxExecutionSeconds` (1–180),
`approvalEvidenceSha256` and `messages`. Each message has `requestId`, `eventId`,
`messageSha256`, and `reviewKey`, all immutable. Expiration is at most 31 days
from grant creation. A batch may contain more messages than its allowed count.

Review input is `{projectId, grantId, reviewKey, request}`. `request` is the existing
Web work input: `{projectId, requestId, expectedTargetGeneration,
expectedTargetSha256, taskText}`. Freeze the complete rendered wire payload
before authorizing its digest. `taskText` uses the existing normalization and
1024-byte limit. Revocation input is `{projectId, grantId}`. Revocation preserves
the original grant and every reservation; it cannot be undone by replaying the
grant. It remains available after a target change or rollover quarantine.

## Delivery and recovery

Preflight checks the current paired registry/Wake target, rollover admission,
activation/history cutoff, existing work/events and outstanding response timers.
It refuses changed payloads, destinations, expired/revoked/exhausted grants and
previously reserved requests. It does not launch a browser or grant platform
permission. Only grant creation/revocation writes consent state.

Run reserves a message once under the existing project registration lock, then
publishes its existing stable WebWork event with a typed grant reference. It
uses stopped-host ownership and the shared browser lock for one event only,
with an absolute execution deadline. It never starts the recurring scheduler,
changes Wake control state, or runs native project code. Ordinary Wake scheduling
cannot submit an unsent scoped-review event. Immediately before submission,
the operator rechecks the exact grant, target and payload and durably consumes
the dispatch before Wake commits `SUBMITTING` and sends one acknowledgment.

A failure does not refund the reservation. Explicit `resume` may continue the
same reserved identity only before its durable dispatch-start record, with a
still-valid grant, no conflicting work, no timer and no `SUBMITTING`/`SENT`
delivery. It neither spends another count nor clears attention. Generic work
publication cannot take over an ID listed by a review grant. Inspect the existing work ledger,
Wake delivery, timer and receipt. `SUBMITTING` or an unknown outcome is never
permission to retry. Use existing read-only review observation and supported
reconciliation; retain predecessor history. A confirmed pre-submit failure is
also held for an explicit decision, not automatically resubmitted. Completed
response observation and work resolution remain separate from authorization.

The explicit `reconcile` command handles one existing `SUBMITTING` source-review
event with the host stopped. It checks the current paired target, exact event
and message, and original consumed grant under the existing registry lock. An
expired or revoked grant may prove historical dispatch; it never grants another
send. The existing cooperative adapter reads for an exact persisted receipt,
without typing, pressing Enter, clicking Send or invoking response retry. Its
90-second browser/cleanup budget is absolute. A negative or unavailable read
retains the unknown outcome; `SCOPED_RECONCILE_RECEIPT_UNPROVEN` does not prove
that the message was never submitted.

Only positive receipt evidence permits the existing timer and `SENT` transition.
If no timer existed, its start is the newly verified receipt time; the original
submission-start witness remains unchanged. An I/O failure during these existing
multi-file writes may retain partial positive evidence, which must be inspected
without rollback or resend. Reconciliation does not verify a response, resolve
work, refund a grant or establish production acceptance.

Compatibility: scoped events use the dedicated `source_review` event type,
which old Wake hosts reject instead of treating it as unrestricted work. New
hosts also refuse ordinary submission of these events without the grant callback.
Old ledger files deserialize unchanged and omit the optional
grant field when empty. Older controllers that reject unknown ledger fields
must not write a ledger after grants have been added. Qualify the new controller
before creating live grants; do not silently downgrade that writer. This does
not require replacing the selected browser profile or protected installation.

## Remaining approvals and release authority

Mandatory platform approvals remain mandatory. New/unregistered destinations,
credentials/authentication, protected deletion, external security tests,
financial actions, ownership/binding changes outside an authorized rollover,
and production promotion outside its separately approved scope are not covered.
Existing protected disclosure grants and signed install approvals are unchanged.

Production review validation requires an acknowledged, `COMPLETED_VERIFIED`
record, exact approved contract, attributable post-baseline artifacts, source
remeasurement and a matching build attestation. It does not inherently require
a new ChatGPT Web message. A genuinely independent qualified reviewer can supply
evidence through the existing supported contract path. Local verifier output,
an advisory record, a send receipt or this consent grant must never be relabeled
as independent review or release authority. Existing C208 attestation remains
distinct from newer source and its promotion owner requirement is unchanged.
