# T-0053 Overnight Ticket Audit Review Bundle

## Architecture and provenance

T-0053 adds the read-only `autonomy_ticket_audit` control-plane operation.
It reports autonomous sessions with durable timing evidence overlapping a
requested Unix-time window. It does not consult ChatGPT history, filenames,
or wall-clock gaps.

The join uses session snapshots and contracts for identity/state, execution
accounting for timing and verification, completion artifacts for verified
change evidence, and review-inbox acknowledgement for independent-review
state. `.catdesk/todo.md` contributes optional descriptive queue metadata
only; it never proves that work ran or creates an audit row.

The response emits fixed completion-artifact references, not raw diffs, final
reviews, provider transcripts, arbitrary files, credentials, browser/profile
data, or tunnel identifiers.

## Status and supersession rules

`SUPERSEDED` is emitted only for a persisted explicit session-to-session
relation in `supersessions.json`. The controlled
`autonomy_session_supersede` operation validates both IDs before writing that
small non-secret relation; task-name and date-based inference do not exist.

- A verified completion with an acknowledged completion review is
  `COMPLETED_REVIEWED`; otherwise it is `COMPLETED_UNREVIEWED`.
- Waiting/user/paused/rate-limited/blocked/credit-exhausted states are
  `WAITING`; failed and lease-expired are `FAILED`; cancelled is `CANCELLED`.
- Draft, queued, running, verifying, and restart recovery are
  `RUNNING_QUEUED`.
- A verified snapshot missing completion evidence is
  `UNKNOWN_INCOMPLETE_EVIDENCE`.

Sessions without bounded accounting or review timing cannot honestly be put in
a historical window. They are omitted and counted in
`incompleteEvidenceCount`, rather than guessed from state or chronology.

## Interface and bounds

`autonomy_ticket_audit` requires `startUnix` and `endUnix`; optional
`projectId`, `afterSessionId`, `limit`, and `maxBytes` provide filtering and
pagination. Zero, inverted, malformed, oversized, or over-31-day requests are
rejected. Output is ordered by first activity then session ID and returns
`nextSessionId` plus `truncated`; a row that cannot fit the byte bound is
rejected rather than silently dropped.

```json
{
  "startUnix": 1723500000,
  "endUnix": 1723586400,
  "records": [{
    "taskId": "T-0053",
    "sessionId": "adc-t0053-example",
    "auditStatus": "COMPLETED_REVIEWED",
    "independentReview": "ACKNOWLEDGED"
  }],
  "truncated": false,
  "incompleteEvidenceCount": 0
}
```

## Changed files and tests

- `src/delegated/autonomy_state.rs`: all-review reader and bounded explicit
  supersession relation storage.
- `src/delegated/autonomy_supervisor.rs`: audit join, status mapping,
  pagination/redaction, relation writer, and deterministic fixtures.
- `src/mcp.rs`: first-class audit and supersession operation discovery.

Focused coverage includes overlapping inclusion, active work, reviewed
completion, explicit supersession, multiple sessions for one ticket,
pagination, invalid windows, fixed references without raw artifact content,
and redaction policy. Existing durable readers fail closed on corrupt optional
artifacts/accounting. The accepted W13 wake contract is unchanged: no wake
bridge, configuration, durable wake state, browser, tunnel, Scheduler, or
dispatcher code was modified.

## Verification and handoff

`cargo fmt -- --check`, `cargo clippy --all-targets --all-features -- -D
warnings`, `cargo test` (478 passed, 18 ignored), and `git diff --check`
completed against the preserved dirty workspace. This worker performed no
browser, wake, tunnel, or provider fallback action. CatDesk independently
captures the authoritative diff and verification result.
