# T-0267 / T-0224-R1B-R3H — final host-live single-writer W13 acceptance

## Disposition

`READY_FOR_INDEPENDENT_SOURCE_REVIEW` only. T-0266 is accepted for its child-lifetime-safe retained launch lease, but R1B remains **open**. Source or controller green cannot close R1B: independent host-live CAS, one fresh normal W13 canary, exact receipt/acknowledgement readback, and bounded restart/replay evidence are still required.

Codex did not activate the real selector, modify real `.catdesk`, provision a runtime, launch/type/click/send a browser, create a live event, change target or profile, or operate daemon/release/Scheduler/service/Secure-MCP state.

## Exact owner and authority trace

The sole event authority is `.catdesk/autonomy/review-inbox.json`. The sole delivery authority is `.catdesk/wake-bridge/state.json`, schema 4 with receipt schema 1. The sole durable owner choice is `.catdesk/wake-bridge/owner.json`, schema 1, with the exact vocabulary below.

| Selector result | Eligible automatic browser/state owner | Rejected path |
| --- | --- | --- |
| absent or exact `legacy_python` | existing legacy `scripts/wake_bridge.py` route | `dispatch_if_rust_selected` rejects before claim |
| exact `rust` | `catdesk-stable-wake-owner` -> Rust owner -> stateless adapter | automatic dispatcher cannot reach legacy `Command::new(&python)`; Python bridge refuses |
| malformed, unknown, unsafe, or unavailable | none; terminal fail-closed | both owners |

`dispatch_actionable_wake_with_policy` evaluates `selected_owner(workspace)` before legacy receipt/retry/bridge handling. Rust evaluates the same selector before opening a claim. The legacy bridge independently accepts only exact `legacy_python` and returns `WAKE_OWNER_RUST_SELECTED` under Rust. The adapter has no inbox or delivery-state write authority. This is a mutually exclusive selected owner, not a fallback preference.

The production-caller inventory is bounded: the automatic runtime dispatcher
selects before either branch; the fixed Rust owner binary can only reach
`dispatch_if_rust_selected`; the MCP legacy facade checks the same selector and
refuses under Rust; and `scripts/wake_bridge.py` is the legacy state/browser
owner only after its own exact-legacy predicate. The activation binary may
write only the selector after preflight/CAS and launches neither owner nor
browser. The manual legacy facade is therefore not an acceptance path.

The closed no-argument `catdesk-stable-wake-activate` facade is the only reviewed activation surface. It invokes `activate_reviewed_rust_owner(workspace, LegacyPython)`: canonical workspace, kernel-backed lock, full preflight, expected-old comparison, atomic replacement, and protected readback. There is no caller input for workspace, selector, executable, runtime, Python, profile, target, inbox, state, or record. Stale or opposite expected owner, malformed selector, concurrent loser, or failed readback fails closed. Exact Rust idempotence is allowed only after every prerequisite is revalidated.

## Preserved preflight, runtime, and W13 semantics

Legacy-to-Rust activation requires canonical inbox schema/conflict freedom, exact protected target/config and T-0256 profile/reparse identity, safe bounded schema-4 delivery state, and fixed reviewed durable runtime identity. An unresolved `SUBMITTING`, top-level attention, post-submit ambiguity, invalid receipt, or conflicting state blocks it. T-0262 permits only exact `OPERATOR_ATTENTION` records with `CHATGPT_NOT_IDLE` or `LOGIN_OR_PROFILE_REQUIRED` and null browser timestamp, message digest, target digest, and receipt schema; their ordinary classification remains `SubmittingAmbiguous`, never retryable.

W13 remains exact record ID, normalized message SHA-256, exact target SHA-256, positive browser-sent time, `CLAIMED -> SUBMITTING -> SENT`, receipt schema 1, terminal SENT suppression, and no automatic replay from unresolved SUBMITTING. T-0252 retirement remains limited to exact canonical acknowledged/non-actionable terminal receipts.

After Rust selection, `FixedScriptBrowserAdapter` uses only the fixed durable runtime. `prepare_launch()` retains reviewed interpreter, adapter, primitive, and pinned root authority. T-0266 `RetainedAdapterChild` owns both child and `RuntimeLaunchLease`: normal completion, timeout, poll/wait error, output or parser error, and uncertain finalization retain the lease until terminal/reaped; irreducible reap uncertainty leaks rather than release a possibly-live pathname authority. There is no PATH, `py`, repository venv, current release, daemon, MCP, manifest, LKG, or caller-selected executable/profile/target fallback.

## Source and process evidence

