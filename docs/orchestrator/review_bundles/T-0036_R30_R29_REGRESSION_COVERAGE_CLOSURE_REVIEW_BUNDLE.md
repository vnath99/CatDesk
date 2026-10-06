# T-0036 R30: R29 regression-coverage closure

## Scope and changed paths

R30 changes only deterministic tests in `src/delegated/github_bootstrap.rs`
and this review bundle. No production authority semantics changed. No live
external workspace, GitHub, repository, remote, push, daemon, browser,
tunnel, Scheduler, release, protected-state, or CatDesk Git-publication action
occurred.

## Exact test mapping

- `numeric_repository_identity_requires_positive_id_and_node_id` accepts only
  the real numeric positive `id` plus bounded nonempty `node_id`, and rejects
  zero, negative, float, string, missing id; missing, empty, oversized, and
  non-string node id; and wrong owner/name.
- `expired_journaled_only_recovery_is_rejected_without_mutation` seeds a
  matching durable `JOURNALED` transaction after TTL expiry and proves recovery
  returns before any added runner call, preserves exactly `JOURNALED`, and
  cannot create/add origin/push/mutate content or index.
- `expired_unstarted_recovery_is_rejected_without_mutation` retains the
  no-transaction expiry gate.
- `expired_started_unborn_transaction_recovers_without_a_second_create`
  retains positive `REPOSITORY_CREATED` recovery and exact origin/no-push
  behavior. R27/R28 retain terminal replay, no-duplicate-create, no-origin
  replacement, post-create authority matrix, and committed-head coverage.
- The cached schema and handler retain the exclusive
  `projectId` + `decision=GITHUB_BOOTSTRAP_RECOVER` + `confirmationToken`
  shape; mixed/extra fields are rejected by the existing closed-key tests.

## Security invariants

Only the two fixed private `vnath99` mappings are eligible. Trusted Git/gh
slot/path/SHA revalidation, canonical non-reparse workspace checks, exact
account/scope and REST identity proof, durable journal ordering, and all
no-add/no-commit/no-force/no-delete/no-origin-replacement/no-content-mutation
rules remain intact.

## Verification

`cargo fmt --check` and strict clippy passed. The focused 21-test bootstrap
suite passed. Full `cargo test` passed: 555 passed, 0 failed, 18 ignored.
`git diff --check` passed. No daemon reload or live Bug Bounty recovery was
performed.
