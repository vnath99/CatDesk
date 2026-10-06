# T-0262 / T-0224-R1B-R3E — legacy pre-submit attention cutover compatibility

## Scope and T-0261 split acceptance

T-0261 source/preflight is independently accepted, while live Rust-owner
activation remains deliberately unperformed. Host preflight identified seven
historical schema-4 `OPERATOR_ATTENTION` entries that the ordinary delivery
machine correctly treats as ambiguous. This slice changes only the *read-only*
activation cutover predicate so those exact, provably pre-submit historical
shapes do not prevent a future reviewed activation. It does not change a
record's normal classification, claimability, retry behavior, receipt handling,
history compaction, selector CAS, or any durable input.

## Exact safe shape

`is_provably_pre_submit_legacy_attention` is private to
`src/stable_wake_delivery.rs` and called only by `validate_cutover_safe()`.
It returns true only when all six conditions hold:

1. `status == "OPERATOR_ATTENTION"`;
2. `attention == "CHATGPT_NOT_IDLE"` or `"LOGIN_OR_PROFILE_REQUIRED"`;
3. `browser_sent_at_unix == null`;
4. `message_sha256 == null`;
5. `target_sha256 == null`;
6. `receipt_schema_version == null`.

`SUBMITTING` always blocks. Any other attention, top-level operator attention,
malformed entry, conflicting state, or even one populated submission-evidence
field blocks. There is no prefix, age, count, generic null-receipt, or other
legacy exception.

## Legacy ordering evidence

`scripts/wake_bridge.py` was inspected read-only and not edited. Its browser
adapter raises `LOGIN_OR_PROFILE_REQUIRED` during readiness/pre-typing checks
and `CHATGPT_NOT_IDLE` during idle/pre-typing checks before the callback.
`CdpSink.wake()` invokes `before_submit()` only after readiness, idle, exact
target, empty-editor, typing, and send-control checks. The source locations are
the reason branches around lines 463/527/537/543 and `before_submit()` around
line 673. Thus the two permitted historical reasons are compatible only when
all four submit-evidence fields are absent.

## Isolated fixture and behavior matrix

The delivery fixture creates seven entries, alternating the two exact reasons,
with an otherwise valid schema-4 state, canonical inbox, and protected config.
`validate_cutover_safe()` succeeds and byte comparisons prove the fixture
`state.json`, canonical inbox, and config are unchanged. The activation fixture
uses the same seven-entry JSON plus valid fixed reviewed-artifact/Python/profile
prerequisites; `activate_reviewed_rust_owner(..., LegacyPython)` may change only
its isolated `owner.json`. Its state, inbox, and config bytes remain identical.

| Case | Cutover result | Ordinary record behavior |
| --- | --- | --- |
| Seven exact legacy pre-submit entries | permitted read-only | every ID remains `SubmittingAmbiguous`; `claim()` returns that result and creates no retryable transition |
| `CHATGPT_NOT_IDLE` plus each of timestamp, message digest, target digest, or receipt schema | blocked | state parser/predicate fails closed |
| `LOGIN_OR_PROFILE_REQUIRED` plus each individual evidence field | blocked | state parser/predicate fails closed |
| Unknown `POST_SUBMIT_UNKNOWN` attention | blocked | ambiguous/non-retryable |
| Missing/malformed attention | blocked | malformed state |
| `SUBMITTING` with `SUBMIT_RECEIPT_UNPROVEN` | blocked | unresolved submission authority |
| Conflicting duplicate delivery IDs or malformed JSON | blocked | unsafe state |

The existing `classify()` branch is unchanged: both `SUBMITTING` and
`OPERATOR_ATTENTION` return `SubmittingAmbiguous`. No adapter is reachable
unless an earlier claim reaches the safe pre-submit state, so an exact
historical attention record cannot authorize an attempt.

## Preserved authorities

- Canonical `.catdesk/autonomy/review-inbox.json` remains the sole event
  authority and is read only.
- Schema-4 `.catdesk/wake-bridge/state.json`, schema-1 receipts, W13 exact
  record/message/target/time binding, T-0251 kernel mutex/non-retry ambiguity,
  and T-0252 terminal retirement are unchanged.
- T-0260-R1 reviewed artifact identity and expected-old-owner CAS are unchanged
  except that their existing delivery preflight can now accept the exact safe
  historical shape. Stale/replay/concurrency/readback semantics retain their
  existing test coverage.
- The real protected state, selector, inbox, config, target, profile, and
  browser were not accessed as writable test inputs or modified.

## Verification truth

| Command | Result |
| --- | --- |
| `cargo test stable_wake_delivery -- --nocapture` | PASS — 13 tests, including the seven-entry immutable fixture and all evidence/ambiguity negatives. |
| `cargo test stable_wake_owner_mode -- --nocapture` | PASS — 8 tests, including isolated activation with seven exact historical entries and unchanged state/inbox/config bytes. |
| `cargo test stable_wake_owner -- --nocapture` | PASS — 15 tests; existing restart, one-attempt, target/actionability-drift, and concurrency behavior remains covered. |
| `cargo test --test stable_wake_delivery_lock -- --nocapture` | PASS — 3 real process hard-kill/contention/claim-serialization tests. |
| `cargo fmt --check` | PASS. |
| `cargo clippy --all-targets --all-features -- -D warnings` | PASS. |
| `cargo build --bins` | PASS. |
| `git diff --check` | PASS (only pre-existing LF/CRLF advisories). |
| `cargo test --test stable_wake_adapter_python -- --nocapture` | BLOCKED: the fixed project venv points to missing `C:\\Users\\Volap\\AppData\\Local\\Programs\\Python\\Python312\\python.exe`; no substitute or repair was attempted. |
| `cargo test` | Rust/unit targets completed (775 tests: 754 pass, 21 ignored) and the final fixed-venv adapter fixture failed for that same external interpreter condition. |
| project `rust_full` | No separately runnable project command is defined; no fabricated pass is claimed. |

## Narrow attribution

The workspace was broadly dirty before T-0262. Narrow T-0262 changes are:

- `src/stable_wake_delivery.rs`: the single private exact-shape predicate,
  targeted preflight condition, and immutable positive/negative classification
  regression.
- `src/stable_wake_owner_mode.rs`: seven-entry isolated activation/CAS
  compatibility fixture with byte-equality checks.
- This exact review bundle.

No script was edited. Broad repository status/diff is deliberately not claimed
as task attribution.

## Prohibited mutations and residual boundary

No live owner activation, real `owner.json` write, delivery/inbox/config
mutation, browser launch/type/click/send, target/profile change, daemon/release
operation, tunnel/Scheduler/service action, Git publication, or legacy-history
deletion/acknowledgement/compaction occurred.

Residual work is a separate host-live acceptance ticket: repair/validate the
authorized fixed venv, invoke the closed expected-old-owner activation surface,
perform one fresh normal W13 canary, acknowledge only that fresh canonical
record, and verify restart/replay suppression. T-0262 does not perform or
claim that host procedure.
