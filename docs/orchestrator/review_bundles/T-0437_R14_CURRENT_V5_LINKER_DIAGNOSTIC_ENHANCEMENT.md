# T-0437 R14 current V5 linker diagnostic enhancement

## Scope and terminal evidence

This is a current-source, test/manual-only diagnostic enhancement. It does
not retry or modify the protected reviewed-build generation, its result,
policy, toolchain, cache, target, or runtime state.

The active immutable R12 attempt is `0af244208aec45a9a9b094efe2e0c96a`.
Its terminal result remains `BUILD_FAILED_OR_AMBIGUOUS` /
`REVIEWED_BUILD_FAILED`, phase `CARGO_BUILD`, exit `101`, and classification
`CARGO_LINK_LIBRARY_NOT_FOUND`. The durable bounded capture has SHA-256
`d2b22d1413ad41cce78014694833b29de7f7f84b19ed46d8079fc765c1d7302f`,
retained length `4096`, and `stderrTruncated=true`. It retains no safe missing
library or input basename.

No protected build or ignored manual diagnostic was run for this task.

## Finding and bounded repair

The prior `FixedV5HostLinkerDiagnosticOutcome` retained only three actionable
outcomes: linker executable missing, library/input missing, and an
`OTHER_LINKER_EXIT` fallback. Although the full-stream classifier already
recognized unresolved external, PDB, and out-of-memory linker conditions, the
manual outcome collapsed those conditions into that fallback. A generic link
failure could therefore provide no fixed actionable signal.

`src/reviewed_build.rs` now changes only `cfg(test, windows)` diagnostic
state/output:

- recognizes the fixed ordered LNK-code vocabulary `LNK1102`, `LNK1104`,
  `LNK1181`, `LNK2001`, `LNK2019`, `LNK1120`, and `LNK1318` across the fully
  drained stderr stream;
- reports only that fixed code list, the existing fixed outcome category,
  existing validated safe basenames, retained-prefix SHA-256/length, and the
  truncation flag;
- maps the existing known classifier categories to
  `LINK_UNRESOLVED_EXTERNAL`, `LINK_PDB_FAILURE`, and
  `LINK_OUT_OF_MEMORY` instead of flattening them to `OTHER_LINKER_EXIT`.

The production classifier, capture size, redaction boundary, persistence
schema, worker environment, policy, and authority paths are unchanged.
`OTHER_LINKER_EXIT` remains the fail-closed fallback when no recognized safe
condition exists.

## Redaction and regression coverage

The focused tests cover each fixed outcome and prove that signals beyond the
retained 4 KiB prefix are still found. The hostile-stream regression includes
path-shaped text, an environment-shaped value, a credential-shaped value, and
an arbitrary linker-argument-shaped value. The diagnostic's debug and manual
summary representations retain only the fixed code list and validated
`kernel32.lib` basename; they exclude all hostile values. No raw stderr,
absolute path, environment value, credential, or arbitrary linker argument is
persisted or printed.

## Verification

- `cargo fmt --all -- --check`: passed.
- `cargo test reviewed_build::tests::fixed_v5_host_linker_diagnostic -- --nocapture`:
  passed; 4 tests passed and the manual host-only runner remained ignored.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`:
  passed.
- `cargo test --workspace --all-targets --all-features`: passed; 1,017 root
  tests passed with existing manual/host-only tests ignored.
- `git diff --check`: passed. It emitted only inherited CRLF conversion
  warnings for existing dirty tracked files; no whitespace error was reported.

## Recommendation

Run the existing ignored host-local diagnostic only through a separately
authorized bounded diagnostic task. If it reports a recognized fixed class or
safe basename, evaluate that evidence before changing the fixed Windows linker
environment. If it remains `OTHER_LINKER_EXIT` with an empty code list, retain
the fail-closed result and design a further bounded diagnostic rather than
infer a toolchain repair.
