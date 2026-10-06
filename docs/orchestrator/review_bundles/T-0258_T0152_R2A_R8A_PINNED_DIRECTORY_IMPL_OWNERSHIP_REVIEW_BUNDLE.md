# T-0258 / T-0229-R2A-R8A PinnedDirectory implementation ownership

## Result

The mechanical ownership correction is present in source. T-0239 accepted
only the concrete type declaration; T-0240 was rejected as a no-op because the
inherent implementation and Windows `Drop` remained in snapshot. T-0258 moves
both into the shared protected-filesystem module and removes their snapshot
definitions.

## Before and after ownership

| Authority | Before T-0258 | After T-0258 |
| --- | --- | --- |
| `struct PinnedDirectory` | `windows_protected_fs.rs` (T-0239 foothold) | `windows_protected_fs.rs` |
| `impl PinnedDirectory` | `reviewed_source_snapshot.rs` | `windows_protected_fs.rs` |
| Windows `impl Drop for PinnedDirectory` | `reviewed_source_snapshot.rs` | `windows_protected_fs.rs` |
| RootDirectory-relative child open used by the type | snapshot-local helper | shared `nt_open_relative_with_error_domain` |
| `NtCreateFile`, `OBJECT_ATTRIBUTES`, `IO_STATUS_BLOCK`, `UNICODE_STRING` support | overlapping snapshot/shared declarations | shared production declaration/types; snapshot imports only the types it still needs for snapshot-owned set/query operations |

`reviewed_source_snapshot.rs` imports and consumes `PinnedDirectory` plus the
shared component validator and narrowly exposed native support. It has no
inherent `PinnedDirectory` implementation or `Drop` fallback.

## Minimum closure moved or deduplicated

The shared module now owns the minimum PinnedDirectory closure:

- acquire/open/create/open-for-delete/clone/revalidate methods and `Drop`;
- exact protected-component validation and pre-open directory classification;
- Windows `CreateFileW`, `GetFileInformationByHandle`, `CloseHandle`,
  `DuplicateHandle`, and `GetCurrentProcess` use needed by the type;
- the RootDirectory-relative `NtCreateFile` helper, `OBJECT_ATTRIBUTES`,
  `UNICODE_STRING`, `IO_STATUS_BLOCK`, and file identity structure;
- test-only SDDL conversion needed only by the moved type test method.

The moved methods preserve component validation, `FILE_OPEN_REPARSE_POINT`,
RootDirectory-relative child opening, volume/FileId identity, exact snapshot
error domain, and close-on-Drop behavior. Acquisition preserves the original
fail-closed `is unsafe` error and closes the handle if identity inspection
fails. Revalidation preserves `identity drifted` for an unavailable or
changed handle identity.

## Deliberately retained snapshot authority

This slice did not relocate `ProtectedDirectoryGuard`, staging rename/commit,
directory enumeration/classification, handle-bound disposition/delete or
recursive cleanup. Snapshot retains `NtSetInformationFile` and
`NtQueryDirectoryFile` only for those retained operations. No stable-supervisor
or named-pipe authority was changed.

## Mechanical source evidence

The focused ownership regression counts source lines and requires exactly one
of each in `windows_protected_fs.rs` and zero in snapshot:

```text
pub(crate) struct PinnedDirectory   shared=1 snapshot=0
impl PinnedDirectory                shared=1 snapshot=0
impl Drop for PinnedDirectory       shared=1 snapshot=0
crate::reviewed_source_snapshot     shared=0
```

The source search also finds the only `NtCreateFile` declaration in the shared
module. The existing reverse-dependency test continues to reject any shared
production reference to snapshot.

## Verification

| Command | Result |
| --- | --- |
| `cargo fmt --check` | PASS |
| `cargo test windows_protected_fs` | PASS, 2 ownership/dependency tests |
| `cargo test reviewed_source_snapshot` | PASS, 20 focused containment/reparse/replacement/cleanup tests |
| `cargo clippy --all-targets --all-features -- -D warnings` | PASS |
| `git diff --check` | PASS (tracked diff; affected source was already untracked in the broad dirty workspace) |
| `cargo test` | BLOCKED by pre-existing T-0256 `stable_wake_adapter_python` fixed-venv failure after the main 766-test suite passed; it reports missing configured Python 3.12 base interpreter |
| project `rust_full` | no runnable project command is defined in Cargo metadata; not represented as a fabricated pass |

The full-test failure is outside this R2A relocation and is recorded by the
active plan as the separate T-0256 wake-environment blocker. It was not
weakened, skipped, or repaired in this ticket.

## Attributable paths

- `src/windows_protected_fs.rs`
- `src/reviewed_source_snapshot.rs`
- this review bundle

The working tree was broadly dirty and these source files were already
untracked at local Git level, so repository-wide diff/stat output cannot serve
as T-0258 attribution. The narrow source ownership search and focused tests
above are the attributable evidence. No `.catdesk` content, scripts, Git
branch, browser, daemon, release, service, tunnel, signing, or provenance
state was mutated.

## Mechanical gate status

- Single shared type/impl/Drop ownership: **PASS**
- No snapshot impl/Drop residue or fallback: **PASS**
- No shared-to-snapshot production dependency: **PASS**
- Native declaration deduplication needed by the moved closure: **PASS**
- Focused behavioral/security coverage: **PASS**
- Full repository verification: **BLOCKED by the separately tracked T-0256
  environment failure, not by T-0258 source**

Independent review remains required; this bundle does not claim independent
acceptance.
