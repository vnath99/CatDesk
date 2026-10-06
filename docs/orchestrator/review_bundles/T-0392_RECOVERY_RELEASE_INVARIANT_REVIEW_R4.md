# T-0392 Recovery/Release Invariant Review — R4

Date: 2026-09-19  
Session: `adc-chat27-recovery-release-review-r4-20260919`  
Scope: independent review of the corrected CatDesk recovery/release and Wake reliability snapshot.

## Findings

1. **Release candidate isolation is present and regression-covered.**
   `scripts/provision-catdesk-release.ps1` stages its build beneath an isolated candidate target instead of treating `target\release\catdesk.exe` as both ordinary Cargo output and accepted recovery authority. The focused Rust regression `release_candidate_provisioning_cannot_mutate_canonical_release_pair` passes. This preserves the rule that normal release compilation cannot silently advance canonical recovery/LKG authority.

2. **Legacy recovery classification remains fail-closed while distinguishing missing authority from damaged authority.**
   `scripts/catdesk-release-recovery.ps1` does not accept a legacy `operational_verified` snapshot as rollback authority. A recognized legacy-only escrow is classified as `LKG_AUTHORITY_MISSING`; malformed, unsafe, contradictory, or otherwise untrusted recovery state remains a damaged/ambiguous fail-closed condition. The recovery PowerShell fixture passes in the exact full package test.

3. **The stale Wake-version regression fixture was corrected without weakening the invariant.**
   The Wake package and immutable installer currently declare the same source release, `1.0.0-dev.17`. The previous test hardcoded `1.0.0-dev.14`, making a legitimate immutable-release version bump fail even when installer and Cargo manifest agreed. The test now derives the version from `wake/Cargo.toml`, requires the installer to declare that exact version, and retains a reviewed development-release-family check. The focused `wake_installers_use_windows_powershell_compatible_utf8_without_bom` test passes.

4. **Current-generation Wake anti-replay behavior remains intact.**
   The focused `wake_protocol_client::tests::current_target_generation_change_is_the_replay_cutoff` test passes. The full suite also exercises the related activation-cutoff and missing-generation fail-closed cases. This review found no source change that weakens the current target-generation replay boundary.

5. **Verification is clean on the corrected source snapshot.**
   - `cargo fmt --check`: PASS.
   - `cargo clippy --all-targets --all-features -- -D warnings`: PASS.
   - `cargo test`: PASS across the package. The primary binary aggregate is 943 passed, 0 failed, 21 ignored; all auxiliary binary/integration test aggregates finish successfully. The package log contains intentionally spawned nested child-test failures for the AppContainer negative-path fixture, but their parent tests pass and the overall Cargo invocation exits successfully.
   - focused release candidate-isolation test: PASS.
   - focused current-generation Wake anti-replay test: PASS.
   - `cargo test --manifest-path wake/Cargo.toml`: PASS.
   - `git diff --check`: PASS; existing CRLF-conversion warnings only.

## Host-state residuals and acceptance boundary

- **Source correctness is not yet host recovery acceptance.** The live host previously exhibited a mismatch among canonical release binary, fingerprint, reviewed-promotion authority, and reviewed LKG. This review does not treat an ad-hoc runnable binary as accepted recovery authority.
- A fresh reviewed build and reviewed promotion must establish one atomic accepted generation in which the promoted binary, fingerprint, promotion record, and reviewed LKG agree. Only then should `start-catdesk-stack.ps1 -Mode recover -Execute` be used as the end-to-end recovery acceptance test.
- The external Secure MCP tunnel, dirty worktree, current chat-target authority, and unrelated project state must remain untouched by promotion/recovery reconciliation.
- The source Wake package is dev.17 while the currently running installed Wake host was last observed as dev.15. That installed-package reconciliation is a separate host step and must use the immutable reviewed Wake install/upgrade path.
- Natural Wake acceptance remains separate: it still requires a real Wake-generated user message in the canonical chat plus a correlated durable `EXACT_USER_MESSAGE_APPENDED` receipt. A claimed event, browser readiness state, or manual delivery is insufficient.

## Review conclusion

The corrected source snapshot is suitable to proceed into the existing reviewed-build and reviewed-promotion pipeline. The recovery/release source repair and the Wake-version regression-fixture repair are verification-clean. Final operational acceptance remains contingent on durable host authority reconciliation, successful one-command recovery against that accepted generation, and the separate natural Wake message-plus-receipt proof.
