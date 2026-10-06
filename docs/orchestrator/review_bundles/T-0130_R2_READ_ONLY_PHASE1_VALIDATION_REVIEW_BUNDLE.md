# T-0130-R2: Read-only Phase-1 validation

## Scope

This change modifies only:

- `scripts/promote-reviewed-catdesk-build.ps1`
- `scripts/test-promote-reviewed-catdesk-build.ps1`
- this review bundle

No Rust/MCP source was inspected or changed. No live promotion, reload, tunnel,
browser, Scheduler, external-project, or Git-publication action occurred.

## Contract

`promote-reviewed-catdesk-build.ps1` now exposes exactly one additional mode:
`-ValidatePhase1`. It is mutually exclusive with `-Execute`; supplying both
terminates before the script's promotion `try` block and before any action.
Default plan mode and `-Execute` retain their prior paths.

The mode first uses the existing contained regular-file candidate validation,
candidate SHA-256 calculation, and canonical release/manifest validation. Its
bounded JSON result remains the existing schema-1 promotion result shape:
`state`, `candidateValidated`, `canonicalReleaseValidated`,
`promotionRequired`, and `tunnelAction=NONE`.

- Equal candidate/canonical hashes return `ALREADY_CURRENT` with
  `promotionRequired=false`.
- A required promotion calls the existing `Get-ProvenPhase1Resume` unchanged.
  `PROVEN` returns `PHASE1_PROVEN`; `UNSAFE` returns
  `OPERATOR_ATTENTION_PHASE1`.
- For `ABSENT`, the existing `Get-PromotionListener` must positively prove the
  exact canonical binary/hash. It then returns `PHASE1_NOT_STARTED`; missing or
  ambiguous proof returns `OPERATOR_ATTENTION_PHASE1`.

The new branch returns before `Invoke-PromotionHandoff`, backup creation,
transaction writes, canonical pair copy/swap, rollback, or canonical handback.
It does not own or act on a tunnel.

## Deterministic fixture evidence

The fixture now covers read-only validation for: already-current; native and
legacy receipt proof; same-PID dual-stack proof; absent Phase 1 with exact
canonical listener; malformed receipt, timestamp/hash/PID drift; distinct PID,
three-row, path, and process-start listener drift; missing/wrong canonical
listener; and `-ValidatePhase1 -Execute` rejection.

Every validation outcome asserts `tunnelAction=NONE`, unchanged canonical hash,
zero handoff calls, and no promotion-recovery backup/transaction directory.
Existing promotion, native/legacy receipt, listener, and rollback coverage is
retained.

The required initial invocation was attempted:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/test-promote-reviewed-catdesk-build.ps1 .
```

It was invalid in this environment because the fixture has no positional
parameter. The required immediate correct invocation was the one that passed:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/test-promote-reviewed-catdesk-build.ps1
```

Result: exit code 0; `reviewed build promotion fixture tests passed`.

## Security boundary

T-0127's `Get-ProvenPhase1Resume`, including native/legacy receipt, listener,
PID/path/hash, and process-continuity validation, remains the sole Phase-1
authority. The new mode consumes only its bounded state and never recreates
that authority or begins an execution handoff. Validation failure is
fail-closed and non-mutating.
