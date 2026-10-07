# T-0419 R1 — Source-Current Runtime / Recovery Independent Review

Date: 2026-10-07
Session: `adc-t0419r1-runtime-recovery-review-20261007`
Logical task: `T-0419-R1-RUNTIME-RECOVERY-REVIEW`
Baseline source commit: `f2d1f114eb2946f5d7fe280b6c2df825b2744919`
Review outcome: **PASSED**

## Review scope

Independent review of the exact source-current T-0419 runtime/recovery convergence implementation. This review is authority-only: it does not reload CatDesk, mutate the canonical release, alter Wake state, take ownership of the external Secure MCP runtime, or authorize reuse of the stale T-0418 reviewed-build attempt.

Reviewed surfaces:
- `catdesk.ps1` consumer lifecycle / diagnosis / recovery facade;
- `scripts/test-catdesk-lifecycle.ps1` deterministic failure matrix;
- `tests/recovery_powershell.rs` public PowerShell integration coverage;
- `src/tunnel.rs` serving executable vs canonical reviewed-release provenance;
- `src/state.rs` bounded transport identity projection;
- `src/delegated/autonomy_supervisor.rs` read-only daemon-reload RESULT surface and its tests.

## Findings

### 1. Recovery UX is reduced to one read-only diagnosis and one repair action — PASS

The supported facade is coherent:
- `catdesk.ps1 diagnose` performs read-only layer diagnosis;
- `catdesk.ps1 recover` delegates to the existing release-only recovery engine;
- `catdesk.ps1 status` remains the normal concise state surface.

Recovery does not introduce a second repair engine. It preserves recognized fixed gates such as `LOCAL_MCP_RESPONSE_TIMEOUT` and `RUNTIME_STATUS_TIMEOUT`, while unexpected internal output is reduced to a redacted fixed state.

### 2. Failure localization is deterministic and bounded — PASS

The doctor uses nine fixed ordered layers:

`LIFECYCLE_ENGINE -> CANONICAL_RELEASE -> RECOVERY_AUTHORITY -> LOCAL_MCP_CONFIG -> LOCAL_DAEMON -> LOCAL_MCP_PROTOCOL -> WAKE_RUNTIME -> OFFICIAL_RUNTIME -> CODEX_CLI`

Each diagnosis selects one primary layer and one bounded next action from `NONE`, `RUN_RECOVER`, `RUN_INSTALL`, or `OPERATOR_ATTENTION`.

The deterministic lifecycle fixture covers configuration failure, listener identity mismatch, MCP response timeout, Wake runtime failure, official runtime timeout, unclassified official-runtime failure, missing runtime client, Codex CLI unavailable, canonical release recovery, damaged recovery authority, and healthy state. Diagnostic output is checked against leakage of injected runtime details and URLs.

### 3. Serving-build provenance is materially stronger than compile-time git metadata — PASS

Transport identity measures the bytes of the actual running executable using SHA-256 and separately reads the canonical `target/release/catdesk.exe.sha256` sidecar. It reports `MATCH`, `MISMATCH`, or `UNKNOWN`.

This correctly avoids treating historical `gitCommit=unknown` / `dirtyBuild=unknown` as proof of source skew. The source includes a deterministic regression proving MATCH and MISMATCH behavior. Caching the serving executable digest per process with `OnceLock` is appropriate because the identity being proved is the currently executing process image.

### 4. Daemon-reload observability is read-only and preserves reviewed mutation boundaries — PASS

`catdesk_daemon_reload {"action":"RESULT"}` accepts only `action`, does not measure or execute a replacement candidate, and reports:
- whether active autonomous mutation blocks reload;
- whether a readable persisted preflight is present;
- the next valid action;
- `tunnelAction=none-external-tunnel-untouched`.

`PREFLIGHT` and `CONFIRM` remain the only mutating reload actions. Tests prove RESULT remains usable while mutation is active and does not weaken the existing active-mutation block on PREFLIGHT.

### 5. External Secure MCP ownership remains unchanged — PASS

The reviewed recovery/reload changes do not add tunnel creation, tunnel takeover, arbitrary executable selection, arbitrary shell input, or credential flow. Existing official-runtime ownership stays external, and the reload status explicitly projects no tunnel action.

## Verification evidence reviewed

- Recovery PowerShell integration suite: **5 passed, 0 failed**.
- Daemon-reload focused family: **9 passed, 0 failed**.
- Serving/canonical provenance regression: **passed**.
- Strict all-target/all-feature Clippy with `-D warnings`: **passed** for the T-0419 Rust slice.
- `cargo fmt --all -- --check`: **passed**.
- `git diff --check`: **passed**.
- The broad Rust run reached 1006 passing tests before the previously documented Windows AppContainer `PermissionDenied / Access is denied` host fixture. The reviewed T-0419 failures found during the first broad run were corrected and rerun green; the AppContainer host permission condition is not introduced by T-0419.

## Deployment decision

**PASSED for fresh source-current reviewed-build authority.**

This review does **not** authorize the stale T-0418 active reviewed-build generation `314db15b5d5a4569af43dabea186e46f`. The next valid sequence is:

`this fresh T-0419 review authority -> reviewed build PREPARE/CONFIRM -> require BUILD_ATTESTED -> reviewed promotion PREPARE/CONFIRM -> canonical release parity -> reviewed daemon reload -> live serving provenance + read-only reload RESULT acceptance`.

Any failure in that sequence remains fail-closed and must be diagnosed at the layer where it occurs. No raw reload or Secure MCP ownership change is approved.
