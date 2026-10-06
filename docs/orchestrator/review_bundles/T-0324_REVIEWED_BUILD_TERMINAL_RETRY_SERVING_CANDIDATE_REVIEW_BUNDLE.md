# T-0324 reviewed-build terminal retry serving candidate

SERVING_CONTROLLER_CANDIDATE_DEFECT

## Accepted source authority

The current `src/reviewed_build.rs` contains the independently accepted
`REVIEWED_BUILD_TERMINAL_RETRY_GENERATION_REPAIRED` boundary recorded in
`T-0324_REVIEWED_BUILD_TERMINAL_RETRY_GENERATION_REPAIR_REVIEW_BUNDLE.md`:
`prepare_terminal_retry`, `terminal_failure_audit`, and
`resolve_active_control_root` preserve terminal evidence before publishing a
fresh active generation. This session did not alter that source.

## Fixed-profile results

- `cargo fmt --all -- --check` completed successfully.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
  completed successfully.
- `cargo test --workspace --all-targets --all-features` completed successfully
  (934 tests reported).
- The authoritative independent rerun of `cargo build --release --locked
  --target-dir .catdesk/verification-targets/autonomy-release` failed while
  attempting to remove the fixed `release/catdesk.exe` with Windows `Access is
  denied. (os error 5)`. This is the exact closed
  `CARGO_BUILD_RELEASE_ISOLATED` profile and its nonzero result overrides the
  earlier local inference from a readable output file.
- `git diff --check` completed successfully (with inherited CRLF warnings).

The existing fixed-path file remains observable but is explicitly **not** a
fresh serving candidate because the authoritative profile could not replace it:

- path: `.catdesk/verification-targets/autonomy-release/release/catdesk.exe`
- SHA-256: `e11eb8e56fdaba5e4482dea609ecd740ea85f462ad3c84fd790607efe692ff63`
- byte length: `26340864`
- last-write UTC observed during readback: `2026-09-12T04:47:50Z`

The measurement above is retained solely to identify the rejected stale/locked
file. It does not establish fresh-build identity and was not executed or
reloaded.

## Attribution and prohibited-action audit

The only intended source-tree mutation for this session is this review bundle.
`src/reviewed_build.rs` and the wide dirty worktree are inherited; neither was
cleaned, reset, or edited. The isolated target directory is verification-only
and is not release authority.

No daemon reload, candidate execution, live PREPARE/CONFIRM/RESULT,
release/wake/target mutation, Secure MCP or tunnel action, Scheduler/service
action, Git publication, signing/UAC, or external-project mutation occurred.

## Next action

No serving-controller candidate is available from this run. The exact next
boundary is separate operator/independent-review disposition of the locked
fixed verification output; this provider must not delete, replace, unlock, or
reload it. No daemon reload is authorized.
