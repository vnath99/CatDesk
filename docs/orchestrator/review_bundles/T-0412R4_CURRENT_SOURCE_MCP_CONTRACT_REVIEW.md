# T-0412R4 — Current-source MCP contract review

## Scope

This review establishes a fresh authority boundary for the current source after the accepted T-0412R3 MSVC/Windows SDK environment repair. It does not reopen T-0412R2/R3 and does not treat the older source-current bootstrap session as authority for changes made after that session captured its baseline.

The attributable current-source scope is limited to:

1. `catdesk_reviewed_build` MCP schema/handler agreement.
   - PREPARE/PREFLIGHT advertises and requires exactly `action` + `recordId`.
   - CONFIRM advertises and requires `action` + `confirmationToken`.
   - RESULT advertises and requires `action`.
   - action-specific schemas remain closed with no unrelated caller-controlled fields.
   - the advertised PREPARE/PREFLIGHT requirement matches the strict `reviewed_build` handler's required `recordId`.

2. `catdesk_turn_timer` MCP schema/handler agreement.
   - the generic plan-guard schema layer advertises optional `allow_without_plan`.
   - the timer handler now accepts that same advertised guard key instead of rejecting a schema-valid call.
   - `allow_without_plan` does not alter timer target, generation, digest, duration, path, executable, browser profile, credentials, or other Wake authority.
   - START remains server-generated; STATUS/STOP still require the exact timer ID; only manual timers may be stopped through this surface.

## Regression evidence

- Existing reviewed-build regression `reviewed_build_schema_is_closed_and_accepts_minimal_preflight` expects PREPARE/PREFLIGHT required fields `["action","recordId"]`.
- New timer regression `shared_turn_timer_accepts_advertised_plan_override_argument` verifies a schema-valid `allow_without_plan` argument reaches the normal timer semantic validation rather than the unsupported-key rejection.
- The earlier full `cargo test` that had timed out at the ChatGPT client was recovered from its retained CatDesk log. That pre-timer-fix run completed its main suite with 964 passed, 0 failed, 22 ignored and green downstream suites. It is historical evidence only and is not claimed as verification of the new timer source change.

## Required final verification

Use the contract-approved `rust_full` verification path: formatting, strict Clippy, full Cargo tests, authoritative diff checks, and independent final review. Only failures attributable to this narrow scope may be repaired here. Do not clean/reset the inherited dirty worktree.

## Preserved boundaries

- No reviewed promotion, daemon reload, Wake installation, Wake target mutation, Git publication, or recovery-authority mutation in this review session.
- Current canonical project/Wake target remains Chat36 generation 19.
- Existing PAUSED T-0425 remains untouched and must not be duplicated.
- T-0424 remains quarantined and non-replayable.
- The externally owned official Secure MCP runtime remains untouched.
