# T-0407 R5 GitHub Publication Executor

## Classification

`GITHUB_PUBLICATION_EXECUTOR_NOT_READY`.

## Exact missing seam

`src/delegated/github_publication.rs` owns a typed approval and durable
idempotency journal but deliberately has no process, network, or MCP authority.
`src/git_workflow.rs` can perform a locally confirmed generic staged commit,
but it accepts caller-supplied files/message, does not consume a
`GithubPublicationPermitV1`, does not persist `REMOTE_OUTCOME_UNKNOWN` before
a push, and has no exact remote-ref reconciliation. No supervisor or MCP
catalog route connects those two components. The existing publication-gate
test explicitly proves that no executor exists.

## Bounded change made

`src/delegated/github_publication.rs` now contains a pure frozen-manifest
validator for the exact reviewed T-0407 R3 snapshot. It accepts only the
schema-v2 manifest digest
`bd5ea6b42ddf628203ad0d4312cb626edf784993b574fd6be286466770b0dfb7`, 934
literal `INCLUDE_RECONSTRUCTION` paths, 12 ZIP archival exclusions, and the
one generated approval-request exclusion. It rejects path escapes, generic
staging shapes, classification drift, and digest drift. Two deterministic
tests cover the exact snapshot and hostile paths.

This is deliberately not an executor: it introduces no Git command, no
network client, no credential handling, no commit, no push, and no MCP route.
The pre-existing no-executor proof remains green.

## Required follow-on implementation

A separately reviewed implementation must add a closed supervisor-owned
PREPARE/CONFIRM/RESULT operation that:

1. loads exact independently reviewed authority and `PASSED` verification;
2. consumes `GithubPublicationJournalV1::authorize` with a Git-push approval;
3. obtains bounded fixed-origin/feature-branch/HEAD/staging evidence;
4. uses the frozen literal parser before any Git mutation;
5. persists the journal transition to `REMOTE_OUTCOME_UNKNOWN` immediately
   before a fixed `git push origin <feature-branch>` invocation without force;
6. reconciles `refs/remotes/origin/<feature-branch>` against local HEAD before
   `Confirmed`, and exposes only bounded/redacted RESULT states.

It must not reuse generic `git_workflow` caller-selected stage/message inputs
as a publication authority.

## Verification

| Gate | Result |
| --- | --- |
| publication-gate focused tests | 8 passed |
| `cargo fmt --all -- --check` | passed before the full profile |
| strict Clippy | passed before the full profile |
| `git diff --check` | passed |

The full workspace/all-target/all-feature test command was launched after the
source change, but this worker must not claim completion because long-running
test processes remained active at the time this record was written. No real
commit or push was executed.
