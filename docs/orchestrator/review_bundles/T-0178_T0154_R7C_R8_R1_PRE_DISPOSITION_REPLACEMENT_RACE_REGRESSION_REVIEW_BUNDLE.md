# T-0178 / T-0154-R7C-R8-R1 — Pre-disposition replacement-race regression

## T-0177 rejection reproduced

The previous final-disposition test rewrote bytes in an already-open file. It
did not change the directory entry or object identity, so it could not prove
the handle-bound cleanup behavior under true replacement races.

## Test-only race seams

The private `cfg(test)` disposition hook remains absent from production builds
and is serialized for deterministic tests. New cases use it after the target
handle is opened and immediately before native disposition to attempt:

- rename/move of the opened file followed by a replacement at the same name;
- rename/move of an opened nested directory followed by a replacement tree;
- a reparse/file-link replacement at the same name when Windows capabilities
  permit it; and
- current-session stale staging replacement after root enumeration but before
  its relative open/classification.

If Windows sharing denies the attempted replacement, the test records that
denial as the expected fail-closed outcome. If replacement succeeds, the
native disposition applies only to the already-opened original. The new entry
remains, cleanup stops on the nonempty substituted parent, and the outside
sentinel remains unchanged.

## Results and preserved behavior

The race matrix asserts file replacement, nested-directory replacement,
reparse substitution, and enumeration-to-open staging substitution have zero
outside deletion. Existing stale-current-session reclamation, other-session
preservation, malformed refusal, committed replay, source-drift replay, v4
digest stability, and legacy/tamper coverage are retained. No production race
injection, pathname deletion, schema, promotion, or build-worker change was
introduced.

## Changed files

- `src/reviewed_source_snapshot.rs` — private deterministic final-boundary and
  stale-reclaim-open hooks plus object-replacement/reparse regression tests.
- This review bundle.

## Verification

`cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D
warnings`, and `git diff --check` completed cleanly. The focused snapshot
suite passed all 20 tests. The full Rust suite passed with 610 passed and 18
ignored tests, both Windows project fixtures passed, and `cargo build
--release` completed successfully.

## Scope

No live build, promotion, reload, recovery, tunnel, browser, scheduler, or
external project operation was invoked. T-0169/R7D remains blocked and out of
scope.
