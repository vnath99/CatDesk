# T-0437 — V5 linker input diagnostic repair

## Trigger

Fresh protected V5 attempt `95071b8b1c704f79bac802f359162fa1` reached Cargo/linking under the source-current T-0436 controller, proving the previous `REVIEWED_BUILD_TOOLCHAIN_UNAVAILABLE` blocker was fixed. The attempt then terminated with:

- state: `BUILD_FAILED_OR_AMBIGUOUS`
- phase: `CARGO_BUILD`
- exitCode: `101`
- classification: `CARGO_LINK_LIBRARY_NOT_FOUND`
- `missingLinkLibrary`: absent

The manual fixed-host replay did not reproduce a missing-library classification; it produced `OTHER_LINKER_EXIT`. Inspection showed the production classifier labeled **every** LNK1104/LNK1181 as a missing library even when no safe `.lib` basename was extracted. Since those linker codes can refer to non-library files, the persisted classification was over-broad.

## Repair

Only `src/reviewed_build.rs` is changed.

- Added `CARGO_LINK_INPUT_NOT_FOUND`.
- LNK1104/LNK1181 is now `CARGO_LINK_LIBRARY_NOT_FOUND` **only** when the safely extracted basename ends in `.lib`.
- Otherwise the same linker codes are `CARGO_LINK_INPUT_NOT_FOUND`.
- Added optional `missingLinkInput` diagnostic while preserving historical `missingLinkLibrary`.
- Safe basename retention remains path-free and raw-stderr-free.
- The closed allowed extensions are:
  - `.lib`
  - `.obj`
  - `.pdb`
  - `.ilk`
  - `.exe`
  - `.dll`
  - `.exp`
  - `.res`
- A basename must remain <= the existing bounded basename cap and contain only ASCII alphanumeric, dot, underscore, or hyphen.
- Unsafe basenames are not retained.
- Fixed host linker diagnostic mapping now exposes `LINK_INPUT_UNAVAILABLE` for the new coarse classification.
- The host diagnostic terminal-evidence gate accepts both historical `CARGO_LINK_LIBRARY_NOT_FOUND` and current `CARGO_LINK_INPUT_NOT_FOUND`.

No protected-build authority, toolchain environment, Cargo argv, source-snapshot authority, daemon reload authority, promotion/LKG, Wake, Secure MCP, or T-0425 behavior is changed.

## Regression coverage

PASS:

- `cargo_failure_diagnostic_classifies_lockfile_linker_and_compiler_failures`
  - `.lib` LNK1104/LNK1181 -> `CARGO_LINK_LIBRARY_NOT_FOUND`
  - safe `.pdb` LNK1104 -> `CARGO_LINK_INPUT_NOT_FOUND`
- safe library path persists only `legacy_stdio_definitions.lib`
- safe non-library path persists only `catdesk.pdb`
- unsafe basename `not safe.lib` persists no basename and is not mislabeled as a library failure
- chunk-boundary extraction preserves safe library basename
- fixed host redacted mapping accepts the new `LINK_INPUT_UNAVAILABLE` outcome
- fixed V5 host diagnostic terminal-evidence gate remains immutable/fail-closed
- raw paths remain absent from serialized diagnostics

## Verification

- `cargo test cargo_failure_diagnostic -- --nocapture`: PASS, 5/5.
- `cargo test fixed_v5_host_linker_diagnostic_maps_only_redacted_fixed_outcomes -- --nocapture`: PASS.
- `cargo test fixed_v5_host_linker_diagnostic_gates_terminal_v5_and_immutable_evidence -- --nocapture`: PASS.
- `cargo clippy --bin catdesk -- -D warnings`: PASS.
- `cargo test --bin catdesk`: PASS, 977 passed / 0 failed / 23 ignored.

## Required continuation after independent review

1. naturally Wake-deliver and ACK this repair review;
2. build an exact non-serving source-current candidate containing T-0437;
3. create a new candidate-specific daemon-reload approval artifact/review for that exact path/SHA-256/length;
4. naturally Wake-deliver and ACK that candidate review;
5. reviewed reload to the T-0437 candidate;
6. issue one fresh protected V5 retry under a current ACKed source snapshot;
7. inspect the new bounded diagnostic:
   - if `BUILD_ATTESTED`, continue promotion/LKG/recovery;
   - if `CARGO_LINK_LIBRARY_NOT_FOUND`, use the safe retained `.lib` basename to repair the closed LIB environment;
   - if `CARGO_LINK_INPUT_NOT_FOUND`, use only the safe retained basename/extension to repair the corresponding linker-output/input condition;
   - otherwise stop on the fixed-vocabulary failure and diagnose that exact class.

No daemon reload, protected build retry, promotion/LKG/recovery mutation, Wake mutation, T-0425 resume, Secure MCP mutation, or Git publication is performed by this repair task.
