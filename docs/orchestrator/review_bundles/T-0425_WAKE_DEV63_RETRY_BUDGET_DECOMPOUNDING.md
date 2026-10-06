# T-0425 — Wake dev.63 pre-submit retry-budget decompounding

## Trigger

Installed dev.62's separate readiness journal made the production failure sequence visible. The exact receiptless diagnostic `manual-wake-dev59-canonical-readiness-002` repeatedly landed on HOME and exhausted the Python adapter's bounded target recovery windows. Rust then treated terminal `TARGET_DRIFT` as fresh-browser-session retryable, multiplying the inner readiness budget by the outer three-session budget and recreating the operator-observed browser reopen churn.

The same receiptless TARGET_DRIFT was also automatically re-armed on every WakeHost startup/install, allowing an unchanged navigation fault to consume the budget again indefinitely.

## dev.63 repair

- `TARGET_DRIFT` is no longer eligible for a fresh Rust browser-session retry.
  - The adapter already owns the bounded exact-target readiness/recovery budget.
  - True `BROWSER_NETWORK_ERROR` and `BROWSER_ATTEMPT_TIMEOUT` remain eligible for the bounded pre-write fresh-session retry budget.
  - No retry occurs after the submit/write boundary.
- Startup recovery no longer automatically re-arms receiptless `TARGET_DRIFT`.
  - TARGET_DRIFT remains durable ATTENTION until an explicit sanctioned `retry-pre-submit` is requested after the underlying target/profile/navigation condition changes.
  - Receiptless `LOGIN_OR_PROFILE_REQUIRED` retains the existing startup re-arm behavior so a one-time operator login can recover without reconstructing state.
  - `CHATGPT_NOT_IDLE` keeps its existing delayed/finite automatic re-arm policy.
- No target, receipt, timer, timeout-Retry, submission, or immutable-install authority changed.
- dev.62's separate backward-compatible readiness journal remains intact.

## Verification

- `cargo test --locked --offline --manifest-path wake/Cargo.toml --features test-support`: PASS.
  - library 37/37
  - manual CLI integration 1/1
  - process tree 3/3
  - protocol/store 26/26
  - Python validation 3 passed + 1 intentional manual harness ignore
- Added/updated deterministic tests prove:
  - TARGET_DRIFT is not fresh-session retryable even while Claimed;
  - BROWSER_NETWORK_ERROR and BROWSER_ATTEMPT_TIMEOUT preserve bounded fresh-session retries;
  - no retry after Submitting;
  - receiptless TARGET_DRIFT remains ATTENTION across startup inspection rather than silently re-arming;
  - login-required and CHATGPT_NOT_IDLE recovery behavior remains separately bounded.
- Strict Wake library Clippy `-D warnings`: PASS.
- Scoped `git diff --check`: PASS; only the existing LF/CRLF notice for `scripts/wake_bridge.py`.

## Live acceptance

Installed dev.62's last diagnostic is terminal receiptless `ATTENTION / TARGET_DRIFT`, so no active USER delivery blocks activation.

After reviewed dev.63 activation:
1. confirm exact Chat33 generation-16 target and CatDesk status parsing;
2. confirm the old TARGET_DRIFT remains terminal rather than auto-rearming on startup;
3. publish exactly one fresh generation/digest-bound MANUAL diagnostic;
4. if HOME/TARGET_DRIFT persists, require only one adapter-owned bounded target-recovery sequence and terminal receiptless ATTENTION—no repeated fresh browser-session churn;
5. if navigation succeeds, require exact USER receipt, live timer, terminal SENT/COMPLETE, and no duplicate USER wake.

If HOME persists after decompounding, investigate the navigation primitive/profile/router behavior separately; do not restore multiplied retries.
