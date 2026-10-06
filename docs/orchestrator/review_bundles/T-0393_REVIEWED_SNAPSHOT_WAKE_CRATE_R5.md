# T-0393 Reviewed Source Snapshot Wake-Crate Coverage — R5

Date: 2026-09-19  
Session: `adc-chat27-reviewed-snapshot-wake-crate-r5-20260919`

## Scope

Independent review of the bounded source-snapshot repair made after the R4 reviewed-build attempt proved that the protected materialized source tree was incomplete for the current root Cargo manifest.

## Root cause confirmed

The root `Cargo.toml` contains the product-owned local dependency:

`catdesk-wake = { path = "wake" }`

The R4 reviewed-build attempt was correctly review-bound and reached the protected worker, but terminated with `CARGO_BUILD` exit 101. Inspection of that attempt's protected materialized source tree showed that root `Cargo.toml`, `Cargo.lock`, and `src` were present while `wake/Cargo.toml` was absent. Therefore Cargo was asked to build a manifest whose required local path dependency did not exist inside the reviewed source snapshot.

This was a snapshot-coverage defect, not a reason to weaken build attestation or promote an ordinary Cargo artifact.

## Repair reviewed

`collect_release_inputs` in `src/reviewed_source_snapshot.rs` now treats the product-owned Wake crate as part of the fixed release source surface:

- `wake/Cargo.toml` is a required reviewed input alongside root `Cargo.toml` and `Cargo.lock`.
- `wake/src` is recursively captured by the same safe-tree traversal already used for root `src`.
- optional `wake/build.rs` is handled by the same regular-file/reparse-safe build-script checks as root `build.rs`.
- all captured Rust sources, including Wake sources, participate in the existing literal `include_str!` / `include_bytes!` coverage pass.
- no caller-selected dependency path, new MCP authority, Git authority, release destination, or promotion bypass was added.

The unit-test workspace fixture now contains a minimal local Wake crate so existing snapshot integrity tests exercise the same product source shape. A new negative test removes `wake/Cargo.toml` and confirms snapshot collection fails closed instead of creating another incomplete reviewed build input.

## Verification performed before finalization

- `cargo fmt --check`: PASS.
- `cargo clippy --all-targets --all-features -- -D warnings`: PASS.
- `reviewed_source_snapshot::tests::release_inputs_require_the_local_wake_crate_manifest`: PASS.
- `reviewed_source_snapshot::tests::current_workspace_source_inputs_are_collectable`: PASS and now explicitly requires `wake/Cargo.toml`, `wake/src/lib.rs`, and `wake/src/bin/CatDeskWakeHost.rs`.
- `reviewed_source_snapshot::tests::creates_nested_binary_snapshot_and_replays_without_live_source_reads`: PASS.
- `git diff --check`: PASS with only the pre-existing CRLF-conversion warnings.

The first CatDesk direct-work finalizer attempt exposed a follow-on regression in the contract-wide `CARGO_TEST` gate rather than an R5 production-policy defect. The complete suite ran 965 tests and ended `941 passed; 3 failed; 21 ignored`. All three failures were legacy/minimal test fixtures that became invalid when R5 correctly made the local Wake crate mandatory: `delegated::autonomous_controller::tests::checkpoint_precedes_snapshot_failure_and_expired_restart_finalizes_once`, `reviewed_build::tests::end_to_end_fixture_snapshot_uses_real_r7c_authority`, and `reviewed_build::tests::replay_preopen_producer_attestation_rejects_same_length_swap`. The latter two failed with `reviewed source snapshot path is unavailable`; the controller fixture reached `WaitingForChatgpt` instead of its expected completed restart state because the fixture omitted the newly required Wake tree.

The bounded repair updates only those test fixture builders to include a minimal `wake/Cargo.toml` plus `wake/src/lib.rs`; it does not relax `collect_release_inputs`, reviewed-build attestation, promotion, or recovery authority. The related `checkpointed_output_hash_drift_fails_closed_without_a_second_turn` fixture was updated at the same shared setup boundary so its intended hostile-drift assertion remains about output drift rather than an accidentally missing Wake crate.

Post-repair focused verification is green for all three previously failing tests and for the related checkpointed-output-hash-drift test. `cargo fmt --check`, strict `cargo clippy --all-targets --all-features -- -D warnings`, and `git diff --check` are also green. Durable complete `cargo test -q` evidence is now retained twice: `.catdesk/logs/1789842443-20169123-a1b2-41e4-b762-254f1238a403.log` and `.catdesk/logs/1789842481-c2eb2d90-f095-4eb4-87ce-73c168c0e067.log` each record the primary suite as `944 passed; 0 failed; 21 ignored`, followed by green binary/integration suites. The logs contain nested `appcontainer_fixed_helper_writes_only_fixed_output_child` Access Denied output because the enclosing tests intentionally execute the helper in a negative-permission child process; the enclosing tests and root suite pass.

This review still must not be marked `COMPLETED_VERIFIED` from host-side verification alone. The same CatDesk direct-work session is `WAITING_FOR_CHATGPT` stateVersion 7 and must cross its first-class `autonomy_session_finalize_direct_work` boundary to capture the authoritative diff and independent final review. The current ChatGPT connector schema is stale and omits that operation while the local serving source/catalog includes it. No raw autonomy-state edit, unrestricted-shell call, or alternate trust path is authorized as a workaround.

## Residual acceptance boundary

This repair only restores completeness of the reviewed source input. It does not itself establish a release generation. After verified completion, a fresh reviewed-build attempt must prove that the protected build now reaches `BUILD_ATTESTED`. Only that attested candidate may enter reviewed-promotion preflight. Canonical recovery acceptance still requires a completed reviewed promotion and a successful one-command recovery against the reconciled generation.

Installed Wake package reconciliation (currently older than source dev.17) and natural browser-Wake acceptance remain separate follow-up work.

## Review conclusion

The change is narrowly scoped to the missing product-owned local crate and reuses the existing protected snapshot primitives. It closes the demonstrated incomplete-source failure without weakening reviewed-build, promotion, rollback, or Wake authority.
