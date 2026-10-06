# T-0055 Active Work Time Accounting — Review Bundle

## Scope

T-0055 extends the existing durable execution-accounting ledger; it does not
replace task accounting, T-0053 ticket-audit joins, or the wake contracts. The
extension records bounded activity spans and exposes a read-only,
window-bounded `autonomy_work_time_report` for GUI and operator consumption.

## Durable schema and migration

`ExecutionAccountingRecordV1` retains its schema version and existing fields.
The new `activitySpans` field is `serde(default)`, so a historical v1 record
without the field remains readable. Such a record is reported with
`INCOMPLETE_EVIDENCE`, zero known active time, and an explicit unknown
remainder; no old task elapsed value is reclassified as active work.

Each bounded `ActivitySpanV1` contains a span ID, the owning session ID and
task ID, actor, phase, start/end Unix milliseconds, evidence enum, and
completion status. Persisted actors are:

- `CODEX_PROVIDER_ACTIVE`
- `CATDESK_VERIFICATION_REVIEW_ACTIVE`
- `CATDESK_ORCHESTRATION_ACTIVE`
- `CHATGPT_WEB_ACTIVE`
- `WAITING`
- `UNOBSERVED_IDLE_OR_UNKNOWN`

Evidence is a compact enum only (`PROVIDER_LIFECYCLE`, `VERIFIER_LIFECYCLE`,
`CONTROLLER_TRANSITION`, exact-target ChatGPT generation observation, or
historical unknown). It stores no prompts, command content, browser content,
credentials, route IDs, cookies, or process diagnostics.

## Timing semantics

- Provider spans begin only after the host has a provider-turn handle and end
  on terminal completion, cancellation, or a pause-for-rate-limit transition.
  Retry/backoff is recorded as `WAITING`, never provider active.
- Verification/review spans surround the CatDesk-owned verification lifecycle.
- Short orchestration spans cover controller transitions only; they do not
  represent sleeping, provider wait, or task wall time.
- On runtime recovery, every open span is marked interrupted at its last
  authoritative start observation. Downtime is not silently appended.
- A read-only report never extends an open span to `asOfUnix`: absent fresh
  process ownership proof it is incomplete and contributes no invented active
  duration. `activeNow` therefore remains false unless that proof is added by
  a future authoritative observer.

`CHATGPT_WEB_ACTIVE` has no live observation producer in this change. The
report explicitly uses `UNKNOWN_NOT_OBSERVABLE` unless a bounded positive,
exact-target generating-state observation was persisted. It does not infer
work from Chrome lifetime, wake timing, or a timeout.

## Overlap math and report bounds

The report clips each span to `[startUnix, endUnix]`. Per-actor totals use an
interval union for that actor. Total known active is the union across active
actors, and aggregate wall/active totals are unions across returned records;
overlapping actors or sessions cannot double-count the aggregate.

`autonomy_work_time_report` accepts required nonzero `startUnix` / `endUnix`,
optional exact `projectId`, `sessionId`, and `taskId`, plus `limit` and
`maxBytes`. The facade rejects inverted or over-31-day windows, invalid
filters, more than 100 rows, and output over 24 KiB. It is listed as read-only
in MCP discovery and does not create an empty ledger. Output includes
per-ticket/session wall, union active, actor totals, observed/unknown ChatGPT
status, waiting, unknown remainder, provenance, evidence completeness,
current persisted state when available, and conservative `activeNow` /
`currentActor` fields.

## Changed implementation areas

- `src/delegated/autonomy_accounting.rs`: durable spans, validation,
  crash-safe interruption, clipped interval-union report, historical default.
- `src/delegated/autonomous_controller.rs`: provider, verifier, waiting, and
  bounded orchestration source-boundary instrumentation.
- `src/delegated/autonomy_runtime.rs`: recovery interruption of open spans.
- `src/delegated/autonomy_supervisor.rs` and `src/mcp.rs`: bounded read-only
  MCP report discovery and response shaping.

W13/T-0054 wake submit/receipt behavior was not changed. T-0053 continues to
use the existing execution-accounting records and fields unchanged.

## Deterministic evidence run locally

The following commands completed after the T-0055 changes:

```text
cargo fmt -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test delegated::autonomy_accounting::tests -- --nocapture
cargo test delegated::autonomy_supervisor::tests::work_time_report_is_bounded_read_only_and_reports_unknown_chatgpt -- --nocapture
cargo test
git diff --check
```

Focused coverage includes provider → waiting → provider resume, verification
totals, overlapping actors and sessions, union aggregation, interrupted spans,
historical v1 reads, and report bounds/explicit ChatGPT-Web unknown status.
The local full suite completed with 488 passed, 18 ignored, and no failures.
Independent CatDesk verification and authoritative diff capture remain pending.
