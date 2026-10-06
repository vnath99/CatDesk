# T-0435 — V5 historical terminal retry compatibility review

## Root cause

The protected reviewed-build active pointer currently binds source attempt
`c10522a0315f4416bf517b058f1e8072` to active attempt
`96c4e905f8e94e7da50d5e4b05cb6ec6`. The active attempt is terminal
`BUILD_FAILED_OR_AMBIGUOUS / CARGO_LINK_LIBRARY_NOT_FOUND` and has no child
`terminal-history/96c4...` or `retry-plans/96c4...` yet. Therefore the fresh
T-0434 PREPARE refusal is not an already-published competing retry lineage.

The active attempt records V5 environment-policy digest
`461ed38d8a3a90a9d4b7beb3f5cbe1935f38cd5ab15a799f4fb6299d2dc3af26`.
That digest is the exact earlier V5 environment policy without the later
`validated-fixed-root-msvc-sdk-environment` component. Current V5 source uses
the same V5 policy version/argv but a newer exact environment policy including
that fixed-root linker environment.

`validate_historical_terminal_attempt` calls `known_historical_build_policy`.
Before this repair, the V5 branch accepted only the current environment-policy
digest, while V3 and V4 already had exact historical digest allowances. The
already-terminal V5 attempt therefore failed historical validation after the
V5 linker-environment evolution and PREPARE mapped that refusal to
`REVIEWED_BUILD_BINDING_IMMUTABLE`.

## Repair

Current source adds one exact historical-only V5 environment digest:

`LEGACY_BUILD_ENVIRONMENT_POLICY_V5_PRE_FIXED_ROOT_LINKER_SHA256 =
461ed38d8a3a90a9d4b7beb3f5cbe1935f38cd5ab15a799f4fb6299d2dc3af26`.

`known_historical_build_policy` accepts that digest only when the policy still
has the exact V5 version, exact V5 argv and argv digest, and valid policy
version digest. Fresh `fixed_policy()` remains unchanged and continues to use
the current fixed-root MSVC/SDK environment. No arbitrary or caller-selected
legacy environment is accepted.

Regression coverage explicitly proves the exact legacy V5 digest is accepted
as historical evidence and an arbitrary replacement digest is rejected.

## Verification already observed before this authority review

- scoped `cargo fmt -- src/reviewed_build.rs`: PASS
- focused `historical_policy_acceptance_is_closed_to_exact_v3_v4_or_current_v5`: PASS
- strict `cargo clippy --bin catdesk -- -D warnings`: PASS
- isolated CatDesk build under `target-verify/t0435-v5-lineage`: PASS

## Safety boundary

This source repair does not modify protected reviewed-build state, active
attempt/pointer, Wake target/runtime, T-0425, Secure MCP ownership, recovery/LKG
or Git history. A live protected PREPARE may be retried only after this current
source is independently COMPLETED_VERIFIED and the reviewed controller is
loaded. PREPARE must then create/advance only through the existing immutable
terminal retry transaction; no protected state may be edited directly.

## Conditional verdict

`CURRENT_SOURCE_ELIGIBLE_FOR_T0435_REVIEWED_CONTROLLER` only if the CatDesk
direct-review session for this bundle completes with required tests, strict
Clippy, authoritative diff, and independent final review.

## Direct-review completion marker

Direct ChatGPT ownership was claimed under `adc-t0435-v5-historical-retry-compat-review-20260928` after the source repair was present. Final authority is granted only by the session's own verifier/final-review result; this marker itself does not authorize protected-build mutation.
