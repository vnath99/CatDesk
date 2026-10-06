# T-0176 / T-0154-R7C-R7 — Handle-bound recursive cleanup deletion

## T-0175 rejection

R7C-R6 correctly made snapshot capture, validation, enumeration, and commit
handle-relative, but intentionally retained failed-attempt staging trees.  It
had no recursive delete primitive that could prove each object selected for
deletion was beneath the exact pinned staging directory.

## Native cleanup semantics

`remove_owned_staging` now requires that its guard is an identity-proven direct
child of the pinned reviewed-source-snapshots root, with the CatDesk-generated
`.session-<uuid>.staging` shape.  It can never target a workspace root,
`.catdesk`, snapshot root, committed session directory, or an outside object.

`cleanup_staging_tree` obtains names through `NtQueryDirectoryFile`, opens
each file or directory with `NtCreateFile` rooted in the already-pinned parent,
rejects reparse objects and malformed names, recurses only into opened
directories, and deletes only the exact opened handle.  It first requests
native `FileDispositionInformationEx(DELETE)` and falls back only for older
Windows that reject the Ex information class; the fallback is the same
already-opened DELETE handle with `FileDispositionInformation`.  There is no
path-based remove-file/remove-dir fallback.

## Failure model

Any enumeration inconsistency, unsafe child, root/staging identity mismatch,
or native disposition failure stops cleanup fail closed.  Committed snapshots
remain untouched.  Non-Windows builds continue to reject cleanup rather than
using weaker pathname deletion.

## Regression coverage

The focused suite includes an owned staging tree with nested directory and
file cleanup, proving the staging object disappears after its pinned handle is
released while `Cargo.toml` remains present.  Existing R7C regressions retain
protected-root redirection, child redirection, manifest/content tamper,
legacy rejection, source/output drift, nested binary bytes, dynamic include
rejection, and committed replay coverage.

## Changed files

- `src/reviewed_source_snapshot.rs` — handle-bound recursive cleanup and
  native disposition helpers plus focused nested cleanup regression.
- This review bundle.

## Verification

`cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D
warnings`, and `git diff --check` completed cleanly. The focused snapshot
suite passed all 14 tests. The full Rust suite passed with 604 passed and 18
ignored tests, both Windows project fixtures passed, and `cargo build
--release` completed successfully.

## Scope

No schema, digest, source inventory, completion ordering, promotion, build,
reload, recovery, or external MCP behavior changed. T-0169/R7D remains out of
scope and blocked pending host acceptance.
