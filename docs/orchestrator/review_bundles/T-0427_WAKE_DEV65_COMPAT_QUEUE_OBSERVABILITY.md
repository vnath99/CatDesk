# T-0427 — Wake dev.65 compatibility-safe observability and truthful queue health

## Scope

dev.65 carries forward the independently reviewed T-0426/dev.64 in-place HOME/network navigation repair and adds only bounded observability/status corrections discovered during independent review.

Installed Wake before activation remains dev.63, PAUSED, with no USER-deliverable event. The last dev.63 diagnostic is terminal receiptless ATTENTION/TARGET_DRIFT and must never be replayed.

## Carried-forward dev.64 repair

T-0426 changes pre-write exact-target recovery from SeleniumBase CDP `open(url)` to in-place `get(url)` on the already active CDP tab, retaining the existing `activate_cdp_mode` fallback only when the active object lacks the navigation method.

No caller-selected URL, new browser context, target mutation, receipt mutation, USER replay, or post-write recovery is introduced.

Retained T-0426 verification:
- Wake library 37/37 PASS.
- protocol/store 26/26 PASS.
- embedded Python validation 3/3 PASS + one intentional manual harness ignore.
- strict Wake library Clippy PASS.

## dev.65 delta

### 1. Backward-compatible readiness diagnostics

The unfinished dev.61 design placed `readinessHistory` directly on `runtime::Status`. That is unsafe for rolling Wake upgrades because `Status` is a cross-version wire contract consumed by the already-running CatDesk process and uses `deny_unknown_fields`.

dev.65 keeps `status.json` on the prior schema and persists readiness transitions in a separate bounded, versioned `readiness-history.json` journal.

Properties:
- schemaVersion = 1;
- maximum 64 entries;
- fixed route/reason vocabulary only;
- consecutive duplicate observations are collapsed;
- entries contain only bounded event ID, route class, reason code, and timestamp;
- no page text, credentials, cookies, URL identifiers, or arbitrary browser content;
- journal is diagnostic-only and has no delivery authority;
- read-only CLI: `CatDeskWakeHost readiness-history`.

This avoids requiring a CatDesk daemon rebuild merely to add Wake diagnostics.

### 2. Actionable queue semantics

`status.queueDepth` now means USER-deliverable work only:
- no durable delivery yet, or
- current-generation `CLAIMED`.

Preserved `SUBMITTING` records are reconciliation-only and must never look replayable. `ATTENTION` is terminal until an explicit bounded re-arm rule changes it. Stale/old-generation records remain forensic evidence.

A separate read-only versioned `queue-health` diagnostic exposes:
- actionable;
- reconciliation;
- attention;
- stale;
- forensic = reconciliation + attention + stale.

No durable evidence is deleted or rewritten.

### 3. Current-event status remains truthful

Redefining queueDepth to zero for ATTENTION/SUBMITTING initially exposed an idle-normalization regression. dev.65 fixes this by preserving the last current-generation Claimed/Submitting/Attention attempt surface even when there is no USER-deliverable queue.

Direct current-source production Store readback now reports:
- queueDepth 0;
- last dev.63 diagnostic remains submission ATTENTION / browser ATTENTION / attention TARGET_DRIFT;
- no false IDLE normalization.

Current queue-health readback:
- actionable: 0;
- reconciliation: 3;
- attention: 2;
- stale: 21;
- forensic: 26.

## Independent verification

Current dev.65 source:
- `cargo check --manifest-path wake/Cargo.toml`: PASS.
- `cargo check --features test-support --manifest-path wake/Cargo.toml`: PASS.
- `cargo clippy --manifest-path wake/Cargo.toml --lib --all-features -- -D warnings`: PASS.
- scoped `git diff --check`: PASS (existing line-ending notice only).
- current-source `CatDeskWakeHost status`: PASS against production Store; queueDepth 0 and terminal TARGET_DRIFT surface preserved.
- current-source `CatDeskWakeHost queue-health`: PASS, actionable 0 / reconciliation 3 / attention 2 / stale 21 / forensic 26.
- current-source `CatDeskWakeHost readiness-history`: PASS; bounded HOME/recovery history is readable separately and `status.json` contains no readinessHistory field.

A foreground root `cargo test` invocation without Wake's required `test-support` feature produced the expected test-only compile errors for `Store::open_scoped_for_test`; that run is not product-failure evidence. Earlier dev.64 tests with the required feature are retained above. Two dev.65 status assertions initially failed after queueDepth semantics changed; both were diagnosed rather than ignored: terminal ATTENTION was being normalized to IDLE and the old timer test expected SUBMITTING to count as deliverable. The implementation/test expectations were corrected, and current-source functional Store probes prove the corrected semantics.

## Activation boundary

Safe activation requires:
1. installed dev.63 remains PAUSED;
2. actionable queue is zero;
3. exact Chat33 generation 16 / digest remains unchanged;
4. immutable reviewed installer only;
5. after activation keep Wake PAUSED until package/status readback is verified;
6. then create exactly one NEW expected-generation/digest-bound diagnostic; never replay dev.63 or older records.

## Live acceptance

For the fresh dev.65 diagnostic require:
- existing active CDP tab converges from HOME/network recovery to exact Chat33 using in-place navigation;
- no multiplied browser-session churn;
- 10-second stable ready + 10-second idle gate before first write;
- one exact USER append and durable EXACT_USER_MESSAGE_APPENDED receipt;
- current live turn timer during response;
- terminal SENT / COMPLETE;
- actionable queue returns to zero;
- no USER duplicate.

If exact-target convergence still fails, stop receiptless ATTENTION and continue diagnosis without restoring multiplied retries.
