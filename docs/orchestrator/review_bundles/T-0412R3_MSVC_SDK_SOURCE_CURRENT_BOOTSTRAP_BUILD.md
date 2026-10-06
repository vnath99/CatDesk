# T-0412R3 — MSVC/SDK Source-Current Bootstrap Build

## Purpose

This session produces one fresh workspace-contained isolated release candidate from the current source after T-0412R3 reached `COMPLETED_VERIFIED`.

Prerequisite accepted source authority:

- T-0412R2 short active-generation target-path repair: `COMPLETED_VERIFIED`.
- T-0412R3 fixed-root MSVC/Windows SDK linker-environment repair:
  - session `adc-t0412r3-v5-msvc-sdk-env-repair-20260926`
  - contract `fnv1a64:2d1876d29e0cbca1`
  - state `COMPLETED_VERIFIED`
  - independent final verification `PASSED`
  - exact review record `review-adc-t0412r3-v5-msvc-sdk-env-repair-20260926-18-independent_final_review` ACKed.

## Contract

Session: `adc-t0412r3-msvc-sdk-source-current-bootstrap-20260926`

Contract: `fnv1a64:670ea66884004943`

The session may run only:

- `CARGO_BUILD_RELEASE_ISOLATED`
- `GIT_DIFF`

It may write only this review bundle.

## Security boundary

This is a bootstrap candidate build, not reviewed promotion or LKG authority.

It must not:

- mutate `target/release/catdesk.exe` or its fingerprint;
- create or alter reviewed-promotion/LKG authority;
- alter Wake target, profile, events, receipts, or installed package;
- stop/recreate the external Secure MCP tunnel runtime;
- publish Git;
- clean/reset/stage unrelated dirty worktree state;
- modify source.

Any later daemon reload must use the existing bounded reload preflight and independently remeasure the exact candidate.

## Verification state

Pending the contract-approved finalizer. A candidate is not usable until `CARGO_BUILD_RELEASE_ISOLATED` and authoritative diff verification both pass and this session reaches `COMPLETED_VERIFIED`.

