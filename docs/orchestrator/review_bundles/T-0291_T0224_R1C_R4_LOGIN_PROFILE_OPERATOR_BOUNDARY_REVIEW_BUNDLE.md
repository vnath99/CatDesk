# T-0291 T-0224-R1C-R4 Login/Profile Operator Boundary

## Scope and prohibited actions

This is a documentation/state-only reconciliation. It does not inspect browser
profile or storage contents and does not invoke or retry a browser wake. It
does not alter the ChatGPT target, wake configuration/state/owner, Secure
MCP/tunnel, host lifecycle, external projects, Git publication, or
signing/provenance/dedicated-producer work.

## Exact T-0290 durable result

The preserved T-0290 session
`adc-t0290-fresh-natural-delivery-acceptance-20260901` reached
`COMPLETED_VERIFIED` for task T-0290 on the preserved Codex/Terra-high thread,
with one provider turn and its contract-approved verification record. Its fresh
project review record is
`review-adc-t0290-fresh-natural-delivery-acceptance-20260901-7-independent_final_review`.

Its exact schema-4 delivery evidence is:

| Field | Durable value |
| --- | --- |
| `status` | `OPERATOR_ATTENTION` |
| `attention` | `LOGIN_OR_PROFILE_REQUIRED` |
| `browser_sent_at_unix` | `null` |
| `message_sha256` | `null` |
| `target_sha256` | `null` |
| `receipt_schema_version` | `null` |

The login/profile classification occurs before a send receipt, so the result is
pre-submit. It is not the retry-safe `CHATGPT_NOT_IDLE` condition; the exact
T-0290 record remains non-retryable and must never be retried.

## Repository versus host boundary

T-0288/T-0289 already established the repository boundary: the registered
project target, protected wake configuration, and guarded CAS agree, and the
reviewed event-to-delivery path validates that binding before dispatch. No
repository target-binding defect is proven.

The remaining minimum boundary is host/operator-only: restore an authenticated
ChatGPT session in the already-configured CatDesk wake browser/profile. That
action must not change the target, configuration, delivery state, or owner.
After it is complete, acceptance requires a separately fresh ordinary CatDesk
final-review event; it must not retry or replay T-0290.

## T-0224 live-success predicate

For the later fresh record, durable evidence must contain all of:

1. schema-4 delivery status `SENT`;
2. positive `browser_sent_at_unix`;
3. receipt schema `1`; and
4. exact record, message, and target binding.

`TARGET_DRIFT`, login, CAPTCHA/security, and post-submit ambiguity remain
fail-closed and must not result in a duplicate submit. A manual wake, profile
inspection, target change, or host activation is not acceptance evidence.

## Critical path and safe repository work

The critical path remains T-0224 → T-0223 → T-0222/T-0139 → T-0152 → T-0155.
While the T-0224 live gate is parked at the authentication boundary, T-0292 is
the next safe repository-only task: revalidate the accepted stable-supervisor
lifecycle/status-preflight boundary and integrated T-0223 readiness evidence
without host activation, browser/wake action, or ownership mutation.

## Attribution and verification

T-0291 attribution is limited to:

- `CATDESK_MILESTONES.md`
- `.catdesk/current_plan.md`
- this exact review bundle

| Command | Result |
| --- | --- |
| `cargo test --all-targets --all-features --no-fail-fast` | Passed: 860 tests. Expected platform-dependent ignored cases remained ignored; the command emitted a non-fatal `could not canonicalize path C:\\Users\\Volap` warning after successful test execution. |
| `git diff --check` | Passed (exit 0). The three documentation paths are untracked in the intentionally dirty accumulated worktree, so this records whitespace safety without asserting a synthetic clean-tree baseline. |

**Status: READY_FOR_INDEPENDENT_REVIEW of the T-0291 reconciliation only.**
T-0224 live natural-delivery acceptance remains open pending the distinct
operator authentication boundary and a later fresh ordinary final-review
record. No manual wake was performed.
