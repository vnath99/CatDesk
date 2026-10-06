# T-0150 - GitHub authority and failure-handling convergence

## Scope and baseline

This source/test-only slice examined the existing local `git_workflow` MCP
helpers, autonomous-contract Git policy, and the separately fixed-policy
external-project `github_bootstrap` transaction.  None was treated as an
implicit CatDesk publication authority:

| Existing surface | Role after T-0150 | Publication authority |
| --- | --- | --- |
| `git_workflow` / MCP status, branch, commit helpers | Existing local-worktree tooling | No push, pull request, or merge operation exists there. |
| `AutonomousGitPolicyV1` | Immutable contract policy input | Required but insufficient on its own. |
| `github_bootstrap` | Existing, fixed two-external-project bootstrap path | Not generalized or invoked by this ticket. |
| `github_publication` (new) | Contract-bound, non-executing pre-dispatch gate | It has no process, network, credential, Git, or GitHub-client authority. |

`CANONICAL_CURRENT_ARCHITECTURE.md` and the active plan continue to prohibit
publication for current reliability work.  No branch was changed and no
remote GitHub operation was attempted.

## Change

Added `src/delegated/github_publication.rs` and made its journal a private,
durable part of `AutonomousSessionSnapshotV1`.

Before a future separately approved executor can dispatch a feature-branch
push, pull-request creation, or approved pull-request merge, the new gate
requires all of the following:

| Boundary | Fail-closed check |
| --- | --- |
| Repository | Observed origin must exactly equal the approved contract origin. |
| Branch | Observed branch must exactly equal the contract feature branch and may not equal the base branch. |
| Head | Observed head is a bounded hexadecimal commit identity. |
| Contract | The exact operation must be enabled by the immutable autonomous Git policy. Force-push/direct protected-branch mutation are not representable. |
| Approval | The approval must be current, unconsumed, contract-hash-bound, and of type `GitPush` for push/PR or `Merge` for merge. |
| Idempotency | An approval idempotency key binds one stable intent hash; a different intent using that key is refused. |

The journal is atomically persisted by the existing session-state save path.
Its allowed states are `PREPARED`, `REMOTE_OUTCOME_UNKNOWN`, `CONFIRMED`, and
`RECONCILED_NOT_APPLIED`.

| Condition | Result |
| --- | --- |
| First fully valid request | Records `PREPARED`; returns opaque permit only. |
| Before remote dispatch | Executor must persist `REMOTE_OUTCOME_UNKNOWN`. |
| Restart while prepared/unknown | `GITHUB_PUBLICATION_RECOVERY_REQUIRED`; no blind replay. |
| Trusted reconciliation proves no effect | `RECONCILED_NOT_APPLIED`; exactly one retry can be authorized. |
| Trusted success | `CONFIRMED`; same exact replay is `GITHUB_PUBLICATION_ALREADY_COMPLETED`. |
| Different intent for same key / foreign transition / corrupt record | Fails closed. |

This is intentionally a gate, not a new publication route.  A future remote
executor must explicitly consume this durable gate under its own approved
ticket; current no-publication contracts remain denied before any dispatch.

## Regression evidence

`github_publication` tests cover:

- exact contract-origin, feature-branch, head, and typed-approval binding;
- disabled policy, protected-base branch, malformed head, and wrong approval
  category refusal;
- serialized restart recovery with no automatic replay; trusted
  `not-applied` reconciliation; one retry; and terminal idempotent success;
- conflicting same-key intent and invalid state-transition refusal;
- session snapshot persistence plus corrupted-journal save refusal; and
- a scoped source guard proving the new module contains no generic process,
  Git push, GitHub CLI/HTTP, or socket executor authority.

## Verification

| Command | Result |
| --- | --- |
| `cargo test github_publication` | Passed: 6 focused tests. |
| `cargo fmt --all -- --check` | Passed. |
| `cargo clippy --all-targets --all-features -- -D warnings` | Passed. |
| `cargo test --all-targets --all-features --no-fail-fast` | Passed: 851 tests (the pre-existing platform-gated ignored tests remain ignored). |
| `cargo build --all-targets --all-features` | Passed. |
| `git diff --check` | Passed. |

Cargo emitted the pre-existing environmental warning that it could not
canonicalize `<USER_PROFILE>`; it did not affect compilation or tests.

## Attributable files

- `src/delegated/github_publication.rs`
- `src/delegated/autonomy_state.rs`
- `src/delegated/autonomy_observability.rs`
- `src/delegated/mod.rs` (one new private module declaration; other existing
  dirty lines are not attributed)
- this review bundle

## Prohibited actions

No push, pull-request creation, merge, remote probe, credential access,
branch change, force-push, `main` modification, Git publication, host
mutation, browser/wake action, Secure-MCP/tunnel change, signing/provenance
work, or Git publication occurred.

## Result

`READY_FOR_INDEPENDENT_REVIEW`

Independent review is required before any future executor is allowed to use
the gate.  This ticket does not authorize publication.
