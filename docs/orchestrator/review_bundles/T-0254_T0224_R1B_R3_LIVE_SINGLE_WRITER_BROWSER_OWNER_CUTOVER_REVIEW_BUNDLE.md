# T-0254 / T-0224 R1B-R3 — controlled single-writer browser-owner cutover review bundle

## Scope and T-0253 boundary

T-0253 was accepted as a **dormant** Rust owner boundary.  It correctly
persisted `SUBMITTING` before an opaque adapter and did not create a second
live browser owner.  Its `cargo build --release --locked` attempt exceeded the
fixed 120-second local command ceiling without a compiler error.  That caveat
also applies to this slice; it is not recorded as a release-build pass.

T-0254 adds source/test cutover machinery only.  It does **not** write the
real workspace's `.catdesk` selector, invoke the new executable, launch a
browser, send a ChatGPT message, alter the configured target/profile, change
daemon/release ownership, or modify the legacy bridge/setup scripts.

The workspace was already broadly dirty, including prior untracked T-025x
sources.  Whole-worktree Git output is not a task diff.  Bounded session
attribution is:

| Path | Change |
| --- | --- |
| `src/stable_wake_owner_mode.rs` | Fixed, protected schema-1 `legacy`/`rust` selector; malformed/unsafe selector authorizes neither owner. |
| `src/stable_wake_owner.rs` | Fixed local Python adapter runner, 60-second kill boundary, bounded strict result parser, and source/behavior regressions. |
| `src/bin/catdesk-stable-wake-owner.rs` | No-argument Rust owner entrypoint selecting one canonical pending record only when the fixed selector is `rust`. |
| `scripts/stable_wake_browser_adapter.py` | New fixed stateless adapter, separate from the unedited legacy bridge. |
| `src/mcp.rs` | Legacy `catdesk_wake_bridge_run_once` checks the selector before resolving or launching Python and refuses in Rust mode. |
| `src/main.rs` | Registers the owner-mode module. |
| this bundle | Review evidence. |

## Old and new owner/writer paths

### Before T-0254

The live browser and schema-4 writer was the existing MCP route
`catdesk_wake_bridge_run_once` in `src/mcp.rs`.  After explicit confirmation it
launched the fixed project-local `scripts/wake_bridge.py`.  That legacy Python
bridge read canonical `.catdesk/autonomy/review-inbox.json`, wrote
`.catdesk/wake-bridge/state.json`, and drove browser interaction.

### Selected Rust mode after T-0254

`<workspace>/.catdesk/wake-bridge/owner.json` is an exact, bounded schema-1
selector:

```json
{"schemaVersion":1,"owner":"rust"}
```

The selector is read-only in this source slice.  Absence preserves the
existing legacy mode; `legacy` and `rust` are the only accepted values.  A
linked, non-regular, oversized, malformed, unknown, or otherwise unsafe
selector fails closed.  In `rust` mode the legacy MCP route returns
`WAKE_OWNER_RUST_SELECTED` **before** it resolves a record, hashes a bridge,
or launches Python.  Therefore no configured selector value enables both
legacy and Rust ownership.

The fixed no-argument `catdesk-stable-wake-owner` executable is the Rust path.
It requires `rust` selection, reads only the canonical inbox, picks one
deterministically classified pending record, then calls `StableWakeOwner` and
`FixedScriptBrowserAdapter`.  There are no caller-provided executable, script,
workspace, profile, target, state, inbox, or record arguments.

The adapter executes only the fixed project-local
`.catdesk/wake-bridge/venv/Scripts/python.exe` and
`scripts/stable_wake_browser_adapter.py`, with record/message/target SHA-256
values from the durable Rust submission boundary.  It has a 60-second
kill-on-timeout limit; stderr is suppressed and stdout must be a <=256-byte,
strict schema-1 fixed-vocabulary JSON result.  The adapter derives the exact
project config/profile itself and does not accept any path, target, profile,
credential, or workspace parameter.  It neither reads nor writes schema-4
state, receipts, locks, inbox acknowledgements, or a second event spool.

The adapter uses the already-proven browser primitives from the unmodified
legacy source only for browser interaction; its own source has no `state.json`,
canonical-inbox, write/replace/unlink, workspace-argument, or target-argument
authority.

## Durable W13 semantics and revalidation

Rust remains the sole state transition owner in Rust-selected mode:

