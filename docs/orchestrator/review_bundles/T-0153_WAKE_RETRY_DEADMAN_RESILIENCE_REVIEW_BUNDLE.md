# T-0153 — Wake Retry / Deadman Resilience Review Bundle

## Status

**SOURCE IMPLEMENTATION COMPLETE; LIVE ACCEPTANCE DEFERRED UNTIL T-0154 RECOVERY SAFETY IS IN PLACE.**

This ticket was implemented against the existing dirty CatDesk worktree. The whole-file Git diff is **not** task attribution because these files contained substantial pre-existing changes. T-0153 attribution is limited to the exact symbols/behaviors listed below.

The initial delegated local-Qwen run `T-0153-R1` was cancelled after repeated whole-file reads of `src/delegated/autonomy_runtime.rs` and an Ollama continuation failure (`HTTP 500: no user query found in messages`). No useful Qwen mutation was accepted. The host/ChatGPT reviewer then implemented the bounded changes directly with targeted edits.

## Objective

Make a proven pre-submit `CHATGPT_NOT_IDLE` wake failure survive daemon restart and retry approximately five minutes later without weakening the exactly-once submission boundary. Keep bridge singleton/process contention on a separate short retry path. Preserve exact project/target/record binding, and never retry login/CAPTCHA/security/post-submit ambiguity.

The independent ChatGPT-side hourly condition watch (`CatDesk Hourly Recovery Watch`) is intentionally outside CatDesk's browser submit ownership. It is a deadman fallback only; if CatDesk is actively progressing it does nothing.

## Task-attributable files

- `src/delegated/autonomy_runtime.rs`
- `scripts/wake_bridge.py`
- `tests/test_wake_bridge.py`
- `docs/orchestrator/review_bundles/T-0153_WAKE_RETRY_DEADMAN_RESILIENCE_REVIEW_BUNDLE.md`

No T-0153 implementation requires edits to `catdesk.ps1`, promotion/recovery scripts, canonical release files, tunnel configuration, browser profile state, Git history, or authenticated provider state.

## Architecture / implementation

### 1. Distinct pre-submit diagnostics

`scripts/wake_bridge.py` now emits the exact fixed token `CHATGPT_NOT_IDLE` when the exact ChatGPT target is still generating **before typing or the submit boundary**. The durable delivery remains a clean `CLAIMED` record with no receipt/attention fields.

Singleton/process contention continues to emit `WAKE_BRIDGE_BUSY`.

Both use exit code 3, but Rust no longer authorizes retry from exit code alone. `classify_wake_exit` requires the exact diagnostic:

- `3 + WAKE_BRIDGE_BUSY` → `RetrySoon`
- `3 + CHATGPT_NOT_IDLE` → `RetryAfter(300s)`
- unrecognized exit-3 diagnostics → terminal/fail closed

### 2. Append-only durable retry journal

A busy-ChatGPT deferral is persisted under the fixed `.catdesk/wake-bridge` root as an append-only per-review journal:

`retry-<sha256(record_id)>-<attempt>.json`

Only bounded non-secret metadata is stored:

- schema version
- exact `record_id`
- target SHA-256 digest (never the conversation URL)
- `not_before_unix`
- bounded attempt number
- fixed reason `CHATGPT_NOT_IDLE`

No message text, browser profile path, endpoint/route, token, credential, or authentication data is persisted.

The journal is append-only (`create_new` + `sync_all`) so a crash cannot replace a previously valid deadline with a partial overwrite. Generation gaps, malformed/oversized files, symlinks, unexpected fields, wrong schema, wrong exact record, wrong target digest, impossible-future timestamps, or invalid attempt values fail closed.

A duplicate owner observing a still-future journal reuses the existing deadline and **does not push it forward**.

### 3. Bounded five-minute retry behavior

`WAKE_CHAT_BUSY_RETRY_SECONDS = 300`.

A maximum of 12 proven pre-submit busy deferrals is persisted. This covers roughly one hour. Once attempt 12 becomes due, CatDesk refuses to launch a thirteenth busy retry. The independent hourly ChatGPT-side deadman is the fallback rather than an unbounded browser loop.

Lock/process coordination remains separate:

- `WAKE_SHORT_RETRY_LIMIT = 3`
- `WAKE_SHORT_RETRY_DELAY = 2s`

### 4. Restart-safe rehydration

Startup rehydration and the normal reviewer loop now use the same retry driver. `dispatch_actionable_wake` consults the durable retry journal **before Python/browser runtime checks**:

