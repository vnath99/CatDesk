# T-0471 R1 — Reviewed promotion MCP schema parity

Date: 2026-10-10. Canonical conversation: https://chatgpt.com/c/6acaae13-4b18-83e9-b6ca-3c5e00cf47a8.

## Live finding

The user authorized the assistant to attempt supported reviewed promotion. Read-only live `catdesk_reviewed_build_promotion({action:"RESULT"})` returned `RESULT_UNAVAILABLE_OR_PENDING`. The reviewed-build result remains `BUILD_FAILED_OR_AMBIGUOUS`; supervisor preflight remains `SUPERVISOR_STARTUP_POLICY_UNPROVEN`. A promotion PREFLIGHT using the task-attributed T0464 scratch candidate and T0465 *reload* review record was rejected by the connector schema before controller execution. No confirmation, signed authorization, promotion, canonical image replacement, Wake restart or tunnel mutation occurred. The scratch image is not a reviewed, attested product release and must not be promoted.

Source audit of `src/delegated/autonomy_supervisor.rs::tool_schemas` found an exact defect: both promotion branches declare `buildPath`, `expectedSha256` and `recordId` or `confirmationToken` as required, yet branch `properties` declared only `action` while `additionalProperties:false`. This is internally contradictory JSON Schema and explains the observed pre-dispatch validation refusal. The RESULT branch is unaffected. Current daemon/source version differences and lack of a signed product-root main image are **additional, independent blockers**.

## Source-only correction

Declare all existing required parameters and their bounded string constraints in the respective closed PREFLIGHT/CONFIRM schemas. Preserve action-specific oneOf branch closure and every controller-side review, snapshot, attestation, hash, token and protected promotion check. No new executable, operator input, shell permission, remote endpoint, fallback authority or signing route is introduced. Add test `reviewed_promotion_schema_declares_required_properties` checking each closed branch declares every required field.

## Evidence and limits

- `cargo test --locked --offline --bin catdesk reviewed_promotion_schema_declares_required_properties -- --nocapture`: PASS (1/1).
- `cargo test --locked --offline --bin catdesk`: PASS, exit 0.
- `cargo clippy --locked --offline --workspace --all-targets --all-features -- -D warnings`: PASS.
- `cargo fmt --all -- --check`: PASS.
- `git diff --check`: PASS (line-ending warning only).
- This is a local developer test result and source correction, **not independent Codex final review, signed image readback, product-root image, BUILD_ATTESTED, promotion, or serving-image update**. Fresh Windows CI required after commit.

## Next gates

Obtain independent T0471 source review and verify new CI. The current installed controller must still be updated using a genuinely signed, independently approved source-current main image; T0469 fixed operator-host signed-image readback remains outstanding. After new source-current serving and genuine `BUILD_ATTESTED` provenance, invoke the fixed reviewed-promotion PREFLIGHT/CONFIRM path, run nine-layer recovery diagnostics, and migrate both project and Wake targets atomically to this Chat51 URL before resuming WakeHost. Do not treat this source-only fix as permission for raw reload, unsigned debug image execution or direct protected file access.
