# T-0268 / T-0229-R2A-R8B ProtectedDirectoryGuard ownership

## Result

PASS for the bounded R2A-R8B extraction. `ProtectedDirectoryGuard`, its inherent low-level pinned-directory methods, and its `PinnedParent` bridge now have one physical owner in `src/windows_protected_fs.rs`. `src/reviewed_source_snapshot.rs` consumes the shared guard while retaining snapshot-specific rename/commit, enumeration, delete/disposition, cleanup, staging, validation, and orchestration logic.

One formatting-only import-order correction was required in `src/reviewed_build.rs` during ChatGPT review. No behavior was changed by that correction.

## Ownership after T-0268

| Authority | Owner after T-0268 |
| --- | --- |
| `struct ProtectedDirectoryGuard` | `src/windows_protected_fs.rs` |
| `impl ProtectedDirectoryGuard` | `src/windows_protected_fs.rs` |
| `impl PinnedParent for ProtectedDirectoryGuard` | `src/windows_protected_fs.rs` |
| acquire/path/assert/descend/create/clone/direct-child identity methods | `src/windows_protected_fs.rs` |
| test-only descriptor-at-create child helper | `src/windows_protected_fs.rs` |
| snapshot rename/commit | `src/reviewed_source_snapshot.rs` |
| snapshot enumeration/query | `src/reviewed_source_snapshot.rs` |
| snapshot file/directory delete/disposition and recursive cleanup | `src/reviewed_source_snapshot.rs` |
| snapshot staging/validation/orchestration | `src/reviewed_source_snapshot.rs` |

The shared module exposes two narrow compatibility seams needed by retained snapshot-specific operations: validated access to the final already-open handle (`last_handle`) and local path bookkeeping after a successful same-parent native rename (`mark_renamed_as_direct_child`). Neither seam opens a caller-supplied pathname or moves snapshot-specific native orchestration into the shared module.

## Mechanical source evidence

The ownership regression in `windows_protected_fs.rs` requires exactly one shared declaration/implementation and zero snapshot definitions for:

```text
pub(crate) struct ProtectedDirectoryGuard
impl ProtectedDirectoryGuard
impl PinnedParent for ProtectedDirectoryGuard
```

It also requires the snapshot to continue consuming `ProtectedDirectoryGuard`, requires the shared test-only descriptor helper and rename bookkeeping seam, and rejects direct snapshot access to guard internals (`.directories`).

The production reverse-dependency regression constructs the forbidden `crate::reviewed_source_snapshot` token and asserts that `windows_protected_fs.rs` does not contain it. Source inspection confirms the production shared module has no reverse dependency on `reviewed_source_snapshot`.

## Behavioral/security preservation

The focused Windows guard regression covers:

- root acquisition and stable identity validation;
- pinned-chain cloning;
- handle-relative child creation;
- direct-child identity validation;
- cloned child stability;
- delete-share child descent;
- renameable child creation.

The moved methods continue to validate one protected component, validate every retained ancestor, use handle-relative child opens rooted in the already-open parent, reject reparse/non-directory objects, preserve volume/FileId identity, and keep ancestor handles alive for the guard lifetime.

Snapshot source inspection confirms retained snapshot authority still includes `rename_pinned_staging`, `query_relative_directory_names`, delete/disposition helpers, recursive `cleanup_staging_tree`, staging create/reclaim/remove, and higher-level snapshot validation/orchestration.

## Verification

| Command | Result |
| --- | --- |
| `cargo fmt --check` | PASS |
| `cargo clippy --all-targets --all-features -- -D warnings` | PASS |
| `cargo test windows_protected_fs::tests` | PASS; 3 focused tests, including Windows pinned-chain behavior |
| project `verify_project` / `cargo test` | PASS |
| project `verify_project` / `cargo build` | PASS |
| `git diff --check` | PASS; only existing LF/CRLF conversion warnings |

The initial project verification exposed a single rustfmt import-order mismatch in `src/reviewed_build.rs`. ChatGPT corrected only that import ordering and reran all gates above successfully.

## Attributable paths

- `src/windows_protected_fs.rs`
- `src/reviewed_source_snapshot.rs`
- `src/reviewed_build.rs` (format-only import ordering correction during review)
- this review bundle

The repository has a long-lived broadly dirty/untracked working tree, including these newer source files, so repository-wide Git diff statistics are not valid ticket attribution. Mechanical ownership checks, bounded source inspection, focused tests, and complete project verification are the attributable evidence.

No browser invocation, daemon/release/tunnel mutation, Scheduler mutation, Git publish/merge, signing/provenance work, dedicated-producer work, or Secure MCP change is part of T-0268.

## Gate status

- Single shared `ProtectedDirectoryGuard` physical ownership: **PASS**
- Required inherent method closure moved: **PASS**
- `PinnedParent` bridge moved: **PASS**
- Snapshot-specific higher-level operations retained in snapshot: **PASS**
- Zero shared-to-snapshot production reverse dependency: **PASS**
- Narrow compatibility seams do not reopen pathname authority: **PASS**
- Focused adversarial/behavioral regression: **PASS**
- Full repository verification: **PASS**

T-0268 is technically ready for independent final acceptance and R2A continuation planning.
