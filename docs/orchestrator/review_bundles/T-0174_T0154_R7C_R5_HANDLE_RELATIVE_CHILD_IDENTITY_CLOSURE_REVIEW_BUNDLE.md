# T-0174 / T-0154-R7C-R5 — Handle-relative child identity closure

## Reproduced R4 gap

R7C-R4 pinned the workspace, `.catdesk`, and snapshot-root directories, but
the staging directory, byte-object parents, manifest/content opens, commit,
and cleanup still started from ordinary path operations.  A substituted child
could therefore be selected after its parent had been checked.  This ticket
keeps the R7C v4 manifest and digest model unchanged and closes that child
parent hand-off.

## Ownership model

`ProtectedDirectoryGuard` now remains the authority parent at every staged
child operation.  The new `pin_create_relative_directories` helper acquires a
stable guard for every existing or newly-created component one at a time;
each parent is checked before descent and each created child is opened and
classified before it becomes the next parent.  Staging is created beneath a
pinned snapshot root and is itself pinned.  On Windows, directory handles are
opened with reparse-point inspection and no delete sharing for protected
parents.  The staging handle alone permits its same-parent final rename while
retaining its file identity; byte descendants are released only after staged
content validation and before that rename.

Manifest and content objects are created with `create_new`, are bounded and
synced before use, are reclassified as ordinary regular files, and are written
only with a pinned parent.  Existing children cannot be overwritten.  Owned
cleanup now accepts pinned root/staging guards and re-pins directory children
before recursive enumeration/deletion; an unsafe or drifted component stops
cleanup rather than traversing it.

## Commit and replay

The pinned root and the exact staging identity are checked immediately before
rename.  A rename failure may be idempotent only if the independently
committed final directory passes the existing exact v4 validation; it never
recaptures mutable workspace bytes.  Staging remains non-authoritative.
Authority/manifest/snapshot-id digests, structured completion-output binding,
source inventory, lexical ordering, and size bounds are unchanged.

## Regression coverage

The shared-module suite now includes child link-redirection refusal with an
outside directory remaining empty, and a pre-existing child-file test proving
`create_new` does not replace it.  Existing coverage continues to exercise
nested binary snapshots, literal/dynamic include handling, content/manifest
tamper, legacy-state refusal, source drift replay, completion-output drift,
stale staging, protected-root redirection, and bounds/ordering invariants.

## Changed files

- `src/reviewed_source_snapshot.rs` — child guard propagation, staged
  directory pinning, no-overwrite file creation, guarded cleanup, and focused
  deterministic regressions.
- This review bundle.

## Verification evidence

`cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D
warnings`, and `git diff --check` completed without errors.  The focused
`cargo test reviewed_source_snapshot` suite passed (12 tests); the full
`cargo test` surface passed with 602 passed and 18 ignored tests, plus both
Windows project fixtures.  `cargo build --release` completed successfully.

## Scope and preserved invariants

No build worker, promotion, reload, recovery, tunnel, or external MCP action
was invoked.  R6/R6A/R6B promotion/LKG/direct-script controls, redaction,
and dirty-workspace compatibility remain outside and unchanged.  T-0169/R7D
remains blocked pending host acceptance of this R7C-R5 closure.
