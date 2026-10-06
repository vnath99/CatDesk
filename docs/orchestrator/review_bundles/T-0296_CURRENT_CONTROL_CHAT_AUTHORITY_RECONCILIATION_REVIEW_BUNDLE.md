# T-0296 Current Control-Chat Authority Reconciliation Review Bundle

## Scope and conclusion

T-0296 is a repository-only reconciliation. It did not call a target setter,
wake owner, browser/profile, supervisor, or host lifecycle surface.

The current operator-authorized canonical target is the exact coherent
readback from the CatDesk project registry and protected effective-wake
configuration:

```text
https://chatgpt.com/c/6a932bca-64b8-83ea-8aff-fc7963272ac0
sha256: 4c26e6cabd38d597eb6d366ac53a236925c1a68f74853e8c222b90b675b8d562
```

T-0295's claim that a historical conversation URL and its then-recorded digest
were permanently mandated is superseded. Its mutation-path audit remains
historical evidence only: it still supports the limited conclusion that no
bounded T-0292 fixture/provider artifact causally proves a production-target
write.

## Authority map

| Authority | Current role | Acceptance rule |
| --- | --- | --- |
| Project registry | Stores the registered project URL and SHA-256 under the reviewed guarded update path. | A readback accepts only one canonical workspace project and an exact canonical URL/digest. |
| Protected effective wake configuration | Stores the wake conversation URL under its fixed protected writer. | It must canonicalize to exactly the registry target before either can be presented as authoritative. |
| `operator_read_designated_chat_target` | Read-only composition of the two authorities. | Returns the current digest only when both stores agree; any absent, malformed, duplicate, or divergent state is a fixed protected-state failure. |
| T-0293/T-0294 preflight/status | Read-only consumer of that composition. | Passes the current readback digest to receipt validation; it has no historical URL constant or target-update authority. |
| Historical documentation | Context only. | It cannot select an acceptance target or upgrade a receipt. |

The approved guarded update remains a paired transaction: it validates the
displayed current digest, proves coherent prior readback, stages the wake CAS,
commits the registry CAS, compensates the wake change if the registry commit
fails, and requires coherent final readback. T-0296 did not exercise that
writer outside isolated existing fixtures.

## Stale-state reconciliation

The stale T-0295 conclusion appeared in its own bundle, the milestone entry,
the current plan, and the T-0295 queue summary. The bundle now begins with an
explicit supersession notice; the milestone and plan identify T-0295 as
historical and record the current canonical URL/digest above. Historical audit
text is retained rather than deleted, but is not current authority.

No production logic encoded the historical URL.
`read_fixed_core_host_acceptance_preflight` gets its expected receipt target
from `operator_read_designated_chat_target`, then independently compares the
stable-wake readiness digest. Therefore no production repair was needed.

## Deterministic regression evidence

| Case | Evidence |
| --- | --- |
| Authorized guarded target becomes canonical | Existing isolated guarded-update coverage changes a fixture from a prior URL to a new URL and requires coherent project/wake readback. The preflight regression then accepts a structurally valid receipt only when its digest equals the current authority input. |
| Historical target does not remain mandated | The new `structurally_sent_receipt_must_match_the_current_authoritative_target` test accepts the current target and rejects the prior target with `T0224_LIVE_EVIDENCE_INVALID`. |
| Registry/wake disagreement | The new `project_and_wake_target_disagreement_stays_a_deterministic_failure` test returns `FAILED_DETERMINISTIC_PREREQUISITE` / `PROJECT_WAKE_TARGET_MISMATCH` before evaluating a receipt. |
| Wrong-target receipt | Existing target/project/session mismatch coverage and the new current-vs-prior receipt test keep `t0224_accepted == false` for a different target. |
| Fixture-only safety | The target-editor tests inject a fixture authority; guarded-update tests use temporary fixture roots. No test opens the configured production project registry or wake configuration. |

The positive fixture receipt advances only to the still-missing T-0223 host
gate. It does not fabricate T-0224 live acceptance, because source fixtures
and provider completion are not durable live evidence.

## Acceptance impact and remaining live gate

T-0224 remains open. The current coherent target needs no repository repair.
After the separately documented browser-authentication operator boundary is
satisfied, a **fresh ordinary** final-review event must naturally yield exact
schema-4 `SENT` evidence: positive `browser_sent_at_unix`, receipt schema 1,
and exact fresh record/message/current-target binding. Null, stale, malformed,
wrong-project/session, registry/wake-drift, or wrong-target evidence remains
fail-closed. T-0223, T-0222/T-0139, T-0152, and T-0155 remain separately
unaccepted in their established order.

## Verification

| Command | Result |
| --- | --- |
| `cargo test core_host_acceptance_preflight --all-features` | Passed: 9 focused tests. |
| `cargo test designated_chat_target --all-features` | Passed: 2 guarded target-fixture tests. |
| `cargo fmt --all -- --check` | Passed after the one mechanical formatter repair. |
| `cargo clippy --all-targets --all-features -- -D warnings` | Passed. |
| `cargo test --workspace --all-targets --all-features` | Passed: 882 unit tests plus all workspace integration targets. |
| `cargo build --workspace --all-targets --all-features` | Passed. |
| `rust_full` project profile | Satisfied by the all-target/all-feature Rust profile; no separate configured profile was present. |
| `git diff --check` | Passed; accumulated dirty-tree CRLF warnings were non-fatal. |

The known non-fatal `C:\\Users\\Volap` canonicalization warning did not affect
command exit status.

## Attributable diff and prohibited-action audit

T-0296 attribution is limited to:

- `src/core_host_acceptance_preflight.rs` (two deterministic authority-binding regressions)
- `CATDESK_MILESTONES.md`
- `.catdesk/current_plan.md`
- `.catdesk/todo.md`
- T-0295's historical bundle supersession notice
- this exact bundle

The accumulated dirty worktree was preserved. No live target/config/registry,
browser/profile/wake, supervisor/host, Secure MCP/tunnel, external-project,
signing/provenance/dedicated-producer, branch, commit, or Git publication action
occurred.

**Status: READY_FOR_INDEPENDENT_REVIEW - CURRENT AUTHORITY RECONCILED; T-0224
REMAINS OPEN FOR FRESH NATURAL CURRENT-TARGET EVIDENCE.**
