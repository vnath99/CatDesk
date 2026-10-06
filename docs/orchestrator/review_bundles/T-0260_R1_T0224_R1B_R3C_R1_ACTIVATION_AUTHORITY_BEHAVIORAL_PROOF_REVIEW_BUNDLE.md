# T-0260-R1 — activation authority behavioral proof

## Prior rejection and scope

The preceding T-0260 source slice was a false green because it had no
executable proof of selector CAS, concurrency, persistence, or artifact
adversaries. This corrective slice adds isolated-fixture behavioral tests only.
It did not invoke the dormant activation binary against this workspace, modify
the real `.catdesk/wake-bridge/owner.json`, launch a browser, or change the
legacy live owner.

## Exercised authority

The tests call the production `activate_reviewed_rust_owner` API. Its fixed
preflight uses the shared `bind_reviewed_regular_artifact` handle-relative,
no-follow reviewed identity primitive, fixed owner/adapter/Python paths, the
accepted canonical inbox parser, protected target reader, and schema-4 delivery
gate. No test API supplies an executable, adapter, Python, target, profile,
inbox, state, selector, or root path to the production call.

## Behavioral matrix

| Case | Result |
| --- | --- |
| Missing selector | `legacy_python` default accepted. |
| Exact legacy_python → rust | Succeeds and protected selector readback is `rust`. |
| Stale/replayed expected legacy after Rust | Fails CAS; no legacy replay is accepted. |
| Exact Rust idempotence | Succeeds only after repeating full fixed preflight. |
| Unknown selector | Refused before activation. |
| Two concurrent legacy contenders | Production delivery kernel mutex serializes them; exactly one crosses to Rust. |
| Injected failure after durable temp before replace | Selector remains parseable legacy; a subsequent transition yields parseable Rust. |
| Reviewed adapter digest mismatch after replacement | Handle-bound digest check refuses the replaced object. |
| Malformed target config/inbox and unresolved SUBMITTING state | Refused; fixture selector is not created. |
| Immutable durable inputs | CAS test verifies canonical inbox/config bytes unchanged and no delivery state is created. |

The fault seam is thread-local test-only instrumentation located immediately
before atomic replacement. Production still uses synced temporary bytes plus
Windows `MoveFileExW(REPLACE_EXISTING | WRITE_THROUGH)` and a protected
readback. It introduces no browser or inbox mutation authority.

## Changed paths and attribution

The workspace was broadly dirty before this task. T-0260-R1 changes are the
new behavioral fixture/fault tests in `src/stable_wake_owner_mode.rs` and this
bundle. The prior source-only T-0260 files remain untracked in the dirty
workspace and were preserved rather than reset.

## Verification truth

| Command | Result |
| --- | --- |
| `cargo test stable_wake_owner_mode` | PASS: 7 tests, including CAS/readback/replay/concurrency/fault/replacement. |
| `cargo fmt --check` | PASS. |
| `cargo clippy --all-targets --all-features -- -D warnings` | PASS. |
| `cargo test --test stable_wake_adapter_python` | BLOCKED: fixed project venv reports missing `C:\\Users\\Volap\\AppData\\Local\\Programs\\Python\\Python312\\python.exe`. No bypass or host repair was attempted. |
| Full `cargo test` rerun | All preceding Rust/unit targets completed, then the same exact fixed-Python fixture failed; therefore full verification is not green. |
| `git diff --check` | PASS; only pre-existing CRLF advisory warnings were emitted. |

No project-defined independently runnable `rust_full` command was discovered;
it is not represented as a pass.

## Prohibited mutations and residual boundary

No real selector/state/inbox/config/profile, browser, daemon/release,
Scheduler/service, Secure MCP/tunnel, Git, or external host state was changed.
Legacy Python remains the sole live browser-submit owner. The residual T-0261
boundary is independent source acceptance, repaired fixed-venv verification,
then a separately authorized host-live single-owner cutover/canary/restart
procedure.
