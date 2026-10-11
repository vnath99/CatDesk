# T0478 R3B4-B3B1 — Read-only retirement inventory guard

Date: 2026-10-11. Canonical Chat52: https://chatgpt.com/c/6acba3bf-e740-8331-8896-8a25fefdbf16.

## Precise increment

Added `StagedCandidateSnapshot::review_retirement_inventory` to the existing dormant `src/isolated_fixture_snapshot.rs`. This is a **non-destructive, fail-closed preliminary gate** for a future owner-bound cleanup operation. It has no way to remove a file/directory, follows no untrusted symlink for metadata, and does not grant permissions to MCP or production CatDesk.

- Check that the retained pinned root is an exact canonical OS temporary-directory direct child named `catdesk-dev-fixture-<canonical-UUID-simple>`, then validate the internally owned snapshot manifest.
- Enumerate immediate directory entries under the held root and reject every leaf not exactly in the manifest. Refuse malformed UTF-8 names, repeated names, subdirectories, symlinks or Windows reparse attributes, missing expected entries and unexpected additions.
- Re-read each expected file using the existing pinned no-follow, single-link `read_relative_regular_unique` and compare exact SHA-256 and byte length to its retained manifest. Confirm root identity remains stable.
- Tests operate only in a fresh test-owned OS-temp fixture. They prove success for an expected manifest file, refusal when an extra file and nested directory have been injected, and no release of retained content or root leases as a side effect. Existing tests still cover refusal for pre-existing hardlinks and unapproved temp roots.

## Local verification

`cargo fmt --check` PASS; `cargo test --locked --offline --bin catdesk isolated_fixture_snapshot` 3/3 PASS including new retirement assertions; strict `cargo clippy --locked --offline --workspace --all-targets --all-features -- -D warnings` PASS. Full main Rust suite and independent GitHub CI for this exact follow-up must be verified before acceptance.

## B3B1 follow-up: missing-leaf proof

Added a fourth focused test constructing a **test-only** pinned temp root with a legitimate expected manifest but no staged file; `review_retirement_inventory` returns an error. The test releases the exact guard and removes only its own empty temp directory. Windows local `cargo fmt --check`, strict all-target Clippy, focused snapshot tests **4/4 PASS**, and full main Rust suite **exit 0** after this follow-up. This additional source is not independently accepted until its own GitHub Windows CI is terminal success.

## Explicit security limits

**A read-only inventory check does not authorize filesystem deletion.** A hostile same-user process could replace or inject children after this check. It is never safe to pass a mutable pathname to `remove_dir_all` based only on these results. The actual B3B2 retire implementation must re-open or retain exact Windows DELETE handles under a pinned trusted parent, refuse reparse and foreign objects and compare identity at the point of deletion, using the reviewed native disposal logic already in `src/reviewed_source_snapshot.rs` instead of inventing a parallel unsafe deletion implementation.

The source-only producer is not activated until owner-only directory ACL-at-create and bounded partial-stage/crash cleanup have independent Windows tests/CI. The existing `create_child_with_security_descriptor` and `create_relative_with_sddl` helpers in `src/windows_protected_fs.rs` are currently **test-only** `#[cfg(all(test, windows))]`; they must not be silently used as active runtime ACL proof. A reviewed OS-derived user SID and exact owner ACL policy must be established before promoting that capability. Root ownership cannot be inferred just from a generated name.

R3B4-C still needs to connect a trusted in-process `StagedCandidateSnapshot` to the real isolated MCP read/search router, and R3B4-D needs bounded owner PID/port/stop/restart. T0479 stable versioned worker and production Recovery remain separate. The signed installed T0215 epoch1/pending T0366 epoch2 and externally owned official Secure MCP tunnel are untouched. Seven unrelated old untracked files preserved.
