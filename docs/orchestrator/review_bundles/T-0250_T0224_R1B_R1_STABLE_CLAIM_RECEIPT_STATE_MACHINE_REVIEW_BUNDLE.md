# T-0250 / R1B-R1 Stable Claim/Receipt State Machine

## Scope and authority

T-0249 is accepted and R1A remains closed. T-0250 adds only
`src/stable_wake_delivery.rs`: a pure Rust schema-4 state machine. It has no
browser/Selenium/CDP/process launch, no Python edit, and no live dispatch
wiring. The versioned runtime remains the only browser-submit owner.

Canonical immutable event authority remains
`<workspace>/.catdesk/autonomy/review-inbox.json`. Writable authority is only
`<workspace>/.catdesk/wake-bridge/state.json`; no second spool exists.

## W13 compatibility matrix

| Item | Accepted rule |
| --- | --- |
| Outer state | `schema_version: 4`, `deliveries`, optional null/bounded `operator_attention` |
| History | at most 128 unique `record_id` records |
| Clean claim | `CLAIMED`, positive `claimed_at_unix`, all receipt fields and attention null |
| Submit boundary | `SUBMITTING` persisted before any future browser boundary; no receipt fields |
| Receipt | `SENT`, positive finite `browser_sent_at_unix`, schema 1, exact normalized W13 message SHA-256 and exact target SHA-256 |
| Existing current target SENT | `ALREADY_SENT` / idempotent classification |
| Existing other target SENT | distinct `SentOtherTarget`, never reusable |
| Ambiguity | `SUBMITTING`, `OPERATOR_ATTENTION`, malformed receipt, duplicate record, unknown status, invalid combination => fail closed/no resubmit |

The fixed W13 message and normalized SHA-256 now live in
`stable_wake_core`, shared by this state module. `claim` is retry-safe only
while clean. `begin_submitting` requires canonical actionability and an opaque
previous target digest; it revalidates that digest immediately before persisting
`SUBMITTING`. A target drift cannot select a replacement and fails before the
boundary. `record_receipt` accepts only exact receipt schema/message/target
proof.

## Persistence and containment

State uses create-new `state.lock` single-writer exclusion, a same-directory
unique temporary file, flush, pre-rename identity recheck, and rename. Stable
root, `.catdesk`, wake-bridge, state, and lock are classified as contained
non-link/non-reparse regular objects. Final state replacement by a directory,
malformed/oversized state, duplicate delivery identity, and concurrent mutation
fail closed. Lock deletion checks its captured identity first. This is bounded
path hardening only; it does not resume the paused T-0229 migration.

## Deterministic evidence

`stable_wake_delivery` focused tests cover clean claim/reopen, durable
SUBMITTING/restart no-resubmit, exact receipt/SENT/reopen, immutable inbox and
config bytes, stale actionability, target drift, duplicate state, concurrent
claims, other-target SENT, invalid receipt, 129-record history, malformed JSON,
and final state directory refusal. A source regression rejects Selenium, CDP,
`Command::new`, tunnel, daemon-reload, and reviewed-release ownership in the
production module.

## Verification

| Command | Result |
| --- | --- |
| `cargo test stable_wake_delivery` | passed: 6 |
| `cargo clippy --all-targets --all-features -- -D warnings` | passed |
| `cargo fmt` / focused format | passed |
| `cargo test --quiet` main binary | passed: 731, 21 ignored |
| supervisor binary | passed: 15 |
| standalone host binary | passed: 8 |
| aggregate command | main/binary suites passed; process tool cap occurred while entering existing recovery fixture; no attributable failure shown |

## Attribution and residual work

Attributable source is `src/stable_wake_delivery.rs`, the small W13 message
helper in `src/stable_wake_core.rs`, and `src/main.rs` module registration, plus
this bundle. The worktree was pre-existing dirty; no broad diff is claimed.

No `.catdesk` live state, browser, target mutation, daemon/release/MCP/tunnel,
ProgramData/Scheduler/service, Git publication, signing, or provenance action
occurred. R1B-R2 remains browser-owner cutover; later R1B remains stable
installation and desktop ownership. Independent final review is requested.
