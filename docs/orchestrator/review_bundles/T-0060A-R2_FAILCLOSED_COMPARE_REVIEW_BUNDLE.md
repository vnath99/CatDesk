# T-0060A-R2 Fail-Closed Comparison Review Bundle

## Scope

This repair changes only the internal, read-only comparison path in
`scripts/catdesk-production-acceptance.ps1` and its deterministic fixtures.
It preserves the R1 measured wake, official-runtime, and canonical-listener
gates. It does not perform any live acceptance action.

## Decision

Comparison now validates each input as the complete fixed
`PRE_REBOOT_PREFLIGHT` schema-version-1 contract before considering build or
instance evidence. The validator rejects missing, duplicate, reordered,
unknown, malformed, or unsupported snapshot fields and gates without exposing
the input content or parse errors.

Final `PASS` requires both valid snapshots, unchanged canonical build, a new
bounded canonical instance fingerprint, `READY`, post overall `PASS`, and
`PASS` for every fixed post gate. The only permitted `ATTENTION` is
`LOCAL_READY_EXTERNAL_RUNTIME_PENDING`, where `LIFECYCLE_STATUS` is
`ATTENTION` and every other gate is `PASS`. All other cases are `FAIL`.

## Regression evidence

The fixture suite covers complete healthy PASS, valid pending-runtime
ATTENTION, failed wake binding, failed external runtime ownership, autostart
attention, missing/duplicate gates, wrong stage/schema, invalid identities,
unchanged instance, and handcrafted minimal snapshots. It also retains the R1
redaction and measured-evidence cases.

Observed local checks:

- `scripts/test-catdesk-production-acceptance.ps1` passed.
- `scripts/test-catdesk-lifecycle.ps1` passed.
- `scripts/test-catdesk-autostart-supervisor.ps1` passed.
- `scripts/test-start-catdesk-stack.ps1` passed.
- `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, and `git diff --check` passed.
- `cargo test` ran 461 tests: 443 passed, 11 ignored, and 7 pre-existing
  host-only failures remained: unavailable advisor program and Windows
  process-tree cancellation access-denied cases. No Rust source changed for R2.

## Boundaries and review handoff

No task registration/removal, daemon/runtime/tunnel start-stop-reload, browser
launch, reboot, shell-policy change, credential access, or Git publication was
performed. Direct live preflight through MCP `run_command` remains blocked by
the existing restricted allowlist; R2 does not change it. Independent review
and normal review-inbox wake are CatDesk-host actions and remain pending.
