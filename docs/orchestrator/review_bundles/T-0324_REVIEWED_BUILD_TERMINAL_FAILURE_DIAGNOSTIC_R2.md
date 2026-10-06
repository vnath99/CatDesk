# T-0324 reviewed-build terminal-failure diagnostic R2

## Classification

`REVIEWED_BUILD_TERMINAL_FAILURE_REPAIRED`, pending fresh independent final review.

## Scope and authority preservation

This is the sole predeclared completion artifact for the bounded diagnostic
session. The accepted reviewed-source authority finalization remains the only
reviewed-source authority. No review record, snapshot, confirmation token,
attempt identifier, result, or protected reviewed-build control state was
changed by this work.

The two fresh terminal generations `c82a43d9626e4c61a47d64d5d6eff4b3` and
`323f13fac0fc447cab1420a3221528ef` were inspected read-only. Each retained a
valid reserved owner/claim and reached `BUILD_FAILED_OR_AMBIGUOUS` before a
Cargo build or build attestation. Both durable results report exactly
`protected filesystem child scripts is unavailable (0xc0000035)`. Historical
confirmation tokens were neither disclosed here nor reused.

## Root cause and minimal repair

`materialize_snapshot` walks immutable source entries one at a time. For two
entries such as `scripts/first.ps1` and `scripts/second.ps1`, its prior target
walk called `create_child("scripts")` for each entry. The second call therefore
received Windows `STATUS_OBJECT_NAME_COLLISION` (`0xc0000035`) even though the
same pinned target parent already contained the intended child directory.

The target-parent step in `src/reviewed_build.rs` now calls the existing shared
`ProtectedDirectoryGuard::descend_or_create` helper. That helper performs only
the protected sequence: open the exact existing child; create only after exact
not-found; reopen only after exact create-collision. Every successful branch
stays beneath the pinned `RootDirectory`/parent-handle chain and positively
checks directory type, no-reparse status, and stable identity. Files,
reparse/substitution, access failures, ambiguous statuses, and all unrelated
errors still fail closed. The change does not affect review authority,
snapshot-entry measurement, Cargo spawning, attestation, result truthfulness,
or single-use tokens.

## Regression evidence

`materialize_snapshot_reopens_repeated_scripts_parent` invokes the real
`materialize_snapshot` production path with two separately measured immutable
entries under `scripts/`. It verifies both exact files are materialized, thus
covering the repeated-parent shape observed in the durable failures. Existing
shared protected-filesystem tests continue to cover ordinary existing and fresh
children plus file/reparse refusal and exact NT-status discrimination.

## Attributable changes

- `src/reviewed_build.rs`: use the shared pinned no-follow
  `descend_or_create` transition while materializing each target parent; add
  the focused repeated-`scripts` production-path regression.
- `docs/orchestrator/review_bundles/T-0324_REVIEWED_BUILD_TERMINAL_FAILURE_DIAGNOSTIC_R2.md`:
  this completion artifact.

The workspace is an existing large dirty/untracked worktree. No reset, stage,
commit, or modification of unrelated changes was performed. `git diff --check`
completed successfully; its CRLF notices are Git working-copy warnings, not
whitespace errors.

## Local verification

- `cargo test materialize_snapshot_reopens_repeated_scripts_parent -- --nocapture` — passed.
- `cargo fmt --all -- --check` — passed.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` — passed.
- `cargo test --workspace --all-targets --all-features` — passed: main crate
  `919 passed; 0 failed; 21 ignored`, with all binary and integration targets
  also passing.
- `git diff --check` — passed.

## Prohibited-action audit and next boundary

No daemon reload; live reviewed-build PREPARE/CONFIRM/RESULT; protected
release/wake/target mutation; browser action; Secure MCP/tunnel action; signing
or elevation; Git publication; or external-project action occurred.

The next action, only after independent final review, is a separately
authorized fresh reviewed-build attempt under the existing authority. It must
mint and use a new opaque token through the normal PREPARE/CONFIRM/RESULT flow;
this diagnostic does not authorize or perform it.
