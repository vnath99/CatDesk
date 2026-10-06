# T-0060F-R3 Phase-1 Promotion Resume Review Bundle

## Scope

This repair adds a narrow, fail-closed continuation to
`scripts/promote-reviewed-catdesk-build.ps1` for the observed Phase-1 state:
the reviewed candidate already owns the sole loopback CatDesk listener, while
the canonical binary and manifest are still the validated prior pair and no
promotion transaction exists. It does not execute a promotion, reload a
daemon, inspect credentials, or operate the externally owned Secure MCP
runtime.

## Resume gate

Execute mode reaches the new gate only after the existing interrupted
promotion-transaction check, candidate containment/hash validation, and
canonical binary/manifest validation. It accepts resume only when all of the
following agree:

- the restart-handoff record is a bounded regular workspace file, has a known
  successful `RECOVERED_PENDING_TRANSPORT_CHECK` schema/state, and is no more
  than 15 minutes old;
- its SHA-256 equals the measured requested candidate; schema 2 records also
  bind the exact workspace and candidate path; the existing schema-1 handoff
  format remains safely usable because its record location binds the workspace
  and the live listener independently binds the exact candidate path/hash;
- exactly one loopback listener exists on the fixed MCP port, is `catdesk`,
  has the handoff's replacement PID, and has the exact resolved candidate path
  and measured hash.

An existing but stale, malformed, unknown, oversized, reparse-ambiguous,
mismatched, non-sole, wrong-path, wrong-hash, or otherwise unproven handoff
record returns `OPERATOR_ATTENTION_PHASE1` before backup, transaction, swap,
or any new candidate handoff. This avoids creating a duplicate candidate
daemon. A pre-existing promotion transaction continues to take the existing
T-0060C-R2 recovery path and is never bypassed.

On proof, the helper reuses that measured candidate PID and enters the already
existing sequence: validated backup, flushed transaction, canonical
binary/manifest swap, and one PID-scoped candidate-to-canonical handback.
`tunnelAction` remains `NONE`; the helper has no tunnel, browser, scheduler,
or direct lifecycle ownership.

## Durable handoff identity

The detached restart wrapper and worker now emit schema-2 handoff records with
the resolved workspace and build paths alongside the existing non-secret PID,
port, status, stage, timestamp, and SHA-256 evidence. Resume parsing retains
strict compatibility with the previously emitted schema-1 success record so
the already-observed Phase-1 event can be recovered without rewriting live
evidence.

## Deterministic fixture coverage

`scripts/test-promote-reviewed-catdesk-build.ps1` now covers a successful
already-live Phase-1 resume, legacy schema-1 evidence, post-success
idempotency, stale evidence, mismatched workspace/path/hash, malformed and
unknown state, no/wrong/duplicate listeners, transaction precedence, and
invalid canonical evidence. Each unsafe case asserts no promotion handoff or
canonical mutation. The fixture uses a synthetic temporary workspace only.

## Local verification

Completed during this worker pass:

- `scripts/test-promote-reviewed-catdesk-build.ps1`
- PowerShell parser checks for the promotion and restart scripts
- `cargo fmt --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test` — 457 passed, 18 ignored, 0 failed
- `git diff --check` (passed; existing CRLF notices only)

Authoritative full dirty-tree capture and independent CatDesk verification
remain pending. No live acceptance was attempted by this worker.

## Safe operator command after independent review

Only after CatDesk-host approval, the normal execute command can safely resume
the observed candidate if and only if the gate above proves it:

```powershell
.\scripts\promote-reviewed-catdesk-build.ps1 -BuildPath .\target\t0060f-r1-candidate\release\catdesk.exe -Execute
```

If evidence is not exact and fresh, it returns operator attention and leaves
canonical files untouched; do not retry through a separate daemon or tunnel
operation.
