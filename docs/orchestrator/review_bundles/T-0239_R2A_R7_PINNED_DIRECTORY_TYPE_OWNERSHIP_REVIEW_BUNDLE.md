# T-0239 R2A-R7 PinnedDirectory type ownership

## Mechanical result

**PASS for the deliberately narrow type-ownership gate.**

`PinnedDirectory` is declared once in `src/windows_protected_fs.rs` and is
imported by `src/reviewed_source_snapshot.rs`; the snapshot-local declaration
was removed. The shared module has no production dependency on snapshot.

## Changed files

- `src/windows_protected_fs.rs`: owns the concrete three-field type with
  `pub(crate)` field visibility.
- `src/reviewed_source_snapshot.rs`: imports the shared type; retains the
  existing inherent implementation and Windows `Drop`.
- This review bundle.

The field visibility is the minimum required for the existing sibling-module
methods and `Drop` to continue using `path`, Windows `handle`, and Windows
volume/FileId tuple without behavior change. `Drop` remains in snapshot because
Rust permits same-crate trait implementation and moving it is outside this
one-type scope.

## Ownership regression

The focused shared-module test scans source lines for actual
`pub(crate) struct PinnedDirectory` declarations, requires one in the shared
module and none in snapshot, requires the snapshot import, and separately
rejects a production shared-to-snapshot dependency.

## Residual authority

Deliberately left in snapshot: all `PinnedDirectory` inherent methods,
`ProtectedDirectoryGuard`, root/descent/identity authority, staging rename,
enumeration, and cleanup/delete. Those are later slices; no live system, Git,
daemon, tunnel, ProgramData, service, Scheduler, or browser action occurred.

## Verification

Focused regression was repaired after its initial self-counting assertion. Full
format/clippy/test/rust_full evidence remains pending independent verification.