- future deadline → return only remaining duration;
- due valid deadline → revalidate the active session/review/target/receipt, then permit the bridge attempt;
- malformed/ambiguous schedule → terminal/fail closed;
- exhausted schedule → terminal, no thirteenth browser attempt.

Therefore a daemon restart at minute 2 of a five-minute delay rehydrates the same exact review and waits the remaining ~3 minutes rather than forgetting the timer or starting a fresh full delay.

### 5. Exactly-once / post-submit safety

Existing durable bridge receipt state remains authoritative.

- proven current-target `SENT`/`ALREADY_SENT` retires retry journal evidence and never relaunches;
- `SUBMITTING`, unproven/invalid `SENT`, or post-submit ambiguity is `Unsafe` and never rehydrates;
- a clean `CLAIMED` pre-submit record is retryable;
- only the historical compatibility state `OPERATOR_ATTENTION + CHATGPT_NOT_IDLE` is treated as pre-submit retryable;
- login/CAPTCHA/security/selector/target/operator-attention states remain `Unsafe` and never rehydrate.

An in-process active-dispatch set is now enforced as a real single owner: a second host dispatcher cannot launch a second bridge process for the same session; it gets the short coordination retry instead. The Python singleton remains defense in depth.

### 6. Conversation rebind during a pending retry

A valid retry journal is bound to the target digest that existed when it was created. If the registered project conversation later changes, startup does **not** reuse the old exact review against the new target. It validates the old journal without needing the former URL, appends a durable rebind event, mints a distinct successor review record for the current project target, acknowledges the old inbox item, and retires the old retry journal.

The existing already-sent rebind rule remains unchanged: the old immutable `SENT` receipt is never replayed; a successor review is created for the new conversation.

## Deterministic regression coverage

Rust tests now cover, among the surrounding existing wake tests:

- exit-3 diagnostic split (`WAKE_BRIDGE_BUSY` vs `CHATGPT_NOT_IDLE` vs unrecognized);
- 300-second retry duration and independent short-retry constants;
- future deadline preservation (duplicate owner cannot extend it);
- due generation advancing exactly one append-only journal entry;
- 12-deferral exhaustion / refusal of a thirteenth deferral;
- exact target-digest binding;
- unknown JSON fields fail closed;
- generation gaps fail closed;
- restart retains a future retry schedule;
- proven current-target `SENT` retires stale retry evidence;
- dispatcher honors a future durable schedule before checking for a Python/browser runtime;
- pending unsent retry + target rebind creates a successor instead of replaying the old record;
- login/CAPTCHA operator attention and `SUBMITTING` never rehydrate as unsent;
- historical `OPERATOR_ATTENTION: CHATGPT_NOT_IDLE` remains a bounded compatibility migration;
- existing `SENT` rebind and unproven-receipt fail-closed behavior continue to pass.

Python deterministic tests were updated to assert the fixed stdout token:

- `not-idle` → exit 3 + `CHATGPT_NOT_IDLE` + clean `CLAIMED`
- singleton contention → exit 3 + `WAKE_BRIDGE_BUSY` + no sink call

## Verification evidence

### Passed

- `cargo check` — PASS
- `cargo test --no-run` — PASS
- `cargo test` — PASS after all T-0153 regression additions
- CatDesk `verify_project` / `rust_full` — PASS:
  - `cargo fmt --check` — PASS
  - `cargo test` — PASS
  - `cargo build` — PASS
- `cargo clippy --all-targets --all-features -- -A clippy::len-zero -D warnings` — PASS

Representative logs:

- `.catdesk/logs/1787102672-05d5c7f1-bc52-4353-9312-6ef1722886ef.log` (`cargo check`)
- `.catdesk/logs/1787103314-788a4ded-9fb2-4925-a054-f465b2fba2f0.log` (final Rust tests before bundle)
- `.catdesk/logs/1787103457-6dc5b73d-1523-4b9e-b005-924d0325c888.log` (clippy with only known unrelated lint allowed)

### Historical T-0153 limitations / non-T-0153 blockers

1. The original T-0153 run required allowing a pre-existing unrelated `clippy::len-zero` warning in `src/delegated/patch_engine.rs`. The current closure's strict `cargo clippy --all-targets --all-features -- -D warnings` run passes; this historical note does not describe the current verification result.

2. Direct execution of `.catdesk/wake-bridge/venv/Scripts/python.exe -m unittest tests.test_wake_bridge` is blocked by CatDesk's `SHELL_MODE_BLOCKED` interpreter policy. The policy was not bypassed or weakened. Existing approved wrappers (`setup_wake_bridge.ps1`, `repair_wake_bridge_environment.ps1`, `run_live_wake_acceptance.ps1`) either mutate the environment or cross into live browser acceptance, so they were intentionally not invoked during source review.

