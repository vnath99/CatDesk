# T-0416 V5 host linker diagnostic runner

## Classification

`V5_HOST_LINKER_DIAGNOSTIC_RUNNER_VERIFICATION_BLOCKED`

The bounded test-only runner is implemented, but this source-review ticket is
not ready for positive approval because the current workspace cannot compile
the Wake dependency before the focused or full Rust test profiles reach the new
reviewed-build tests. No host diagnostic was executed.

## Evidence read

- T-0412 records the original V5 terminal linker evidence and requires one
  bounded host-local diagnostic rather than a speculative linker/environment
  repair.
- T-0412R1 establishes that Cargo stderr is fully drained and classified before
  only bounded digest/length/truncation metadata is retained.
- The active-generation pointer currently resolves to
  `ea299f644c6b4eb89e2331a5b6098dce`, a V5 retry lineage whose immutable
  result is `BUILD_FAILED_OR_AMBIGUOUS` / `REVIEWED_BUILD_FAILED` /
  `CARGO_LINK_FAILED`, phase `CARGO_BUILD`, exit `101`.

## Bounded implementation

`src/reviewed_build.rs` now contains a Windows `cfg(test)`-only ignored unit
test runner. It has no MCP, CLI, or production entry point and accepts no
caller-selected path, attempt, environment, toolchain, target, or
classification.

Before it can start Cargo, it resolves the product-owned active generation and
revalidates:

1. active attempt digest, exact current V5 policy, committed snapshot, and
   attested Cargo/Rustc evidence through the existing validation helpers;
2. immutable claim and owner proof; and
3. exact terminal `BUILD_FAILED_OR_AMBIGUOUS`,
   `REVIEWED_BUILD_FAILED`, `CARGO_BUILD`, exit `101`, and broad
   `CARGO_LINK_FAILED` evidence.

It rechecks the same active-generation pointer immediately before spawning
Cargo, so a concurrent lineage change fails closed rather than diagnosing a
stale attempt.

It materializes the verified snapshot and isolated Cargo closure only beneath a
fresh UUID child of `target-verify/reviewed-build-host-linker-diagnostic`.
That directory is non-authority material: the runner cannot write a control
record, candidate, attestation, promotion record, or active-generation pointer.
It reuses the worker's pinned Cargo/Rustc launch validation, fixed
`build --release --locked --offline` argv, `env_clear`, toolchain-parent-only
`PATH`, OS-derived `SystemDrive`, and isolated per-diagnostic
`CARGO_HOME`/`CARGO_TARGET_DIR`/`TEMP`/`TMP`. It does not inherit `LIB`,
`LIBPATH`, `INCLUDE`, `VCINSTALLDIR`, `VSINSTALLDIR`, caller `PATH`, or a
developer shell environment.

Cargo stderr is drained through the existing complete-stream classifier. The
runner returns only:

- `LINKER_EXECUTABLE_NOT_FOUND` for `CARGO_LINKER_NOT_FOUND`;
- `MSVC_OR_WINDOWS_SDK_LIBRARY_ENV_MISSING` for
  `CARGO_LINK_LIBRARY_NOT_FOUND`; or
- `OTHER_LINKER_EXIT` for every other nonzero result.

Its outcome holds only the fixed class plus SHA-256, retained byte length, and
truncation bit. It has no raw stderr field or serialization/persistence path.
Cargo success and every authority mismatch fail closed.

The shared snapshot-materialization, Cargo-home seeding, and command setup were
minimally factored so the runner reuses the same protected validations rather
than implementing parallel path or environment authority. The production worker
continues to use the same helper behavior.

## Deterministic coverage added

- fixed classifier mapping for linker-not-found, LNK1104, and generic linker
  exit, including success refusal and redacted outcome shape;
- terminal/V5/failure-code/classification/attempt-digest gate rejection and
  immutable audit non-mutation;
- exact closed command environment allowlist and source-level assertions for
  no inherited developer-toolchain variables or control/attestation persistence;
- an explicitly ignored manual-only runner test. It was intentionally not run.

## Verification

| Command | Result |
| --- | --- |
| `cargo test --workspace --all-targets --all-features fixed_v5_host_linker_diagnostic -- --nocapture` | BLOCKED before reviewed-build tests: inherited `wake/src/runtime.rs` E0384, assignment to immutable `deadline` at line 1846. |
| `cargo test --workspace --all-targets --all-features` | BLOCKED by the same inherited Wake compile error. |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | BLOCKED by the same inherited Wake compile error. |
| `rustfmt --edition 2024 --check src/reviewed_build.rs` | Parsed the edited file, but reports pre-existing formatting drift throughout the existing dirty file; no broad reformat was applied. |
| `git diff --check` | PASS, with inherited CRLF warnings only. |

## Independent-verifier follow-up

The independent verifier reproduced the same out-of-scope compilation blocker
while running the required strict Clippy profile: `wake/src/runtime.rs:1846`
assigns to immutable `deadline` (`E0384`). It also reported broad unrelated
workspace formatting drift, beginning in
`examples/local_mcp_binagotchy_retire.rs`, from `cargo fmt --all -- --check`.

Neither file is within this task's reviewed-build-only implementation surface.
Accordingly, no Wake, example, or other unrelated source was changed to mask
those failures. The classification remains
`V5_HOST_LINKER_DIAGNOSTIC_RUNNER_VERIFICATION_BLOCKED` until the inherited
workspace blockers are independently repaired and the required profiles can
reach the T-0416 code.

No Wake dev.51/dev.52, target, browser, tunnel, active reviewed-build attempt,
claim, result, attestation, candidate, release/LKG, daemon, Git history,
credentials, or external project was mutated. The host-local ignored diagnostic
was not invoked.
