# T-0251 / T-0224-R1B-R1A crash-safe claim authority review bundle

## Scope and inherited defects

T-0249's R1A canonical inbox reader remains the event authority. T-0250 added
the R1B Rust-only claim state machine but was rejected for two authority
defects:

1. It evicted `deliveries[0]` whenever the schema-4 128-entry history was
   full. An unresolved `SUBMITTING` entry could therefore disappear and its
   still-actionable inbox record could later become `Unclaimed`.
2. It used `OpenOptions::create_new(true)` on `state.lock` and deleted that
   path in `Drop`. A killed process did not run `Drop`, so an inert pathname
   permanently blocked the next owner.

This slice repairs only durable claim/receipt authority. It does not launch a
browser, submit a wake, alter `scripts/wake_bridge.py`, or cut over browser
ownership.

## Exact changed files

| Path | T-0251 attributable change |
| --- | --- |
| `src/stable_wake_delivery.rs` | Bounded safe-only compaction; active operator-attention refusal; monotonic receipt timestamp validation; Windows kernel-mutex lock; process-probe seam; focused state-machine tests. |
| `src/bin/catdesk-stable-wake-lock-probe.rs` | New bounded test-process executable that acquires the exact production kernel lock or performs a real claim probe. It has no browser or state-transition authority beyond the test seam. |
| `tests/stable_wake_delivery_lock.rs` | New child-process hard-kill, lock-contention, and two-process durable-claim tests. |
| `docs/orchestrator/review_bundles/T-0251_T0224_R1B_R1A_CRASH_SAFE_CLAIM_AUTHORITY_REVIEW_BUNDLE.md` | This review evidence. |

The workspace was already broadly dirty and these product files were already
untracked before this turn, so Git cannot provide a clean contract-baseline
patch for them. Session attribution is therefore bounded to the concrete
symbols below rather than a misleading whole-worktree diff:

- `compact_before_claim`, invoked before every new `CLAIMED` record;
- `require_no_operator_attention`, invoked by classification and all mutable
  claim/submit/receipt paths;
- the `StateLock` Windows mutex implementation and `DeliveryLockProbe` seam;
- `claim_with_transition` plus the process-only observation method; and
- the nine delivery unit tests and three process integration tests listed
  below.

`git diff --check` was run successfully. The initial and final status include
substantial unrelated modified and untracked work, deliberately not claimed by
this bundle.

## Schema-4 / W13 compatibility matrix

| State | Required shape | T-0251 classification/transition |
| --- | --- | --- |
| `CLAIMED` | positive claim time; no receipt fields or attention | Retry-safe pre-submit only. May be compacted when space is required. |
| `SUBMITTING` | no receipt fields; bounded optional attention | Permanently non-retryable ambiguous state. Never compacted. |
| `OPERATOR_ATTENTION` | no receipt fields; bounded nonempty attention | Ambiguous/non-retryable and never compacted. |
| `SENT` | receipt schema 1, exact message/target hashes, positive receipt time not before claim | `ALREADY_SENT` for the exact current target; `SentOtherTarget` otherwise. Never compacted. |
| top-level `operator_attention` | null or bounded fixed-vocabulary value | Any active value fails closed for classify, claim, begin-submit, and receipt reconciliation. |

The history remains bounded at 128 entries. On a new claim at capacity,
compaction removes only a valid clean `CLAIMED` entry. It retains every SENT
receipt and every unresolved `SUBMITTING` or `OPERATOR_ATTENTION` identity. If
there is no such safe pre-submit entry to remove, the new claim is refused with
the fixed failure `stable wake history retains unresolved delivery`; it never
forgets safety-relevant identity.

`begin_submitting` still revalidates canonical inbox actionability and the
exact protected target digest immediately before its durable boundary.
`record_receipt` still requires schema 1 plus exact record/message/target
binding, and now also rejects a receipt timestamp earlier than the claim time.
The canonical review inbox and target config are only read; focused tests
compare their bytes before and after transition operations.

## Crash-safe mutual exclusion and path behavior