1. capture the protected exact target digest;
2. claim only a uniquely actionable canonical record;
3. immediately revalidate canonical actionability plus captured target digest;
4. atomically persist schema-4 `SUBMITTING`;
5. make exactly one adapter attempt;
6. accept only a schema-1 receipt bound to exact record ID, normalized message
   digest, target digest, and a positive monotonic sent timestamp.

`CLAIMED -> SUBMITTING -> SENT` is retained.  Any unresolved `SUBMITTING`,
invalid/missing receipt, timeout, adapter crash, login/auth, CAPTCHA,
chat-unavailable, target-not-ready, DOM ambiguity, or post-submit uncertainty
sets bounded operator attention or remains non-retryable `SUBMITTING`.
Automatic replay is never authorized.  Only a typed definite-pre-submit result
restores a clean `CLAIMED` record.  T-0251 unresolved-ambiguity retention and
kernel-lock behavior, and T-0252 acknowledged/non-actionable exact terminal
retirement at the 128/129 boundary, are unchanged.

Canonical `.catdesk/autonomy/review-inbox.json` remains the only event
authority and is read-only.  The adapter cannot acknowledge it; the Rust state
machine uses the accepted canonical parser and safe nested-reference handling.

## Focused evidence

| Evidence | Result |
| --- | --- |
| `stable_wake_owner` focused tests | Passed: fake adapter success, definite pre-submit failure, timeout/crash/error and all uncertain outcomes, invalid receipt, stale record refusal, restart from `SUBMITTING` with zero resubmit, concurrent owner threads with one adapter invocation, parser bounds/duplicate-key/secret-field refusal, adapter source containment. |
| `stable_wake_owner_mode` | Passed: exact mutually exclusive selector and malformed-selector fail-close. |
| MCP `t0254_rust_owner_selection_disables_legacy_bridge_before_launch` | Passed: Rust selector blocks legacy dry-run before bridge launch. |
| `stable_wake_delivery` | Passed: 12 T-0251/T-0252 state, drift, immutable-input, attention, receipt, and 128/129 history tests. |
| `stable_wake_delivery_lock` | Passed: real child hard-kill kernel-mutex recovery and two-process contention/claim serialization. |
| Full `cargo test` | Passed: 745 passed, 21 ignored, 0 failed, including the existing child-process suites. |
| `cargo fmt --check` / strict clippy | Passed. |
| `cargo build --locked` | Passed. |
| `cargo build --release --locked` | Exceeded the fixed 120-second command ceiling during compile without a compiler error; **not a pass**. |
| `git diff --check` | Passed. |

The local shell did not have a `python` executable available, so Python syntax
or browser execution was not claimed as verified.  No Python adapter was run;
that avoids any possibility of a live browser submission in this provider
turn.

## Source regression / single-writer proof

- `selected_owner` admits exactly one of `Legacy` or `Rust`, never a combined
  state; invalid selection is an error.
- The legacy MCP handler gates before its bridge invocation.  A Rust-selected
  fixture verifies the gate.
- The Rust owner binary refuses any argument and refuses any non-Rust selector.
- `stable_wake_owner` source regression rejects legacy bridge route references,
  arbitrary workspace/target adapter arguments, Selenium, former event spools,
  and direct adapter state/inbox mutation patterns.
- The new adapter receives only opaque digests and fixed paths.  It emits no
  URL, cookie, token, profile, message, inbox, receipt, or browser-storage
  data.

## Bounded host-live canary procedure (not performed)

After independent source review, a host operator may perform exactly this
separate canary:

1. On an isolated reviewed candidate, write the exact `owner.json` selector
   atomically and prove the legacy MCP route returns `WAKE_OWNER_RUST_SELECTED`.
2. Verify the fixed Rust owner executable, fixed adapter script, fixed venv,
   protected target, and dedicated profile identity without changing them.
3. Generate one new canonical normal review event; do not reuse a historical
   `SUBMITTING` or `SENT` event.
4. Invoke only the no-argument Rust owner executable once.  Inspect schema-4
   state for `SENT`, schema-1 receipt, positive timestamp, and exact
   record/message/target digest binding.
5. Acknowledge/read back the canonical event by the ordinary continuation path,
   then test one restart.  A terminal or unresolved event must not resend.
6. If uncertainty occurs, retain `SUBMITTING`/operator attention and stop;
   never force a retry.  Do not revert selection to invoke legacy for that
   record.

No host-live canary, browser operation, scheduler/service install, daemon or
release cutover, target/profile mutation, tunnel operation, Git publication,
or signing/provenance work occurred here.  Remaining R1B work is independent
host installation/desktop ownership and independent canary review.
