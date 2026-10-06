# T-0324 snapshot child collision repair serving-controller candidate

## Classification

`SERVING_CONTROLLER_CANDIDATE_DEFECT`

A fresh isolated serving-controller candidate containing the accepted snapshot
child collision repair was not materialized in this run. Separate ChatGPT
review is required before any future guarded action.

## Accepted repair inspected

Current `src/reviewed_source_snapshot.rs` contains the accepted
handle-relative repair: `pin_create_relative_directories` calls
`ProtectedDirectoryGuard::descend_or_create` for every fixed relative
component. The shared protected filesystem uses the already-pinned parent,
opens-or-creates exactly one component, and positively validates that the
opened object is a stable non-reparse directory before it can become the
parent for the next component.

The hostile coverage remains present:

- `pinned_snapshot_directories_create_and_reopen_existing_bytes_scripts`
  covers fresh and repeated `bytes/scripts` creation.
- `pinned_snapshot_directories_refuse_existing_file_collision` refuses a
  wrong-type existing child.
- `pinned_child_parent_rejects_link_redirection_before_content_write_when_supported`
  retains reparse/redirection refusal.

No product source was changed by this candidate-materialization session.

## Fixed isolated build and measurement

The only release build profile invoked was:

```text
cargo build --release --locked --target-dir .catdesk/verification-targets/autonomy-release
```

The fixed profile failed before materializing a fresh candidate. Its bounded
verifier diagnostic is exact:

```text
error: failed to remove file
`C:\\Users\\Volap\\OneDrive\\Desktop\\Projects\\CatDesk-codex-loop\\.catdesk/verification-targets/autonomy-release\\release\\catdesk.exe`

Caused by:
  Access is denied. (os error 5)
```

The read-only post-failure measurement remained:

```text
.catdesk/verification-targets/autonomy-release/release/catdesk.exe
SHA-256: 21f24c34a766ccc398f4d048c1bfdabaa33c23ac2fca505a79f832441ec0bfdd
Length: 26342400 bytes
LastWriteTimeUtc: 2026-09-12T02:14:41.9172503Z
```

That was the same identity and timestamp observed before this run's fixed
build. It is therefore not fresh evidence for the snapshot-child-collision
repair and is not accepted as this ticket's candidate.

## Verification outcomes

- `cargo fmt --all -- --check` - PASS.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` -
  PASS; only the existing `<USER_PROFILE>
- `cargo test --workspace --all-targets --all-features` - PASS: 932 primary
  tests plus target-specific binaries.
- `git diff --check` - PASS after this bundle addition; only inherited CRLF
  conversion warnings appeared.
- Fixed isolated release profile - FAIL: Windows denied removal of the fixed
  output with `os error 5`; candidate replacement/freshness is not
  established. This is the blocking defect classification.

## Attribution and prohibited-action audit

The worktree was already broadly dirty: tracked modifications and a large
pre-existing untracked set cover product source, scripts, durable state, and
documentation. They are not attributed to this task. The only intended
source-tree mutation attributable here is this review bundle; the fixed
verification target is an isolated build artifact, not release or host state.

No daemon reload, reviewed-build PREPARE/CONFIRM/RESULT, wake or target change,
browser action, protected host/release mutation, ProgramData/Program Files
write, Secure MCP/tunnel action, signing/UAC, Git publication, or
external-project mutation occurred.

## Exact next bounded action

Independently review the locked fixed-output condition. A separate authorized
host-bound action must first make that exact fixed output replaceable; only
then may a separate ticket rerun the same fixed isolated profile and require a
new executable measurement before any candidate-ready classification. No
reload or reviewed-build action is implied by this ticket.
