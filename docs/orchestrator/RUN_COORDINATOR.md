# Run Coordinator

Status: T-0018 implementation
Date: 2026-07-25

## Scope

T-0018 adds provider-neutral run coordination primitives. The coordinator owns readiness checks, run locks, budgets, approvals, escalation packet construction, cancellation, final verification gates, and final-review packaging.

It does not own provider transport and it does not push, merge, release, deploy, or publish.

## Responsibilities

- validate contract readiness;
- prevent duplicate active runs with run locks;
- stop safely on budget exhaustion;
- validate supervisor decisions against approval requests;
- produce structured escalation packets with evidence;
- cancel safely and release locks;
- require passed verification before final completion;
- produce final-review packages with `pushed=false` and `merged=false`.

## Verification Boundary

A model completion claim is not enough. Final packaging requires:

- `FinalRunResultV1.status == COMPLETED_VERIFIED`;
- verification status `PASSED`;
- non-empty actual diff summary.

## Implementation

Rust module:

- `src/delegated/coordinator.rs`

Export:

- `src/delegated/mod.rs`

## Verification Coverage

Focused tests cover:

- readiness requiring configured start approval;
- ambiguity escalation with evidence;
- duplicate run lock rejection;
- budget exhaustion;
- stale approval rejection;
- final completion requiring verification;
- final review never marking push or merge;
- cancellation releasing locks.
