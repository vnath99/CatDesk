# T-0252 / T-0224-R1B-R1B safe terminal-history retirement review bundle

## Scope

T-0251 is accepted as the dormant, crash-safe Rust claim/receipt foundation:
unresolved `SUBMITTING` is non-evictable, the Windows kernel mutex survives a
hard-killed owner, and the legacy Python bridge remains the only browser-submit
owner. Independent review found the distinct liveness defect that all valid
`SENT` records were retained forever, so 128 successful historical deliveries
could permanently refuse the 129th claim.

This change adds only terminal `SENT` retirement. It does not enable the Rust
claim/receipt foundation as a browser writer and does not edit or invoke
`scripts/wake_bridge.py`.

## Canonical acknowledgement authority

The only event authority remains
`<workspace>/.catdesk/autonomy/review-inbox.json`. The existing durable
acknowledgement operation preserves the exact record and changes only
`unread` to `false`; it does not delete it. T-0252 factors
`canonical_inbox_records` from the accepted stable-core reader. It performs
the same fixed-path containment, regular-file/reparse refusal, 2 MiB byte
bound, 512-record bound, camelCase schema, identity/reference validation, and
exact `catdesk` project binding as normal discovery, but retains raw record
cardinality for retirement.

This permits a state-machine caller to require exactly one raw matching record.
Normal `discover` retains its accepted deterministic exact-duplicate collapse
and conflicting-duplicate refusal behavior.

## Retirement rules

At the 128-record limit, compaction proceeds in this order:

1. Remove one clean pre-submit `CLAIMED` record, as before.
2. Otherwise load the canonical inbox fail-closed. A `SENT` delivery is
   eligible only if every stored terminal receipt validates, exactly one raw
   canonical record has the same `record_id`, and that record is stale under
   the shared actionability rule (`unread=false` or otherwise not actionable).
3. If no such entry exists, reject the new claim with the fixed bounded error
   `stable wake history retains unresolved delivery`.

Before a `SENT` entry can retire, T-0252 validates schema-4 terminal shape,
receipt schema 1, bounded ID, monotonic receipt timestamp, SHA-256 target
shape, and exact W13 message digest for that record. A malformed or mismatched
receipt aborts compaction rather than dropping the entry. Current target drift,
receipt age, FIFO position, missing inbox evidence, and count alone are never
retirement authority. `SUBMITTING`, `OPERATOR_ATTENTION`, top-level operator
attention, and unread/actionable `SENT` remain non-retirable.

## Changed files and attribution

| Path | T-0252 attributable content |
| --- | --- |
| `src/stable_wake_core.rs` | Shared bounded canonical raw-record loader and shared actionability visibility. |
| `src/stable_wake_delivery.rs` | Safe `SENT` retirement policy, exact terminal receipt validation, and focused 128/129/refusal/repetition tests. |
| `docs/orchestrator/review_bundles/T-0252_T0224_R1B_R1B_SAFE_TERMINAL_HISTORY_RETIREMENT_REVIEW_BUNDLE.md` | This evidence. |

The workspace was pre-existing dirty and these stable-wake files were already
untracked before this provider turn. A broad Git diff would therefore falsely
attribute unrelated work. The session-attributable symbols are
`canonical_inbox_records`, `StableWakeDelivery::compact_before_claim`, and
`validate_terminal_receipt`, plus the three new focused delivery tests.
`git diff --check` completed successfully; existing CRLF advisories are not
attributable changes.

## 128/129 acceptance evidence

| Test | Result | Evidence |
| --- | --- | --- |
| `acknowledged_terminal_sent_retires_at_128_but_unread_sent_does_not` | pass | 127 unread retained `SENT` plus one exact `unread=false` canonical record retires and admits `review_next`; restart remains `ClaimedRetryable`; state remains exactly 128. The same full state with every terminal record unread refuses the new claim. Inbox and config bytes are compared before/after. |
| `terminal_retirement_requires_exact_safe_canonical_evidence_and_receipt` | pass | Missing record, malformed inbox, oversized inbox, wrong project, conflicting duplicate, unsafe traversal reference, and a message-digest-mismatched receipt all fail closed and do not authorize retirement. |
| `repeated_acknowledged_terminal_retirement_stays_bounded` | pass | Three repeated acknowledged-terminal retire/admit cycles retain the <=128 bound. |
| `submitting_and_sent_authority_survive_129_later_claims` | pass | T-0251 coverage remains: unresolved submit stays ambiguous and a valid SENT receipt remains suppressing under later churn. |
| `stable_wake_delivery_lock` child tests | pass | Existing hard-kill recovery, kernel contention, and two-process claim serialization remain green. |

The test records use the canonical safe nested reference
`artifacts/completion.json`; no secondary spool is read or written. The core
suite also preserves existing malformed/count/unsafe-reference and
read-only-discovery coverage.

## Architecture and no-owner-cutover evidence

Production `stable_wake_delivery` continues to contain no Selenium, CDP,
browser launch, typing, click, process spawn, tunnel, daemon reload, or
reviewed-release authority. The regression source test continues to reject
those capabilities. The terminal-retirement logic reads only the canonical
inbox; no `.catdesk/stable-wake/review-events` path or second event authority
was added. Python was inspected solely as W13 compatibility evidence and was
not edited. Consequently Rust and Python delivery writers were not activated
concurrently.

## Local verification evidence

| Command | Result |
| --- | --- |
| `cargo test stable_wake_core` | pass: 7 stable-core tests |
| `cargo test stable_wake_delivery` | pass: 12 delivery tests |
| `cargo test --test stable_wake_delivery_lock` | pass: 3 real child-process tests |
| `cargo fmt --check` | pass |
| `cargo clippy --all-targets --all-features -- -D warnings` | pass |
| `cargo test` | pass: all main, binary, and integration suites (758 total test registrations; 21 existing ignored) |
| `cargo build` | pass |
| `cargo build --release --bin catdesk-stable-wake-lock-probe` | pass |
| full `cargo build --release` | attempted twice; exceeded the provider foreground 120-second cap during the final crate build, with no compiler error emitted |
| `git diff --check` | pass |

The repository Rust verification profile is its test/build sequence. The
focused/full evidence above is local only; CatDesk independent verification is
the acceptance authority.

## Prohibited mutations and residual work

No live `.catdesk` state was modified, no browser was launched or submitted,
and no target, daemon, release, promotion, recovery, Secure MCP/tunnel,
Scheduler/service/ProgramData, Git publication, signing, or provenance action
occurred. Browser owner cutover remains the separate T-0253 atomic
single-writer boundary. Independent final review is requested.
