# T-0158 / T-0154-R2 LKG recovery authority bootstrap

## Reproduced live result

The closed cached request `{"decision":"CATDESK_CANONICAL_RECOVERY"}` scheduled a detached recovery worker.  Its durable result was `RECOVERY_AUTHORITY_REQUIRED`; `.catdesk/release-recovery/current.json` and `slot-a/manifest.json` were absent.  The traced path is `catdesk.ps1 recover` → `scripts/start-catdesk-stack.ps1` → `scripts/catdesk-release-recovery.ps1`: only a broken on-disk canonical pair enters LKG/interrupted-promotion restoration, so absent escrow cannot repair such a pair.

## Authority model and changes

- A disk binary plus sidecar is never recovery authority by itself.
- Normal reviewed promotion already snapshots the proven prior pair before mutation and the final proven canonical pair before transaction cleanup using alternating validated slots and an atomic pointer.
- A pre-LKG installation may now seed escrow only when its valid canonical pair matches an exact durable native reviewed-reload receipt *and* the receipt's exact replacement PID, process path, live binary hash, start-time continuity, and one/two loopback listener rows remain proven.  This is a migration for old reviewed convergence, not a hash-and-bless operation.
- A healthy canonical pair still restarts/replaces a mismatched running candidate through existing lifecycle authority without requiring LKG restoration.  Missing/damaged escrow blocks only broken-pair restoration.
- Recovery reports fixed redacted states: `RECOVERY_COMPLETED`, `RESTORED_KNOWN_GOOD`, `INTERRUPTED_TRANSACTION_COMPLETED`, `LKG_AUTHORITY_MISSING`, `LKG_AUTHORITY_AMBIGUOUS_OR_DAMAGED`, `TRANSPORT_VERIFICATION_FAILED`, and `RECOVERY_AUTHORITY_REQUIRED`.
- Durable recovery records now have generation/attempt identity.  A `RECOVERY_SCHEDULED` owner remains exclusive; a terminal record is retained as evidence but a later invocation creates a fresh generation and worker.  An old worker must match the currently scheduled attempt before it can act or persist a result.

## Crash ordering

Promotion first snapshots the proven prior pair, records its transaction, swaps and validates the candidate pair, proves canonical handback, then snapshots the newly proven pair before transaction cleanup.  A crash before the latter leaves the reviewed transaction and prior escrow for exact recovery; a crash after slot write but before pointer write is reconciled only from a unique valid generation.  The migration path writes a fully validated slot before the pointer and only after native convergence proof.  No recovery path creates authority from current bytes alone.

## Changed files

- `scripts/catdesk-release-recovery.ps1`
- `scripts/start-catdesk-stack.ps1`
- `catdesk.ps1`
- `scripts/test-start-catdesk-stack.ps1`
- `scripts/test-catdesk-lifecycle.ps1`
- `src/daemon_reload.rs`
- this review bundle

## Deterministic evidence

Added/extended coverage verifies a reviewed-convergence LKG migration fixture, refusal of an equally valid but unproven pair, LKG slot/pointer repair and damage behavior, binary/sidecar split-brain restoration, promotion LKG persistence, and terminal attempt rearm generation behavior.  Existing fixtures retain interrupted-promotion recovery, exact native receipt/PID/listener continuity, promotion crash windows, and healthy lifecycle paths.

Local commands passed:

- `cargo fmt --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test` — 586 passed, 18 expected ignores; two Windows lifecycle/recovery integration tests passed.
- `git diff --check`
- `powershell -NoProfile -ExecutionPolicy Bypass -File scripts/test-catdesk-lifecycle.ps1`
- `powershell -NoProfile -ExecutionPolicy Bypass -File scripts/test-start-catdesk-stack.ps1`
- `powershell -NoProfile -ExecutionPolicy Bypass -File scripts/test-promote-reviewed-catdesk-build.ps1`

No live recovery, fault injection, reload, promotion, tunnel/browser/wake/Scheduler action, external-project access, or Git publication occurred.

## Host acceptance

1. Build and deploy only through separately approved CatDesk release controls; prove the isolated candidate and normal `CONNECTED_VERIFIED` state.
2. Invoke exactly one closed recovery request (`catdesk_release_recovery` with `{}` or the fixed cached bridge).
3. Observe the durable bounded result.  Accept only `RECOVERY_COMPLETED`, `RESTORED_KNOWN_GOOD`, or `INTERRUPTED_TRANSACTION_COMPLETED` after local MCP READY and official Secure MCP CONNECTED/READY are independently proven.
4. Treat `LKG_AUTHORITY_MISSING`, `LKG_AUTHORITY_AMBIGUOUS_OR_DAMAGED`, `RECOVERY_AUTHORITY_REQUIRED`, and `TRANSPORT_VERIFICATION_FAILED` as fail-closed.  A later legitimate recovery may use a new generation; it must not overlap a scheduled attempt.
