# T-0060C One-Command Promotion Review Bundle

## Operator workflow

The PowerShell-only maintainer helper has one short command shape:

```powershell
./scripts/promote-reviewed-catdesk-build.ps1 -BuildPath <reviewed-build>
./scripts/promote-reviewed-catdesk-build.ps1 -BuildPath <reviewed-build> -Execute
```

Without `-Execute`, it is a read-only preflight and returns compact JSON state
only. It reports candidate/canonical validation, whether promotion is needed,
and `tunnelAction: NONE`; it does not emit paths, hashes, configuration, or
runtime details.

## Promotion and rollback model

The helper requires a regular candidate file contained by the workspace with no
reparse traversal, and validates the existing canonical binary plus SHA-256
manifest before mutation. Execute mode uses the existing PID-scoped
`restart_catdesk_daemon.ps1` helper for both daemon handoffs:

1. Canonical listener to reviewed candidate; candidate listener/readiness must
   be confirmed before canonical disk state changes.
2. Save one validated prior canonical binary/manifest pair under
   `.catdesk/promotion-recovery`, atomically replace canonical binary/manifest,
   then candidate listener back to canonical release.

If phase one fails, the canonical disk release is untouched. If swap or final
handoff fails, the helper restores the validated prior pair and permits at most
one recovery handoff. Any unproven outcome is fixed operator attention; it does
not retry indefinitely or retain an unbounded promotion archive.

T-0060C-R1 hardens the on-disk binary/manifest boundary: staged candidate
binary and manifest are both checked before replacement; after every failure
window, canonical state is classified as valid candidate, valid prior, or
inconsistent. An inconsistent pair is restored immediately from the validated
prior pair. `OPERATOR_ATTENTION_SWAP` is never emitted for a mismatched pair;
unproven restoration is `ROLLBACK_UNPROVEN_OPERATOR_ATTENTION`.

## Security boundaries

The helper does not read credentials, connector/tunnel configuration, browser
or wake state, process command lines, or environment secret values. It does not
directly start/stop processes, create tasks, use browser automation, or manage
the external official Secure MCP runtime. Restart behavior is delegated only to
the existing PID-scoped helper.

## Fixture coverage

The dedicated PowerShell fixture suite covers read-only plan mode, outside and
reparse candidate rejection, invalid canonical manifest, already-current
candidate, successful two-stage promotion, phase-one failure without swap, swap
failure, final handoff rollback/recovery, bounded backup retention, compact
redaction, and absence of tunnel/browser/credential operations. All execute
fixtures use temporary files and injected handoff/listener seams; no live
binary, daemon, scheduler, browser, wake profile, or tunnel is touched.

Observed local checks: the promotion fixture plus lifecycle, autostart,
bootstrap, and production-preflight fixtures passed; `cargo fmt --check`,
`cargo clippy --all-targets --all-features -- -D warnings`, and `git diff
--check` passed. `cargo test` ran 467 tests: 449 passed, 11 ignored, and 7
host-only failures remained for the unavailable advisor executable and Windows
process-tree cancellation access denial. No Rust source changed for T-0060C.
