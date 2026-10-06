# T-0265 / T-0224-R1B-R3G-R1 — Launch-bound reviewed runtime lease

## Disposition

T-0264 was a false green: it verified the durable runtime, then dropped every
reviewed artifact handle before `Command::new` opened `python.exe` and before
Python reopened the adapter and its same-root browser primitive by pathname.
That left a same-user revalidate-to-spawn/import replacement window.

This source-only correction closes that window. It does not provision a
runtime, select Rust ownership, mutate real `.catdesk`, or launch a real
browser. Legacy Python remains the sole live submit owner.

## Authority lifetime

Before:

```text
validate descriptor/evidence -> open/hash files -> drop handles
  -> Command::new(runtime/python.exe)
  -> Python opens adapter and imports primitive by pathname
```

After:

```text
prepare_launch()
  -> revalidate fixed evidence + descriptor + owner identity
  -> retain interpreter + adapter + primitive handles
  -> Command::new(lease.interpreter_path())
  -> Python reads lease.adapter_path() and same-root primitive
  -> wait for child + bounded output
  -> lease.release_after_child_exit()
```

`DurableAdapterRuntime` also retains pinned handles for the workspace,
`.catdesk`, `wake-bridge`, and fixed `stable-runtime-v1` ancestors. The lease
borrows that runtime, so those directory objects and the three reviewed file
objects remain open through child completion.

## Windows binding semantics

`retain_reviewed_regular_artifact` reuses the existing protected no-follow
`RootDirectory`-relative open and stable SHA-256/object-identity validation.
The retained `fs::File` is opened with `FILE_SHARE_READ` only. Therefore the
child may read the fixed pathname, but a writer cannot obtain write access and
rename/delete cannot obtain the required `DELETE` access while the lease is
alive. This is not a claim of direct handle execution: CreateProcess and Python
still use their fixed reviewed pathnames, made non-replaceable by the retained
Windows handles. The lease is explicitly consumed only after child exit/output
collection.

No pathname-only recheck substitutes for the lease. `FixedScriptBrowserAdapter`
calls `prepare_launch`, uses only lease-derived paths, and releases the lease
after wait/output. It no longer performs a free-standing `revalidate()` then
uses runtime-derived pathname accessors.

## Preserved authority

- The fixed runtime root, schema-1 descriptor, schema-2 reviewed evidence,
  artifact names/hashes/sizes/provenance, and no PATH/`py`/system-Python/venv
  fallback remain unchanged.
- Existing delivery `begin_submitting` continues to revalidate canonical
  actionability and exact protected target binding before the adapter boundary.
  T-0256’s exact profile/reparse validation remains in the fixed adapter.
- The canonical review inbox, schema-4 delivery state, schema-1 receipt,
  unresolved `SUBMITTING` non-replay, terminal `SENT` suppression, selector
  CAS, and attention compatibility are unchanged.

## Adversarial/process evidence

The Windows `launch_lease_blocks_artifact_replacement_until_child_boundary`
test opens an isolated reviewed fixture, obtains the production launch lease,
and spawns a second test process against that fixture. The child attempts each
of write, rename, and delete substitution for `python.exe`,
`stable_wake_browser_adapter.py`, and `wake_bridge.py`, plus a runtime-root
rename. All attempts fail while the lease is held. After lease release, file
writes succeed; after runtime drop, the runtime-root rename succeeds. This
demonstrates that the retained handles, not a textual path check, are the
authority.

Existing T-0264 negatives remain: absent/corrupt/stale descriptor, runtime
reparse, adapter replacement before validation, interpreter removal, and
unrelated current-release/daemon/MCP/LKG changes fail closed or remain
irrelevant exactly as before. A second process-level fixture copies the bounded
test executable to the isolated reviewed `python.exe` path, launches it through
`Command::new(lease.interpreter_path())`, and has the child verify its own
digest plus the adapter/primitive digests against the descriptor. It imports
neither Selenium nor browser code and never contacts a ChatGPT conversation.

## Verification truth

| Command | Result |
| --- | --- |
| `cargo test stable_wake_adapter_runtime -- --nocapture` | PASS — 7 focused runtime tests, including cross-process replacement denial and lease-selected interpreter/adapter/primitive digest proof. |
| `cargo test stable_wake_owner -- --nocapture` | PASS — 15 owner/lease source-boundary tests. |
| `cargo test --test stable_wake_adapter_python -- --nocapture` | PASS — reports the separately gated unprovisioned durable root; no old venv or system-Python fallback is selected. |
| `cargo fmt --check` | PASS. |
| `cargo clippy --all-targets --all-features -- -D warnings` | PASS. |
| `cargo test` | PASS — 783 unit tests plus integration suites. |
| `cargo build --release --bin catdesk-stable-wake-owner` | PASS. |
| project `rust_full` | No separately runnable project command was discovered; not represented as a pass. |
| `git diff --check` | PASS. |

## Attribution and remaining boundary

The workspace was broadly dirty before T-0265. Narrow attributable paths are:

- `src/windows_protected_fs.rs`
- `src/stable_wake_adapter_runtime.rs`
- `src/stable_wake_owner.rs`
- this bundle

No runtime was provisioned. No real `.catdesk` selector, inbox, delivery
state, target, profile, or reviewed evidence was changed. No browser was
launched, typed into, clicked, or sent. No daemon/release/tunnel/Scheduler/
service operation or Git publication occurred.

Separate host provisioning must still install the reviewed durable runtime and
evidence under the fixed root. Only after independent source review and that
gated preflight may the separate expected-old-owner CAS, fresh ordinary W13
canary, receipt/acknowledgement readback, and restart/replay-suppression host
acceptance proceed.
