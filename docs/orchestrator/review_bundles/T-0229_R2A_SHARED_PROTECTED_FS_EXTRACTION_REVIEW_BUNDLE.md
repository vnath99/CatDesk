# T-0229 R2A / T-0232-R2A-R1: shared protected-filesystem extraction

## Scope and attribution

This bounded slice changes only the reviewed-source protected-filesystem
boundary and its crate registration.  The session-attributable artifacts are:

| Path | Change |
| --- | --- |
| `src/windows_protected_fs.rs` | New shared handle-bound child-I/O boundary. |
| `src/reviewed_source_snapshot.rs` | Existing bounded child I/O now enters the shared boundary; the reviewed NT implementation remains the sole implementation behind it. |
| `src/main.rs` | Registers `mod windows_protected_fs;` at the approved crate registration point. |
| `docs/orchestrator/review_bundles/T-0229_R2A_SHARED_PROTECTED_FS_EXTRACTION_REVIEW_BUNDLE.md` | This R2A evidence bundle. |

The repository is intentionally dirty from unrelated earlier work.  No
repository-wide `git diff` is used as this ticket's attribution boundary.

## Old-to-new primitive and callsite map

| Existing reviewed primitive/callsite | Shared route |
| --- | --- |
| `write_new_regular_in` | `windows_protected_fs::write_new_regular_in` -> existing handle-bound implementation. |
| `read_relative_regular` | `windows_protected_fs::read_relative_regular` -> existing handle-bound implementation. |
| `create_relative_regular_file` | `windows_protected_fs::create_relative_regular_file` -> existing relative `NtCreateFile` implementation. |
| `open_relative_regular_file` | `windows_protected_fs::open_relative_regular_file` -> existing relative `NtCreateFile` implementation. |
| `ProtectedDirectoryGuard` root descent, child classification, identity checks, relative staging rename, enumeration, and contained disposition | Remain the reviewed implementation in this narrow extraction; the new shared child boundary accepts only an already-pinned guard and never a parent path. |

The shared boundary deliberately does not introduce `fs::read`, `fs::write`,
`OpenOptions`, `File::open`, or a metadata-then-path fallback.  Its parent is
an existing `ProtectedDirectoryGuard`; the guard's pinned NT directory handle,
`FILE_OPEN_REPARSE_POINT`, child component validation, and identity
revalidation remain the authority for Windows operations.  Non-Windows keeps
the existing deterministic fail-closed compilation/test behavior.

## Invariant-preservation matrix

| Invariant | Preserved mechanism |
| --- | --- |
| No-follow / reparse rejection | Existing relative NT opens use `FILE_OPEN_REPARSE_POINT` and reject reparse attributes. |
| Parent and child identity | Existing `ProtectedDirectoryGuard::assert_stable` runs before/after bounded I/O. |
| Bounded regular-file I/O | Existing length bound, exact-read check, flush, and post-write length check. |
| Relative authority | The shared functions receive a pinned guard, not a caller pathname. |
| Snapshot schema, source coverage, replay and atomic commit | Unchanged snapshot producer/consumer and staging commit machinery. |
| Windows failure mode | Handle/no-follow identity failure remains an error; no weaker path fallback was added. |

## Adversarial coverage

The focused `reviewed_source_snapshot` suite exercised the existing
production route through the shared façade, including intermediate link
redirection with outside sentinel, pinned-child link redirection, final-child
reparse replacement, child and nested-directory replacement races, stale
staging enumeration/open replacement, committed-handle replacement,
handle-bound cleanup, exact replay, and create-new non-overwrite behavior.
The added `windows_protected_fs` regression proves the shared child operations
forward only to the reviewed handle-bound implementations and retain the
pinned-guard-only public shape.

## Verification evidence

- `cargo fmt --check` — passed.
- `cargo test "reviewed_source_snapshot::tests" --no-fail-fast` — passed: 20 tests.
- `cargo test windows_protected_fs --no-fail-fast` — passed: 1 test.
- `cargo clippy --all-targets --all-features -- -D warnings` — passed.
- `cargo test` (`rust_full` test component) — source test harness: 714 passed,
  21 ignored; the separate `recovery_powershell` integration fixture failed in
  `scripts/test-promote-reviewed-catdesk-build.ps1` at its pre-existing
  assertion that a future native reload receipt fails before canonical
  mutation. This R2A slice does not modify that promotion/recovery path, so it
  was not repaired here.
- `git diff --check` — passed after the full-run failure.

## Limitations and remaining work

This is R2A only.  It neither migrates stable-supervisor state/install
authority nor implements the named-pipe ACL and OS peer-attestation closure.
Those remain explicit R2B/named-pipe work.  No ProgramData, service,
Scheduled Task, daemon, release, port 3201, Secure MCP/tunnel, browser,
provenance, or Git mutation occurred in this provider turn.

Independent final review must evaluate the bounded session-attributable files
above rather than unrelated pre-existing worktree changes.
