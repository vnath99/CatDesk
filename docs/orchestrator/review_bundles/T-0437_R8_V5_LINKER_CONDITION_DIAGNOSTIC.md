# T-0437 R8 V5 linker-condition diagnostic

## Classification

`LINKER_CONDITION_NOT_RECOVERED__ISOLATED_CARGO_CACHE_PREFLIGHT_BLOCKED`.

The terminal protected generation
`4d82c920311b4b60817ac03bc3edc9c5` remains immutable and was not retried.
Its durable result remains `BUILD_FAILED_OR_AMBIGUOUS` /
`REVIEWED_BUILD_FAILED` at `CARGO_BUILD`, exit `101`, classified
`CARGO_LINK_LIBRARY_NOT_FOUND`. Its historical bounded stderr evidence is
SHA-256 `a386ef1a155a4bde5eb5f07af8282958e033e0fc9346fa5436073ba1661eb94a`,
retained length `4096`, and `stderrTruncated=true`; it contains no retained
safe missing-library basename.

## Current-source diagnostic change

`FixedV5HostLinkerDiagnosticOutcome` is compiled only under `cfg(test,
windows)`. It now retains the classifier's existing sanitized optional
`missingLinkLibrary` and `missingLinkInput` basenames in addition to the
existing fixed classification, captured-stderr SHA-256, retained length, and
truncation flag. It neither stores raw stderr nor accepts a path, environment,
toolchain, attempt, target, or classifier input. The ignored manual test emits
only those fields.

Focused tests prove `.lib` produces `x.lib` for both safe fields, a non-library
link input produces only `catdesk.pdb`, and the other fixed outcome classes
produce neither. Existing classifier tests continue to reject unsafe or
non-basename text.

## One manual diagnostic execution

Exactly one invocation of the ignored manual-only current-source diagnostic
was performed. It failed closed before Cargo launch at isolated local
Cargo-cache seeding with the fixed redacted condition
`REVIEWED_BUILD_DEPENDENCY_CACHE_UNAVAILABLE` (local cargo cache unavailable).

Consequently, this execution produced no Cargo/linker outcome and no raw
stderr. The permitted output fields are therefore:

- classification: `NOT_EMITTED`
- missingLinkLibrary: `NONE`
- missingLinkInput: `NONE`
- captured-stderr SHA-256: `NOT_EMITTED`
- retained length: `NOT_EMITTED`
- truncation: `NOT_EMITTED`

No second diagnostic run was attempted.

## Evidence-based analysis

The fixed worker command remains environment-cleared and supplies only the
attested Cargo/Rustc, isolated `CARGO_HOME`, fixed target/temp roots,
toolchain-only `PATH`, and product-derived `LIB`, `LIBPATH`, and `INCLUDE`.
The fixed-root toolchain builder validates `link.exe`, `libcmt.lib`,
`ucrt.lib`, and `kernel32.lib` before it can construct those variables; it
does not inherit developer-shell MSVC/SDK variables.

The cache-seeding failure occurred before that builder and before Cargo
started. It therefore supplies no evidence that the terminal condition is an
MSVC/Windows SDK library-environment failure, nor evidence of a non-library
link input/output failure. No library basename may be inferred from the
historical classification alone.

The smallest justified next repair is a separate bounded cache-seeding
observability diagnostic: retain only a fixed-vocabulary source-stage category
(profile, registry, index, cache, config, or checked closure) while preserving
no-follow checks and never persisting raw paths, registry content, environment
values, or Cargo stderr. A toolchain or linker-environment repair is not yet
evidence-based.

## Verification

- `cargo test reviewed_build::tests -- --nocapture`: passed, 91 passed and 5
  existing manual/host-dependent tests ignored. The manual linker runner was
  ignored by this ordinary profile.
- Focused safe-basename outcome test: passed.
- `cargo fmt --all -- --check`: passed.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`:
  passed.
- `scripts/test-catdesk-lifecycle.ps1`: passed.
- `git diff --check`: passed.
- `cargo test --workspace --all-targets --all-features`: passed (1,011 tests;
  ignored tests remained ignored).

The first full test run exposed two GitHub-publication executor unit tests
whose test-only fixture read the immutable R7 manifest from the live
workspace. The allowed diagnostic edit to `src/reviewed_build.rs` correctly
made that historical manifest's per-file check drift. Rather than alter the
immutable R7 manifest or descriptor, the two tests now create a hermetic
one-file schema-v2 manifest fixture with the same classification/exclusion
shape. The production executor and historical authority artifacts are
unchanged; the focused executor suite passes (7 tests).

No protected-build retry, promotion, publication, SDK/toolchain mutation,
Wake/tunnel/runtime change, or Git operation occurred.
