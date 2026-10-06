# T-0392 Recovery/Release Invariant Review

Date: 2026-09-19
Session: adc-chat27-recovery-release-review-r2-20260919
Scope: bounded independent review of the current CatDesk recovery/release and Wake reliability repair state.

## Findings

1. **Canonical release candidate isolation is required and now represented in the current source.**
   `scripts/provision-catdesk-release.ps1` builds the release candidate beneath `target\catdesk-release-candidate` and returns a candidate path/hash with `REVIEWED_PROMOTION_REQUIRED`. It no longer provisions `target\release\catdesk.exe` or its sidecar directly. This removes the prior dual-use hazard where a normal Cargo release build could silently replace the canonical recovery image without advancing reviewed promotion/LKG authority.

2. **Legacy LKG classification is now more precise without weakening authority.**
   `scripts/catdesk-release-recovery.ps1` still refuses `operational_verified` snapshots as rollback authority. A well-formed legacy-only escrow is classified as `LKG_AUTHORITY_MISSING` rather than `LKG_AUTHORITY_AMBIGUOUS_OR_DAMAGED`. Malformed, unsafe, unexpected, or contradictory recovery state remains fail-closed as ambiguous/damaged.

3. **Regression coverage exists for candidate isolation and legacy-only classification.**
   The normal Rust suite contains `release_candidate_provisioning_cannot_mutate_canonical_release_pair`. The stack-recovery PowerShell fixture contains an assertion that a recognized legacy operational-only escrow is reported as missing current reviewed authority rather than damaged authority.

4. **Current source verification is clean.**
   - `cargo fmt --check`: PASS.
   - `cargo clippy --bin catdesk -- -D warnings`: PASS.
   - focused candidate-isolation Rust test: PASS.
   - `cargo test --bin catdesk`: PASS, final aggregate 943 passed, 0 failed, 21 ignored.
   - `cargo test --manifest-path wake/Cargo.toml`: PASS.
   - `git diff --check`: PASS (existing line-ending warnings only).

5. **Current host recovery authority is not yet reconciled by this review itself.**
   The live incident showed `target\release\catdesk.exe` and its fingerprint/LKG authority were not in one accepted generation. Source correctness alone does not repair that host state. A fresh reviewed build and reviewed promotion must still establish one atomic accepted generation where canonical binary, fingerprint, reviewed-promotion authority, and reviewed LKG agree.

## Residual risks / required acceptance

- Do not declare one-command recovery fixed until the fresh reviewed promotion completes and `start-catdesk-stack.ps1 -Mode recover -Execute` succeeds end-to-end against the resulting authority.
- Do not promote an ad-hoc existing release binary merely because it runs; use the reviewed-build producer and reviewed-promotion transaction bound to this completed review.
- Preserve the external Secure MCP tunnel, dirty worktree, Wake target authority, and unrelated project state during promotion/recovery acceptance.
- Wake/browser acceptance remains a separate follow-up after release recovery is stable.

## Review conclusion

The current source changes address the identified structural recovery regression without weakening rollback authority. The implementation is suitable to proceed into the existing reviewed-build and reviewed-promotion pipeline. Final recovery acceptance remains contingent on durable host reconciliation and a successful one-command recovery test.
