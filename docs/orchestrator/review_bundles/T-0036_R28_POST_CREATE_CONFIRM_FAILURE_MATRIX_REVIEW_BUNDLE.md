# T-0036 R28: Post-create confirm failure-matrix review

## Scope

R28 adds test-only, deterministic coverage around the existing
`GithubBootstrapStore::confirm` unborn-head transition. It uses temporary
CatDesk-local fixtures and the existing fake runner only. No external
workspace, GitHub, repository, remote, push, daemon, browser, tunnel,
Scheduler, release, protected state, or Git publication action occurred.

## Matrix coverage

`confirm_unborn_postcreate_authority_failure_matrix_is_fail_closed` drives a
positive unborn/zero-history preflight through `JOURNALED`,
`REPOSITORY_CREATED`, and `ORIGIN_ADDED`, then injects the post-create
authority response at the actual confirm boundary. It covers:

- repository HTTP 200 with wrong owner, wrong name, missing id, empty id, and
  invalid non-string opaque id;
- post-create account wrong login, missing `repo` scope, and gh process
  failure;
- repository non-404 process failure, malformed framing, and multiple
  envelope framing; and
- HTTP 401, 403, 429, and 500 repository responses.

Each case asserts that the durable transaction retains the three intended
post-create stages but never records `PUSH_SKIPPED_NO_COMMITTED_HISTORY`.
The shared trace assertion rejects push, add, commit, checkout/switch/init,
branch mutation, and content-generation calls.

R27 fresh success, wrong-origin, authoritative post-create 404, journaled
recovery/replay, and committed-head current-branch push tests remain intact.
No runtime authority or production request/response semantics changed.

## Verification

- `cargo fmt --check` passed.
- `cargo clippy --all-targets --all-features -- -D warnings` passed.
- `cargo test` passed: 551 passed, 0 failed, 18 ignored.
- `git diff --check` passed.

## Preserved invariants

The fixed two-project private `vnath99` policy, canonical non-reparse
workspace checks, fixed-slot trusted Git/gh selection and fingerprint drift
checks, same-gh account/scope proof, scope-qualified REST authority,
journal-before-mutation, exact-origin handling, and committed-current-branch
publication path are unchanged. The unborn path remains zero-push and makes no
content or index mutation. Raw command/API output remains internal.

## Host sequence after independent review

Use an isolated reviewed build/reload and require `CONNECTED_VERIFIED`. Run
exactly one fresh Bug Bounty preflight, then confirm only with fully positive
unborn-head, zero-history, account/scope, ABSENT, and drift-free evidence.
Verify the exact policy origin before continuing to the reviewed registration,
thread-adoption, and project-target initialization sequence. Any ambiguity or
staleness remains fail-closed; do not retry a prior token or use a manual
fallback.
