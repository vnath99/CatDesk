# T-0253 / T-0224 R1B-R2 — Single-writer browser-owner integration review bundle

## Scope and attribution

This is the source-only, dormant R1B-R2 handoff seam after accepted T-0251
(crash-safe claim authority) and T-0252 (safe terminal history retirement).
The workspace was already broadly dirty and the T-0251/T-0252 Rust modules are
untracked in the pre-existing working tree; therefore a repository-wide Git
diff is not task attribution. This session's bounded attributable changes are:

| Path | T-0253 change |
| --- | --- |
| `src/stable_wake_owner.rs` | New dormant Rust owner and stateless typed adapter boundary, plus deterministic fake-adapter tests. |
| `src/stable_wake_delivery.rs` | Two owner-only transition helpers: `return_to_clean_claim` and `note_operator_attention`. |
| `src/main.rs` | Registers the dormant owner module only; it is not selected by any command, MCP route, daemon, or startup path. |
| `docs/orchestrator/review_bundles/T-0253_T0224_R1B_R2_SINGLE_WRITER_BROWSER_OWNER_INTEGRATION_REVIEW_BUNDLE.md` | This review artifact. |

No legacy bridge, setup/bootstrap script, target, inbox, delivery state, daemon,
release, tunnel, service, scheduler, browser, or Git state was activated or
mutated by this task.

## Existing and new ownership

| Authority | Existing live owner | T-0253 dormant path |
| --- | --- | --- |
| Canonical events | `.catdesk/autonomy/review-inbox.json`; legacy W13 bridge reads it | `StableWakeDelivery` revalidates it read-only through `stable_wake_core`. No second spool exists. |
| Schema-4 state / receipt | Existing W13 Python bridge when explicitly invoked through `catdesk_wake_bridge_run_once` | `StableWakeOwner` is the only writer in the new dormant path, through `StableWakeDelivery`. |
| Browser action | Existing `scripts/wake_bridge.py` legacy route | None. The `BrowserAdapter` trait is a testable future one-attempt seam, not an executable or activation. |
| Live owner selection | Existing MCP/legacy route | Unchanged. `stable_wake_owner` has no command, MCP, server, daemon, or startup caller. |

The current legacy writer remains separately present in `src/mcp.rs` and
`scripts/wake_bridge.py`; it was inspected only as compatibility evidence and
not edited. The dormant owner is deliberately not activated, so there is no
dual live writer.

## Owner sequence and bounded adapter contract

`StableWakeOwner::dispatch_one` does this in order:

1. Capture the protected exact target digest.
2. Claim one actionable canonical record using the existing kernel-locked,
   atomic schema-4 state machine.
3. Revalidate actionability and the previously captured target digest inside
   `begin_submitting`, then durably write `SUBMITTING` before any adapter call.
4. Invoke `BrowserAdapter::attempt` exactly once with only `record_id`,
   `message_sha256`, and `target_sha256`. It gets no workspace path, URL,
   inbox/config/state handle, token, profile, or command authority.
5. On definite success, persist exactly one schema-1 receipt using the
   submission-bound record/message/target values.
6. On a definite pre-submit failure, restore only the clean `CLAIMED` state.
7. On target-not-ready, authentication, captcha, unavailable chat, ambiguous
   post-submit result, crash, or timeout, retain `SUBMITTING` and set bounded
   per-record and top-level operator attention. That state fails closed and
   cannot be retried automatically.

The Rust enum is the only adapter result representation in this dormant slice;
there is no subprocess/stdout parser, no browser executable, and no raw,
unbounded or secret-bearing adapter output accepted. A future process adapter
must add a separate bounded parser and activation review.

## Crash, ambiguity, concurrency, and immutability evidence

`stable_wake_owner` focused tests cover:

| Case | Result |
| --- | --- |
| Definite success | One adapter call; exact schema-1 receipt; classification becomes `AlreadySent`. |
| Definite pre-submit failure | One adapter call; only returns to retry-safe `CLAIMED`. |
| Invalid success receipt/timestamp | Retains `SUBMITTING`, records operator attention, and does not retry. |
| Target/auth/captcha/chat unavailable, ambiguous result, adapter error | One adapter call; durable operator attention; classification fails closed. |
| Owner restart after a durable unresolved `SUBMITTING` boundary | No adapter call; returns `AlreadyOwned(SubmittingAmbiguous)`. This covers crash after durable boundary, including an adapter success before receipt is persisted. |
| Stale/acknowledged canonical record | Claim fails before adapter call. |
| Two concurrent owners | Existing state lock allows only one `SUBMITTING` transition and one adapter call. |
| Adapter-only call | Canonical inbox and protected target config bytes remain unchanged. |

T-0251 lock hard-kill/contention and T-0252 terminal-retirement tests remain
part of the full test run. The owner does not acknowledge or rewrite canonical
inbox records.

## Architecture regressions

The in-source test asserts that the production portion of
`stable_wake_owner.rs` contains none of `wake_bridge.py`, `selenium`,
`Command::new`, `catdesk_wake_bridge_run_once`, or the former `review-events`
spool. Source search confirms the live legacy route remains only under the
pre-existing MCP/Python locations and is not referenced by the owner module.
The owner source has no browser launch, typing, clicking, Selenium/CDP,
subprocess spawn, daemon/release/LKG/provider dependency, or second canonical
event authority.

## Verification performed in this session

| Command | Result |
| --- | --- |
| `cargo test stable_wake_owner` | Passed: 4 focused owner tests. |
| `cargo fmt --check` | Passed. |
| `cargo clippy --all-targets --all-features -- -D warnings` | Passed. |
| `cargo test` | Passed: 740 passed, 21 ignored, 0 failed; additional binary/integration suites also passed. |
| `git diff --check` | Passed. |
| `cargo build --release --locked` | The local command reached its 120-second execution limit while compiling and emitted no compiler error before timeout; it is **not recorded as passed**. |

The repository's `rust_full` Rust components (`fmt`, strict clippy, and full
test) passed above. Independent CatDesk verification remains authoritative.

## Remaining boundary

T-0253 intentionally does not create a browser driver/executable, parse an
untrusted adapter result, activate owner selection, acknowledge the inbox,
install host ownership, or cut over the legacy bridge. Those are separate
R1B browser-owner and installation/desktop-ownership work.

## Mechanical status

**PASS (source slice):** a dormant single-writer Rust state-owner orchestration
path exists; it writes durable `SUBMITTING` before a one-attempt stateless
adapter seam, binds receipts exactly, and keeps ambiguity non-retryable.

**NOT an activation claim:** no live browser submission or owner-selection
cutover was performed. Release-build verification needs an independently
rerun command with a longer host allowance.
