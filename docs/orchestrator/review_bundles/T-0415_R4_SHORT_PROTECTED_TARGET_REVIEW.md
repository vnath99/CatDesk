# T-0415 R4 — Short protected Cargo target repair

## Scope

Session: `adc-t0415-r4-short-protected-target-20261004`  
Task: `t0415r4`

This repair is limited to the protected V5 reviewed-build Cargo output layout in `src/reviewed_build.rs` plus focused regression coverage. It does not retry a protected build, promote/reload CatDesk, mutate Wake/runtime/toolchain state, or publish GitHub.

## Evidence and root cause

Fresh protected attempt `a95011df33804af1bcead836a60a63ea` terminated at `CARGO_BUILD`, exit 101. The serving dev.83 result classified the retained stderr as `CARGO_LINK_LIBRARY_NOT_FOUND`, but direct inspection of the immutable Cargo fingerprint outputs shows the actual repeated linker failure across unrelated build-script crates:

- MSVC `link.exe` is found and invoked.
- Required SDK libraries such as `kernel32.lib` are present on the linker line.
- Multiple crates fail with `LNK1181: cannot open input file '.obj'`.
- Examples include crossbeam-utils, libc, parking_lot_core, proc-macro2, serde_core, typenum, and zerocopy.
- A real crossbeam-utils object path under the generation-scoped target is approximately 275 characters.

The active-generation target still inherited the long prefix:

`.catdesk/reviewed-build-control/generations/<attempt>/target/...`

T-0412R2 had already removed one redundant `builds/<attempt>` layer for this same class of practical MSVC linker path ceiling, while preserving attempt authority. Current source growth/workspace placement made the remaining generation-scoped target long enough to cross that ceiling again.

## Repair

Generation state remains the authority-bearing source for exact attempt id/digest, reviewed-source validation, claim ownership, result, and attestation.

For an active generation only, Cargo output is now placed beneath a fixed protected workspace subtree:

`target-verify/rb/<attempt>/target`

The exact full attempt id remains in the output path to prevent cross-attempt reuse. Every component is opened/created through `ProtectedDirectoryGuard` / `descend_or_create`; no caller path is introduced. Before selecting the short output root, the worker verifies that the active generation guard's final component is the exact attempt id.

Legacy unversioned control roots retain their historical `builds/<attempt>/target` behavior.

The Cargo target guard remains pinned from before process launch through release output acquisition, so the built executable is still opened through the protected pinned target rather than reacquired from an untrusted path.

## Verification

- Focused regression `reviewed_build::tests::active_generation_target_layout_uses_short_attempt_bound_workspace_root`: PASS.
- `cargo fmt --all -- --check`: PASS.
- `cargo test --bin catdesk`: PASS, 1,025 tests discovered; ignored host/manual diagnostics remain intentionally ignored.
- Strict Clippy and git diff checks are required before finalization and are recorded by the session verifier/final review.

## Acceptance boundary

Do not reuse terminal attempt `a95011df33804af1bcead836a60a63ea`.

After independent review/ACK of this R4 source, use one NEW protected V5 PREPARE/CONFIRM attempt. Require `BUILD_ATTESTED` before reviewed promotion. Only after serving parity exposes `catdesk_github_publication` may the GitHub recovery snapshot be freshly frozen, authority-bound, committed, and non-force pushed.
