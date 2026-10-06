# T-0247 / T-0224-R1A-R5 — Canonical Review-Inbox Unification

## Independent review request

Independent final review is requested. Controller terminal state is advisory;
review the source authority, tests, and bounded verification evidence.

## T-0246 rejection and authority correction

T-0246 made a real read-only status call path but was rejected because it
invented `<workspace>/.catdesk/stable-wake/review-events`, which no canonical
producer writes, and it rejected real nested references such as
`artifacts/completion.json`.

The sole stable-wake production event authority is now the existing producer
and bridge authority:

`<workspace>/.catdesk/autonomy/review-inbox.json`

`src/stable_wake_bootstrap.rs` no longer reads or creates the duplicate spool.
It only reads the canonical JSON array. The existing Python bridge remains
unchanged architecture evidence for the same canonical path.

## Attributable files

- `src/stable_wake_bootstrap.rs` — canonical inbox reader, fixed path
  containment, producer-compatible schema validation, actionability and tests.
- `src/delegated/autonomy_state.rs` — exposes the already-used review-record
  validator as a crate-local, side-effect-free shared wire-contract validator.
- `src/server.rs` — transport-status integration fixture now writes the
  canonical inbox and asserts canonical stable-wake readiness/counts.
- `docs/orchestrator/review_bundles/T-0247_T0224_R1A_R5_CANONICAL_REVIEW_INBOX_UNIFICATION_REVIEW_BUNDLE.md` — this artifact.

The workspace was broadly dirty before this slice. `src/state.rs` retains the
already-attributable T-0246 production caller; it was not changed for T-0247.
No broad worktree diff is presented as task attribution.

## Wire contract, bounds, and containment

The reader deserializes the exact producer type
`AutonomousReviewInboxRecordV1`, retaining camelCase fields:

`schemaVersion`, `recordId`, `projectId`, `sessionId`, `state`, `nextAction`,
`reference`, `createdAtUnix`, and `unread`.

- The shared producer validator enforces schema version, conservative
  record/project/session/next-action identities, reference secret-marker
  rejection, and its 1024-byte reference maximum.
- The stable reader adds workspace-relative path authority checks: `reference`
  is non-empty, at most 1024 bytes, contains no NUL, is not rooted/prefixed,
  and consists only of normal components. Safe nested values such as
  `artifacts/completion.json` are accepted.
- The inbox is a bounded JSON array of at most 512 records. The reader also
  bounds raw inbox bytes at 2 MiB before parsing.
- The workspace, `.catdesk`, and `autonomy` components are individually
  opened via `symlink_metadata` then canonicalized; the final inbox must be a
  non-symlink regular file whose canonical location remains under `autonomy`.
- Discovery is strictly read-only: no acknowledge, claim, rewrite, receipt,
  deletion, rename, target change, or browser submission occurs.

## Actionability and duplicates

Unread records are pending for exactly the two canonical bridge forms:

1. `COMPLETED_VERIFIED` + `independent_final_review`
2. `WAITING_FOR_CHATGPT` + `chatgpt_decision_required`

All other valid records are stale/non-actionable. This matches both
`latest_actionable_review_for_session` and the existing bridge predicate.
Exact semantic duplicate `recordId`s collapse in deterministic `BTreeMap`
order; a shared ID with any field difference fails closed.

## Production caller

The concrete non-test path remains:

`server::post_mcp` → `AppState::transport_status_payload` →
`stable_wake_bootstrap::workspace_readiness` → `discover`.

The status payload exposes only `discoveryAvailable`, `pendingCount`, and
`staleCount`. It does not require transport, daemon, MCP, tunnel, release,
manifest, promotion, LKG, or browser health to inspect the canonical inbox.

## Focused matrix and results

- Producer serialization of the exact durable type: passed.
- Nested canonical reference and byte-for-byte read-only preservation: passed.
- Completed final-review pending, stale, and waiting-for-decision semantics:
  passed.
- Exact duplicate collapse and conflicting duplicate fail-closed: passed.
- Wrong project, unsupported schema, malformed inbox: passed.
- Missing inbox, 512-record limit overflow, oversized inbox, invalid identity,
  empty/absolute/rooted/prefixed/traversal/oversized reference: passed.
- Final-link refusal is covered where deterministic symlink support exists.
- Duplicate spool source regression: passed; production source contains
  `review-inbox.json` and no `.catdesk/stable-wake/review-events` authority.
- `catdesk_transport_status` canonical inbox integration: passed.

## Verification evidence

| Check | Result |
| --- | --- |
| `cargo test stable_wake_bootstrap` | passed: 7 tests |
| `cargo test transport_status_tool_returns_redacted_status` | passed |
| `cargo fmt --check` | passed |
| `cargo clippy --all-targets --all-features -- -D warnings` | passed |
| `cargo build` | passed |
| main binary test suite | passed: 722 passed, 21 ignored |
| supervisor binary suite | passed: 15 passed |
| `recovery_powershell` | passed: 2 passed |
| `t0215_measure` | passed: 2 passed |
| `t0217_release_measure_tmp` | passed: 0 tests |

The repository Rust verification plan is fmt, test, and build. Test binaries
were run separately to preserve bounded command windows; no gate was skipped.

## Prohibited mutations and residual work

No live `.catdesk` state was modified by the provider; all inbox writes were
temporary-test fixtures. No browser submit, claim/receipt, target mutation,
daemon/release reload or promotion, Secure MCP/tunnel mutation, ProgramData,
Scheduler/service, signing/provenance, or Git publication occurred.

Remaining R1A work is stable target-config integrity plus the final
runtime/version-independence matrix. Claim/receipt/browser/desktop/install
ownership remains R1B.
