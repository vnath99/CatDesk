# T-0324 reviewed-build snapshot child collision production-repair serving candidate

## Classification

`SERVING_CONTROLLER_CANDIDATE_DEFECT`

The authoritative independent verifier rejected the required fixed isolated
release profile because Windows denied removal of the fixed output. Therefore
no fresh serving-controller candidate is established and separate review must
not treat the prior measurement as candidate authority.

## Accepted source authority inspected

The independently reviewed
`T-0324_REVIEWED_BUILD_SNAPSHOT_CHILD_COLLISION_PRODUCTION_REPAIR_REVIEW_BUNDLE.md`
remains present. Current `src/windows_protected_fs.rs` retains its production
shared path used by `reviewed_source_snapshot`:

- `PinnedDirectory::open_or_create_relative` opens first, creates only after
  exact `STATUS_OBJECT_NAME_NOT_FOUND` (`0xc0000034`), and reopens only after
  exact create-side `STATUS_OBJECT_NAME_COLLISION` (`0xc0000035`);
- every branch remains under the same pinned parent/RootDirectory chain and
  retains component, directory, non-reparse, and stable-identity validation;
- the direct Windows regression
  `descend_or_create_uses_real_open_existing_then_fresh_create_and_refuses_file_or_reparse`
  still exercises real `bytes/scripts` fresh and existing handling plus file
  and reparse refusal.

No product source was edited in this materialization session.

## Fixed isolated build and measurement failure

Only the product-defined fixed profile was attempted. The provider made two
bounded attempts, neither of which can establish a fresh output after the
authoritative independent failure:

```text
cargo build --release --locked --target-dir .catdesk/verification-targets/autonomy-release
```

The provider attempted the fixed profile, but CatDesk's independent verifier
reported this exact failure:

```text
error: failed to remove file
`C:\\Users\\Volap\\OneDrive\\Desktop\\Projects\\CatDesk-codex-loop\\.catdesk/verification-targets/autonomy-release\\release\\catdesk.exe`

Caused by:
  Access is denied. (os error 5)
```

The existing fixed output was measured read-only:

```text
Path: .catdesk/verification-targets/autonomy-release/release/catdesk.exe
SHA-256: 1ffcfc3c3f7ef5f72fba6a2bf5729ba78560abfcc8a2f937f53c6fb60127604f
Length: 26403840 bytes
CreationTimeUtc: 2026-09-12T07:39:49.8214596Z
LastWriteTimeUtc: 2026-09-12T07:39:50.0336831Z
```

The identity is distinct from the prior stale candidate recorded by the
earlier collision-repair serving-candidate bundle:
`21f24c34a766ccc398f4d048c1bfdabaa33c23ac2fca505a79f832441ec0bfdd`
at 26,342,400 bytes. That difference does not establish freshness for this
run: the required profile did not complete successfully, so the
26,403,840-byte measurement is retained only as post-failure observation and
is not a serving candidate.

## Local verification results

- `cargo fmt --all -- --check` — passed.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` —
  passed; only the existing `C:\\Users\\Volap` canonicalization warning
  appeared.
- `cargo test --workspace --all-targets --all-features` — completed locally
  with 936 primary tests plus target-specific binaries and no reported test
  failure.
- Fixed `CARGO_BUILD_RELEASE_ISOLATED` profile — **failed** under independent
  verification with Windows `os error 5`; this is the blocking defect.
- `git diff --check` — passed; inherited CRLF warnings were non-failing.

## Attribution and prohibited-action audit

The workspace was already broadly dirty, with both tracked modifications and
a large untracked set across product source, scripts, documents, and durable
state. They remain inherited. The only intended source-tree mutation
attributable to this session is this review bundle; the fixed verification
target is an isolated build artifact.

No daemon reload; reviewed-build PREPARE, CONFIRM, or RESULT; protected-state
edit; release activation; wake/target/browser action; Secure MCP/tunnel
action; ProgramData/Program Files mutation; signing/UAC; Git publication; or
external-project mutation occurred. The executable was measured only and was
not executed or reloaded as a serving daemon.

Read-only inspection observed a pre-existing `catdesk.exe` process whose path
string named the fixed verification output. It was not stopped, signaled,
reloaded, adopted, or otherwise modified. Its locked-output condition is
consistent with the authoritative build failure; no process identity is
asserted as a replacement candidate.

## Next boundary

An independently authorized host-bound action must first make the fixed
verification output replaceable without broadening this task's authority.
Only then may a separate materialization ticket rerun the exact fixed profile,
obtain a fresh measurement, and request review. Nothing here authorizes a
reload or a reviewed-build retry.
