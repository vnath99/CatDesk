# T-0324 reviewed-build snapshot child collision production repair

## Classification

`REVIEWED_BUILD_SNAPSHOT_CHILD_COLLISION_PRODUCTION_REPAIRED`

This is a bounded source/test repair that requires fresh independent final
review. It neither retries nor authorizes a live reviewed build.

## Exact observed production failure and cause

The durable result for generation
`bd60e10b52674136a36fdf5d3a1e7368` is
`BUILD_FAILED_OR_AMBIGUOUS` with:

```text
reviewed source snapshot child scripts is unavailable (0xc0000035)
```

`0xc0000035` is `STATUS_OBJECT_NAME_COLLISION`. The real snapshot path is
`reviewed_source_snapshot::pin_create_relative_directories` ->
`ProtectedDirectoryGuard::descend_or_create` ->
`PinnedDirectory::open_or_create_relative`. The old one-shot `FILE_OPEN_IF`
operation returned that collision status for the already-existing `scripts`
child, and the helper reported it as a generic unavailable child. It did not
perform a protected reopen, so the accepted collision intent was not expressed
by the production operation.

## Repair and retained invariants

`src/windows_protected_fs.rs` now implements the shared production helper as
an exact three-step state transition under the same already-pinned parent:

1. `FILE_OPEN` the exact component.
2. Only exact `STATUS_OBJECT_NAME_NOT_FOUND` (`0xc0000034`) may use
   `FILE_CREATE`.
3. Only exact create-side `STATUS_OBJECT_NAME_COLLISION` (`0xc0000035`) may
   issue a second `FILE_OPEN` beneath that same parent.

Every successful open/create/reopen path retains the `RootDirectory` handle
chain, validates the component before use, asserts the parent stable before
the native operation, checks the returned handle is a directory and not a
reparse point, records its stable identity, and is followed by the guard's
post-descent stability assertion. Access denial, malformed status strings,
wrong object types, reparse/substitution, identity drift, unrelated NTSTATUS
values, and a failed reopen remain errors. There is no pathname fallback,
overwrite, delete/recreate, caller input, or change to reviewed-build
authority.

## Regression evidence

The shared helper used by `reviewed_source_snapshot` now has Windows-focused
coverage:

- `descend_or_create_uses_real_open_existing_then_fresh_create_and_refuses_file_or_reparse`
  acquires a real protected guard, creates `bytes/scripts`, reopens that same
  existing nested sequence through the production helper, and refuses a file
  or directory reparse substitute.
- `open_or_create_status_discrimination_is_exact_and_fail_closed` proves that
  only exact terminal renderings of `0xc0000034` and `0xc0000035` receive the
  two permitted transitions; trailing, access-denied, and arbitrary errors do
  not.

Focused commands passed:

```text
cargo test --workspace --all-targets --all-features open_or_create -- --nocapture
cargo test --workspace --all-targets --all-features descend_or_create_uses_real -- --nocapture
```

The required local verification commands also completed with no reported test
or lint failure: `cargo fmt --all -- --check`, strict workspace/all-target/
all-feature Clippy, `cargo test --workspace --all-targets --all-features`
(936 primary tests plus target-specific binaries), and `git diff --check`.
The existing benign `C:\\Users\\Volap` canonicalization warning and inherited
CRLF warnings remain non-failing. Independent verification remains required.

## Attribution, limits, and next action

Attributable changes are this bundle and the narrow
`src/windows_protected_fs.rs` protected-child open/create seam plus its tests.
The workspace was already broadly dirty, including tracked and untracked
source, scripts, documents, and durable state; those inherited changes are not
attributed here.

No reviewed-build PREPARE/CONFIRM/RESULT, daemon reload, release or protected
state mutation, wake/target/browser action, Secure MCP/tunnel action,
ProgramData/Program Files write, signing/UAC request, Git publication, or
external-project mutation occurred.

After independent final review, the only next bounded action is a separately
authorized serving-candidate materialization and guarded live retry using a
new reviewed-build generation. The failed attempt and its token remain
non-reusable.
