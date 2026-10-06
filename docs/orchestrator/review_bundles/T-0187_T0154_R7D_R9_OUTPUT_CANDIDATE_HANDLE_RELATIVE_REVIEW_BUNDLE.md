# T-0187 — reviewed-build output and candidate handle-relative authority

## Before-state inventory

The T-0186 control records were handle-relative, but the reviewed-build worker
still established data-plane authority through paths.  The concrete residues
were `target.join("release").join("catdesk.exe")`, pathname
`sha256_regular_file` for built and candidate evidence,
`safe_workspace_output_file` for the candidate destination,
`copy_regular_file_create_new` for the executable copy, and
`safe_workspace_regular_file` plus pathname hashing during attestation replay.

## Handle chains

T-0187 reuses the R7C `ProtectedDirectoryGuard` and its Windows
`NtCreateFile` RootDirectory/no-follow child operations.  The worker keeps a
pinned target guard rooted below the isolated control attempt.  Cargo receives
the guard path only as fixed process argv/environment material; after it exits,
the release directory and `catdesk.exe` are opened as exact children of that
guard.  The executable is never reacquired from `target/release` by pathname.

The candidate chain starts from a positively classified workspace root and
creates or opens `target`, `reviewed-builds`, and the exact 32-hex attempt
component one-at-a-time under pinned parents.  `catdesk.exe` is create-new
under that final pinned parent.  Existing, reparse, directory, special, or
ambiguous children fail before authority is granted.

## Evidence and replay

`evidence_from_open_regular` has no path parameter.  It measures metadata,
streams SHA-256, rewinds the same handle, and obtains Windows
volume/file-index identity through `GetFileInformationByHandle` on that same
opened object.  Candidate construction streams the exact built-output handle
to the exact candidate handle, bounds and verifies the count, syncs the
destination, then derives candidate SHA/length/identity from that destination
handle.  The identity is bound into the producer attestation.

Attestation replay pins the approved candidate parent and opens its exact
`catdesk.exe` child through the shared no-follow primitive.  It recomputes
SHA/length/identity from that handle and rejects any mismatch.  No candidate
replay uses `safe_workspace_regular_file`, pathname open, or pathname hashing.

## Negative/race model

Pinned parent guards reject parent drift, symlink/junction/reparse traversal,
and unsafe descendant types before descendant authority I/O.  The child open
and create primitives classify the opened child object rather than trusting a
prior pathname metadata check.  Create-new candidate semantics refuse an
existing name; there is no truncate or overwrite path.  The exact open-handle
copy/evidence sequence also rejects a changed byte count or post-open length.

The static regression
`output_and_candidate_authority_stay_handle_relative` rejects the former
output/candidate helper calls and the direct target/release pathname join while
requiring the relative-open, relative-create, handle-copy, and handle-evidence
boundaries.

## Scope preserved

T-0186 control-state helpers remain the authority for attempt/claim/owner/
result/attestation JSON.  This ticket does not claim promotion-mirror or global
cleanup closure, which remain outside its deliberately narrow data-plane
scope.  One-owner startup, trusted Cargo/Rustc pins, fixed build policy,
CREATE_SUSPENDED Job ownership, R7C snapshots, and promotion consumption were
not weakened.

## Changed files

- `src/reviewed_build.rs`
- `docs/orchestrator/review_bundles/T-0187_T0154_R7D_R9_OUTPUT_CANDIDATE_HANDLE_RELATIVE_REVIEW_BUNDLE.md`

## Verification

Focused checks completed:

- `cargo fmt`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test reviewed_build -- --nocapture` — 11 passed

Full verification completed after this bundle:

- `cargo fmt --check` — passed;
- `cargo clippy --all-targets --all-features -- -D warnings` — passed;
- `cargo test` — 624 passed, 18 ignored, plus two recovery fixtures passed;
- `cargo build --release` — started but exceeded the bounded 120-second
  command window while compiling, without a compiler diagnostic before timeout.
