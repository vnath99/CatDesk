# T-0411 V5 offline-cache closure final review

## Classification

`CURRENT_SOURCE_ELIGIBLE_FOR_REVIEWED_SOURCE_AUTHORITY_AND_PROTECTED_V5_BUILD`

This is an eligibility review only. It neither freezes a reviewed-source
snapshot nor starts a protected build.

## Terminal evidence and bounded repair

- Immutable generation `149f1d2522fe4054a2283cfa4ff5373c` recorded
  `protected filesystem child is unavailable`. The repair handles valid
  multiple lockfile versions of one crate: `copy_locked_registry_closure`
  deduplicates the sparse-index leaf only by `name.to_ascii_lowercase()`.
- Immutable generation `9b173414074f46b8be88a1e4ac58e43e` recorded `reviewed
  build local cargo index is unsafe`. The bounded limit is now 8 MiB
  (`8 * 1024 * 1024`), above the observed 4,378,891-byte `web-sys` sparse-index
  entry and still finite.

Each locked `name-version.crate` archive is still processed for every package,
read through the handle-relative protected helper, limited by the unchanged
64 MiB archive bound, SHA-256 checked against its lockfile checksum, and written
create-new into the isolated cache. The sparse-index read continues to use the
same protected regular-file/no-follow/type and stable-handle checks. The
execution policy remains exactly `build --release --locked --offline`; it has no
network fallback and does not execute with the ambient Cargo home.

The temporary `diagnostic_first_ambient_sparse_index_refusal` test is absent
from the current source.

## Verification

- `reviewed_build::tests::offline_cache_seed_copies_one_index_leaf_for_multiple_locked_versions`:
  passed.
- `reviewed_build::tests::offline_cache_seed_accepts_observed_large_sparse_index_entry_within_bound`:
  passed.
- `reviewed_build::tests::`: 83 passed, 0 failed, 3 documented ignored.
- `cargo test --workspace --all-targets --all-features`: passed. Main suite:
  954 passed, 0 failed, 21 documented ignored; all target and integration
  suites passed.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`:
  passed.
- `git diff --check`: passed after this bundle was created.

## Scope

No product-source repair was made in this review. The existing dirty worktree
was preserved, and this bundle is the sole session-attributable output. No
protected reviewed-build state, live Wake, tunnel, release/LKG, daemon,
credentials, Git history, or external project was mutated.

