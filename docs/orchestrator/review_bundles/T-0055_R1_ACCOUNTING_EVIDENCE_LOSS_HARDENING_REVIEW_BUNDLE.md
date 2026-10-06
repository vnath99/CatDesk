# T-0055-R1 Accounting Evidence-Loss Hardening — Review Bundle

## Root cause

The bounded activity ledger limits each accounting record to 256 spans. Before
R1, `activity_started` stopped adding spans at that limit but retained no
durable indication that later source boundaries had been dropped. A report
could therefore say `OBSERVED_SPANS` while omitting later active or waiting
periods. Separately, recovery marked open spans `INTERRUPTED` with zero elapsed
time, but did not lower evidence completeness.

## Repair and migration semantics

`ExecutionAccountingRecordV1` now has
`activityEvidenceTruncated: bool` with `#[serde(default)]`. New records begin
with `false`; old ledgers that lack the field deserialize to `false` without a
schema-version change or data rewrite.

At the first source boundary that cannot be stored because the 256-span cap is
already full, CatDesk persists `activityEvidenceTruncated = true`. It retains
all earlier spans, never removes or overwrites evidence, and does not fail the
controller lifecycle because storage has reached its bounded limit. The marker
is permanent for that record, so later reporting cannot present the record as
fully observed.

`autonomy_work_time_report` returns `INCOMPLETE_EVIDENCE` whenever this marker
is true. It continues to calculate known totals from retained completed spans;
the missing time remains in `unobservedIdleOrUnknownMillis` rather than being
guessed as active or waiting.

Any `INTERRUPTED` span also makes the record incomplete. Its recovery interval
remains zero-duration and is excluded from known-active totals, so CatDesk
does not stretch activity across downtime.

## Coverage

`autonomy_accounting` deterministic coverage now verifies:

- exact cap exhaustion retains exactly the bounded number of spans;
- the first dropped span persists the marker without failing the lifecycle;
- subsequent reporting is `INCOMPLETE_EVIDENCE` with an explicit unknown
  remainder;
- absent `activitySpans` and `activityEvidenceTruncated` fields deserialize
  safely as historical data;
- interrupted recovery spans are incomplete and not counted active;
- pre-existing provider/wait separation and overlap-union tests continue to
  pass.

## Compatibility boundaries

This is an accounting-only extension. It preserves the existing
`autonomy_execution_accounting` record surface, T-0053 ticket-audit joins,
interval-union calculations, MCP report bounds/redaction, and provider/wait
separation. It does not alter T-0054 wake policy or the canonical W13
schema-4 / receipt-schema-1 submit and receipt behavior. No browser, wake,
tunnel, Scheduler, daemon, credential, provider-fallback, or Git publication
action is included.

## Local verification

The following are run locally for this repair:

```text
cargo fmt -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test delegated::autonomy_accounting::tests -- --nocapture
cargo test
git diff --check
```

The local full suite completed with 489 passed, 18 ignored, and no failures.
Independent CatDesk verification and authoritative diff capture remain
required before closure.
