# T-0324 terminal-failure repair serving-candidate R1

## Classification

`SERVING_CONTROLLER_CANDIDATE_DEFECT` — the exact fixed isolated release-build
profile could not replace its designated output, so no fresh serving candidate
exists for independent review.

## Accepted source authority and parity check

The accepted repair completion artifact is
`T-0324_REVIEWED_BUILD_TERMINAL_FAILURE_DIAGNOSTIC_R2.md`, with independently
acknowledged review record
`review-adc-t0324-reviewed-build-terminal-failure-diagnostic-r2-20260913-6-independent_final_review`.
The acknowledged record is `COMPLETED_VERIFIED`.

Current `src/reviewed_build.rs` retains the accepted repeated-parent repair:
`materialize_snapshot` uses the existing protected
`ProtectedDirectoryGuard::descend_or_create` transition when walking each
target-parent component. The focused repeated-`scripts` regression remains
present. No product source or test file was changed in this session.

## Fixed-profile result

The only attempted build command was the product-defined
`CARGO_BUILD_RELEASE_ISOLATED` profile:

```text
cargo build --release --locked --target-dir .catdesk/verification-targets/autonomy-release
```

It reached compilation but failed while replacing the fixed verification output:

```text
error: failed to remove file
`.catdesk/verification-targets/autonomy-release/release/catdesk.exe`

Caused by:
  Access is denied. (os error 5)
```

The candidate was not freshly rebuilt. Consequently, this session deliberately
does not report a SHA-256 or byte length and does not reuse or remeasure any
pre-existing output at that path as fresh evidence.

## Follow-up verifier evidence

CatDesk's subsequent bounded verification reproduced the same fixed-profile
failure at the same `release/catdesk.exe` removal step. A read-only attempt to
identify an owning process through the OS process inventory was itself denied
by Windows access control. That observation neither identifies an owner nor
authorizes stopping, reloading, or otherwise altering any process. The durable
classification therefore remains an output-lock candidate defect.

A further direct retry of the same exact locked isolated profile also reached
compilation and then reproduced the identical `failed to remove file` / `Access
is denied. (os error 5)` result for the same fixed output. No output was
measured or reused after that failed retry.

## Verification boundary

Per the approved fail-closed instruction for an unsuccessful isolated build,
the remaining fmt, Clippy, test, and final diff profiles were not run in this
attempt. The output-lock failure is the first blocking condition; source was
not altered to work around it.

## Attribution and prohibited-action audit

The only task-attributable source-tree mutation is this review bundle. Isolated
build artifacts are not source authority. The workspace remains an inherited
dirty worktree; no reset, staging, commit, or unrelated edit occurred.

No daemon reload, live reviewed-build PREPARE/CONFIRM/RESULT, wake or target
change, protected runtime/release mutation, tunnel or Secure MCP action,
signing/elevation, Git publication, or external-project action occurred.

## Next safe step

A separately authorized host-lock resolution is required before another use of
the exact fixed `CARGO_BUILD_RELEASE_ISOLATED` profile. Only a successful fresh
replacement followed by exact remeasurement may produce a
`SERVING_CONTROLLER_CANDIDATE_READY` record.
