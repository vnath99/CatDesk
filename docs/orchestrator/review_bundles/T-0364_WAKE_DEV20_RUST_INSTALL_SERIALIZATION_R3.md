# T-0364 Wake dev.20 Rust-owned publication/activation serialization

Date: 2026-09-19
Canonical Wake target: https://chatgpt.com/c/6aaecf61-e314-83e9-afa7-ad10a5c495a5
Canonical target generation: 10
Canonical target digest: 86752787e1be8a3f1ff0dd9d76a3224061c35952ff3b22a073ff7cda47513732

## Starting live state

- dev.19 remains the installed healthy fallback while dev.20 is built.
- dev.19 immutable current directory: 1.0.0-dev.19-1a7c36141896-35374192c52d.
- Wake status: RUNNING, browser/login NOT_OBSERVED, submission IDLE, queueDepth 0, staleCount 0.
- Secure MCP CONNECTED_VERIFIED; local MCP READY; generation-10 target/digest unchanged.
- Previous lingering installer process is gone and install.lock readback is FREE.

## Defect being removed

PowerShell previously opened install.lock for the whole installer lifetime. A successfully activated package could leave the PowerShell process alive, retaining install.lock and blocking later upgrades. In addition, the pre-publication same-version scan was not sufficient to serialize two concurrent installers: two processes could both observe no existing final candidate and publish conflicting immutable directories.

## dev.20 transaction model

- PowerShell no longer opens or owns install.lock.
- PowerShell builds artifacts and creates a unique inert .staging-<id>-<pid>-<guid> directory only.
- New Rust command publish-reviewed-install acquires the OS-backed exclusive install.lock.
- Rust publication validates staging name, manifest schema/version/protocol/acceptance, all four artifact hashes, and the canonical host/gui-derived install ID.
- Under the exclusive lease it refuses any different final identity for the same Wake version.
- Re-publication of the exact same artifact identity is idempotent; the redundant staging directory is removed.
- Drift in adapter.py or wake_bridge.py under the same host/gui-derived install ID is refused as HOST_INSTALL_ARTIFACT_IDENTITY_CONFLICT.
- Reviewed candidate discovery explicitly ignores every .staging-* directory, even if an interrupted staging directory contains a complete manifest.
- activate-reviewed-install acquires the same exclusive install lease before candidate resolution and handoff.
- Zero reviewed candidates now report HOST_REVIEWED_CANDIDATE_UNAVAILABLE; multiple final candidates remain HOST_REVIEWED_CANDIDATE_AMBIGUOUS.

## Verification so far

- cargo fmt --check: PASS.
- cargo test -q -p catdesk-wake: PASS: 8 runtime/install tests, 3 process-tree tests, 22 protocol/store tests.
- New hostile tests cover complete-staging invisibility, idempotent same-identity publication, same-version different-identity refusal, hidden adapter drift refusal, and Windows exclusive install lease.
- cargo clippy -q -p catdesk-wake --lib --bins -- -D warnings: PASS.
- Focused Windows installer/recovery contract test: PASS; it now asserts PowerShell has no package lock authority and delegates publication+activation to Rust.
- git diff --check: PASS (normal Windows LF/CRLF warnings only).
- dev.20 WakeHost release build: COMPLETE.
- dev.20 Binagotchy/CatDesk release build: currently active in rustc LTO/link; active compiler repeatedly demonstrated ~full-core CPU progress. Root `[profile.release]` explicitly sets `lto = true` and `codegen-units = 1`, so multi-minute final linking is expected and must not be misclassified as a hang merely because the MCP command window expires. Additional Cargo chains are lock waiters from wrapper retries and must not be treated as separate active builds.

## Deployment acceptance for dev.20

ACCEPTED for package deployment and installer lifecycle:

- immutable current/reviewed directory: 1.0.0-dev.20-3edc6fe612cc-b1b13e47a959;
- reviewed-install-status reports compiled/current/reviewed version 1.0.0-dev.20 and alreadyCurrent=true;
- all four installed artifact hashes match manifest and current source;
- live WakeHost is RUNNING on PID 36064;
- generation-10 target URL/digest is unchanged;
- Secure MCP is CONNECTED_VERIFIED and local MCP is READY;
- queueDepth=0, staleCount=0, browser/login=NOT_OBSERVED, submission=IDLE;
- install.lock is FREE after activation;
- a repeated exact dev.20 installer run completed in 1.8 seconds, remained on the same immutable package/PID, reported AlreadyMaterialized=true / alreadyCurrent=true, and returned ShortcutState=READY;
- process inventory after the repeated run contains no wake/install.ps1 process. This proves the bounded shortcut child closes the prior presentation-only PowerShell lifetime leak;
- an earlier apparent duplicate installer/replay converged on the same immutable package, providing live evidence that Rust publication/activation serialization is idempotent for the exact same artifact identity.

Natural Wake acceptance remains a separate boundary: a fresh legitimate generation-10 CatDesk review event must yield a real persisted USER message in the canonical chat plus a correlated durable EXACT_USER_MESSAGE_APPENDED receipt. Review inbox pagination confirmed there are no existing post-generation-10 records, so no historical event is eligible. Test-chat/manual diagnostics do not count.