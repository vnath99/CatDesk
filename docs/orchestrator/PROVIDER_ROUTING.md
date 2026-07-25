# Provider Routing

Status: T-0019 implementation
Date: 2026-07-25

## Scope

T-0019 adds provider selection and fallback primitives while keeping Ollama/Qwen as the only required live provider. Fake API and fake browser providers are used for offline routing tests.

## Provider Registry

The registry stores:

- provider ID;
- provider capabilities;
- availability state;
- credential environment-variable name.

Credential values are not stored in the registry, logs, fixtures, or repository files.

## Availability States

- `AVAILABLE`
- `UNAVAILABLE`
- `RATE_LIMITED`
- `TIMED_OUT`
- `MALFORMED_OUTPUT`

## Routing Policy

`ProviderRoutingPolicyV1` defines:

- primary provider ID;
- fallback provider IDs;
- disclosure policy.

Remote API and browser providers require `REMOTE_ALLOWED`. The default local-only posture rejects remote/browser fallback before any repository context can leave the machine.

## Handoff

Replacement uses `ProviderHandoffContextV1` from T-0015. Handoff includes compact checkpoint state and pending tool-call IDs.

Completed and failed tool calls are filtered out so a provider switch cannot repeat completed operations.

## Escalation

If no configured candidate is available, routing returns an escalation packet with provider state evidence and a recommendation to pause for supervisor guidance.

## Browser State Boundary

Browser-specific details such as cookies, selectors, and browser profile paths are not represented in the handoff schema or journal-facing records.

## Implementation

Rust module:

- `src/delegated/provider_router.rs`

Export:

- `src/delegated/mod.rs`

## Verification Coverage

Focused tests cover:

- simulated provider replacement;
- completed tool-call filtering during switch;
- all-providers-unavailable escalation;
- browser state not leaking into handoff JSON;
- remote/browser disclosure policy enforcement;
- credential values absent from logs.
