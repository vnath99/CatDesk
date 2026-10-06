# T-0264 / T-0224-R1B-R3G - Stable browser-adapter runtime bootstrap independence

## Disposition

Source/bootstrap preparation is complete for independent review. The durable runtime is intentionally absent from the real host, so the exact executable fixture reports the separately gated unprovisioned state without selecting an old venv or system interpreter. A present but incomplete, linked, or invalid runtime remains a failing fixture. This ticket does not activate Rust ownership, close T-0263/R1B, or authorize a live browser canary.

## Before and after authority chain

Before, the Rust `FixedScriptBrowserAdapter` used:

```text
StableWakeOwner -> .catdesk/wake-bridge/venv/Scripts/python.exe
                -> <workspace>/scripts/stable_wake_browser_adapter.py
                -> repository scripts/wake_bridge.py
                -> protected config/profile/target
```

The venv launcher in this workspace named an absent per-user Python 3.12 base interpreter.  Its ordinary venv ancestry was consequently a release-independent *failure mode*, but not a durable reviewed runtime.  The old source also depended on the repository copy of the adapter/browser primitives.

After, the only Rust adapter execution authority is:

```text
StableWakeOwner -> DurableAdapterRuntime
                -> <workspace>/.catdesk/wake-bridge/stable-runtime-v1
                   (pinned root + reviewed descriptor + fixed artifacts)
                -> fixed python.exe -I + fixed stable_wake_browser_adapter.py
                -> same-root reviewed wake_bridge.py primitives
                -> exact protected config/profile/target
```

No runtime path is selected from PATH, `py`, a per-user Python installation, repository venv ancestry, current `target/release`, daemon/MCP/tunnel state, reviewed-release manifest, promotion, or LKG.  Those sources are neither read nor used as fallbacks by `DurableAdapterRuntime`.

## Durable runtime contract

The fixed protected runtime root is `.catdesk/wake-bridge/stable-runtime-v1`.  Handle-pinned/no-follow directory traversal opens only that exact root.  It must contain the following fixed components:

| Component | Authority rule |
| --- | --- |
| `adapter-runtime.json` | schema 1, runtime version 1, bounded closed fields; exact descriptor SHA-256 is anchored in reviewed evidence. |
| `catdesk-stable-wake-owner.exe` | fixed reviewed regular artifact. |
| `python.exe` | fixed reviewed regular interpreter artifact. |
| `stable_wake_browser_adapter.py` | fixed reviewed stateless adapter artifact. |
| `wake_bridge.py` | fixed reviewed browser-primitives artifact beside the adapter. |

The external reviewed evidence remains at the fixed protected path `.catdesk/reviewed-build-control/wake-owner-artifacts/reviewed-artifacts.json`, now schema 2 with `runtimeDescriptorSha256` and `provenanceSha256`.  The descriptor binds the four artifact SHA-256 values.  Descriptor, runtime root, and each artifact are opened through `PinnedDirectory`/`bind_reviewed_regular_artifact`; symlink/junction/reparse, nonregular, oversized, replacement, digest, and stable-object identity drift all fail closed.

`FixedScriptBrowserAdapter::open()` opens only this runtime, and `attempt()` revalidates every descriptor/artifact immediately before launch.  It invokes the reviewed interpreter with Python isolated mode (`-I`) and the reviewed same-root adapter.  There is no interpreter/profile/target/script argument and no fallback path.  The adapter script resolves browser primitives only as its same-root reviewed sibling; it does not fall back to the repository copy.

## Preserved boundaries

- T-0256 exact target/profile identity, reparse refusal, and read-only config behavior remain intact.
- T-0260 expected-old selector CAS remains closed; preflight is stricter and now validates the durable runtime instead of `target/release`, source script, and repository venv artifacts.
- T-0262's exact no-evidence historical attention exception remains cutover-only; ordinary classification is still non-retryable.
- T-0263 selector-first dispatch remains unchanged: `legacy_python` is legacy-only, `rust` is Rust-owner-only, and malformed selection is terminal.
- Schema-4 delivery/receipt semantics, unresolved `SUBMITTING` non-replay, terminal `SENT` suppression, immutable canonical inbox, and one event/state authority are unchanged.

## Adversarial and dry-preflight evidence

`stable_wake_adapter_runtime` deterministic isolated fixtures cover:

