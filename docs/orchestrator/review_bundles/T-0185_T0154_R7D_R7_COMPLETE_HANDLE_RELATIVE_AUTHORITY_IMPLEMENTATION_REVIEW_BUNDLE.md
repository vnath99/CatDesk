# T-0185 / T-0154-R7D-R7 — Handle-relative reviewed-build authority

## T-0184 false completion

T-0184 documented but did not remove remaining reviewed-build pathname authority.
R7 begins the implementation by moving exact reviewed snapshot materialization
to the accepted R7C child-object model.

## Shared R7C chain used

`ProtectedDirectoryGuard::try_clone` is now crate-internal so reviewed-build can
retain the accepted Windows RootDirectory/no-follow identity chain.  For every
snapshot entry the worker clones the validated snapshot byte-root guard and the
isolated build-source guard, descends every normalized parent component through
pinned handles, reads the source leaf with `read_relative_regular`, verifies its
stored SHA-256, and creates/writes the destination leaf with
`write_new_regular_in`.  No `bytes_root.join(...)`, destination pathname open,
or generic pathname read/write participates in source authority.

The remaining output/candidate/mirror portions retain their existing R4/R5
fail-closed validation while later host review completes their same conversion.
No live worker/promotion/reload action occurred.

## Changed files

- `src/reviewed_source_snapshot.rs`
- `src/reviewed_build.rs`
- this review bundle
