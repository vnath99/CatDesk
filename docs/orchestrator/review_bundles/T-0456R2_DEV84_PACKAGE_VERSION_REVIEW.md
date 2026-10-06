# T-0456 R2 — dev.84 immutable package identity

## Scope

Session: `adc-t0456r2-dev84-package-version-20261004`  
Task: `t0456r2`

Wake-only packaging closure after independent acceptance of T-0456R1. No installation/activation, Git/GitHub mutation, target-authority change, browser-profile mutation, protected-build action, or ordinary autonomy is authorized by this session.

## Change

Advance the CatDesk independent Wake package identity from `1.0.0-dev.83` to `1.0.0-dev.84` consistently in:

- `wake/Cargo.toml`
- `wake/Cargo.lock`
- root `Cargo.lock`

No behavior changes are introduced by R2. The behavior being packaged is the independently accepted R1 selector-tier receipt repair in `scripts/wake_bridge.py`, together with the already-present same-canonical Chrome-error receipt recovery.

## Reason

The reviewed installer derives immutable package identity from `wake/Cargo.toml`. Reusing dev.83 after changing `wake_bridge.py` would collide with the existing immutable dev.83 package identity and would make source/package provenance ambiguous. A fresh dev.84 version is therefore required before reviewed installation.

## Verification

Readback confirms exactly one `1.0.0-dev.84` catdesk-wake package identity in each of the three manifests/lockfiles above.

Ad-hoc direct Cargo invocation through the serving MCP command policy was rejected before execution. That guard is preserved. Contract finalization must run the approved project-test, strict Clippy, and Git-diff verification profiles; R2 is not acceptable unless the finalizer returns PASSED.

## Acceptance

After R2 independent review and ACK, run only the existing `wake/install.ps1` reviewed immutable install path. Preserve canonical Chat45 generation 28 and the dedicated authenticated profile. Then create exactly one fresh manual diagnostic/canary. Require exact generation-28 `EXACT_USER_MESSAGE_APPENDED`, no duplicate USER message, response completion, terminal `SENT`, timer `COMPLETE`, queue depth zero, and browser cleanup before lifting the Wake freeze.
