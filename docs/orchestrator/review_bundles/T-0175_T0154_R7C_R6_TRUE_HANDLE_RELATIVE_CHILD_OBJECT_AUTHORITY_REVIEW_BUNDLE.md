# T-0175 / T-0154-R7C-R6 — True handle-relative child authority

## R5 rejection reproduced

R7C-R5 kept outer directory handles but still created/opened staged children,
read manifest/content, enumerated bytes, renamed staging, and removed attempt
trees by pathname.  A child replacement between the parent check and those
operations could therefore change the object used as authority.

## Windows child-object mechanism

The snapshot module now uses `NtCreateFile` with
`OBJECT_ATTRIBUTES.RootDirectory` for every protected child directory and
regular-file operation.  Components are individually validated before they
are converted to a relative UTF-16 object name.  A child is opened under the
already-pinned parent handle with reparse-point inspection, and descendant
work retains that child guard.  Directory guards are duplicated with
`DuplicateHandle`, rather than re-opening their stored paths.

Regular manifest/content files use `FILE_CREATE` for no-overwrite writes and
`FILE_OPEN` for reads.  The opened handle is checked for a normal,
non-directory, non-reparse identity and then used for bounded sync/write or
bounded read/hash.  Validation enumerates bytes with `NtQueryDirectoryFile`
on the pinned directory and opens each enumerated child relative to that same
handle.  The former metadata-then-path-read validation is gone.

Commit uses `NtSetInformationFile(FileRenameInformation)` on the pinned
staging handle and a rooted destination name.  It therefore does not invoke
`fs::rename` on mutable source/destination paths.  A failed or competing
commit still accepts only a separately exact committed v4 snapshot.

## Cleanup and permitted path use

Failed attempts retain their uniquely named staging directory.  It is
non-authoritative and ignored by committed replay.  Retention is deliberate:
until every recursive delete operation can be represented as a handle-bound
object delete, the code fails closed rather than performing path-based
cleanup.  Remaining path reads are restricted to initial workspace/source
input collection and non-authority test diagnostics; protected snapshot
manifest/content/session/bytes descendant authority uses rooted handles.

## Regression coverage

Focused tests retain protected-root and child redirection refusal, nested
binary byte replay, content and manifest tamper rejection, extra-object and
legacy rejection, source/output drift refusal, literal/dynamic include
coverage, stale-staging replay, and create-new no-overwrite behavior.  The
handle path exercises capture, commit, committed replay, and exact content
hash validation on Windows.

## Changed files

- `src/reviewed_source_snapshot.rs` — rooted `NtCreateFile` child guards,
  handle-based regular-file reads/writes, `NtQueryDirectoryFile` enumeration,
  and rooted `NtSetInformationFile` commit.
- This review bundle.

## Verification

`cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D
warnings`, and `git diff --check` completed cleanly.  The focused snapshot
suite passed all 13 tests.  The full Rust suite passed with 603 passed and 18
ignored tests, and both Windows project fixtures passed.  `cargo build
--release` completed successfully.

## Scope

R7C v4 schema/digests, task-output authority binding, source inventory and
bounds remain unchanged.  No build worker, promotion, daemon reload, recovery,
tunnel, or external MCP action was invoked.  T-0169/R7D remains blocked
pending host acceptance.
