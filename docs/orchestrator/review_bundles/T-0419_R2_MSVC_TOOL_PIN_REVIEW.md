# T-0419 R2 — MSVC Tool Pin Independent Review

Date: 2026-10-07
Session: `adc-t0419r2-msvc-tool-pin-review-20261007`
Logical task: `T-0419-R2-MSVC-TOOL-PIN-REVIEW`
Reviewed commit: `3acd2cd29139d4e17343be3bce62b7264e79baec`
Outcome: **PASSED**

## Scope

Independent review of the exact T-0419 candidate repair that pins C compiler/archive selection for the env-cleared reviewed Windows build.

Only `src/reviewed_build.rs` changed. No Wake, tunnel, runtime ownership, promotion, canonical-release, or reload code changed.

## Findings

### Closed tool derivation — PASS

CatDesk continues to select the MSVC root only from its existing closed, product-derived Visual Studio layouts. The candidate derives:

- `CC` from `<validated-msvc-bin>\cl.exe`
- `AR` from `<validated-msvc-bin>\lib.exe`

Both files must pass the same `require_reviewed_toolchain_file` checks already used for `link.exe`: regular file, non-symlink, non-reparse-point.

No PATH search, registry lookup, caller path, `VCINSTALLDIR`, `VSINSTALLDIR`, developer-shell state, or ambient environment is accepted as authority.

### Worker environment remains closed — PASS

The Cargo worker still begins with `.env_clear()`. The candidate adds exactly two deterministic values, `CC` and `AR`, from the validated toolchain object. Existing fixed `PATH`, `LIB`, `LIBPATH`, `INCLUDE`, `CARGO_HOME`, `RUSTC`, temp, and system-drive handling are unchanged.

The environment-contract test was updated so `CC` and `AR` are required members of the exact environment set, and it continues to assert that ambient Visual Studio variables are not read.

### Scope against observed failure — PASS

The preceding fresh reviewed build reached the `ring 0.17.14` custom-build stage and failed `CARGO_EXIT_NONZERO`. The existing bounded diagnostic identified a custom build failure for ring with no linker code or missing library/input. This repair does not claim to prove root cause; it removes C tool-selection ambiguity while preserving the closed build policy. A fresh reviewed protected build is required for acceptance.

## Verification reviewed

- `cargo fmt --all -- --check`: PASS.
- `cargo test --locked --offline fixed_v5_host_linker_diagnostic_command_is_env_closed_and_has_no_persistence_seam -- --nocapture`: PASS.
- `cargo test --locked --offline offline_worker_policy_never_inherits_ambient_cargo_home -- --nocapture`: PASS.
- strict `cargo clippy --locked --offline --all-targets --all-features -- -D warnings`: PASS.
- `git diff --check`: PASS.
- Local and remote branch HEAD matched exact reviewed commit before this R2 session.

## Decision

**PASSED for one fresh reviewed protected-build attempt.**

This review authorizes only the exact reviewed commit above. If that build fails, diagnose the new failure and do not broaden the environment by assumption. If it reaches `BUILD_ATTESTED`, continue only through the existing reviewed promotion / parity / reload chain.
