# T-0426 — Wake dev.64 in-place HOME navigation repair

## Trigger

Installed dev.63 correctly decompounded TARGET_DRIFT retry ownership, but the fresh generation-16 diagnostic `manual-wake-dev63-decompounded-readiness-003` still terminated receiptless `ATTENTION / TARGET_DRIFT`. The separate readiness journal makes the failure deterministic: the browser remained on `HOME`, then two adapter-owned recovery attempts each emitted `RECOVERY_BEGIN -> RECOVERY_RETURN` and still returned `HOME`. No outer Rust fresh-session churn occurred.

The configured Wake target itself remains exact canonical Chat33 generation 16 / digest `d32961ec350c42d039f5514e9171a59574674ecc291b82c0c28f4d9c14336212`, so this is not authority drift.

## Root cause and repair

The production adapter's HOME recovery used SeleniumBase CDP `open(url)`. The established smoke-test navigation path uses `get(url)` to navigate the already-active CDP tab. In production dev.62/dev.63, the `open` recovery returned to ChatGPT HOME instead of converging on the exact conversation.

dev.64 changes only the pre-write HOME/network exact-target navigation primitive from `cdp.open(self.url)` to `cdp.get(self.url)`, retaining `activate_cdp_mode(self.url)` only as the existing fallback when the active CDP object lacks the navigation method. The target remains fixed by the bound Wake event; no caller-selected URL, new browser context, target mutation, receipt mutation, or post-write retry is introduced.

The persistent-HOME tests now model `get` navigation, and the direct recovery test requires the active CDP tab to receive the exact target.

## Verification

- Wake library + protocol/store: PASS — 37/37 + 26/26.
- Embedded Python validation: PASS — 3/3, one intentional manual harness ignored.
- Strict Wake library Clippy `-D warnings`: PASS.
- Production readiness journal was read-only inspected before source mutation:
  - dev.59 diagnostic: HOME/TARGET_DRIFT followed by three multiplied recovery sequences (historical evidence).
  - dev.63 diagnostic: HOME/TARGET_DRIFT followed by exactly one adapter-owned pair of bounded HOME recovery attempts; no multiplied Rust fresh-session churn.

## Live boundary

Installed dev.63 remains PAUSED and its fresh diagnostic remains terminal receiptless `ATTENTION / TARGET_DRIFT`. Do not retry/replay it. dev.64 is source-only and MUST NOT be installed until an independent review accepts this narrow navigation change.

After reviewed immutable dev.64 activation:
1. confirm exact Chat33 generation-16 authority and status parsing;
2. publish exactly one new generation/digest-bound diagnostic ID;
3. require HOME recovery to navigate the existing active CDP tab to the exact conversation without creating another context;
4. if exact target becomes ready, require 10-second stable-ready + 10-second idle, exact USER receipt, live timer, terminal SENT/COMPLETE, and no duplicate USER wake;
5. if HOME still persists, stop receiptless ATTENTION and investigate profile/router behavior without restoring retry multiplication.

External Secure MCP ownership, dirty worktree, historical quarantined evidence, and no-Git-publication policy remain unchanged.
