# T-0419 Runtime / Recovery Convergence — Source-Current Review Bundle

Date: 2026-10-07
Project: CatDesk
Ticket: T-0419
Branch: `orchestrator/chatgpt-codex-autonomous-loop`

## Scope

T-0419 makes ordinary CatDesk recovery diagnosable and operator-simple without taking ownership of the externally managed Secure MCP runtime.

Supported consumer lifecycle:
- `catdesk.ps1 status` — concise non-mutating state.
- `catdesk.ps1 diagnose` — bounded non-mutating layer diagnosis.
- `catdesk.ps1 recover` — the single ordinary repair action, reusing the existing reviewed release-only recovery engine.

## Diagnostic model

The source-current doctor reports one primary failing layer and one fixed recommended action across nine ordered layers:

1. `LIFECYCLE_ENGINE`
2. `CANONICAL_RELEASE`
3. `RECOVERY_AUTHORITY`
4. `LOCAL_MCP_CONFIG`
5. `LOCAL_DAEMON`
6. `LOCAL_MCP_PROTOCOL`
7. `WAKE_RUNTIME`
8. `OFFICIAL_RUNTIME`
9. `CODEX_CLI`

The output is bounded/redacted and does not expose credentials, routes, complete endpoints, browser storage, or arbitrary filesystem paths. Repairable local daemon/protocol/Wake/runtime failures map to `RUN_RECOVER`; missing official runtime client maps to `RUN_INSTALL`; damaged authority, unclassified runtime verification, and Codex CLI/auth failures map to explicit operator attention instead of blind mutation.

## Serving provenance / reload observability

Source-current transport identity additionally measures the SHA-256 of the actual running CatDesk executable and compares it to the canonical reviewed-release SHA sidecar, yielding `MATCH`, `MISMATCH`, or `UNKNOWN`.

Source-current `catdesk_daemon_reload` adds a closed read-only `RESULT` action reporting:
- active-mutation blocker;
- readable persisted preflight presence;
- next valid reload action;
- `tunnelAction=none-external-tunnel-untouched`.

Reviewed `PREFLIGHT` and `CONFIRM` remain the only reload mutations.

## Verification

- Full recovery PowerShell integration suite: **5 passed, 0 failed**.
- Expanded lifecycle failure matrix proves configuration, listener identity, MCP protocol, Wake runtime, official runtime timeout/unclassified failure, missing official runtime client, Codex CLI unavailable, recoverable canonical release, damaged recovery authority, and healthy state.
- Focused daemon-reload family: **9 passed, 0 failed**.
- Serving/canonical provenance regression: **passed**.
- Strict all-target/all-feature Clippy with `-D warnings`: **passed** for the Rust slice.
- `cargo fmt --all -- --check`: **passed**.
- `git diff --check`: **passed**.
- Broad Rust suite previously reached 1006 passing tests; the remaining host-only failure is the historical Windows AppContainer `PermissionDenied / Access is denied` fixture and is not introduced by T-0419.

## Deployment authority status

Do **not** reuse the current active reviewed-build generation as T-0419 authority. It is bound to T-0418 review record `review-adc-t0418r1-git-codex-allowlist-daemon-review-20261006-6-independent_final_review` and is terminal `BUILD_FAILED_OR_AMBIGUOUS` with protected Cargo exit 101. The current stable-supervisor preflight also reports `SUPERVISOR_STARTUP_POLICY_UNPROVEN`.

A legitimate live deployment therefore requires a fresh T-0419 independent review/snapshot authority covering the exact source-current bytes, followed by the existing closed sequence:

`fresh review authority -> reviewed build PREPARE/CONFIRM -> BUILD_ATTESTED -> reviewed promotion PREPARE/CONFIRM -> canonical release parity -> reviewed daemon reload -> live provenance/RESULT verification`.

No raw reload, stale review reuse, arbitrary shell, or Secure MCP ownership change is authorized by this bundle.

## Current source checkpoint

Source-current branch was verified remotely through commit `041ad9f71d1b11cf2814cb460036d6d02ee70e4c` before this review-bundle commit. The review authority created after this bundle must bind the later exact branch bytes, including this file and any milestone-only checkpoint commit.
