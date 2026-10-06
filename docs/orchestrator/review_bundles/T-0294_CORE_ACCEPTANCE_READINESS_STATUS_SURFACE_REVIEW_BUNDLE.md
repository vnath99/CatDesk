# T-0294 Core Acceptance Readiness Status Surface Review Bundle

## Scope and result

T-0294 adds a compact, native Windows GUI **Core Acceptance Readiness** card.
It is a presentation of the accepted T-0293 evaluator, not another acceptance
engine or evidence store. The card shows the overall fixed classification and
separate rows for T-0224, T-0223, T-0222/T-0139, and T-0152. An explicit
`Refresh readiness` action replaces only in-memory presentation state.

The repository-only source/test slice is complete. It does not accept any live
gate and does not constitute literal visible-GUI acceptance.

## Architecture reuse and UI behavior

| Surface | Reused authority | Behavior |
| --- | --- | --- |
| GUI production read adapter | `read_fixed_core_host_acceptance_preflight(...).presentation()` | Calls the one accepted T-0293 read-only evaluator. |
| GUI controller | `CoreAcceptanceReadinessControllerV1` | Holds presentation data in memory; construction and refresh only read. |
| GUI card | `render_core_acceptance_readiness` | Renders bounded fixed classification, gate state, reason, and next-step wording. |
| Existing T-0293 evaluator | Its existing target/readiness/inbox/receipt readers | Remains the sole place that interprets live-evidence rules. |

The card is explicit: `READY` means deterministic readiness for the named next
ordered live step unless exact durable evidence already proves a gate. Display
or refresh is never live acceptance. The UI labels accepted evidence only as
`EXACT_DURABLE_EVIDENCE`; all other gate states remain `NOT_ACCEPTED` with a
bounded reason.

## Fail-closed semantics

| T-0293 result/evidence | GUI result |
| --- | --- |
| Deterministic prerequisite failure | Displays `FAILED_DETERMINISTIC_PREREQUISITE`; it never falls through to a later gate. |
| Missing live proof | Displays the evaluator's `READY`/next-gate result and `NOT_ACCEPTED`; it does not claim acceptance. |
| Stale, null, source-only, provider-only, target/project/session-mismatched proof | Displays `BLOCKED_BY_LIVE_ACCEPTANCE` and the evaluator's bounded invalid-evidence reason. |
| Exact fixture proof for all gates | Displays its exact-evidence rows, while retaining the explicit statement that display is not live acceptance. |

T-0224 authenticated natural delivery, T-0223 host activation, T-0222/T-0139
literal visible GUI, and T-0152 aggregate acceptance remain independently
bound to their own durable live evidence. The GUI never writes such evidence.

## Deterministic regression evidence

- Initial render covers the overall classification, next live gate, all four
  gate rows, `NOT_ACCEPTED` states, and the non-acceptance operator wording.
- Fixture cases preserve deterministic-failure precedence and invalid
  live-evidence classifications without modifying the supplied presentation.
- An all-exact durable fixture renders four evidence rows but still carries the
  explicit non-acceptance display note.
- Refresh reads the fixture again and replaces only the controller's in-memory
  presentation; the fixture exposes no write/live-action operation.
- Rendered content is capped at 1,024 characters and test coverage rejects URL,
  path, browser-profile, and token-like detail disclosure.

No test runs the native window or reaches live browser, host, or target state.

## Verification

| Command | Result |
| --- | --- |
| `cargo test windows_gui --all-features` | Passed: 19 focused GUI tests. |
| `cargo fmt --all -- --check` | Passed after one formatting-only correction. |
| `cargo clippy --all-targets --all-features -- -D warnings` | Passed. |
| `cargo test --all-targets --all-features --no-fail-fast` | Passed: 880 unit tests plus all integration targets. |
| `cargo build --all-targets --all-features` | Passed. |
| `rust_full` project profile | Satisfied by the all-target/all-feature Rust test profile; no separate configured profile was present. |
| `git diff --check` | Passed; pre-existing dirty-tree CRLF warnings were non-fatal. |

Commands continue to emit the pre-existing non-fatal warning that
`C:\\Users\\Volap` could not be canonicalized; all listed commands exited zero.

## Attributable files

- `src/core_host_acceptance_preflight.rs`
- `src/windows_gui.rs`
- `CATDESK_MILESTONES.md`
- `.catdesk/current_plan.md`
- `.catdesk/todo.md`
- this bundle

The worktree was intentionally dirty before this task. No reset, clean,
revert, staging, commit, or publication occurred; unrelated changes are not
attributed here.

## Prohibited actions and residual boundary

No browser launch/wake/profile inspection, designated-chat update, supervisor
activation, ProgramData/Scheduler/service/daemon/release mutation, Secure
MCP/tunnel action, external-project change, Git publication, or
signing/provenance/dedicated-producer work occurred.

The next live gate remains the ordered T-0224 authentication/natural-delivery
boundary described in the current plan; it requires separate authorization.

**Status: READY_FOR_INDEPENDENT_REVIEW.**
