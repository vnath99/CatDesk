# T-0433 — dev.69 package pre-install review

## Verdict

ACCEPT DEV.69 FOR THE REVIEWED ZERO-ARGUMENT IMMUTABLE INSTALLER TRANSACTION,
CONDITIONED ON THE INSTALLER'S OWN EXACT FOUR-ARTIFACT HASH/READBACK GATES.

This review does not claim activation has occurred and does not waive artifact
identity checks.

## Source/package verification

- T-0432 current source is accepted by
  `T-0432_R7_RESILIENCE_SOURCE_REVIEW.md`.
- `adc-t0433-dev69-verification-r1-20260927` is
  `COMPLETED_VERIFIED`.
- Its verifier explicitly compiled `catdesk-wake v1.0.0-dev.69`.
- Strict contract-approved Clippy passed.
- Full root Cargo suite passed: 991 tests.
- The permanent root Wake-local harness is part of that suite and therefore the
  ordinary Wake-local tests passed under dev.69 identity.
- Locked/offline WakeHost release build passed.
- Locked/offline Binagotchy release build passed.
- `wake/Cargo.toml`, `wake/Cargo.lock`, and root `Cargo.lock` consistently
  identify `catdesk-wake 1.0.0-dev.69`.

## Artifact-hash boundary

The hardened MCP shell permits the checksum commands in dry-run but rejects
their live execution. That command-policy boundary is not weakened.

Therefore pre-install ChatGPT-side hashing is not used as an alternate authority.
Instead, the already-reviewed zero-argument `wake/install.ps1` transaction must
remain the only promotion path. Before any pointer activation it:

1. rebuilds WakeHost locked/offline;
2. obtains the existing isolated Binagotchy release artifact;
3. computes SHA-256 for:
   - `CatDeskWakeHost.exe`
   - `CatDeskBinagotchy.exe`
   - `adapter.py`
   - `wake_bridge.py`;
4. derives the immutable install ID from version + host/binagotchy hash prefixes;
5. copies into a unique inert staging directory;
6. re-hashes every staged artifact and refuses any mismatch;
7. writes the exact manifest;
8. delegates publication to `publish-reviewed-install`, which validates the
   staged candidate under Rust authority;
9. delegates stop -> exact-pointer switch -> desired-state restore to
   `activate-reviewed-install` under the reviewed install lease;
10. returns the exact artifact hash map and activation readback.

Installation is accepted only if that transaction returns version dev.69,
the exact immutable install directory, all four artifact hashes, preserved prior
desired state, and successful current-pointer/host readback. Any mismatch leaves
dev.68 as the accepted serving package.

## Included T-0432 behavior

dev.69 contains the accepted singleton-held resilient WakeHost loop with:

- narrow transient-error retry allowlist;
- fail-closed integrity/activation/target/receipt/package errors;
- desired=RUNNING, no-install-handoff, exact current-package/hash authority
  before retry;
- bounded backoff 1,2,4,8,16,32,60 seconds;
- maximum 8 consecutive recovery attempts in the five-minute reset window;
- in-memory carry-forward of recovery telemetry when immediate state persistence
  itself is unavailable;
- `hostRecoveryCount`, `lastHostError`, and `lastHostErrorUtc` surfaced
  through status/MCP/Binagotchy;
- existing CLAIMED/ATTENTION/SUBMITTING/SENT exactly-once semantics unchanged.

## Activation sequencing hold

Do not run the installer while the already-delivered R6 review wake is still
observing the current ChatGPT response. Its event-specific timer is still
`OBSERVING`, so stopping dev.68 now could interrupt the durable receipt/close
round trip for that wake.

The next bounded turn must first verify R6 reached terminal receipt/SENT/timer
COMPLETE/browser closed. Only then may dev.69 be installed.

### 2026-09-28 sequencing addendum

Fresh validated Store readback later proved that R6 did not converge to SENT: it remains historical `SUBMITTING` with no receipt and an expired `OBSERVING` timer. At the same time, current runtime state proves the browser observer this hold was protecting is no longer active: browser `NOT_OBSERVED`, submission `IDLE`, attention null, actionable queue depth 0, and canonical authority has advanced from generation 20 to 21.

The exact follow-up review `T-0433_R2_R6_HISTORICAL_SUBMITTING_SEQUENCING_REVIEW.md` therefore supersedes this sequencing hold only. R6 remains immutable ambiguous post-submit evidence and must never be replayed, force-completed, retired, or assigned a synthetic receipt. The reviewed zero-argument dev.69 install may proceed only while the R2 live preconditions remain true.

## After activation

Live T-0432 acceptance must safely induce a recoverable host-loop error with no
ambiguous USER submission and prove recovery telemetry, bounded same-process/
same-singleton retry, no replay/retarget, and successful normal event delivery.
