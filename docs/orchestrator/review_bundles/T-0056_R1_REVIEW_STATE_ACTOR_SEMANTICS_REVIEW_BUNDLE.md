# T-0076 / T-0056-R1 Review-State Actor Semantics — Review Bundle

## Root cause

The initial T-0056 snapshot treated any unread review record for the selected
session as operator attention. It also selected only active sessions, so an
inactive completed session with a pending independent review could disappear
into `IDLE`. Both behaviors misclassified ChatGPT/CatDesk review work as a
human-operator action.

## Repair

`ATTENTION / OPERATOR` is now reserved for durable operator-owned session
states: `WAITING_FOR_USER` and `BLOCKED`. An actionable unread
`independent_final_review` is a review wait, not operator attention.

The reader selects state deterministically:

1. newest active operator-attention session;
2. newest active nonterminal session;
3. newest actionable unread review by creation time and record ID, bound back
   to its exact persisted session;
4. remaining active session, or idle when no work/review exists.

An unresolved actionable review session returns bounded `UNKNOWN`, not an
invented idle or operator state. Unread count remains independent of the
selected review. A selected review produces `WAITING / CHATGPT_WEB` with
`await ChatGPT review`; this is a waiting actor, never proof of
`WORKING / CHATGPT_WEB`.

## Coverage

Focused deterministic tests cover:

- inactive completed session plus unread review → retained session context and
  waiting state;
- `WAITING_FOR_CHATGPT` review → waiting, never operator attention;
- operator input still has highest precedence;
- acknowledged review permits otherwise inactive terminal work to return idle;
- newer active work is not hijacked by an older review;
- unresolvable review mapping fails conservatively;
- existing positive-lifecycle actor precedence and all wake labels remain
  covered.

## Compatibility

Only `src/delegated/autonomy_observability.rs` changed. The patch does not
change review acknowledgement, T-0055 timing/accounting, W13 submit/receipt,
T-0054 wake policy execution, transport, daemon lifecycle, browser behavior,
or MCP controls.

## Local verification

```text
cargo fmt -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test delegated::autonomy_observability::tests -- --nocapture
cargo test
git diff --check
```

Completed locally: formatting and clippy passed; the focused observability
tests passed; the full suite reported 496 passed, 18 ignored, and 0 failed;
and `git diff --check` passed. The emitted CRLF notices concern pre-existing
working-copy conversion settings and did not affect command success.

Independent CatDesk verification and authoritative diff capture remain
required before closure.
