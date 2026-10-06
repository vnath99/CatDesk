# T-0324 reviewed-build terminal retry-generation repair

## Design before implementation

### Scope and observed defect

The current protected control root holds one terminal
`BUILD_FAILED_OR_AMBIGUOUS` generation:
`7957dfc3ffbf4fc7b6f0eb91bec768e5`. Existing `PREPARE` is create-once at
`attempt.json`; an exact replay returns its already-consumed token, while a
different authority is rejected as immutable. The accepted snapshot-child
collision repair remains outside this change.

### Fixed transition

A retry is permitted only after revalidating the active generation's exact
attempt, claimed owner and owner proof where present, and terminal result:
`BUILD_FAILED_OR_AMBIGUOUS` with no attestation. The request must also
remeasure the same review authority, committed snapshot, trusted tools, and
fixed build policy. Pending, claimed, malformed, missing, drifted, ambiguous,
and `BUILD_ATTESTED` states are non-retryable.

The implementation will retain the legacy flat control generation as immutable
historical evidence. A fresh generation will live beneath a fixed
`generations/<internally-generated-attempt-id>` directory. Before that
generation can become active, CatDesk will create an immutable, handle-relative
history envelope and one immutable retry plan for the old attempt. The plan
binds the old attempt digest and the fresh attempt/token identities. The
active-generation pointer is published only after the fresh attempt and plan
are durable. The new attempt never reuses the prior attempt ID or confirmation
token.

### Crash and concurrency invariants

- Before a retry plan is published, a partially-created generation is inert.
- After a plan is published, every exact concurrent `PREPARE` resolves the
  same planned fresh generation/token; a missing or malformed planned
  generation fails closed.
- The active pointer may advance only from the plan's exact old generation to
  its exact fresh generation. A crash before the pointer leaves no runnable
  second owner; reconciliation can only complete the already-published plan.
- Every historic envelope is create-once and contains the exact prior attempt,
  claim/owner records when present, and terminal result. No raw delete or
  pathname rotation is used.
- `CONFIRM` reads only the active generation, preserves one-owner
  create-once semantics, and rejects old tokens. `RESULT` reports only that
  active generation truthfully.

### Authority and host boundary

All state changes use existing protected RootDirectory/no-follow,
handle-relative create/read/atomic-replace helpers. No caller chooses a path,
generation, token, hash, command, authority field, or approval field. This
does not invoke PREPARE/CONFIRM/RESULT on the live host, reload a daemon, or
mutate wake, release, tunnel, Git, or external-project state.

## Implemented transition and classification

QUEUED_CHECKPOINT_LOCAL_FINALIZATION_REPAIRED is not relevant to this
reviewed-build control boundary. The exact classification for this repair is
`REVIEWED_BUILD_TERMINAL_RETRY_GENERATION_REPAIRED`.

`src/reviewed_build.rs` now resolves a fixed active generation under the
protected `reviewed-build-control` root. Legacy flat state remains readable as
generation zero while no pointer exists. A retry writes, in order:

1. `terminal-history/<old-attempt-id>.json`, a create-once bounded envelope of
   the exact old attempt, claim, worker-owner proof, and failure result;
2. a create-once `generations/<fresh-internal-attempt-id>/attempt.json`;
3. `retry-plans/<old-attempt-id>.json`, which binds the old attempt digest,
   immutable audit digest, fresh attempt digest, and fresh opaque token; and
4. the fixed `active-generation.json` pointer, atomically replaced beneath the
   same pinned parent when a later retry advances an already modern generation.

The pointer is validated by reopening its exact generation and rechecking the
attempt ID and digest. A missing/malformed pointer, missing generation,
malformed plan/history, nonterminal result, missing owner proof, attested
result, tool/snapshot/policy drift, or conflicting plan fails closed. Replayed
exact PREPARE calls converge on the existing plan/token; an old confirmation
token is rejected because CONFIRM resolves only the selected active generation.
No token, attempt ID, path, digest, command, approval, or generation is caller
input.

## Focused regressions

- `terminal_failure_retry_publishes_one_fresh_generation_and_preserves_audit`
  proves a terminal failure creates a distinct token/attempt, preserves the
  exact old envelope, selects only the new generation, and rejects the old
  token before any worker action.
- `terminal_failure_retry_replay_converges_and_malformed_or_successful_history_refuses`
  proves selected fresh state is not retryable and a `BUILD_ATTESTED` shape
  cannot satisfy the terminal-failure predicate.

The implementation also keeps the existing create-once claim/owner semantics:
pending/claimed records cannot produce a retry plan, and only an exact owner
proof plus `BUILD_FAILED_OR_AMBIGUOUS` result may do so.

## Verification and attribution

Executed locally, without a live reviewed-build invocation:

- `cargo fmt --all -- --check` — passed.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` —
  passed.
- `cargo test --workspace --all-targets --all-features` — passed (934 tests).
- `cargo test --workspace --all-targets --all-features terminal_failure_retry -- --nocapture`
  — passed (2 focused tests).
- `git diff --check` — passed; it reported only inherited CRLF warnings.

Task-attributable files are the pre-existing untracked
`src/reviewed_build.rs` implementation and this review bundle. The repository
contains extensive inherited tracked and untracked work; no claim is made that
this task created or cleaned it. The accepted snapshot-child-collision repair
in `src/reviewed_source_snapshot.rs` was inspected but not modified.

## Prohibited-action audit and next boundary

No live PREPARE/CONFIRM/RESULT, daemon reload, release/wake/target mutation,
Secure MCP or tunnel action, Scheduler/service action, signing/UAC, Git
publication, or external-project action occurred. This source change grants no
host execution authority. A fresh independent final review is required before
any separately authorized host-side attempt reconciliation.
