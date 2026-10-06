# T-0293 Core Host-Acceptance Readiness Preflight Review Bundle

## Scope and result

T-0293 adds one side-effect-free, bounded read-only preflight for the ordered
core host gates. It is exposed only through the existing closed operator
facade as:

```text
operator core-acceptance preflight
```

No arguments, paths, URLs, hashes, process controls, browser controls, or
host-lifecycle choices are accepted. The result contains only the fixed
classification, fixed reason, next gate, and booleans stating whether a gate
has exact durable evidence.

| Classification | Meaning |
| --- | --- |
| `READY` | All deterministic prerequisites are satisfied for the named next live step. This does not claim that a missing live step has passed. |
| `BLOCKED_BY_LIVE_ACCEPTANCE` | Durable live evidence exists but is stale, null, ambiguous, malformed, mismatched, or otherwise cannot prove the required gate. |
| `FAILED_DETERMINISTIC_PREREQUISITE` | Fixed supervisor, project/wake target, inbox/receipt reader, or GUI-observability readback is unavailable or divergent. |

## Reused read-only authorities

| Gate / input | Existing authority reused | Acceptance rule |
| --- | --- | --- |
| T-0224 natural wake | `stable_wake_core` canonical inbox and protected target; `StableWakeDelivery::read_exact_sent_evidence` | One unread completed-final-review record, exact project/session/record binding, schema-4 `SENT`, positive timestamp, message digest, current target digest, and receipt schema 1. |
| T-0223 supervisor | `assess_fixed_supervisor_activation_readiness` | Readiness must be `Ready` before a later host activation can be offered; readiness itself is not host activation evidence. |
| T-0222 GUI | Existing read-only native GUI/autonomy observability reader | Non-stale observability is a deterministic capability prerequisite, not literal visible-window acceptance. |
| Transport/project | T-0292 `operator_read_designated_chat_target` plus stable-wake target readiness | Central project target and protected wake target must read back coherently. |
| T-0152 sweep | No new store; requires a separately exact durable gate binding | Aggregate acceptance is never inferred from any lower gate or source/test result. |

The evaluator intentionally has no durable host-evidence writer. T-0223,
T-0222, and T-0152 therefore remain `Missing` in the production reader until
their independently reviewed host evidence authority exists. This is a
fail-closed boundary, not a claim that those gates are satisfied.

## Fail-closed evidence matrix

| Evidence condition | Result |
| --- | --- |
| Deterministic inputs valid; no T-0224 record/receipt | `READY`, next gate `T-0224`; T-0224 remains unaccepted. |
| Schema-4 state/inbox unavailable, target divergence, stale GUI readback, or supervisor readiness failure | `FAILED_DETERMINISTIC_PREREQUISITE`. |
| Provider completion only, source-only fixture, null receipt fields, stale/ambiguous record, invalid receipt | `BLOCKED_BY_LIVE_ACCEPTANCE`; no gate accepted. |
| Wrong target, project, session, record, message, timestamp, or receipt schema | `BLOCKED_BY_LIVE_ACCEPTANCE`; no gate accepted. |
| Exact durable T-0224 only | `READY`, next gate `T-0223`; no host activation is inferred. |
| Exact durable T-0224/T-0223/T-0222 only | `READY`, next gate `T-0152`; aggregate acceptance is not inferred. |
| All four independently exact durable gate proofs | `READY` with `ALL_CORE_LIVE_GATES_AUTHORITATIVELY_ACCEPTED`. |

## Changes and deterministic tests

- Added `src/core_host_acceptance_preflight.rs`, a pure evaluator and the
  fixed production read-only adapter.
- Added a strict read-only terminal receipt proof in
  `src/stable_wake_delivery.rs`; it reuses canonical inbox and schema-4
  validation, and test coverage confirms the reader changes neither inbox,
  target configuration, nor delivery state.
- Extended `src/operator_facade.rs` with only the exact three-token
  `operator core-acceptance preflight` grammar and adversarial extra/action
  rejection.

Focused tests cover deterministic-ready/live-missing, stale/null/source-only/
provider-only false positives, wrong target/project/session bindings,
deterministic prerequisite precedence, fully bound fixture evidence, output
redaction, exact facade parsing, and no fixture mutation from evaluation.

## Verification

| Command | Result |
| --- | --- |
| `cargo test core_host_acceptance_preflight` | Passed: 7 focused evaluator tests. |
| `cargo test operator_facade::tests::parser_accepts_only_closed_operator_actions` | Passed. |
| `cargo test stable_wake_delivery::tests::claim_submit_receipt_restart_and_immutable_inputs` | Passed. |
| `cargo clippy --all-targets --all-features -- -D warnings` | Passed. |
| `cargo test --all-targets --all-features --no-fail-fast` | Passed: 875 tests; expected platform-dependent ignored tests remained ignored. |
| `rust_full` project profile | Satisfied by the all-target/all-feature Rust test profile; no separate configured profile was present. |
| `cargo fmt --all -- --check` | Passed. |
| `cargo build --all-targets --all-features` | Passed. |
| `git diff --check` | Passed (exit 0); accumulated dirty-tree CRLF warnings were non-fatal and unrelated. |

The commands continue to emit the pre-existing non-fatal warning
`could not canonicalize path C:\\Users\\Volap`; they exit successfully.

## Prohibited live actions

No browser/profile/storage inspection, browser wake/launch, designated-chat
mutation, supervisor activation, ProgramData/Scheduler/service/daemon/release
change, private-pipe activity, Secure MCP/tunnel action, external-project
action, Git publication, signing, provenance, or dedicated-producer work was
performed. The evaluator has no authority for any of these operations.

## Attributable files

- `src/core_host_acceptance_preflight.rs`
- `src/stable_wake_delivery.rs`
- `src/operator_facade.rs`
- `src/main.rs`
- `CATDESK_MILESTONES.md`
- `.catdesk/current_plan.md`
- `.catdesk/todo.md`
- this bundle

The repository was already intentionally dirty. This task neither resets nor
attributes unrelated paths.

**Status: READY_FOR_INDEPENDENT_REVIEW.**
