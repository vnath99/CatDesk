# T-0036 R29: Numeric repository identity and durable partial recovery

## Implementation

HTTP 200 repository `EXISTS` evidence now requires the exact policy owner and
repository name, a positive integral numeric `id`, and a nonempty bounded
string `node_id`. String, zero, missing, and malformed identities fail closed.

The cached `autonomy_project_registry_bind` surface adds the closed decision
`GITHUB_BOOTSTRAP_RECOVER`, requiring only `projectId`, `decision`, and the
existing opaque `confirmationToken`. It delegates to the existing bootstrap
state machine. TTL bypass is allowed only for a matching durable transaction
that already contains `REPOSITORY_CREATED`; missing or `JOURNALED`-only state
fails before external mutation.

Recovery revalidates the preflight fingerprint, canonical workspace, trusted
Git/gh slot/path/fingerprint, head state, branch, dirty state, account/scope,
and repository `EXISTS` authority. A journaled repository with no origin stage
may add only the exact policy origin after those checks. Unborn recovery keeps
the zero-push/no-content-mutation terminal transition; committed behavior is
unchanged.

## Deterministic evidence

- Numeric identity test accepts only positive u64 id plus nonempty `node_id`.
- Expired started unborn recovery resumes `REPOSITORY_CREATED` without a second
  `repo create`, adds the exact origin, re-proves authority, and records the
  no-history terminal stage.
- Expired unstarted recovery rejects with no additional runner call.
- Existing R27/R28 actual-confirm, partial recovery/replay, committed-head,
  404, scope, framing, and post-create failure-matrix tests remain covered.

No live external workspace, GitHub, repository, remote, push, daemon, tunnel,
browser, Scheduler, release, protected-state, or Git-publication action
occurred.

## Verification

- `cargo fmt --check` passed.
- `cargo clippy --all-targets --all-features -- -D warnings` passed.
- `cargo test` passed: 554 passed, 0 failed, 18 ignored.
- `git diff --check` passed.

## Host recovery sequence

After isolated reviewed reload and `CONNECTED_VERIFIED`, do **not** run a
second Bug Bounty preflight. Invoke the exact cached recovery request against
the existing durable transaction:

`projectId=BUG_BOUNTY_RECON_PLATFORM`,
`decision=GITHUB_BOOTSTRAP_RECOVER`, and its existing `confirmationToken`.

Proceed only on fresh positive workspace/tool/head/account/scope/numeric
repository-EXISTS evidence and exact origin readback. Any stale token without
the started durable transaction, drift, ambiguity, unexpected origin, or
authority failure remains fail-closed.
