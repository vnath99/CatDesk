# T-0412R3 — Source-Current Bootstrap Build

## Purpose

Session: `adc-t0412r3-source-current-bootstrap-build-20260926`  
Contract: `fnv1a64:0d0740f142737f42`

This session performs no source repair. T-0412R2 (`adc-t0412r2-v5-short-target-path-repair-20260926`) is already `COMPLETED_VERIFIED` at stateVersion 12 with independent final review. Its reviewed source removes redundant `builds/<same-attempt>` nesting when the reviewed-build control root is already scoped to `generations/<activeAttemptId>`.

## Why a bootstrap build is required

A fresh protected reviewed-build attempt was prepared and confirmed from the exact acknowledged T-0412R2 independent review. New attempt `24c084803d4a41898f6019f4fdee80e8` terminated `BUILD_FAILED_OR_AMBIGUOUS`.

Its raw Cargo diagnostics still show the pre-repair output layout:

`.catdesk/reviewed-build-control/generations/<attempt>/builds/<same-attempt>/target/release/build/...`

and MSVC again returned LNK1104 while creating build-script executables.

That does not contradict the reviewed T-0412R2 source. It proves the currently serving CatDesk daemon still hosts the older reviewed-build worker implementation. The immutable source snapshot for the new attempt contains current source, but the worker that materializes and executes that snapshot comes from the serving CatDesk executable.

Therefore another protected PREPARE/CONFIRM under the same old serving daemon would repeat the same failure and is not authorized.

## Fixed build boundary

Use only the existing `CARGO_BUILD_RELEASE_ISOLATED` verifier profile:

`cargo build --release --locked --target-dir .catdesk/verification-targets/autonomy-release`

The resulting executable is workspace-contained bootstrap/verification material only. It is not a reviewed candidate, reviewed promotion, canonical release, or LKG authority.

The fixed verifier must complete successfully and the executable must subsequently be measured/revalidated by the existing two-phase `catdesk_daemon_reload` surface before any serving handoff.

## Non-actions

This session does not:
- modify CatDesk source;
- modify the canonical release pair;
- mint or reconstruct `reviewed-promotion.json`;
- promote a candidate;
- change Wake target/profile/state;
- modify the external official Secure MCP runtime;
- publish Git;
- clean/reset the intentionally dirty worktree.

## Next boundary after verifier PASS

1. Run daemon-reload dry-run against the fixed isolated executable and capture its exact SHA-256/token.
2. Confirm only that exact preflight.
3. Reconnect and prove `CONNECTED_VERIFIED` / local MCP `READY`, unchanged external tunnel ownership, unchanged canonical Wake target, and source-current tool catalog.
4. If `catdesk_turn_timer` is exposed, start a manual turn timer and poll it during subsequent work.
5. Create/accept fresh source authority and start a brand-new protected V5 reviewed-build attempt. Never reuse terminal attempts `465286253071480b8fb9afa95c296d26`, `ea299f644c6b4eb89e2331a5b6098dce`, or `24c084803d4a41898f6019f4fdee80e8`.
6. Require `BUILD_ATTESTED` before reviewed promotion/LKG work.

OPERATOR ACTION: NONE.
Expected continuation: direct CatDesk fixed isolated verifier, followed by bounded two-phase daemon reload only after durable build PASS.
