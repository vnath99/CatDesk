# T-0060C-R1 Crash-Safe Promotion Review Bundle

## Repair

The canonical `catdesk.exe` and adjacent SHA-256 manifest are now handled as a
logical release pair. Before either canonical path can change, the existing
validated canonical pair is copied to the single bounded promotion-recovery
backup and revalidated. The candidate binary and staged manifest are both
complete and hash-checked before either canonical path is touched.

Each replacement boundary is covered by a pair-disposition check. On an
exception, the helper determines whether canonical state is already a valid
candidate pair, a valid prior pair, or inconsistent. An inconsistent pair is
immediately restored from the validated prior pair and revalidated. Therefore
`OPERATOR_ATTENTION_SWAP` is emitted only while the canonical pair is valid
prior state. If restoration cannot be proven, the helper returns
`ROLLBACK_UNPROVEN_OPERATOR_ATTENTION` instead.

## Deterministic failure windows

The PowerShell fixture uses checkpoint seams to cover:

- failure after canonical binary replacement and before manifest replacement:
  previous binary and manifest are restored and match;
- failure after manifest replacement and before final revalidation: only a
  fully valid candidate-equivalent pair may continue;
- restoration failure: the outcome is explicit rollback-unproven attention.

Existing coverage continues to prove plan mode has no mutation, candidate
containment/reparse rejection, valid-canonical preconditions, two-stage
handoff, bounded backup retention, phase-one no-swap behavior, and final
handoff rollback. Fixtures use only temporary workspaces and injected listener,
handoff, checkpoint, and restoration seams.

## Boundaries and remaining risk

No live promotion, daemon restart, task action, browser/wake action, tunnel
action, configuration/auth inspection, or Git publication occurred. The helper
still relies on the existing PID-scoped restart handoff for real daemon
transition; CatDesk-host live acceptance remains a separate operator and
independent-review activity.

## Authoritative diff and local verification

R1 changes only `scripts/promote-reviewed-catdesk-build.ps1`, its deterministic
fixture, and the T-0060C review material. The script adds candidate/prior/
inconsistent pair classification, validated candidate pair staging, replacement
checkpoints, and fail-closed restoration handling; it does not alter Rust,
connector, tunnel, or lifecycle code.

The promotion fixture and existing lifecycle, autostart, bootstrap, and
production-preflight fixtures passed. `cargo fmt --check`, `cargo clippy
--all-targets --all-features -- -D warnings`, and `git diff --check` passed.
`cargo test` ran 467 tests: 449 passed, 11 were ignored, and seven existing
host-only failures remained (the unavailable advisor executable and Windows
process-tree cancellation access denial). Independent CatDesk verification and
diff capture remain pending.
