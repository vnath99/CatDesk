# T-0407 R11 — R10 GitHub Publication Authority

## Scope

This source-and-test-only change retargets the closed GitHub publication
authority from the stale T-0407 R7 snapshot descriptor to the independently
reviewed T-0407 R10 descriptor. No Git staging, commit, push, remote access,
runtime operation, or protected-state mutation was performed.

## Authority binding

The fixed descriptor, manifest, and supplemental review paths are now:

- `docs/orchestrator/T-0407_R10_GITHUB_PUBLICATION_DESCRIPTOR.json`
- `docs/orchestrator/T-0407_R10_GITHUB_RECOVERY_SNAPSHOT_MANIFEST.json`
- `docs/orchestrator/review_bundles/T-0407_R10_GITHUB_RECOVERY_SNAPSHOT_REFRESH.md`

The R10 descriptor binds manifest SHA-256
`3b6a7e95d698460bb314bddac93e7763647e0047142cdc781c97fe157c1af9b5`.
Its schema-v2 manifest has 945 `INCLUDE_RECONSTRUCTION` entries, 12 archival
binary exclusions, and one runtime-generated exclusion. The executor's exact
publication set is therefore 948 deduplicated literal paths: the 945 entries,
the manifest, and the two fixed supplemental artifacts.

`publication_review_authority` resolves exactly one current-output artifact
with the R10 descriptor path, verifies its reviewed digest, and parses only
that R10 descriptor. The confirmation path reconstructs only the R10 manifest
path from durable prepared evidence. The old R7 descriptor cannot satisfy the
R10 parser because its fixed manifest and supplemental paths differ.

## Regression coverage

- The live R10 manifest parser test asserts 945 included entries and the
  required 12/1 exclusion counts.
- The R10 descriptor/manifest test derives and asserts exactly 948 literal
  publication paths.
- The stale R7 descriptor is explicitly rejected by the R10 parser.
- Existing executor tests continue to cover exact literal staging, approved
  pre-staged subsets, file-drift refusal, trusted Git identity drift, fixed
  non-force push argv, redacted output, and no replay after remote outcome
  unknown.

## Verification evidence

- `cargo fmt --all -- --check`: passed after scoped formatting of the three
  modified delegated Rust files.
- Focused `github_publication::tests`: 11 passed, 0 failed.
- Focused `github_publication_executor::tests`: 7 passed, 0 failed.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`:
  passed.
- `cargo test --workspace --all-targets --all-features`: 1,015 passed,
  0 failed (the suite retains its existing explicitly ignored platform/live
  tests).
- `git diff --check`: passed; it reported only inherited CRLF advisory
  warnings for existing tracked files.

## Boundary statement

This task did not invoke Git publication, change branches, access credentials,
or mutate Wake, tunnel, recovery, daemon, release, or protected reviewed-build
state. Independent final review remains required before any later activation.
