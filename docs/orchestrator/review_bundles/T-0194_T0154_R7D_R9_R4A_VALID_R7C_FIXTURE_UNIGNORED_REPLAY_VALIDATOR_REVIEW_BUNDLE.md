# T-0194 — valid R7C fixture / replay validator attempt

## Fixture corrections and production conditions

The fixture's `project_id` was corrected from `fixture-project` to exact
`catdesk`, satisfying `canonical_expected`.  It now creates real
`Cargo.toml`, `Cargo.lock`, `src/lib.rs`, and `src/out.txt`; the completion
artifact, current-output SHA-256, and ABSENT baseline observation all refer to
the real `src/out.txt` bytes.  The fixture invokes the real R7C snapshot
producer and uses its returned snapshot identities in the reviewed-build
attempt.

`end_to_end_fixture_snapshot_uses_real_r7c_authority` is an ordinary Windows
test for that corrected prerequisite. It creates the snapshot with
`create_or_validate_reviewed_source_snapshot`, then calls the real
`validate_committed_snapshot` replay and requires identical snapshot ID,
authority digest, manifest digest, and nonempty entries. This proves the R7C
fixture authority independently of the later reviewed-build host-tool gate.

The next production condition is `trusted_toolchain()` in `validate_attempt`.
On this host the fixed CatDesk policy slots cannot be resolved and the fixture
fails with `REVIEWED_BUILD_TOOLCHAIN_UNAVAILABLE`.  That occurs before an
untouched `validate_producer_attestation` positive control can succeed, so an
end-to-end replay-pre-open negative result would not be meaningful evidence.

Verifier-visible host inspection confirms the exact mismatch: the production
policy requests `C:\Program Files\Rust\bin\cargo.exe` and
`C:\Program Files\Rust\bin\rustc.exe`, neither of which exists.  The only
available commands are Rustup shims at `<USER_PROFILE>\.cargo\bin`.  Those
are intentionally non-authoritative under this R7D policy and were not used as
a fixture fallback.

## Status

The former ignored test was briefly enabled and executed; it reached the real
snapshot producer, passed the project-ID defect, then failed exactly at the
real fixed-tool policy gate.  It is left ignored because ordinary `cargo test`
cannot establish the required host-owned toolchain evidence in this workspace.
No test exception, mock validator, alternate PATH tool, or weakened policy was
introduced.

Consequently the T-0194 mandatory unignored positive validator and same-length
replay substitution rejection are **not proven**. The exact blocker is host
provisioning of the fixed Cargo/Rustc policy slots, not fixture authority data.

## Verification recorded in this workspace

`cargo test reviewed_build -- --nocapture` discovered 19 tests: 18 passed and
the one producer-attestation end-to-end test remained explicitly ignored for
the unavailable fixed tool slots above. The ordinary R7C fixture test is among
the passing tests. Full unignored end-to-end attestation verification is not
claimed.

## Changed files

- `src/reviewed_build.rs`
- `docs/orchestrator/review_bundles/T-0194_T0154_R7D_R9_R4A_VALID_R7C_FIXTURE_UNIGNORED_REPLAY_VALIDATOR_REVIEW_BUNDLE.md`