| Evidence | Result |
| --- | --- |
| CAS success/readback, stale/replayed legacy rejection, Rust idempotence, malformed selector, atomic old-or-new fault | isolated `stable_wake_owner_mode` fixtures |
| concurrent activators | one kernel-serialized legacy-to-Rust transition; losing contender sees stale CAS |
| legacy selection, terminal SENT, unresolved SUBMITTING | Rust rejects before claim under legacy; terminal/ambiguous records do not invoke another adapter |
| claim/submitting crashes, target/actionability drift, success-before-receipt, timeout/post-submit uncertainty | owner fixtures preserve a single attempt and fail closed |
| schema-4 receipt/history, 129 pressure, unsafe/conflicting state | delivery fixtures preserve bounded W13 authority |
| reviewed runtime replacement and no fallback | durable-runtime process identity fixtures |
| T-0266 injected live-child poll error | child is killed/reaped before lease release; artifact substitution remains blocked during finalization |
| T-0267 addition | closed-entrypoint source regression proves both fixed binaries are no-argument and expose neither direct selector nor caller path authority; legacy Python recognizes only `legacy_python` |

## Independent host-live acceptance procedure — not performed by Codex

1. Read bounded non-secret metadata and require exact `legacy_python`, ready reviewed runtime, valid protected target/profile/config, valid canonical inbox/schema-4 state, and no unresolved non-exempt delivery authority.
2. From the fixed reviewed project workspace, invoke exactly the no-argument `catdesk-stable-wake-activate` facade. It is operator-only because it performs the real expected-old-owner CAS. Do not edit `owner.json`.
3. Read back exact `rust`, then prove the automatic legacy branch and direct Python bridge are ineligible before any Rust adapter attempt. Abort on any unexpected or malformed selection.
4. Create one completely fresh ordinary CatDesk autonomous canary through the normal durable producer. `catdesk_wake_bridge_run_once`, manual wakes, direct selector edits, ad-hoc Selenium/browser commands, fabricated review events, reused records, and historical SENT/SUBMITTING/attention records are explicitly invalid acceptance evidence.
5. Let normal event-driven Rust delivery reach only the exact protected target `https://chatgpt.com/c/6a8df9e6-8438-83ea-8c5c-47d863c89943`; do not expose message text, profile data, cookies, credentials, or a substitute target.
6. Read back the same fresh record's schema-4 `SENT`, schema-1 receipt, exact record ID, normalized message and target digests, and positive send time. Verify canonical acknowledgement/continuation evidence for that record.
7. Perform one bounded reviewed restart/recovery check. Terminal SENT and unresolved/ambiguous SUBMITTING invoke neither adapter nor legacy owner; selector remains Rust. Any uncertainty remains retained, never replayed.

## Verification truth

| Command | Result |
| --- | --- |
| `cargo test stable_wake_owner_mode -- --nocapture` | PASS — 9 focused CAS/preflight tests, including the T-0267 closed-entrypoint regression. |
| `cargo test stable_wake_owner -- --nocapture` | PASS — 17 owner/mode tests, including crashes, concurrency, SENT suppression, ambiguity, and T-0266 poll-error finalization. |
| `cargo test stable_wake_delivery -- --nocapture` | PASS — 13 claim/receipt/history/attention/read-only tests. |
| `cargo test stable_wake_adapter_runtime -- --nocapture` | PASS — 7 reviewed-runtime/replacement/process/no-fallback tests. |
| `cargo test --test stable_wake_delivery_lock -- --nocapture` | PASS — 3 kernel-lock hard-kill/contention tests. |
| `cargo test --test stable_wake_host -- --nocapture` | PASS — 4 standalone read-only process tests. |
| `cargo test --test stable_wake_adapter_python -- --nocapture` | PASS — reports intentionally unprovisioned durable runtime without fallback. |
| `cargo fmt --check` | PASS. |
| `cargo clippy --all-targets --all-features -- -D warnings` | PASS. |
| `cargo test` | PASS — 785 tests plus integration suites; 21 pre-existing platform-dependent tests remain ignored. |
| `cargo build --release --bin catdesk-stable-wake-owner --bin catdesk-stable-wake-activate` | PASS. |
| `git diff --check` | PASS; only pre-existing LF/CRLF advisories were emitted. |
| project `rust_full` | no separately runnable project command is defined; no pass will be fabricated. |

## Narrow attribution and prohibitions

The worktree was broadly dirty before T-0267. T-0267 attribution is limited to the new regression in `src/stable_wake_owner_mode.rs` and this exact bundle. No aggregate dirty-worktree diff is claimed. No real selector, state, inbox, config, target, profile, browser, canary, acknowledgement, runtime, daemon, release, tunnel, Scheduler/service, external project, credential, or Git state was mutated. R1B remains open pending independent host-live evidence.
