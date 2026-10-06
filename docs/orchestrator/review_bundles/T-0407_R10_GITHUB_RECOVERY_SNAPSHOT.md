# T-0407 R10 GitHub Recovery Snapshot

## Classification

`READY_FOR_REVIEWED_BUILD` — a fresh current-source reconstruction snapshot
has been created for independent CatDesk verification. This is not a commit,
publication, or activation authorization.

## Scope

This review freezes a fresh reconstruction-only source snapshot after the R7
publication authority and post-R7 R8/R9D diagnostics. It does not commit,
push, contact a remote, mutate a protected build, or change Wake, tunnel,
toolchain, or runtime state.

## Fresh frozen artifacts

- Manifest:
  `docs/orchestrator/T-0407_R10_GITHUB_RECOVERY_SNAPSHOT_MANIFEST.json`
- Manifest SHA-256:
  `ae0d0de3814c7bb23b42ff474927ffe9465916a4ef6e097946e1572750722a3d`
- Descriptor:
  `docs/orchestrator/T-0407_R10_GITHUB_PUBLICATION_DESCRIPTOR.json`
- Descriptor SHA-256:
  `e16fb3c550a2bf4ec154cfaeefe2422c84e34b39f7f44c95461fb10b9be008c4`

The schema-v2 manifest contains 944 `INCLUDE_RECONSTRUCTION` paths, 12
`EXCLUDE_ARCHIVAL_BINARY` ZIP paths, and one
`EXCLUDE_RUNTIME_GENERATED` path (`src/daemon-reload-approval-request-v1.json`).
It preserves the R7 retention policy and adds the previously supplemental R2,
R3, R7, R8, and R9D reconstruction/review evidence. Its own manifest,
descriptor, and this review bundle remain separately bound supplemental
artifacts, avoiding a hash self-reference.

The required R10 output set is exactly this review bundle plus the manifest
and descriptor named above. The manifest deliberately excludes all three from
its hashed include list; the descriptor binds the manifest and names this
review bundle as a fixed supplemental artifact.

The descriptor binds the manifest, counts, current feature branch
`orchestrator/chatgpt-codex-autonomous-loop`, local HEAD
`b958eb9fff4522168ebb1ae4a726209896a27451`, and the SHA-256 of the existing
origin identity (`ad0fe55abd1661a15106f8aa635ec753b0579bdc0266def9eb51232b80171cd8`).
The origin itself was not emitted.

## Security and containment

Every manifest entry was re-opened as a regular non-reparse file beneath the
workspace. Its normalized relative path, byte length, and SHA-256 matched the
fresh manifest. The scanner rejected private-key blocks, GitHub/OpenAI/AWS
credential shapes, bearer-token values, and actual user-specific Windows or
Unix absolute paths across the exact 944-file include set.

One stale machine-specific README source-link path was found and changed only
to a workspace-relative `src/main.rs` link before the final manifest was
generated. The remaining generic `Bearer`/cookie wording occurs in source
fixtures or redacted documentation, not credential values; browser profiles,
cookies, tunnel state, `.catdesk`, `.codex`, build/cache roots, and the
excluded runtime approval file are not included.

## Verification

- `cargo fmt --all -- --check`: passed.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`:
  passed.
- `cargo test --workspace --all-targets --all-features`: passed (1,013 tests;
  explicitly ignored tests remained ignored).
- `powershell -NoProfile -ExecutionPolicy Bypass -File
  scripts/test-catdesk-lifecycle.ps1`: passed (`consumer lifecycle fixture
  tests passed`).
- `git diff --check`: passed.

No commit, push, remote Git operation, protected-build mutation, Wake/runtime
operation, toolchain change, or external-project mutation occurred. This R10
descriptor is a fresh evidence artifact only; activation or publication still
requires its own reviewed authority and a separately authorized closed
executor path.
