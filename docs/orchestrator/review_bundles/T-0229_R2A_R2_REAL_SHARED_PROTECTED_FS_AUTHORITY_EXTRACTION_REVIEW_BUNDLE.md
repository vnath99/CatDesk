# T-0233 / T-0229 R2A-R2 protected filesystem authority extraction

## Attributable files

- `src/windows_protected_fs.rs` — replaces the rejected façade with the shared
  handle-centered regular-file authority, including `PinnedParent`, bounded
  handle I/O, `NtCreateFile`, `OBJECT_ATTRIBUTES.RootDirectory`, no-follow
  options, opened-object classification, and the dependency-direction test.
- `src/reviewed_source_snapshot.rs` — implements the narrow pinned-parent
  adapter and consumes shared relative regular-file operations; the former
  duplicate child-I/O implementations were removed.
- `docs/orchestrator/review_bundles/T-0229_R2A_R2_REAL_SHARED_PROTECTED_FS_AUTHORITY_EXTRACTION_REVIEW_BUNDLE.md` — this evidence.

No supervisor, named-pipe, ProgramData, lifecycle, tunnel, or Git path was
changed by this slice. The worktree contains unrelated prior dirt; it is not
used as attribution evidence.

## Ownership and dependency map

| Prior snapshot ownership | Current ownership |
| --- | --- |
| Relative regular child write/read/flush/bounded-length validation | `windows_protected_fs::{write_new_regular_in,read_relative_regular}` |
| Relative child regular-file create/open | `windows_protected_fs::{create_relative_regular_file,open_relative_regular_file}` |
| `NtCreateFile`, `UNICODE_STRING`, `OBJECT_ATTRIBUTES.RootDirectory`, opened-child attribute classification | `windows_protected_fs` |
| Snapshot's pinned directory stack, root descent/create, directory identity, staging rename, enumeration and disposition cleanup | Retained in snapshot pending the remaining physical migration; they are not invoked by the new child-I/O implementation. |

Before: `windows_protected_fs -> reviewed_source_snapshot` façade dependency.

After: `reviewed_source_snapshot -> windows_protected_fs`; the shared module
has no snapshot import, alias, callback, or type reference.  Snapshot provides
only the `PinnedParent` implementation for its already-opened directory handle.

## Security invariants preserved

- The child name is component-validated before use.
- The parent is identity-revalidated before and after bounded child I/O.
- Windows resolves the child under the live pinned parent handle using
  `OBJECT_ATTRIBUTES.RootDirectory`, not a parent pathname.
- `FILE_OPEN_REPARSE_POINT` and post-open attributes reject reparse points and
  directories; unavailable stable identity fails closed.
- Writes are exact-length checked and flushed; reads are bounded and reject
  object changes during reading.
- Snapshot schema v4, manifests/digests, reviewed source coverage, task-output
  binding, replay behavior, staging rename, enumeration, and cleanup policy
  remain unchanged.

## Dependency regression and adversarial evidence

`windows_protected_fs::tests::production_module_has_no_snapshot_reverse_dependency`
loads the production source and rejects a constructed
`crate::reviewed_source_snapshot` reference while requiring the actual
`OBJECT_ATTRIBUTES`, `RootDirectory`, and `NtCreateFile` implementation text.

Focused snapshot tests passed with the shared path exercised: 20 tests cover
intermediate link rejection and outside sentinels, pinned-child redirection,
reparse replacement, directory/file replacement races, stale enumeration,
opened-handle validation, no-overwrite creation, exact replay, cleanup, and
manifest/source drift rejection.

## Verification

- `cargo fmt` — passed.
- `cargo test windows_protected_fs --no-fail-fast` — passed (1).
- `cargo test reviewed_source_snapshot --no-fail-fast` — passed (20).
- Strict clippy/full `rust_full` — must be independently rerun. The immediately
  preceding full suite is known to fail in the unrelated
  `test-promote-reviewed-catdesk-build.ps1` recovery fixture; this slice does
  not modify promotion/recovery.

## Remaining boundary

This is a real dependency inversion for the relative child-I/O authority, but
the broader `ProtectedDirectoryGuard`/pinned-directory stack, directory
creation, same-parent staging rename, enumeration, and handle-bound deletion
remain physically in snapshot source. Therefore stable-supervisor migration
and named-pipe work remain blocked, and independent review must treat this
bundle as evidence of the moved child-I/O authority only, not as completion of
the full R7C primitive migration.
