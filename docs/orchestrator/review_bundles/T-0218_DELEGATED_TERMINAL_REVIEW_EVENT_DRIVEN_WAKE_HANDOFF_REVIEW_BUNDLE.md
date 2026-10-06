# T-0218 Delegated Terminal Review Event-Driven Wake Handoff Review Bundle

## Scope and conclusion

This source/test slice makes a completed delegated run hand off to the existing
autonomous review inbox exactly once. It does not create a conversation, change
a project target, invoke a browser in tests, or replace the hourly deadman.
The event-driven handoff is the primary durable signal; the existing bounded
reviewer/deadman behavior remains fallback only.

`COMPLETED_VERIFIED` is the delegated terminal state eligible for this record:
it has both the authoritative diff and final-review package required to bind
the record. Failed, cancelled, and uncertain runs do not fabricate a
final-review/diff identity; their existing durable journal/supervisor paths
remain fail-closed.

## Shared primitive and wiring

| Layer | Responsibility |
| --- | --- |
| `AutonomousStateStoreV1::emit_delegated_review_inbox_record` | Atomically writes one bounded `review-inbox.json` record. Its deterministic record ID is derived from the delegated run ID and replay with different durable evidence is rejected. It does not create an autonomous session. |
| `autonomy_runtime::emit_delegated_review_inbox` | Resolves exactly one registered project for the canonical workspace, validates its existing durable target, and binds run ID, final-review SHA-256, diff identity, and target SHA-256 into the immutable reference. It never writes project registration or target state. |
| `mcp::persist_delegated_terminal_review_handoff` | Called after terminal final-review/diff persistence. A failure remains visible as pending durable replay rather than pretending the review was delivered. |
| `mcp::persist_rehydrated_delegated_terminal_review_handoff` | Reconstructs only the durable final-review/diff from a completed journal after restart, then reuses the same idempotent primitive. No caller supplies review, diff, project, or target data. |
| `rehydrate_persisted_waiting_wakes` | Now separately discovers only structurally valid unread delegated final-review records whose target digest still equals the existing registered target. It schedules the existing delegated dispatcher; autonomous-session candidates retain their original path. |

The delegated reference has exactly four fields:

```text
delegated-run=<run-id>;
final-review-sha256=<64 lowercase/uppercase hex>;
diff=<bounded ASCII diff identity>;
target-sha256=<64 hex>
```

Malformed, extra, ambiguous, target-mismatched, already-sent, or unsafe
records are not rehydrated. A later target change cannot replay an old
delegated record into another conversation.

## Restart and idempotency matrix

| Situation | Result |
| --- | --- |
| Normal completed terminal | Persist one project-scoped unread `COMPLETED_VERIFIED` / `independent_final_review` record, then request the existing bounded dispatcher. |
| Replay with identical run/project/final-review/diff/target identity | Returns the existing record; no second record is appended. |
| Replay with conflicting identity | Fails closed; the durable record is not rebound. |
| Restart after terminal journal persistence before inbox write | Reconstruct final-review/diff from the completed journal and retries the same writer. |
| Restart after inbox write before dispatch | Startup scans the exact delegated record and schedules the existing dispatch path once. |
| Missing/ambiguous project or invalid durable target | No inbox record and no dispatch. |
| Autonomous-session final-review record | Not classified as delegated; existing autonomous behavior is unchanged. |
| Sent/current receipt | Not redispatched. |

## Deterministic coverage

- `delegated_terminal_review_handoff_is_project_bound_and_restart_idempotent`
  proves project/run/final-review/diff/target binding, reopen/replay single
  record behavior, and target immutability.
- `delegated_terminal_review_restart_candidate_is_exact_and_does_not_adopt_autonomous_review`
  proves restart selection is delegated-only and does not change autonomous
  review semantics.
- `delegated_review_reference_rejects_ambiguous_or_malformed_identity` rejects
  malformed, extra-field, and control-character identities.
- Existing state-store tests retain deterministic idempotent replay and
  conflicting-project refusal. Existing autonomous restart/replay tests retain
  session-specific receipt and target-rebind behavior.

Tests inspect durable state and candidate selection only. They create no wake
runtime/configuration and do not invoke Python, Selenium, CDP, or a browser.

## Verification

| Command | Result |
| --- | --- |
| `cargo test delegated_terminal_review_handoff --all-features -- --nocapture` | PASS (2 focused handoff/replay tests) |
| `cargo test delegated_review --all-features -- --nocapture` | PASS (state replay/conflict and reference-parser coverage) |
| `cargo fmt --all -- --check` | PASS |
| `cargo clippy --all-targets --all-features -- -D warnings` | PASS |
| `cargo test --all-targets --all-features --no-fail-fast` | PASS (843 tests; existing platform-specific ignored tests remain ignored) |
| `cargo build --all-targets --all-features` | PASS |
| repository `rust_full` wrapper | NOT CONFIGURED (no repository wrapper was found) |
| `git diff --check` | PASS |

The commands reported the pre-existing environment warning that
`<USER_PROFILE>` could not be canonicalized; no test or build failed because
of it.

## Attribution and prohibited mutations

Task-relevant source surfaces are `src/delegated/autonomy_runtime.rs`,
`src/delegated/autonomy_state.rs`, and `src/mcp.rs`, plus this bundle. The
repository was broadly dirty at task start, including those source files, so
the authoritative working-tree diff must be reviewed with that baseline in
mind; unrelated changes were neither reset nor absorbed.

No live browser wake, ChatGPT target selection/change, Secure MCP/tunnel
operation, external-project access, signing/provenance/dedicated-producer work,
credential access, Git branch/publication, or host lifecycle mutation occurred.

## Residual acceptance boundary

A natural production review event and operator-visible delivery remain a
separate live canary/independent-review boundary. This ticket deliberately
does not manufacture browser evidence. Request independent final review before
treating T-0218 as accepted.
