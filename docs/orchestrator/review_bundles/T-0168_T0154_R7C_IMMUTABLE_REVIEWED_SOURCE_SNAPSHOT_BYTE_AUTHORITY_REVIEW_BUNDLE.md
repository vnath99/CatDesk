# T-0168 R7C Immutable Reviewed Source Snapshot Byte Authority

T-0167 was rejected because it retained only a hash manifest. R7C captures
exact binary-safe copies of required Cargo files, recursive `src/**`, and an
optional safe `build.rs` before completion artifacts become review authority.
The snapshot is staged under protected per-session state then atomically renamed
with a versioned manifest containing session, project, contract, and entry hash
metadata. Unsafe links, reparse points, missing required inputs, and oversized
files fail completion. Historical reviews are not backfilled and remain
`REVIEWED_SOURCE_SNAPSHOT_REQUIRED` downstream.

No live promotion, reload, build worker, or Git publication occurred.
