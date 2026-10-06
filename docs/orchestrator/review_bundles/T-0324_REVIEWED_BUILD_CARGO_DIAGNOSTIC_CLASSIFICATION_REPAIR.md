# T-0324 reviewed-build Cargo diagnostic classification repair

## Classification

`CARGO_DIAGNOSTIC_CLASSIFICATION_REPAIRED_WITH_ISOLATED_OUTPUT_LOCK`

The reviewed-build diagnostic repair is implemented and its full Rust test
profile passes. The fixed isolated release-build comparison reached output
replacement and failed with Windows `Access is denied (os error 5)` while
removing `.catdesk/verification-targets/autonomy-release/release/catdesk.exe`.
That host output lock is not repaired or bypassed by this bounded source task.

## Authority, scope, and historical preservation

This implements the minimum follow-on specified by
`T-0324_REVIEWED_BUILD_8982400_CARGO_FAILURE_DIAGNOSTIC.md`. Generation
`8982400e6a444392ac4a51db25c91cc4` and its consumed confirmation token were
not read for mutation, retried, or otherwise changed. The existing
reviewed-build command, trusted tool selection, isolated Cargo home, protected
control-state paths, source binding, owner semantics, and terminal
`BUILD_FAILED_OR_AMBIGUOUS` behavior remain unchanged.

## Implementation

`src/reviewed_build.rs` now drains every Cargo stderr chunk through an
ephemeral streaming matcher before returning the existing bounded capture. The
matcher recognizes only this closed, redacted vocabulary:

| Signature family | Persisted classification |
| --- | --- |
| crates.io registry plus connection/network/download failure | `CARGO_DEPENDENCY_NETWORK_UNAVAILABLE` |
| lock-file update required with `--locked` | `CARGO_LOCKFILE_OUT_OF_DATE` |
| Cargo linker invocation plus failure/exit indication | `CARGO_LINK_FAILED` |
| Cargo `could not compile` terminal indication | `CARGO_COMPILATION_FAILED` |
| no reviewed signature | `CARGO_EXIT_NONZERO` |

The matcher retains at most a 128-byte lowercased cross-chunk tail solely
while the pipe is being drained, then zeroes and drops it. No raw Cargo stderr,
environment value, credential, URL, path, or arbitrary compiler text is
returned or serialized. Persisted diagnostics retain only the existing phase,
exit code, fixed classification, SHA-256 of the bounded capture, capture
length, and truncation flag. The schema and fail-closed terminal result are
unchanged.

## Focused regression evidence

Focused tests now prove that:

- a registry/network signature appearing after the 4 KiB retained prefix is
  still classified from the complete drained stream, while a sentinel secret
  and registry text are absent from serialized diagnostics;
- lockfile, linker, and compiler signatures map only to their fixed labels;
- an unrecognized error remains `CARGO_EXIT_NONZERO` rather than receiving an
  inferred cause.

## Verification

| Approved profile | Result |
| --- | --- |
| `CARGO_TEST` (`cargo test --workspace --all-targets --all-features`) | passed after the final source edit |
| `CARGO_BUILD_RELEASE_ISOLATED` | failed twice, including the post-verifier rerun, only while removing the fixed isolated `catdesk.exe`: Windows `Access is denied (os error 5)` |
| `GIT_DIFF` (`git diff --check`) | passed |
| `GIT_STATUS` | 172 inherited/current status entries observed; dirty worktree preserved, with this task limited to the reviewed-build source file and this bundle |

The failed isolated profile is comparison verification only, not a
reviewed-build `PREPARE`, `CONFIRM`, or `RESULT`, and it provides no authority
to modify or replace the locked executable.

## Attribution and prohibited-action audit

Task-attributable changes are limited to `src/reviewed_build.rs` and this
bundle. No scripts, configuration, historical generation record, product
authority, runtime state, wake/target state, Secure MCP/tunnel state, Git
history, signing/elevation state, or external project was changed. No daemon
reload or live reviewed-build action occurred.

## Independent final review request

Request independent final review of the fixed-vocabulary classifier and its
redaction boundary. Separately resolve the isolated-output lock through a
properly authorized host action before treating that build profile as green.
