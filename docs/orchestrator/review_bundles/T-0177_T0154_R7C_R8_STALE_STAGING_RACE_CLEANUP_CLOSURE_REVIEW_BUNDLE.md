# T-0177 / T-0154-R7C-R8 — Stale staging race-cleanup closure

## T-0176 gap

R7C-R7 provided a handle-bound recursive deletion primitive, but did not
reclaim crash residue before a new capture/replay and did not provide a
deterministic last-pre-disposition test seam.

## Exact ownership and restart behavior

Before capture/replay, the already-pinned reviewed-source-snapshots root is
enumerated with `NtQueryDirectoryFile`.  Only a direct child named exactly
`.{sessionId}-{canonical UUID}.staging` is eligible.  The name is opened
relative to the root with delete authority, must retain direct-child identity,
and is reclaimed through the existing recursive handle-bound cleanup path.
Committed session directories, malformed current-session candidates,
other-session staging, reparse/special objects, and arbitrary children are
not cleanup targets.  Missing residue is idempotent success; ambiguity fails
closed.

## Test-only disposition seam

`cfg(test)` contains a private hook registry invoked immediately before a file
or directory disposition, including the final staging-root disposition.  It
is absent from production builds and does not participate in authority
decisions.  The test writes replacement content at the last file boundary and
shows deletion is still applied to the already-opened object, not a
re-discovered authority path.

## Regression matrix

The focused suite now covers nested cleanup, current-session stale restart
reclamation, repeated exact replay, other-session preservation, content and
manifest substitution refusal, child redirection, protected-root redirection,
legacy refusal, dynamic include failure, source/output drift, and the
pre-disposition hook.  Existing snapshot bytes/digests are validated through
the unchanged v4 replay path; stale cleanup never reads mutable workspace
sources as authority.

## Changed files

- `src/reviewed_source_snapshot.rs` — canonical staging-name ownership,
  pinned-root stale reclamation, private test hooks, and deterministic restart
  and pre-disposition regressions.
- This review bundle.

## Verification

`cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D
warnings`, and `git diff --check` completed cleanly. The focused snapshot
suite passed all 16 tests. The full Rust suite passed with 606 passed and 18
ignored tests, both Windows project fixtures passed, and `cargo build
--release` completed successfully.

## Scope

R7C schema/digests, source inventory, completion ordering, promotion, build,
reload, recovery, and external MCP behavior remain untouched. T-0169/R7D is
out of scope.
