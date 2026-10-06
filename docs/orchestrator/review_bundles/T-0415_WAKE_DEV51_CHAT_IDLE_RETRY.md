# T-0415 Wake dev.51 `CHATGPT_NOT_IDLE` retry review

## Classification

`CHAT_IDLE_RETRY_VERIFIED_AND_CHAT32_LIVE_ACCEPTED`

The bounded retry implementation is independently reviewed, installed as
immutable Wake dev.51, and now accepted end-to-end in canonical Chat32. The
original provider review encountered inherited sandbox/host verification
blockers, but a fresh post-review local full Wake run subsequently passed and
the exact T-0415 review naturally arrived as a real USER message with a durable
generation-15 `EXACT_USER_MESSAGE_APPENDED` receipt. No manual wake/test bridge
was used as acceptance proof.

## Authority and durable boundaries reviewed

- T-0414 is a completed generation-15 ordinary-review canary. Its event is a
  distinct current-generation event and remains eligible for the repaired
  receiptless pre-submit path; this review did not send or otherwise mutate it.
- T-0403R2 remains the immutable generation-14 `SUBMITTING` ambiguity on the
  previous target. The retry helper neither targets stale generations nor
  permits a `SUBMITTING` delivery to be replayed, downgraded, retargeted, or
  marked `SENT`.
- `wake/Cargo.toml` and `wake/Cargo.lock` identify only the independent Wake
  package as `1.0.0-dev.51`.

## Implementation findings

`maybe_retry_chatgpt_not_idle` reads the durable delivery each scan and returns
without mutation unless all of these are true:

1. the delivery is `Attention`, has no receipt, and its reason is exactly
   `CHATGPT_NOT_IDLE`;
2. `now - Delivery.updatedUtc` is at least 30 seconds; and
3. `now - Event.createdUtc` is no more than six hours.

It then calls the existing `Store::retry_pre_submit`. That method independently
requires `Attention` plus no receipt and exact equality between the stored
delivery target/generation and current configured target. It therefore refuses
receipt-bearing, stale-generation, `SUBMITTING`, and `SENT` records. The
runtime calls the helper in its 250-ms host scan before adapter spawn or claim;
the normal `CHATGPT_NOT_IDLE` error path first persists `Attention` while still
`Claimed` and exits that event pass. This is pre-submit only and restart-safe:
the deadline is recalculated from persisted `updatedUtc`, not process memory.
Other attention reasons, not-yet-due and expired records remain unchanged.

The sole attributable source adjustment in this review is test-only. The
runtime review-discovery and reviewed-install fixtures now use the established
feature-gated `Store::open_scoped_for_test` beneath a fixture-owned canonical
parent. Production `Store::open` and its volume-root/default-root authority are
unchanged. This was needed because the new dev.51 runtime regressions exposed
the provider sandbox's deliberate refusal to inspect the user-profile ancestor
chain through production `Store::open`.

## Verification evidence

| Command | Result |
| --- | --- |
| `cargo test --manifest-path wake/Cargo.toml --features test-support chatgpt_not_idle -- --nocapture` | PASS: 2 passed, 0 failed. |
| `cargo build --manifest-path wake/Cargo.toml --release --locked` | PASS: built `catdesk-wake v1.0.0-dev.51`. |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | PASS. |
| `git diff --check` | PASS (only inherited CRLF warnings). |
| `rustfmt --edition 2024 --check wake/src/runtime.rs` | BLOCKED by pre-existing formatting drift elsewhere in the same untracked runtime file; the bounded fixture edit was not used to reformat unrelated code. |
| `cargo test --manifest-path wake/Cargo.toml --features test-support` | BLOCKED: 20 passed; 3 historical `store::submission_reconciliation_tests` failed before assertions with `CONFIG_ROOT_UNAVAILABLE`. Those out-of-scope fixtures still call production `Store::open` on provider-sandbox temporary roots. |
| `cargo test --workspace --all-targets --all-features` | BLOCKED: Windows linker `LNK1104` could not open the existing locked `target/debug/deps/recovery_powershell-db4075349427637d.exe`. No attempt was made to stop, replace, or otherwise modify that host-owned output. |

`git status --short` reported 156 inherited worktree entries. This task did not
reset, clean, stage, commit, or overwrite any of them. Build outputs under
`wake/target` and `target-verify` are verification artifacts only.

## Non-actions during source review

The independent source-review ticket itself did not start, claim, type, submit,
or reconcile a Wake event and did not mutate target, tunnel, reviewed-build,
release/LKG, daemon, Git history, credentials, or external projects. The later
live acceptance below occurred only after independent review and immutable
installation.

## Post-review verification and live acceptance

After the independent review completed, the host-local verification blockers
were re-run outside the provider sandbox:

- focused `CHATGPT_NOT_IDLE` tests: 2 passed / 0 failed;
- full independent Wake suite: library 23/23, process-tree 3/3,
  protocol-store 24/24, with manual diagnostics ignored as designed;
- locked offline release build: PASS.

The reviewed zero-argument installer activated immutable package
`1.0.0-dev.51-dd90282c4fdf-57791d5851a3` and restored the prior desired
`RUNNING` state without touching the externally owned Secure MCP runtime.

Natural Chat32 acceptance then completed for exact review record
`review-adc-t0415-dev51-chat-idle-retry-20260923-6-independent_final_review`:

- a real USER wake message visibly arrived in canonical Chat32;
- Wake persisted receipt evidence `EXACT_USER_MESSAGE_APPENDED`;
- receipt target generation: `15`;
- receipt target digest:
  `56333f7656f08b0158c2c115562cf1e64d468ac037846a7f5a6bbb0dca8e9cb7`;
- receipt message digest:
  `e412ce06a710eaab1243286ebfb38867508d18134c3c0e4719dec50801ae89ad`;
- receipt `sentUtc`: `1790193011`;
- the exact autonomous review record was acknowledged only after that visible
  USER message and durable receipt were both present.

No manual bridge-run-once, test wake, stale replay, direct Store publication, or
hourly deadman was used as acceptance proof. Ordinary autonomy may therefore
resume. Historical T-0403R2 generation-14 `SUBMITTING` evidence remains
immutable and outside this acceptance lineage.

One observability follow-up remains: live Wake status can retain historical
`attention/submission` fields while other current-generation review events
remain queued, even though `lastReceipt` is a successful T-0415 receipt. This
does not invalidate the exact receipt, but should be folded into the existing
truthful Binagotchy/Wake telemetry plan so current work is never confused with
the last completed delivery.
