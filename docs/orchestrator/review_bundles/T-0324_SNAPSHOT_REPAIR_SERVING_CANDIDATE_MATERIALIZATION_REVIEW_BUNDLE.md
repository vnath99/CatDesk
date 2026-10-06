# T-0324 snapshot-repair serving-controller candidate materialization

## Classification

`SERVING_CONTROLLER_CANDIDATE_READY`

This is a repository/build-verification classification only. It awaits the
separate independent ChatGPT review required before any guarded
serving-controller reconciliation or reload.

## Source and review boundary

The current `src/reviewed_source_snapshot.rs` contains the accepted
snapshot-finalization repair: `literal_include_paths` is the bounded lexical
recognizer for real normal-string `include_str!` and `include_bytes!`
invocations. It ignores macro-shaped text in comments, ordinary and raw
strings, byte strings, and character literals, while unsupported real macro
arguments fail closed as ambiguous. The focused regressions include
`ignores_macro_like_text_but_captures_real_literal_include`,
`ignores_macro_like_text_in_byte_strings`, and
`current_workspace_source_inputs_are_collectable`.

The preceding repair review bundle,
`T-0324_REVIEWED_SOURCE_SNAPSHOT_FINALIZATION_REPAIR_REVIEW_BUNDLE.md`,
classified the repair as
`REVIEWED_SOURCE_SNAPSHOT_FINALIZATION_REPAIRED`. Its established root cause
was a false-positive textual macro counter that could reject the snapshot
module's own documentation and fixtures before protected staging. This task
does not alter that source, its finalization authority, or the SR3N-R3
completion-artifact model. Its approved contract deliberately declares no
`completionArtifactIds`, because the live serving controller lacks this
repair and cannot be used to finalize this materialization record.

## Fixed isolated candidate measurement

The only release build command used was the product-defined
`CARGO_BUILD_RELEASE_ISOLATED` argv:

```text
cargo build --release --locked --target-dir .catdesk/verification-targets/autonomy-release
```

It produced and was measured at the fixed workspace-relative path:

```text
.catdesk/verification-targets/autonomy-release/release/catdesk.exe
SHA-256: 9476b9da909699c9c7eeb3cd00c76aff6a32a6fe479a56c4783dce75455e34fb
Length: 26323968 bytes
```

No default `target/release`, caller-selected target directory, external build
path, shell wrapper, or alternate release profile was used. The isolated
output is a temporary serving-controller bootstrap candidate only. It confers
no ordinary-worker release, reviewed-build, promotion, signing, canonical
release, runtime-selection, wake, tunnel, Secure MCP, or host authority.

## Verification

- `cargo fmt --all -- --check` — PASS.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` —
  PASS (with the environment's existing `C:\\Users\\Volap` canonicalization
  warning).
- `cargo test --workspace --all-targets --all-features` — PASS: 926 primary
  tests plus target-specific test binaries; existing platform-dependent tests
  remained explicitly ignored.
- Fixed isolated locked release build above — PASS; the measured candidate was
  present immediately after the command.
- `git diff --check` — PASS; only inherited CRLF conversion warnings were
  emitted.

## Attribution, prohibited actions, and next boundary

The only intended workspace mutation attributable to this materialization is
this bundle. The `.catdesk/verification-targets/autonomy-release` output is
an isolated verification artifact, not a source or durable-release mutation.
All other dirty files, including the snapshot repair source and its repair
bundle, pre-existed this task and are not attributed here.

No daemon reload, reviewed-build PREPARE/CONFIRM/RESULT, canonical release
state write, wake-target edit, browser action, Secure MCP/tunnel action,
ProgramData/Program Files mutation, signing/UAC, Git publication, or external
project mutation occurred.

The exact next bounded action is independent ChatGPT review of this bundle,
the candidate measurement, the repair boundary, and the attributable diff. A
separate authorization is required before any guarded serving-controller
reconciliation or reload; this materialization neither grants nor performs
that action.
