# T-0060F-W5 Live Submit Receipt Fix Review Bundle

## Observed regression and bounded diagnosis

The fresh W2-R1 durable delivery is `SUBMITTING` with
`SUBMIT_RECEIPT_UNPROVEN`; its browser timestamp and both receipt hashes are
null. That is the intended fail-closed result after the submission boundary,
not evidence of a completed delivery. The bounded autonomy artifacts contain
no browser exception or selector diagnostic, so they cannot establish that the
click itself failed.

The authoritative bridge nevertheless had two source-level receipt gaps that
can produce exactly this outcome on the current ChatGPT surface:

- It hashed `innerText`/`textContent` from the outer fixed user-author
  container. Current message containers can include non-message UI controls,
  making an otherwise newly appended exact message fail the digest comparison.
- It tested a selector for visibility, then asked CDP to click that selector.
  If the selector has another hidden or stale match, the click target need not
  have been the element whose visibility was tested.

The observed idle composer is consistent with post-click receipt proof failing;
it does not safely distinguish a no-op click from a submitted message whose
outer-container text differs. The repair therefore keeps both possibilities
fail-closed while addressing each bounded source-level gap.

## Repair

- Receipt collection remains restricted to
  `div[data-message-author-role='user']`, but now reads only the known
  `[data-message-content]` or `.whitespace-pre-wrap` child when present. It
  falls back to the fixed author container only for older shapes.
- A receipt now requires the complete prior author-message digest sequence to
  be followed by exactly one appended digest of the exact normalized wake
  message. Reordered, removed, missing, wrong, or duplicate matching messages
  do not prove delivery.
- Send-control readiness and the post-boundary click use the same bounded DOM
  evaluation: exactly one visible, enabled control from the fixed selector set
  is accepted. The click is applied to that exact element, not to an arbitrary
  first selector match. A control that changes after the preflight remains
  `SUBMITTING` with operator attention; no fallback resubmit occurs.

`SENT` remains schema 4 and still requires the exact record ID, normalized wake
message hash, configured target hash, positive browser timestamp, and receipt
schema version. Rust confirmation logic is unchanged and still requires a
matching durable receipt plus `WOKE` or `ALREADY_SENT`.

## Deterministic coverage

`tests/test_wake_bridge.py` adds coverage for current content-node receipt
selection, exact one-message append proof, missing/stale/disabled/control-drift
rejection, and the precise latest-canary durable shape (idle-composer receipt
failure remains `SUBMITTING` with all receipt fields null and is never retried).
Existing cases retain typed-but-unsent, malformed receipt, message-not-appended,
post-submit timeout, click-unknown, and crash fail-closed coverage.

## Verification

- `cargo fmt -- --check`: passed.
- `cargo clippy --all-targets --all-features -- -D warnings`: passed.
- `cargo test`: 476 passed, 18 ignored.
- `git diff --check`: passed.
- Project-local Python wake tests were attempted but could not start because
  `.catdesk/wake-bridge/venv/Scripts/python.exe` references a missing Python
  3.12 base interpreter. No environment repair or replacement interpreter was
  used.

## Residual live acceptance

After independent review and candidate loading, allow the normal CatDesk
autonomous dispatcher to perform one fresh read-only canary against the exact
configured conversation. Accept only a complete schema-4 receipt; any
post-boundary uncertainty remains operator attention without resubmission.

No live wake, browser interaction, promotion, tunnel action, Scheduler change,
credential access, or Git publication was performed for this repair.
