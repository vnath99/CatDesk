# T-0060F-W1 Wake Delivery Integrity Review Bundle

## Root cause and repair

The bridge previously marked a delivery `SENT` when the composer became empty
after an attempted submit. The observed dedicated ChatGPT page could accept the
full text into the composer without creating a user message, so that signal did
not prove delivery. The Rust wake dispatcher compounded this by treating exit
code zero alone as `Confirmed` and inserting its in-memory dispatch-suppression
key without reading the durable bridge state.

The bridge now uses state schema 4. A `SENT` record requires all of:

- a positive browser send timestamp;
- the exact record ID;
- a SHA-256 of the normalized exact wake message;
- a SHA-256 of the configured target; and
- receipt schema version 1.

No wake text or target URL is persisted in that receipt. Before the fixed send
control is clicked, the bridge snapshots only the fixed
`div[data-message-author-role='user']` selector. After a click, it requires
both an empty composer and a newly added matching message digest. The selector
set for sending remains restricted to current ChatGPT send-button forms; there
is no generic-button search or arbitrary page interaction.

## Duplicate safety and migration

`CLAIMED` remains retryable before the browser submission boundary. Once the
send click is attempted, click errors, browser evaluation errors, empty-composer
without a matching new message, and crashes after the boundary leave
`SUBMITTING` with bounded operator attention. Later invocations do not blindly
resubmit that record.

Historical `SENT` records without the schema-4 receipt (including the observed
null `browser_sent_at_unix` entries) are unproven. For the same record, the
bridge changes the entry to `OPERATOR_ATTENTION` and does not touch the
browser. Rust likewise rejects them. A valid complete historical/current
receipt is idempotently `ALREADY_SENT`.

## Rust delivery confirmation

The dispatcher now captures at most 256 bytes of bridge stdout/stderr and
reduces it to one fixed diagnostic marker (`WOKE`, `ALREADY_SENT`, busy,
attention, empty, non-UTF8, or unrecognized). It neither logs nor exposes raw
browser output. Exit zero is now retryable transport progress by default.
Only `WOKE` or `ALREADY_SENT` plus a bounded state/config receipt matching the
current review record, exact generated wake-message digest, configured-target
digest, timestamp, and schema can return `Confirmed` and cache the dispatch
key.

## Deterministic coverage

Python fixtures cover exact receipt binding, legacy/null `SENT`, typed-but-
unsent/selector failure, click unknown, composer-empty without a submitted
message receipt, post-submit `SUBMITTING` no-resubmit behavior, valid receipt,
and idempotent `ALREADY_SENT`. Rust tests cover exit-zero invalid receipt,
legacy schema/null timestamp, exact valid receipt/current record binding, and
unrecognized output.

## Local checks

- Focused Rust wake tests: 8 passed.
- `cargo fmt` and `cargo clippy --all-targets --all-features -- -D warnings`
  passed.
- `cargo test` — 459 passed, 18 ignored, 0 failed.
- The workspace’s configured wake virtual-environment interpreter is a broken
  link to an unavailable host Python installation, and no system `python`/`py`
  command is available. The Python deterministic suite was therefore not run
  in this worker pass; it was not replaced with a live browser call.

No live wake, browser, promotion, daemon reload, tunnel, scheduler, credential,
or Git publication action was performed. Independent CatDesk verification and
authoritative diff capture remain pending.