3. **Live five-minute browser acceptance has not yet been run.** This is deliberate: the immediately preceding incident proved that daemon reload/promotion/recovery can strand CatDesk behind a canonical binary/fingerprint mismatch. Reloading T-0153 before T-0154 hardens one-command recovery would recreate the exact operational risk the user asked us to eliminate.

## Safety / non-actions

During T-0153 source implementation and verification:

- no CatDesk daemon reload or promotion was performed;
- no canonical release binary/fingerprint was changed;
- no Secure MCP tunnel process/configuration was changed;
- no browser/wake live submit was launched;
- no Git commit/push/merge was performed;
- no credentials or authentication files were read/exported;
- no user prompt relay was required.

## Independent review recommendation

**Accept T-0153 as source-complete but not production-complete.** The implementation has bounded deterministic Rust coverage and preserves the submission boundary. Production acceptance requires T-0154 first, followed by a T-0155 live soak that intentionally exercises:

1. ChatGPT initially generating;
2. first wake returns the clean pre-submit busy result;
3. durable retry survives daemon restart;
4. no browser attempt occurs before the persisted `not_before` deadline;
5. later retry submits the exact review once;
6. durable `SENT` receipt is proven and retry journal retired;
7. Secure MCP remains/returns `CONNECTED_VERIFIED` without duplicate runtime;
8. one-command recovery can restore a deliberately injected canonical pair mismatch without operator hash/file surgery.

## Current T-0153 restart-classification closure

### Narrow defect found after T-0218

T-0218 correctly emits one durable delegated final-review record and rehydrates an
unsent record after restart. Its rehydration path also accepted a valid
`CHATGPT_NOT_IDLE` retry journal after its twelfth and final permitted deferral.
The dispatcher then terminated without a browser attempt, but every later daemon
restart queued the same terminal no-op again. The durable journal already proved
that the pre-submit retry budget was exhausted; restart recovery was missing that
failure classification.

### Repair

`wake_retry_rehydration_is_retryable` now reads the existing exact-record,
exact-target journal and returns false at the fixed twelve-deferral boundary.
Both ordinary waiting-review and delegated-final-review restart discovery use this
classification. The exhausted journal and its single unread review record remain
durable evidence for the independent reviewer/hourly-deadman fallback. Nothing
is acknowledged, deleted, retargeted, or dispatched.

This is intentionally narrower than a new browser retry mode:

| Durable state | Restart action |
| --- | --- |
| No valid schedule or schedule below the fixed limit | Rehydrate the existing exact review; its existing timer remains authoritative. |
| Valid schedule at attempt 12 | Do not schedule a terminal bridge task; retain the record/journal for review. |
| Malformed, target-mismatched, ambiguous, unsafe receipt, login/CAPTCHA/security/post-submit state | Fail closed; no dispatch. |
| Proven `SENT` for current target | Do not dispatch; stale retry evidence is retired by the existing receipt path. |

### Focused regression evidence

- `delegated_retry_exhaustion_survives_restart_without_duplicate_handoff_or_dispatch`
  fills the bounded journal, proves delegated restart discovery returns no
  dispatch candidate, and proves exactly one unread delegated review remains.
- `exhausted_retry_journal_is_not_rehydrated_but_preserves_the_exact_review`
  proves the same behavior for an ordinary waiting review.
- Existing retry tests continue to prove future schedules remain retryable,
  target-bound, append-only, and non-extensible by duplicate owners.

### Verification for this closure

- `cargo fmt --all -- --check` — PASS
- `cargo test retry --all-features -- --nocapture` — PASS (13 matching tests)
- targeted two new retry/restart tests — PASS
- `cargo clippy --all-targets --all-features -- -D warnings` — PASS
- `cargo test --all-targets --all-features --no-fail-fast` — PASS (845 tests;
  existing Python-advisor tests remain ignored)
- `cargo build --all-targets --all-features` — PASS
- `git diff --check` — PASS
- No `rust_full`/project-verification wrapper is configured in this workspace;
  no substitute host operation was invoked.

### Attribution and non-actions

The intentionally dirty worktree prevents whole-file attribution. This closure is
limited to `src/delegated/autonomy_runtime.rs` (`wake_retry_rehydration_is_retryable`,
the two restart candidate scans, and the two named tests) and this bundle. It
did not invoke the wake bridge/browser, create or alter a registered chat target,
change external Secure MCP/tunnel ownership, mutate host lifecycle state, or
stage/commit/publish Git.

Independent final review is requested. This remains source/test evidence only;
no live wake acceptance is claimed.
