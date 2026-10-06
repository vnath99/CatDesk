# T-0407 R10 GitHub Recovery Snapshot Refresh

## Classification

`READY_FOR_REVIEWED_BUILD` — this is a current-source reconstruction snapshot
for independent CatDesk verification only. It grants no publication,
activation, runtime, Wake, protected-build, or recovery authority.

## Exact completion artifacts

- Manifest:
  `docs/orchestrator/T-0407_R10_GITHUB_RECOVERY_SNAPSHOT_MANIFEST.json`
- Manifest SHA-256:
  `3b6a7e95d698460bb314bddac93e7763647e0047142cdc781c97fe157c1af9b5`
- Descriptor:
  `docs/orchestrator/T-0407_R10_GITHUB_PUBLICATION_DESCRIPTOR.json`
- Descriptor SHA-256:
  `c655bf00626dfb18ee9707d81361427da43de8e2045af6e7dd9152cea645e836`

The schema-v2 manifest binds 945 `INCLUDE_RECONSTRUCTION` files, preserves 12
`EXCLUDE_ARCHIVAL_BINARY` ZIP files and the one
`EXCLUDE_RUNTIME_GENERATED` approval-state file. Its own manifest, descriptor,
and this refresh bundle are fixed supplemental artifacts outside the hashed
include set, preventing self-reference.

The descriptor binds the manifest identity and counts to feature branch
`orchestrator/chatgpt-codex-autonomous-loop`, local HEAD
`b958eb9fff4522168ebb1ae4a726209896a27451`, and the SHA-256 of the current
origin identity (`ad0fe55abd1661a15106f8aa635ec753b0579bdc0266def9eb51232b80171cd8`).
The origin itself was not recorded.

## Evidence

R7 was re-read together with its post-R7 R8 and R9D diagnostic evidence. All
944 included paths were re-opened as regular, non-reparse files beneath the
workspace and remeasured against their manifest SHA-256 and byte length.
Containment, classification, Git-identity, credential/private-key, and actual
user-specific absolute-path checks passed. The one stale machine-specific
README source link was reduced to a workspace-relative link before freezing
this manifest. Generic bearer/cookie wording remains only in source fixtures
or redacted documentation, not credential values.

## Verification

- `cargo fmt --all -- --check`: passed.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`:
  passed.
- `cargo test --workspace --all-targets --all-features`: passed (1,013 tests;
  explicitly ignored tests remained ignored).
- `scripts/test-catdesk-lifecycle.ps1`: passed.
- `git diff --check`: passed.

No commit, push, remote Git operation, protected-state mutation, Wake/runtime
operation, toolchain change, or external-project mutation occurred.
