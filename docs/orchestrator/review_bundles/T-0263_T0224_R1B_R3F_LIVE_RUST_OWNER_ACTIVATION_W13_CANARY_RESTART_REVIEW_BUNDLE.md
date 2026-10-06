# T-0263 / T-0224-R1B-R3F — Rust owner activation and W13 canary preparation

## Disposition

`READY_FOR_HOST_ACCEPTANCE` for the bounded source/test preparation only.  Codex did **not** activate the real selector, change real `.catdesk`, start a browser, submit a wake, or execute the live canary/restart procedure.  R1B is not claimed closed here.

## Inherited authority boundary

- T-0251 supplies the crash-safe schema-4 claim authority and keeps unresolved `SUBMITTING` non-retryable.
- T-0252 permits bounded retirement only for exact canonically acknowledged/non-actionable terminal `SENT` records.
- T-0253 supplies the stateless, one-attempt adapter boundary.
- T-0256 supplies fixed-profile reparse-safe validation and the fixed venv adapter fixture.
- T-0260-R1 supplies protected reviewed-artifact identity and expected-old-owner selector CAS.
- T-0262 permits only exact zero-evidence `OPERATOR_ATTENTION` records with `CHATGPT_NOT_IDLE` or `LOGIN_OR_PROFILE_REQUIRED` to be ignored by *cutover preflight*.  Their ordinary classification remains `SubmittingAmbiguous`; they are not retry authorization.

The sole event authority remains `.catdesk/autonomy/review-inbox.json`.  The sole delivery authority remains `.catdesk/wake-bridge/state.json` (schema 4) with receipt schema 1.

## T-0263 correction: automatic dispatcher ownership seam

Before this slice, `dispatch_actionable_wake_with_policy` could invoke the legacy `scripts/wake_bridge.py` path directly without consulting `.catdesk/wake-bridge/owner.json`.  That bypass could leave the legacy Python writer reachable after a valid Rust selection.

The dispatcher now evaluates `selected_owner(workspace)` before any legacy receipt/retry/bridge handling:

| Selector result | Eligible production owner |
| --- | --- |
| exact `legacy_python` | existing legacy bridge path only |
| exact `rust` | `dispatch_if_rust_selected` / `StableWakeOwner` only |
| absent/malformed/unknown/error | none; terminal fail-closed |

Rust selection delegates through `FixedScriptBrowserAdapter` and rechecks selection before claiming.  A source regression test proves the selector gate occurs before `Command::new(&python)` for the legacy bridge and that malformed selection has no legacy fallback.  The adapter remains stateless with respect to inbox and delivery-state mutation; Rust owns the schema-4 transition/receipt path.

This preserves W13: exact record ID, normalized message SHA-256, exact target SHA-256, positive send time, `CLAIMED -> SUBMITTING -> SENT`, receipt schema 1, no automatic replay from unresolved `SUBMITTING`, and T-0252-only terminal retirement.

## Executable evidence

| Command | Result |
| --- | --- |
| `cargo test automatic_wake_dispatch_selects_one_owner_before_legacy_bridge_invocation -- --nocapture` | PASS |
| `cargo test stable_wake_owner -- --nocapture` | PASS — 15 focused owner/mode tests, including terminal-SENT suppression, unresolved-SUBMITTING zero replay, stale target/actionability, fake adapter outcomes, and concurrency. |
| `cargo test stable_wake_owner_mode -- --nocapture` | PASS — 8 CAS/preflight tests, including expected-old success, stale/opposite replay refusal, atomic fault old-or-new persistence, reviewed artifact replacement refusal, and two contender serialization. |
| `cargo test stable_wake_delivery -- --nocapture` | PASS — 13 delivery tests, including schema/receipt binding, 129-history pressure, exact cutover-attention handling, and immutable inputs. |
| `cargo test --test stable_wake_delivery_lock -- --nocapture` | PASS — hard-kill release and two-process single-transition proofs. |
| `cargo fmt --check` | PASS |
| `cargo clippy --all-targets --all-features -- -D warnings` | PASS |
| `cargo build --bin catdesk-stable-wake-owner` | PASS |
| `cargo test` | BLOCKED after 755 unit tests and subsequent focused integration suites passed: fixed adapter fixture could not execute because its configured base Python is missing. |
| `cargo test --test stable_wake_adapter_python -- --nocapture` | BLOCKED: `No Python at C:\\Users\\Volap\\AppData\\Local\\Programs\\Python\\Python312\\python.exe`. No substitute interpreter, venv repair, or host mutation was attempted. |
| project `rust_full` | No separately runnable project command was found in checked Cargo/project metadata; not represented as a pass. |
| `git diff --check` | PASS |

The current plan records a prior host pass of the exact adapter fixture, but this provider workspace's fresh invocation produced the missing-base-interpreter failure above.  This bundle deliberately preserves the observed result rather than treating the historical statement as a local pass.

## Source/authority scans

Scans locate the canonical inbox at the stable core/owner/delivery boundaries and the schema-4 state only at delivery/legacy compatibility paths.  The automatic Rust dispatcher and MCP legacy path both consult the selector.  The only automatic legacy browser command in the Rust runtime is now downstream of the exact `legacy_python` branch; Rust selection uses the stable owner/adapter path instead.  The fixed adapter source continues to have no delivery-state or inbox write authority.

## Narrow attributable change versus dirty workspace

The repository was broadly dirty before this turn; its aggregate `git diff` is not task evidence.  T-0263's narrow attributable production/test hunk is in:

- `src/delegated/autonomy_runtime.rs` — selector-gated automatic dispatch and its source-order regression test.
- this exact review bundle.

No unrelated dirty path was edited intentionally for T-0263.  `git diff --check` reported no whitespace error.

## Provider-prohibited mutations not performed

No real owner-selector CAS, `.catdesk` write, browser launch/type/click/send, target/profile change, acknowledgement, daemon/release/tunnel/Scheduler/service action, Secure MCP action, external-project action, Git publication, or credential/profile inspection was performed.

## Independent host-live procedure (not performed by Codex)

1. Re-run the fixed project venv adapter/profile fixture and read only bounded non-secret selector, target/profile, inbox, and schema-4 state metadata.
2. Require exact `legacy_python` selected owner, successful reviewed-artifact/fixed-Python/profile/target/inbox/state preflight, and no unsafe unresolved delivery evidence.
3. Invoke only the closed reviewed activation tool with expected old owner `legacy_python`; do not manually edit `owner.json`.  Read back exact `rust` selection.
4. Confirm the legacy bridge is ineligible and the Rust dispatcher is the single submit/state writer.
5. Produce one entirely fresh ordinary W13 review record through the normal autonomy producer; do not reuse `SENT`, `SUBMITTING`, historical attention, or a manual wake record.
6. Require normal Rust-path delivery and read back schema 4 terminal `SENT`, receipt schema 1, exact fresh record ID, exact normalized message/target digests, and a positive browser send timestamp without exposing message or target text.
7. Read back the canonical acknowledgement/continuation evidence for that same record.
8. Restart/recovery-check terminal `SENT` and a controlled ambiguous `SUBMITTING`; both must invoke the adapter/browser zero times.  Any uncertainty remains unresolved and opens a narrow corrective ticket rather than being replayed.

Residual work is independent host-live CAS activation, fresh W13 canary, acknowledgement/receipt readback, and restart/replay verification.  Stable installation/bootstrap ownership remains outside this ticket.
