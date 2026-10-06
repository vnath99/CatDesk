# T-0399 — Reviewed-build worker fix authority

Date: 2026-09-22

## Purpose

This bounded documentation-only review supersedes T-0398 for reviewed-build authority. T-0398's immutable snapshot froze an older worker implementation and is therefore unsuitable for another protected build retry.

No product source, Wake state/profile/target, external Secure MCP runtime, canonical release, recovery state, or Git history is modified by this ticket.

## T-0398 failure classification

Protected reviewed-build generation `f693652337c34fe38e11b0a977a3d84d` failed terminally as:

- state: `BUILD_FAILED_OR_AMBIGUOUS`
- failure code: `CARGO_BUILD_FAILED`
- diagnostic classification: `CARGO_EXIT_NONZERO`
- Cargo exit code: `101`
- candidate: none
- attestation: none
- promotion: none

The immutable T-0398 snapshot is proven stale relative to current source:

1. it does **not** contain
   `.env("SystemDrive", os_system_drive()?)` in the protected Cargo environment;
2. its Windows `require_trusted_final_link_handoff()` still requires the unprovisioned dedicated-producer state and fails closed.

Therefore retrying T-0398 cannot exercise the current worker fixes and would be intentionally rejected.

## MSVC discovery defect and bounded repair

The protected worker uses `env_clear()` and an explicitly reconstructed build environment.

A tiny throwaway Rust/MSVC executable compiled under the former protected baseline:

- pinned Rust toolchain `PATH` only,
- no inherited `LIB`, `LIBPATH`, `INCLUDE`, `VCINSTALLDIR`, or `VSINSTALLDIR`,
- fixed scratch output,

fails with:

`linker 'link.exe' not found`.

A differential probe added one inherited variable at a time to the scrubbed baseline and printed only variable names whose addition restored linking. Exactly these restored linker discovery:

- `SystemDrive`
- `ProgramData`

Current CatDesk chooses the narrower `SystemDrive` dependency.

It does **not** inherit the caller's `SystemDrive`. `os_system_drive()` calls `GetWindowsDirectoryW`, validates an ordinary local drive-root form, and derives only the drive identity (for example `C:`). The worker then sets:

- fixed `CARGO_TARGET_DIR`,
- fixed `RUSTC`,
- isolated `CARGO_HOME`,
- pinned Cargo-toolchain `PATH`,
- OS-derived `SystemDrive`,
- per-attempt `TEMP`/`TMP`.

It continues to omit caller-controlled toolchain variables such as `LIB`, `LIBPATH`, `INCLUDE`, `VCINSTALLDIR`, and `VSINSTALLDIR`.

Current fixed policy strings explicitly bind this behavior:

- build policy: `CATDESK_REVIEWED_BUILD_POLICY_V4_LOCAL_USER_PINNED_OUTPUT`
- environment policy contains
  `clear-inherited-environment`,
  `trusted-profile-rustup-discovery`,
  `os-system-drive`,
  `per-attempt-temp`,
  `pinned-output-immediate-remeasurement`.

The environment-policy digest is carried into the reviewed-build attempt and resulting producer attestation.

## Final-link/output authority policy

Current Windows source no longer depends on the unprovisioned dedicated-producer service for ordinary reviewed builds.

The bounded local-workstation policy is:

- trust the current Windows user and the worker-owned Cargo/linker process tree;
- do not claim resistance to a separate hostile process already executing as that same trusted user;
- allocate the Windows Job before Cargo creation and contain compiler/linker descendants in that job;
- pin the reviewed-build target directory before Cargo starts;
- cross the explicit local output-policy gate before acquiring output authority;
- immediately open the fixed `release/catdesk.exe` child relative to the pinned target parent;
- measure SHA-256, length, and file identity from the opened file;
- copy the exact opened bytes into a create-new immutable reviewed candidate;
- remeasure that candidate;
- refuse any candidate whose measured evidence differs;
- later reviewed promotion independently remeasures and validates the producer attestation again.

On Windows, `require_trusted_final_link_handoff()` now returns success for this bounded policy. Non-Windows remains fail-closed.

This deliberately removes the unfinished dedicated-service/final-link trust domain rather than introducing another signer, service, UAC flow, or per-release authority mechanism.

## Current verification evidence

Current workspace source verification available before this ticket:

- `cargo check --locked`: PASS.
- `cargo build --tests --locked`: PASS; all Rust test targets compile.
- source regression `fixed_policy_is_closed_and_deterministic` asserts the fixed V4 policy and `os-system-drive` environment-policy binding.
- source regression for the worker boundary asserts `SystemDrive` is populated from `os_system_drive()`, `TEMP/TMP` are per-attempt, and inherited Visual Studio/toolchain variables remain absent.
- source regression for local output authority asserts the gate occurs before output open/candidate creation/attestation and that the Windows handoff is accepted.
- direct `cargo test` execution is intermittently rejected by the MCP command wrapper before Cargo starts; this review does not misclassify those wrapper rejections as test failures.

The earlier long isolated release diagnostic exceeded CatDesk's 120-second wrapper window and had no trustworthy terminal result, so this review does not claim it passed.

## Snapshot requirements

Normal completion of T-0399 must emit a fresh immutable reviewed-source snapshot from **current** source. Before using it for a protected build, verify its frozen `src/reviewed_build.rs` contains all of the following:

- `CATDESK_REVIEWED_BUILD_POLICY_V4_LOCAL_USER_PINNED_OUTPUT`
- `os-system-drive` in `ENVIRONMENT_POLICY`
- `.env("SystemDrive", os_system_drive()?)`
- Windows `require_trusted_final_link_handoff()` returning `Ok(())`
- local-user pinned-output explanatory policy text.

The snapshot must also retain the normal root release-input closure including root `Cargo.toml`/`Cargo.lock`, `wake/Cargo.toml`, recursive root `src`, recursive `wake/src`, and required include/build inputs.

## Next authorized sequence

Only after T-0399 completes normally and the frozen snapshot is read back with the exact fixes above:

1. acknowledge the fresh independent T-0399 review;
2. prepare a new protected reviewed-build generation from that exact authority;
3. confirm once;
4. require terminal `BUILD_ATTESTED`;
5. inspect candidate SHA/length/identity and attestation bindings;
6. run reviewed-promotion preflight/confirmation;
7. verify canonical/serving parity and durable `reviewed_promotion` LKG authority;
8. rerun supported one-command recovery.

T-0398 is historical failure evidence only and must not be retried.

## Review conclusion

The current reviewed-build worker source is suitable to freeze into a new immutable reviewed-source authority. The two concrete blockers observed in T-0398 are addressed in current source without widening caller-controlled tool or promotion authority.