On Windows, `StateLock` derives a `Local\\CatDeskStableWakeState-<sha256>`
name from the canonical wake-bridge root and uses `CreateMutexW` plus a
zero-time `WaitForSingleObject`. This kernel mutex -- not file existence -- is the
exclusive transition authority. Mutex ownership is released by Windows on
process death. `WAIT_ABANDONED` is taken only under the existing atomic
old-or-new write discipline; the successor reparses state and fails closed on
any malformed or ambiguous state before a transition.

`state.lock`, if present, is classified as a bounded regular non-link object
only; unsafe/special/reparse objects are rejected, but a stale regular file is
inert and is neither created nor removed by lock acquisition. The existing
same-directory temp write, flush, pre-rename state identity check, and rename
remain the state persistence path. Thus an interrupted write can leave only a
previous parseable state or a completed new state; malformed durable evidence
is never retry-authorizing.

## Deterministic evidence

| Test | Result | Evidence |
| --- | --- | --- |
| `submitting_and_sent_authority_survive_129_later_claims` | pass | Begins with `review_0` in unresolved `SUBMITTING`, then performs 129 later distinct claims. The original remains `SubmittingAmbiguous`; reclaim and re-submit are denied; state stays <=128. Also proves a completely ambiguity-filled history refuses a new claim. |
| `operator_attention_and_non_monotonic_receipts_fail_closed` | pass | Top-level attention blocks classify/claim/begin; an earlier receipt timestamp is refused. |
| `unsafe_lock_link_is_refused_when_symlink_creation_is_available` | pass | A final `state.lock` symlink is refused on Windows when the OS permits test-link creation. |
| existing delivery tests | pass | Preserve clean claim/restart, exact receipt, target drift, stale inbox, duplicate/invalid state, unsafe final state, immutable inbox/config, and other-target SENT semantics. |
| `hard_killed_owner_releases_kernel_lock_without_manual_cleanup` | pass | A child prints `READY` after acquiring the mutex, the parent hard-kills it, and a successor acquires it. A pre-existing `state.lock` marker remains unchanged and does not block. |
| `two_processes_have_one_kernel_lock_owner` | pass | A live owner holds the mutex while the second process receives `BUSY`. |
| `two_process_claims_create_exactly_one_durable_transition` | pass | Two real children race to claim the same canonical record; exactly one prints `TRANSITIONED`, while the other is `NO_TRANSITION` or `BUSY`. |

The production-source regression still checks that the delivery module has no
Selenium, CDP, command spawn, tunnel, daemon-reload, or reviewed-release
authority. The new probe binary is test support only; it does not contain any
browser-driver, typing, click, submission, or wrapper-launch functionality.

## Verification executed in this workspace

| Command | Result |
| --- | --- |
| `cargo test stable_wake_delivery` | pass: 9 focused delivery tests |
| `cargo test --test stable_wake_delivery_lock` | pass: 3 hard-kill/contention/claim child-process tests |
| `cargo fmt --check` | pass |
| `cargo clippy --all-targets --all-features -- -D warnings` | pass |
| `cargo test` | pass: main 734 passed / 21 ignored; supervisor 15; host 8; delivery binary 16; process tests 3; other integration suites passed |
| `cargo build` | pass |
| `git diff --check` | pass (only existing CRLF advisory output) |

The repository's Rust verification profile resolves to its Rust test/build
commands; the full `cargo test` and `cargo build` results above are the local
`rust_full`-equivalent evidence. Independent CatDesk verification remains the
acceptance authority.

## Prohibited mutation confirmation and residual work

No live `.catdesk` provider state was changed. No browser, interactive desktop,
conversation target, daemon, release, promotion, recovery, Secure MCP/tunnel,
Scheduler/service/ProgramData, Git publication, signing, or provenance surface
was changed. `scripts/wake_bridge.py` was inspected only and not edited.

R1B-R2 remains a separate browser-driver owner-cutover boundary. Durable
installation/desktop ownership also remain out of scope. Independent final
review is requested.
