# T-0410R2 V5 index-refusal diagnostic

## Classification

`V5_INDEX_REFUSAL_DIAGNOSTIC_UNAVAILABLE`

## Immutable generation evidence

Generation `9b173414074f46b8be88a1e4ac58e43e` remains unchanged.  Its durable result
is `BUILD_FAILED_OR_AMBIGUOUS` with failure code `reviewed build local cargo
index is unsafe`.

## Corrected diagnostic execution

The temporary Windows-only diagnostic now uses the fixed `0x0400` Windows
reparse attribute mask.  Executed exactly:

```text
cargo test diagnostic_first_ambient_sparse_index_refusal -- --nocapture
```

The test compiled, but failed before it reached a sparse-index parent or leaf:

```text
registry root: "diag registry is unavailable"
```

No `DIAG_INDEX_PARENT` or `DIAG_INDEX_LEAF` line was emitted.  The first fired
safety condition is therefore **pre-index protected registry-root
unavailability**, not an observed oversized entry, reparse/type condition, or
another protected read error for a particular crate/index key.  The sandbox also
reported that it could not canonicalize `C:\\Users\\Volap`, which is consistent
with the protected root acquisition being unavailable in this execution context.

## Scope

No production repair was made.  The historical reviewed-build state, live Wake,
tunnel, release/LKG, credentials, Git history, and external projects were not
mutated.  A future diagnostic would require a separately authorized environment
where the OS-derived current-user `.cargo\\registry` root can be opened by the
same protected-read primitive; only then can the temporary test emit the bounded
first-refusal `DIAG_*` record.

