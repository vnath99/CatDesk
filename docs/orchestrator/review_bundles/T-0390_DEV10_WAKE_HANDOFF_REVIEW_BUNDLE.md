# T-0390 dev.10/dev.11 Wake Handoff Review Bundle

## Classification

`DEV10_WAKE_HANDOFF_REVIEW_READY`

This is a bounded no-product-source-change review of the current dirty
candidate. It does not install, register, start, stop, restart, or otherwise
mutate the independent WakeHost runtime.

## Scope and attribution

Reviewed only `wake/install.ps1`, `wake/Cargo.toml`,
`wake/src/runtime.rs`, `wake/adapter.py`, `src/mcp.rs`, and the directly
relevant `tests/recovery_powershell.rs` coverage. At review start,
`src/mcp.rs` was modified and the other five candidate files were untracked;
those are inherited candidate changes. This bundle is the sole
T-0390-attributable workspace mutation.

## Handoff findings

| Required boundary | Evidence in the reviewed candidate |
| --- | --- |
| Immutable package | `wake/Cargo.toml` and the installer both identify `1.0.0-dev.11`; the installer derives a hash-qualified directory and refuses an existing directory before copying artifacts. |
| Isolated Binagotchy build | The installer invokes the main CatDesk build with `--target-dir $guiTarget`, where `$guiTarget` is `wake/target/catdesk-gui`; it does not use the normal CatDesk target output. |
| Prior state | Before the handoff, the installer reads and validates the prior `control.json` desired state as only `RUNNING`, `PAUSED`, or `STOPPED`, and separately records whether the pre-existing activation has the required schema, owner, and positive acceptance time. |
| Old-owner barrier | It requests `STOPPED`, polls status for up to 50 iterations at 100 ms, and fails if the old owner is not observed stopped before `register-install`. Runtime startup also waits for a still-held lease after a `STOPPED` observation instead of treating it as a successful new start. |
| Activation and artifact validation | `register_install` requires the package to be directly below the canonical versions directory, recomputes all four artifact hashes against `manifest.json`, writes the old pointer to `previous.json`, then atomically publishes `current.json`. `start_installed` validates the current host hash and installed adapter/bridge hashes before launch. |
| Lifecycle preservation | Only a valid pre-existing activation plus prior `RUNNING` invokes `start`; `PAUSED` invokes `pause`; `STOPPED` stays stopped. Missing or invalid activation is not auto-started. |
| Restart surface | The MCP restart handler accepts only `confirm=true`, requires a nonempty configured target, stops and observes the old owner, and invokes the hash-verified `start_installed` path. It accepts no executable, package, target, migration, browser, or shell parameter. |
| Browser-before-claim | `Adapter::spawn` sends initialization and returns only after adapter `READY`; the runtime calls `store.claim` only after that successful return. The adapter emits `READY` only after the SeleniumBase browser session is constructed. |

The directly relevant recovery test asserts the dev.11 installer/version
identity, dedicated GUI target directory, bounded stop and conditional restart
markers, lifecycle handoff fields, console Binagotchy shortcut, and the
adapter readiness marker. The implementation itself supplies the stronger
ordering checks described above.

## Residual risks and boundaries

This review did not execute a Windows install/handoff, observe a real owner
lease release, invoke the MCP restart handler, or start a browser. Those are
host/runtime acceptance steps and remain separate. In particular, the
source-level recovery test checks installer markers rather than simulating a
real old-owner process; the bounded stop/status timeout and fail-closed error
paths remain the protection if a real handoff cannot complete.

No claim is made about live delivery, browser login, target configuration,
Secure MCP/tunnel state, Scheduler/service state, or external projects.

## Verification and prohibited-action audit

The approved contract permits `GIT_STATUS` and `GIT_DIFF` only, with
authoritative diff capture required. Both were used for bounded inspection;
`git diff --check` completed without a whitespace error. The worktree was
already dirty and was preserved without reset, clean, stash, checkout,
revert, staging, or commit.

No product source or test was changed. No Wake runtime/store/config/target or
browser/profile state was read or written. No Wake send/test, bridge run,
SeleniumBase/Python wake action, direct Store publication, daemon or release
operation, scheduler action, Secure MCP/tunnel operation, Git publication,
signing/elevation, or external-project action occurred.

## Independent final review request

Request independent final review of this source-only handoff assessment.
Normal CatDesk final-review publication is the only follow-on contemplated by
this task; it does not authorize a runtime upgrade or wake delivery.
