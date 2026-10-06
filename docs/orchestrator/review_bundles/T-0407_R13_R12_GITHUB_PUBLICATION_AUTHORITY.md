# T-0407 R13 — R12 GitHub Publication Authority

## Scope

This source-and-test-only change rebinds the closed GitHub publication
authority from the stale R10 recovery snapshot to the independently reviewed
R12 post-R11 snapshot. No Git staging, commit, push, merge, network
publication, Wake/runtime, toolchain, or protected-state operation occurred.

## Exact R12 authority

The publication parser, confirmation reconstruction, executor fixture, and
supervisor current-output resolver now bind only these fixed R12 artifacts:

- `docs/orchestrator/T-0407_R12_GITHUB_RECOVERY_SNAPSHOT_MANIFEST.json`
- `docs/orchestrator/T-0407_R12_GITHUB_PUBLICATION_DESCRIPTOR.json`
- `docs/orchestrator/review_bundles/T-0407_R12_POST_R11_GITHUB_RECOVERY_SNAPSHOT.md`

The R12 descriptor binds manifest SHA-256
`6c8ea66d1ac14ef4eb923040a4e71296baab788357103c3025e580e6ff353597`.
The exact frozen set is 949 included reconstruction paths, 12 archival ZIP
exclusions, and one runtime-generated exclusion. Together with the manifest
and two fixed supplemental artifacts, the literal publication set is exactly
952 paths.

## Fail-closed regression coverage

- The live R12 descriptor and manifest parse with the 949/12/1 counts and
  derive exactly 952 deduplicated publication paths.
- The current R12 parser explicitly rejects both stale R10 and stale R7
  descriptor files.
- It also rejects otherwise well-formed descriptors that substitute either
  stale R10 or stale R7 manifest path.
- Existing R11 protections remain exercised: trusted Git identity drift,
  per-file hash/length drift, approved pre-staged subset versus unrelated
  staging, literal staging/commit, bounded/redacted confirmation, fixed
  non-force push argv, remote reconciliation, and no replay after durable
  remote-outcome-unknown.

## Verification evidence

- `cargo fmt --all -- --check`: passed.
- Focused publication parser tests: 12 passed, 0 failed.
- Focused publication executor tests: 7 passed, 0 failed.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`:
  passed.
- `cargo test --workspace --all-targets --all-features`: 1,016 passed,
  0 failed; existing explicitly ignored platform/live tests remained ignored.
- `git diff --check`: recorded after this artifact.

An initial focused-test compilation encountered a transient host memory
failure before executing tests; the unchanged command was retried and passed.
Independent final review remains required before any later publication
activation.
