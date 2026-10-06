# T-0036 R27: Exact unborn confirm/recovery state-machine regression closure

## Scope

This review adds deterministic, local-fake coverage for the production
`GithubBootstrapStore::confirm` state machine. No sibling workspace, GitHub,
repository, remote, push, daemon, browser, tunnel, Scheduler, release, or Git
publication action was used.

## Implementation

The unborn-head terminal path now re-runs the existing bounded
post-mutation authority verifier on every confirm invocation, including a
terminal replay. It records `PUSH_SKIPPED_NO_COMMITTED_HISTORY` only after
the exact policy-origin readback and same selected trusted-gh account/scope
plus exact REST repository `EXISTS` proof succeed. The terminal stage remains
idempotent; an already journaled terminal stage is never treated as authority.

A private test-only policy/tool-candidate seam permits the actual production
preflight/confirm implementation to run against temporary local directories
and fake runners. Production policy, request shapes, and authority decisions
are unchanged.

## Deterministic coverage

- `confirm_unborn_persists_terminal_only_after_fresh_origin_and_exists_proofs`
  drives preflight then `confirm`, checks returned and durable terminal stages,
  and asserts no unborn-path push or content/index mutation calls.
- `confirm_unborn_postcondition_failures_leave_terminal_stage_absent` proves a
  wrong post-add origin cannot record terminal success.
- `confirm_unborn_postcreate_repository_404_leaves_terminal_stage_absent`
  proves an authoritative post-create repository 404 cannot record terminal
  success.
- `confirm_unborn_recovery_revalidates_journal_and_terminal_replay` seeds
  `JOURNALED`, `REPOSITORY_CREATED`, and `ORIGIN_ADDED`; confirm re-proves
  origin and repository existence before terminal persistence, and a later
  replay repeats the authority check while remaining idempotent.
- `confirm_committed_head_still_pushes_the_current_branch` preserves the
  committed-head current-branch push path.

The existing R21-R26 account/scope, exact REST identity, tool fingerprint,
canonical workspace, zero-history, journal, and no-force/no-delete/no-origin
replacement tests remain in the suite.

## Verification

- `cargo fmt --check` passed.
- `cargo clippy --all-targets --all-features -- -D warnings` passed.
- `cargo test` passed: 550 passed, 0 failed, 18 ignored.

## Preserved security invariants

Only the two fixed private `vnath99` mappings remain eligible. Confirmation
still revalidates canonical non-reparse workspace, trusted fixed-slot Git/gh
identity and SHA-256 evidence, unborn/committed state, safe branch, exact
scope-qualified REST authority, and transaction fingerprint. The unborn path
performs no push, staging, commit, checkout, branch creation/rename/init, or
local-content generation. No raw command/API output is returned through the
control surface.

## Host acceptance sequence

After independent review, use an isolated reviewed build/reload and require
`CONNECTED_VERIFIED`. Run exactly one fresh Bug Bounty preflight; confirm only
on fully positive `UNBORN_HEAD`, zero-history, scope, ABSENT, and drift-free
evidence. Independently verify the exact origin before proceeding to the
reviewed R12/R13 registration, thread-adoption, and chat-target initialization
sequence. Any ambiguity remains fail-closed; do not reuse an earlier token or
perform a manual fallback.
