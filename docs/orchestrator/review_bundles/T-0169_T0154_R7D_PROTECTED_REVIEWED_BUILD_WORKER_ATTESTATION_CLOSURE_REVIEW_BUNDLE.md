# T-0169 / T-0154-R7D protected reviewed-build worker

## Closed state machine

`src/reviewed_build.rs` adds CatDesk-owned `PREPARE`, `CONFIRM`, and `RESULT`
operations. PREPARE accepts only an exact acknowledged review record selected
through the existing R6 resolver. It validates the committed R7C snapshot and
persists one immutable attempt with a generated build-attempt id, snapshot
identity/digests, review authority digest, and fixed build-policy digest.

CONFIRM repeats snapshot validation and creates `claim.json` with
`create_new`; only that first claimant can spawn the detached helper. Replays
return bounded pending or attested state and never create a second owner.
RESULT exposes only a fixed vocabulary state.

## Build and attestation authority

The helper materializes only protected snapshot byte objects into an isolated
control-root source directory, runs fixed `cargo build --release --locked` with
an isolated target directory and cleared environment, and never uses mutable
workspace source or canonical `target/release` as the build destination. The
trusted-tool policy uses fixed absolute host slots and hashes both Cargo and
Rustc; it does not consult PATH, caller executable fields, source paths,
flags, or environment values.

Only after the isolated candidate is remeasured does the worker write the v2
producer attestation. It binds the exact review record/session/digest,
snapshot id/authority/manifest digests, policy digest, tool identities and
hashes, build attempt id, candidate relative identity, and candidate SHA.

## Promotion and compatibility

`daemon_reload` now rejects legacy schema-1 or syntax-only attestations and
requires the producer marker plus the exact validated snapshot identities.
The existing promotion authorization continues to bind the attestation file
hash into its one-shot evidence. `catdesk_reviewed_build` is a first-class
operation; `catdesk_daemon_reload` additionally accepts only the fixed
`REVIEWED_BUILD_PREPARE`, `REVIEWED_BUILD_CONFIRM`, and
`REVIEWED_BUILD_RESULT` compatibility decisions with no extra fields.

## Verification and scope

`cargo fmt --check`, `cargo check`, the updated static MCP tool-list test, and
the reviewed-build policy test passed. The full suite was run and identified
one static expected-list update, which was corrected and rerun as a focused
regression. No reviewed-build worker, promotion, reload, recovery, tunnel,
browser, Scheduler, external-project mutation, or Git publication was invoked.

Final rust_full verification after that correction passed: clippy with warnings
denied, `cargo test` (600 passed, 18 ignored, 0 failed), both project
PowerShell fixtures, and `cargo build --release`.

Changed files include `src/reviewed_build.rs`, `src/reviewed_source_snapshot.rs`
(serializable protected expected authority), `src/daemon_reload.rs`,
`src/delegated/autonomy_supervisor.rs`, `src/main.rs`, `src/mcp.rs`, and this
bundle.
