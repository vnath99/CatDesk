# T-0288 Core Milestone Reconciliation / Integrated Acceptance Matrix

## Scope and durable evidence

This is a documentation and repository-verification reconciliation only. It
does not invoke a browser wake, read a browser profile, alter wake target or
state, activate the stable supervisor, mutate ProgramData, or affect Secure
MCP, tunnels, external projects, Git publication, signing, provenance, or a
dedicated producer.

The durable session authority for
`adc-t0286-post-target-repair-natural-wake-canary-20260830` records
`COMPLETED_VERIFIED`, one provider turn, and the preserved canonical Codex
continuity. The project review inbox contains exactly one corresponding
`catdesk` independent-final-review record:
`review-adc-t0286-post-target-repair-natural-wake-canary-20260830-7-independent_final_review`.

The normal schema-4 wake delivery for that exact record is not a successful
browser delivery. Its durable delivery state is `OPERATOR_ATTENTION` with
`TARGET_DRIFT`; `browser_sent_at_unix`, receipt schema, message digest, and
delivery target digest are absent. The previously completed guarded operator
CAS returned target SHA-256
`d40b8d5b1995bb781f1c8e4f56bc6e6397f649678ad8ce9cb21232fdeba82d98`.
Therefore T-0286 is provider-complete, while T-0224 natural-wake acceptance
remains explicitly open.

## Queue disposition

| Item | Durable disposition | Why |
| --- | --- | --- |
| T-0286 | Provider-complete; live wake unaccepted | Its ordinary final-review event exists, but the exact delivery failed closed with `OPERATOR_ATTENTION/TARGET_DRIFT` and no receipt. |
| T-0287 | Superseded | The delegated Qwen reconciliation did not deliver the required result; it is preserved as historical evidence and is not replayed. |
| T-0288 | Provider-complete pending independent review | This ticket reconciles the durable evidence and runs only repository verification. |
| T-0289 | Next repository-only gate | Bounded post-CAS target-drift provenance reconciliation; no wake attempt or target mutation. |
| Signing/provenance/dedicated-producer branches | Parked | They are unrelated to the integrated acceptance gates and are not reopened here. |

## Dependency-ordered integrated acceptance matrix

| Order | Gate | Current state | Acceptance class | Evidence / next action |
| ---: | --- | --- | --- | --- |
| 1 | T-0224 target-drift provenance | Blocked on the absence of a send receipt | Repository-testable now | T-0289 must explain the reviewed event-to-delivery target binding or fail closed with a narrower host/operator prerequisite. |
| 2 | T-0224 fresh ordinary natural wake | Open | Host/live observation after gate 1 | A later ordinary completion must produce one fresh project-scoped record with schema-4 `SENT`, positive `browser_sent_at_unix`, receipt schema 1, and exact record/message/target binding. Manual wake is prohibited. |
| 3 | T-0224 retry/deadman behavior | Parked behind a successful natural delivery | Host/operator-only | `CHATGPT_NOT_IDLE` may defer/retry before submit; target/login/CAPTCHA/security/post-submit ambiguity must remain fail-closed without duplicate submission. |
| 4 | T-0223 stable supervisor/control plane | Source chain accepted; host activation unaccepted | Host/operator-only | Reviewed 3201/3200/pipe and protected-state code is retained. Any activation requires its reviewed lifecycle and independent host evidence, not this ticket. |
| 5 | T-0222 visible GUI host acceptance | Open | Host/operator-only | Requires the accepted supervisor and wake-side prerequisites; no GUI/browser target action is authorized here. |
| 6 | T-0152 post-reconciliation sweep | Blocked | Integrated final sweep | Reconcile only after T-0224, T-0223, and T-0222 have their required independent acceptance evidence. |

## Planning changes

`CATDESK_MILESTONES.md`, `.catdesk/current_plan.md`, and `.catdesk/todo.md`
now distinguish provider completion from browser-delivery acceptance and mark
the failed delegated T-0287 route as superseded rather than a standing core
blocker. The queue adds T-0289 as the only next bounded repository-only action.

## Repository-only verification

The approved Rust profile and `git diff --check` are recorded below after they
run. No source or script change is authorized if verification exposes a defect;
the required response is a narrow queue item with the exact command evidence.

| Command | Result |
| --- | --- |
| `cargo test --all-targets --all-features --no-fail-fast` | Passed: 860 tests; expected platform-dependent ignored cases remained ignored. The command emitted a non-fatal `could not canonicalize path C:\\Users\\Volap` warning after successful test execution. |
| `git diff --check` | Passed (exit 0). The repository is intentionally broadly dirty and the T-0288 planning files are untracked in the existing worktree, so this check establishes whitespace safety but not a synthetic clean-tree baseline. |

## Attributable diff

T-0288 attribution is intentionally limited to:

- `CATDESK_MILESTONES.md`
- `.catdesk/current_plan.md`
- `.catdesk/todo.md`
- this review bundle

No product source, lifecycle scripts, wake configuration/state/profile, or
runtime host surface is changed.

`git status --short -- <T-0288 paths>` reports the four paths above as
untracked in the pre-existing accumulated worktree. This ticket does not claim
ownership of unrelated tracked changes shown by the broad worktree stat.

## Remaining blockers and review request

The exact active blocker is **T-0224 natural wake has no durable successful
send receipt because the fresh T-0286 record ended `TARGET_DRIFT`**. T-0289 is
the next safe source/repository-only investigation. Host/operator-only live
proof remains explicitly parked for T-0224, T-0223, and T-0222.

**Status: READY_FOR_INDEPENDENT_REVIEW of the T-0288 reconciliation only.**
This is not a claim of live wake, host activation, GUI, or integrated-sweep
acceptance. Request ChatGPT independent final review before progressing to
T-0289 or any live/operator gate.
