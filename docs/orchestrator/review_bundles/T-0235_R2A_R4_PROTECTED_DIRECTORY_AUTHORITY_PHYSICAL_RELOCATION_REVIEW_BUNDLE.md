# T-0235 R2A-R4 physical relocation blocker record

## Outcome

This provider turn does **not** claim completion. Direct source inventory found
the required reusable authority remains physically coupled in
`src/reviewed_source_snapshot.rs`; moving a subset would repeat the rejected
facade/partial-ownership design.

## Exact remaining authority inventory

| Snapshot symbol group | Dependency/coupling that blocks a safe partial move |
| --- | --- |
| `ProtectedDirectoryGuard` / `PinnedDirectory` | The guard stores the pinned directory stack used by snapshot policy, cleanup hooks, and commit orchestration. |
| `NtCreateFile`, `NtSetInformationFile`, `NtQueryDirectoryFile`, `OBJECT_ATTRIBUTES`, `IO_STATUS_BLOCK` | Shared native structs/FFI drive directory open/create, rename, enumeration and disposition. |
| `rename_pinned_staging` | Uses exact staging/destination handles plus snapshot commit policy. |
| `query_relative_directory_names` | Feeds replay validation and cleanup; moving it alone leaves duplicate opened-directory authority. |
| `open_relative_regular_file_for_delete`, `dispose_opened_object`, `cleanup_staging_tree` | Coupled to test replacement hooks and recursive snapshot staging lifecycle. |
| `reclaim_stale_current_session_staging` / `remove_owned_staging` | Couples session identity/replay policy to the shared cleanup primitives. |

The inventory identifies approximately 50 direct production callsites. The
current shared module has no reverse snapshot dependency, but snapshot still
physically defines every forbidden directory/root symbol listed above. This
fails the task's mechanical ownership gate.

## Required safe next implementation shape

A completing change must relocate the entire guard/FFI/directory operation
cluster in one atomic refactor, expose a narrow shared API for guard creation,
relative descent/create, identity revalidation, rename/enumeration/disposition,
and leave only session/manifest/replay policy callbacks in snapshot. It must
then remove the corresponding definitions from snapshot and add a source-layout
regression. No forwarding wrapper or duplicated NT declarations is acceptable.

## Changes and verification

No new source relocation was made in this turn because it could not safely be
finished without leaving split authority. No live system, control-plane,
ProgramData, service, Scheduler, daemon, tunnel, browser, provenance, or Git
mutation occurred.