| Case | Result |
| --- | --- |
| Valid durable descriptor and all five fixed components | opens/revalidates without launching a browser (dry preflight). |
| Missing runtime | refused; no venv/PATH/`py` fallback. |
| Corrupt descriptor or unsupported runtime version | refused. |
| Replaced adapter or removed interpreter after open | revalidation refuses before process launch. |
| Runtime-root symlink/reparse redirect | refused when the Windows fixture can construct it. |
| Current release binary replaced/removed; reviewed-release manifest malformed; daemon/MCP/LKG absent | durable runtime validation remains unchanged. |
| Runtime artifact replacement before owner CAS | selector remains absent; activation refuses. |

The existing owner, delivery, lock, core/bootstrap, CAS, attention, receipt, restart, and concurrency fixtures remain green.  They prove no runtime result can choose a second owner, profile, target, canonical event authority, or delivery-state authority.

## Verification truth

| Command | Result |
| --- | --- |
| `cargo test stable_wake_adapter_runtime -- --nocapture` | PASS - 5 runtime identity/reparse/independence tests. |
| `cargo test stable_wake_bootstrap -- --nocapture` | PASS - 3 canonical read-only bootstrap tests. |
| `cargo test stable_wake_owner_mode -- --nocapture` | PASS - 8 closed CAS/preflight/concurrency tests. |
| `cargo test stable_wake_owner -- --nocapture` | PASS - 15 owner/adapter/state-machine tests. |
| `cargo test stable_wake_delivery -- --nocapture` | PASS - 13 delivery/history/ambiguity tests. |
| `cargo test --test stable_wake_delivery_lock -- --nocapture` | PASS - 3 kernel-lock hard-kill/contention tests. |
| `cargo fmt --check` | PASS. |
| `cargo clippy --all-targets --all-features -- -D warnings` | PASS. |
| `cargo build --bin catdesk-stable-wake-owner` | PASS. |
| `cargo test --test stable_wake_adapter_python -- --nocapture` | PASS - the absent durable root reports `stable wake durable runtime is not provisioned`; no old venv or system Python was selected. If the root exists, the exact fixed interpreter and fixture are required and executed. |
| `cargo test` | PASS after the unprovisioned-runtime gate was made explicit; the exact Python fixture remains mandatory after separately authorized host provisioning. |
| project `rust_full` | No separately runnable project command was discovered; it is not represented as a pass. |
| `git diff --check` | PASS. |

## Separately gated host provisioning/preflight (not performed by Codex)

1. Build or obtain the fixed reviewed self-contained Python runtime and the fixed owner/adapter/browser-primitives artifacts through the existing reviewed-artifact process; do not use PATH, `py`, or the old workspace venv.
2. Under the fixed stable root only, provision exactly the five listed components, with no links/reparse points and no alternate profile/target/owner state change.
3. Produce schema-2 reviewed evidence whose descriptor SHA-256 and provenance are verified by the existing protected reviewed-artifact authority.  Do not manually weaken or edit selector/state/inbox/config values.
4. Independently run the exact Cargo adapter fixture.  It must execute only `<workspace>/.catdesk/wake-bridge/stable-runtime-v1/python.exe` and the fixed fixture, with bounded output and no browser send.
5. Re-run durable preflight and the fixed target/profile/reparse checks.  Only after independent acceptance may the separate T-0263 expected-old CAS/canary/restart procedure resume.

## Attributable paths and prohibited mutations

The workspace was broadly dirty before this task.  T-0264 attributable source/test paths are:

- `src/stable_wake_adapter_runtime.rs`
- `src/stable_wake_owner.rs`
- `src/stable_wake_owner_mode.rs`
- `src/main.rs`
- `src/bin/catdesk-stable-wake-owner.rs`
- `src/bin/catdesk-stable-wake-activate.rs`
- `scripts/stable_wake_browser_adapter.py`
- `tests/stable_wake_adapter_python.rs`
- `tests/test_stable_wake_browser_adapter.py`
- this bundle

No real `.catdesk` state, selector, inbox, delivery history, target, profile, venv/runtime, daemon, release, tunnel, scheduler/service, browser, credentials, or external project was mutated.  No browser was launched, typed into, clicked, or sent.  Legacy Python remains the sole live submit owner.  No Git publication occurred.
