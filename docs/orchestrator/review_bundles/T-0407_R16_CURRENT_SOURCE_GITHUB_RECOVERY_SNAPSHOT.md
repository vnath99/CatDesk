# T-0407 R16 current-source GitHub recovery snapshot

## Baseline and working-tree scope

R16 validated the R15 schema-2 manifest and descriptor before reuse. R15's
descriptor matched its manifest digest and supplied 961 reconstruction entries,
12 archival exclusions, and one runtime-generated exclusion.

R16 then overlaid the current working tree: 42 tracked modifications and 766
untracked paths at generation time. New/current CatDesk source, tests,
documentation, and review evidence—including the T-0457, T-0458, and T-0459
materials—are included from their current working-tree bytes.

## Frozen R16 identity

- Manifest: `docs/orchestrator/T-0407_R16_GITHUB_RECOVERY_SNAPSHOT_MANIFEST.json`
- Manifest SHA-256: `bb517b2d101513fef09936519d778b6f8190716331605744666f42cff9e4b34a`
- Descriptor: `docs/orchestrator/T-0407_R16_GITHUB_PUBLICATION_DESCRIPTOR.json`
- Descriptor SHA-256: `5c4dc38ff3d7fa605a5ab94ce67232987b7e48488eb7fee649167c7cb34f2c5a`
- `INCLUDE_RECONSTRUCTION`: 969
- `EXCLUDE_ARCHIVAL_BINARY`: 12
- `EXCLUDE_RUNTIME_GENERATED`: 1

The descriptor binds the existing origin hash, feature branch
`orchestrator/chatgpt-codex-autonomous-loop`, and HEAD
`b958eb9fff4522168ebb1ae4a726209896a27451`.

## Fixed exclusions and supplemental omissions

The exact twelve established opaque ZIPs remain
`EXCLUDE_ARCHIVAL_BINARY`. The sole runtime-generated exclusion remains
`src/daemon-reload-approval-request-v1.json`.

The stable authority pointer, all R16 self-referential artifacts, the two
local linker-diagnostic wrappers, and absent transient generator names are
outside the manifest hash graph. `docs/orchestrator/WAKE2_PROFILE_REAUTHENTICATION.md`
is also omitted: it is Wake2 guidance for a different workspace and contains a
machine-specific path, so it is not safe CatDesk reconstruction material.

## Wake bridge and scan evidence

`scripts/wake_bridge.py` is included from current working-tree bytes:

- SHA-256: `aa4c055990fabc2cea811cc9514bfbfb3e77085e6dbf7a3bab2fb88914e2697c`
- byte length: `108181`

Git still reports `AM scripts/wake_bridge.py`. The manifest binds the current
working-tree version, not the obsolete staged/index copy; R16 did not mutate
the index.

Independent validation of every include found zero hash/length mismatches,
reparse includes, credential/private-key shape hits, user-specific absolute
paths, runtime-state path includes, or ZIP includes. The descriptor’s digest,
counts, origin hash, branch, and HEAD all matched current readback.

## Non-actions

No Git index/history/remote change, publication-authority binding, protected
build, promotion, reload, or Wake/target/browser/control-plane runtime action
occurred. Independent review must remeasure the R16 manifest, descriptor, and
working-tree entries before any later publication authority action.

## Verification

- `cargo fmt --all -- --check`: passed.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`:
  passed.
- `cargo test --workspace --all-targets --all-features`: passed (1,030 root
  tests in the current workspace profile; existing ignored tests remained
  ignored).
- `git diff --check`: passed; it emitted only inherited CRLF conversion
  warnings for existing dirty tracked files.
