# T-0410 V5 index-refusal diagnostic

## Classification

`V5_INDEX_REFUSAL_DIAGNOSTIC_BLOCKED`

## Durable failure evidence

The immutable reviewed-build generation
`9b173414074f46b8be88a1e4ac58e43e` records
`BUILD_FAILED_OR_AMBIGUOUS` with failure code
`reviewed build local cargo index is unsafe`.  Its attempt is bound to the V5
locked/offline policy and remains unmodified.

## Requested diagnostic execution

Executed exactly:

```text
cargo test diagnostic_first_ambient_sparse_index_refusal -- --nocapture
```

The requested test did not compile, so it emitted no `DIAG_INDEX_PARENT` or
`DIAG_INDEX_LEAF` line and no sparse-index safety condition can be identified
truthfully from this session.  The compiler reported two instances of:

```text
error[E0425]: cannot find value `FILE_ATTRIBUTE_REPARSE_POINT` in this scope
```

at `src/reviewed_build.rs:8259` and `src/reviewed_build.rs:8284`.  The test uses
that identifier only to format its reparse metadata; the existing constants are
private to `reviewed_source_snapshot` and `windows_protected_fs`, and are not
visible in `reviewed_build.rs`.

## Scope and next bounded action

No product-source or test repair was made, as required by this evidence-only
diagnostic task.  The minimum next action, under separately granted authority,
is a test-only compilation correction that supplies the fixed Windows reparse
attribute mask to the temporary diagnostic.  Only after that correction can the
same command read the first protected sparse-index parent or leaf and emit its
bounded `DIAG_*` record.  No protected build state, live Wake, tunnel, release,
LKG, credentials, Git history, or external project was accessed or mutated.

