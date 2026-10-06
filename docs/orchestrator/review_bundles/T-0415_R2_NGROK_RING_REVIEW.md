# T-0415 R2 — ngrok 0.19 + ring protected-build dependency repair

## Review objective

Independently review the bounded R2 source repair after protected reviewed-build generation `59febcf984bf4ce198920b333c54cbae` failed. The repair must remove the actual native build-script blocker without broadening CatDesk's protected build PATH/tool trust boundary, and must preserve the previously reviewed T-0415 stable GitHub publication-authority and Wake receipt-safety semantics.

## Failure evidence and root cause

The serving dev.83 protected worker persisted `CARGO_LINK_LIBRARY_NOT_FOUND`, but current-source diagnostic replay against the immutable R1 attempt showed that classification was over-broad:

- fixed diagnostic classification: `OTHER_LINKER_EXIT`
- cargo classification: `CARGO_EXIT_NONZERO`
- cargo failure signals: `CUSTOM_BUILD_COMMAND_FAILED`, `PROCESS_EXITED_UNSUCCESSFULLY`
- failing package: `aws-lc-sys 0.39.1`
- allowlisted LNK codes: none
- safe missing library basename: none
- safe missing linker-input basename: none

The dependency graph traced the package through `catdesk -> ngrok 0.18 -> rustls -> aws-lc-rs -> aws-lc-sys`. The diagnostic additions in `src/reviewed_build.rs` are test/Windows-only fixed-vocabulary evidence extraction; they expose package identity and bounded classifier signals, not raw stderr, paths, environment values, or command lines.

## Repair

`Cargo.toml` now selects:

`ngrok = { version = "0.19.0", default-features = false, features = ["ring"] }`

The intermediate 0.18 + ring probe was rejected because ngrok 0.18 still referenced `rustls::crypto::aws_lc_rs::default_provider()` at `tunnel_ext.rs:233` when AWS-LC was disabled. The controlled 0.19 compatibility bump removes that provider-selection defect.

Current `Cargo.lock` evidence:

- `ngrok 0.19.0`
- `ring 0.17.14`
- no `aws-lc-rs`
- no `aws-lc-sys`

No CMake/NASM/Perl installation or protected PATH expansion is part of this repair.

## Verification evidence established before independent finalization

- `cargo check --locked`: PASS with ngrok 0.19.0 + ring.
- `cargo test --bin catdesk`: PASS.
- `cargo test --workspace --all-targets --all-features`: PASS.
- `cargo clippy --bin catdesk -- -D warnings`: PASS.
- `git diff --check`: PASS (existing LF/CRLF warnings only).
- `cargo test --test recovery_powershell`: PASS after a prior verifier run transiently hit LNK1104 on the test output executable itself.
- A verifier run identified Rustfmt-only wrapping deltas in the new diagnostic helper and a publication test; those exact requested formatting changes were applied. A final standalone formatter result was not claimed before this review-session finalization because the bounded turn reached its checkpoint; contract-approved verification remains authoritative.

## Safety / publication state

- Terminal R1 protected build generation `59febcf984bf4ce198920b333c54cbae` must never be retried.
- This review does not authorize Git publication.
- `docs/orchestrator/CURRENT_GITHUB_PUBLICATION_AUTHORITY.json` remains `UNBOUND`.
- No direct/manual binary copy, promotion bypass, force push, or protected PATH expansion is authorized.
- If R2 independently verifies and is ACKed after terminal Wake receipt, create one NEW reviewed-build attempt through the existing protected PREPARE/CONFIRM boundary.
