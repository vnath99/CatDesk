# T-0412 linker diagnostic classifier review

## Classification

`LINKER_DIAGNOSTIC_CLASSIFIER_REVIEWED`

## Bounded diagnostic change

The refinement is diagnostic-only. It adds fixed persisted categories for:

- exact `link.exe` not-found signatures;
- `LNK1104` and `LNK1181` library/input-not-found failures;
- `LNK2001`, `LNK2019`, and `LNK1120` unresolved externals;
- `LNK1318` PDB failures; and
- `LNK1102` linker out-of-memory failures.

An unrecognized linker invocation/failure remains `CARGO_LINK_FAILED`, and an
unrecognized Cargo failure remains `CARGO_EXIT_NONZERO`. Existing network and
locked-lockfile precedence remains ahead of every linker category. No linker
discovery, MSVC/SDK environment, Cargo command, V5 policy, isolated cache,
output authority, or reviewed-build control behavior changed.

`read_bounded_cargo_stderr` continues to call the streaming classifier before
retaining only the bounded 4 KiB prefix. It drains the whole stderr stream and
keeps only the established digest, retained length, and truncation flag in the
terminal result; raw stderr, paths, credentials, and compiler text are not
persisted. The focused regression places `LNK1104` beyond the retained prefix
and confirms that the full-stream classifier still returns
`CARGO_LINK_LIBRARY_NOT_FOUND`.

## Verification

- `cargo_failure_diagnostic_` focused tests: 3 passed, including redaction,
  generic fallback, fixed linker categories, and late-signature handling.
- `reviewed_build::tests::`: 83 passed, 0 failed, 3 documented ignored.
- `cargo test --workspace --all-targets --all-features`: passed. Main suite:
  954 passed, 0 failed, 21 documented ignored; all target and integration
  suites passed.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`:
  passed.
- `git diff --check`: passed after this bundle was created.

## Scope

No classifier or test repair was needed in this review. The existing dirty
worktree was preserved, and this bundle is the sole session-attributable output.
No protected reviewed-build state, runtime, Wake, tunnel, release/LKG, daemon,
Git history, credentials, or external project was mutated.

