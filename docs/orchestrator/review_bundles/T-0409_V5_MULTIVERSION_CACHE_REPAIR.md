# T-0409 V5 multiversion cache repair review

## Classification

`V5_MULTIVERSION_CACHE_REPAIR_REVIEWED`

The current bounded repair addresses the terminal protected-build generation
`149f1d2522fe4054a2283cfa4ff5373c` without retrying it or altering its
attempt, claim, owner, or result records.

## Failure evidence and repair

The durable result for generation `149f1d2522fe4054a2283cfa4ff5373c` is
`BUILD_FAILED_OR_AMBIGUOUS` with `failureCode` `protected filesystem child is
unavailable`.  Its V5 policy is the fixed locked/offline policy with immutable
snapshot and exact cargo/rustc identities recorded in that terminal attempt.

`copy_locked_registry_closure` now maintains a `BTreeSet` of lowercase crate
names.  It copies each sparse-index leaf once per crate name, avoiding an
otherwise duplicate create-new write when a valid lockfile contains several
versions of one crate.  It still invokes `copy_locked_registry_archive` for
every `name-version.crate` archive.  Each archive is read through the protected
handle-relative helper, bounded, SHA-256 checked against the lockfile checksum,
and written create-new into the isolated Cargo home.  Missing, malformed, or
checksum-mismatched archives fail closed.

The V5 policy remains `build --release --locked --offline`; the seeding path
uses only a protected local registry index and cache closure, never the mutable
unpacked registry source tree.  No ambient Cargo home is inherited as the
execution environment and no network fallback is introduced.

## Verification

- `reviewed_build::tests::offline_cache_seed_copies_one_index_leaf_for_multiple_locked_versions`:
  passed.
- `reviewed_build::tests::`: 82 passed, 0 failed, 3 documented ignored.
- `cargo test --workspace --all-targets --all-features`: passed.  Main suite:
  953 passed, 0 failed, 21 documented ignored; all remaining target and
  integration suites passed.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`:
  passed.
- `git diff --check`: passed after this bundle was created.

## Scope

This review made no product-source, protected reviewed-build Store, runtime,
Wake, tunnel, release, LKG, or Git-history mutation.  The existing dirty
worktree was preserved.  The only session-attributable output is this review
bundle.  A subsequent protected build, if authorized, remains a separate
operation.

