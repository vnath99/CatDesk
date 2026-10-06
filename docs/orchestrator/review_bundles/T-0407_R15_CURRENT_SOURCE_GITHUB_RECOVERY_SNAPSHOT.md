# T-0407 R15 current-source GitHub recovery snapshot

## Scope

R15 refreshes the reconstruction snapshot from current working-tree bytes after
the accepted Wake dev.84 and reviewed-build source work. It does not stage,
unstage, commit, push, bind publication authority, run a protected build,
promote/reload CatDesk, or mutate Wake/runtime state.

The R12 schema-v2 manifest provides the classification baseline. Every current
modified or untracked path is overlaid using its working-tree bytes rather than
the Git index.

## Snapshot result

- Manifest: `docs/orchestrator/T-0407_R15_GITHUB_RECOVERY_SNAPSHOT_MANIFEST.json`
- Manifest SHA-256: `a2a90f59296ce127fc8a55ec49cfeff6213c9a72046b02a92143949db0a50223`
- Descriptor: `docs/orchestrator/T-0407_R15_GITHUB_PUBLICATION_DESCRIPTOR.json`
- Descriptor SHA-256: `2a21f44744372099684e110fd00bd4fc0a4cfc41e2cf92a8cc2700c91d500e5b`
- `INCLUDE_RECONSTRUCTION`: 961
- `EXCLUDE_ARCHIVAL_BINARY`: 12
- `EXCLUDE_RUNTIME_GENERATED`: 1

The manifest preserves exactly the established twelve opaque ZIP exclusions
and the sole runtime-generated exclusion
`src/daemon-reload-approval-request-v1.json`. It omits
`CURRENT_GITHUB_PUBLICATION_AUTHORITY.json`, the R15 manifest, descriptor, and
this review bundle from the manifest hash graph as supplemental reviewed
artifacts. It also omits the local wrappers
`run_current_v5_linker_diagnostic.cmd` and `run_v5_diagnostic_tmp.ps1`.
The temporary generator `r15_snapshot_tmp.ps1` was removed after generation.

`scripts/wake_bridge.py` is an R15 include measured from its current
working-tree bytes:

- SHA-256: `aa4c055990fabc2cea811cc9514bfbfb3e77085e6dbf7a3bab2fb88914e2697c`
- byte length: `108181`

## Index finding

Git status reports `AM scripts/wake_bridge.py`: the index contains an obsolete
staged copy while the current dev.84 working tree has newer bytes. R15 does
not mutate the index. A later, separately authorized publication flow must
reconcile that stale staged copy with this approved working-tree hash and must
not use `git add -A`.

## Security and identity boundaries

The exact include set is subject to private-key, credential-shape,
browser/tunnel/runtime-state, absolute user-path, archive, and reparse-point
checks. The descriptor binds the measured manifest to the existing origin hash,
feature branch `orchestrator/chatgpt-codex-autonomous-loop`, and local HEAD
`b958eb9fff4522168ebb1ae4a726209896a27451`.

Independent post-generation validation passed:

- all 961 include byte lengths and SHA-256 values matched their current files;
- all 12 archival and the sole runtime exclusion matched the fixed policy;
- no include was a reparse point, runtime-state path, or ZIP archive;
- no private-key or high-confidence GitHub/OpenAI/AWS credential shape and no
  unresolved user-specific absolute path was found in text-like includes;
- all seven supplemental/local omissions were absent from the hash graph;
- the descriptor matched the manifest digest, counts, existing origin hash,
  branch, and HEAD; and
- the current `wake_bridge.py` working-tree hash and byte length matched its
  manifest entry while the index remained `AM`.

## Verification

- `cargo fmt --all -- --check`: passed.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`:
  passed.
- `cargo test --workspace --all-targets --all-features`: completed with no
  reported failure (1,025 root tests in the current workspace profile).
- `git diff --check`: passed; it emitted only inherited CRLF conversion
  warnings for existing dirty tracked files.

Independent review must remeasure the R15 manifest, descriptor, and current
working-tree inputs before any authority binding or publication activity.
