# T-0305 / T-0155 Resilience Soak Readiness Review Bundle

## Scope and current gate truth

This is a documentation-only readiness decision. No T-0155 cycle was run and
no browser, wake, target, protected state, supervisor, daemon, Secure MCP,
tunnel, external project, or Git state was changed.

| Gate | Current durable status | Consequence for T-0155 |
| --- | --- | --- |
| T-0224 | Accepted by the fresh natural schema-4 receipt bound to the current project/effective-wake target. | Required evidence source is available, but is not a substitute for the later soak. |
| T-0223 | Source chain ready; `OPERATOR_BOOTSTRAP_REQUIRED` remains. | Blocks every live lifecycle/restart exercise. |
| T-0222/T-0139 | Source-ready; literal visible-host evidence absent. | Blocks the integrated sweep. |
| T-0152 | T-0303/T-0303-R1 readiness matrix accepted; live aggregate authority is absent. | Direct prerequisite: no soak may start. |
| T-0155 | Not started. | Protocol only; no acceptance is claimed. |

The ordered path is unchanged: **T-0223 -> T-0222/T-0139 -> T-0152 ->
T-0155**. T-0303-R1’s accepted status is readiness only, not T-0152 live
acceptance.

## Durable T-0155 intent and existing observability

The queue defines repeated bounded lifecycle cycles covering reviewed candidate
lifecycle, canonical promotion-or-rollback, daemon restart, a pending
`WAITING_FOR_CHATGPT` handoff, ChatGPT-busy deferral followed by natural wake,
and official Secure MCP reattachment. T-0304 additionally makes the
reset-aware Codex-to-local-Qwen handoff observable without changing the
canonical Codex thread.

| Future exercise | Existing first-class/read-only observation surface | Required retained evidence |
| --- | --- | --- |
| Reviewed release lifecycle and recovery | Fixed stable-supervisor status/preflight/result surface; reviewed lifecycle recovery state and the bounded production-acceptance pre/post comparison. | Exact reviewed build/fingerprint, current/LKG or protected receipt state, fixed lifecycle category, pre/post instance identity, and recovery outcome. |
| Canonical promotion or rollback | Existing reviewed promotion/recovery transaction records and current/LKG/hash-pair readers. Legacy canonical recovery intentionally refuses script/daemon/tunnel waterfalls. | Candidate and canonical SHA-256, LKG identity, transaction generation, authorization/receipt identity, and exact rollback or promotion terminal result. |
| Daemon/supervisor-worker continuity | T-0223 fixed lifecycle readback: one fixed loopback supervisor/worker route, generation/CAS state, same-principal/session pipe evidence, and supported worker replacement observations. | Supervisor and worker identities, generation transition, pipe principal/session proof, worker registration/replacement result, and retained valid-backend proof after negative checks. |
| `WAITING_FOR_CHATGPT` handoff | Autonomous-session durable state, project-scoped review inbox, and terminal/final-review identity. | Same project, task, session, canonical Codex thread, review-record/diff/final-review binding, and exactly-once inbox evidence. |
| Busy deferral then natural one-submit success | Schema-4 stable-wake delivery and owner records. `CHATGPT_NOT_IDLE` is valid only before submission; the owner records a receipt only after the normal event-driven send. | One record ID, one owner/claim, current target digest, positive send time, message digest, receipt schema 1, and exact project/session binding. |
| Official Secure MCP reattachment/non-duplication | Existing transport/status and bounded production-acceptance observations; the supervisor/daemon remain explicitly non-owners. | Official transport identity/connection observation before and after the lifecycle event and evidence of exactly one external runtime, with no CatDesk-created tunnel process/configuration. |
| Reset-aware Codex/Qwen continuity | Durable autonomous controller/provider route, bounded provider diagnostics, exact reset boundary when provider-attested, and canonical thread handoff state. | Provider route transitions, redacted diagnostic class, reset boundary if unambiguous, one local-Qwen handoff, no Codex replay before reset, and restored original Codex thread only at a safe post-reset boundary. |

These are observation surfaces, not a second acceptance store. In particular,
the T-0293 production core-acceptance reader intentionally reports later
T-0223/T-0222/T-0152 proof as missing until an independently reviewed durable
host-evidence authority exists.

## Fixed future soak protocol — proposed pending independent review

Durable records do **not** define an accepted cycle count, duration, or cadence.
The smallest bounded recommendation is **three serial cycles, separated by at
least 30 minutes**, each starting only after the prior cycle’s artifacts are
complete. Three serial cycles exercise recovery/restart/retry continuity while
limiting host exposure; this is a reviewable proposal, not an accepted T-0155
parameter or authority.

### Prerequisites before cycle 1

1. Independently accepted live T-0152 binding for the current project, current
   coherent registry/effective-wake target, and exact accepted T-0224/T-0223/
   T-0222/T-0139 evidence.
2. Read-only fixed lifecycle and core-acceptance preflight/readback is clean;
   no missing, stale, malformed, cross-project/session, or wrong-target
   evidence is tolerated.
3. The reviewed candidate/current/LKG state is exact and attributable; the
   official Secure MCP transport is present as one external runtime, not a
   CatDesk-owned service.
4. Each cycle has a fresh project-scoped task/session/final-review record and
   a baseline ledger of target digest, build/fingerprint, generation, provider
   route/reset boundary, supervisor/worker identity, and wake-record ID.

### Cycle boundaries and observations

**Start:** Capture the prerequisite ledger through read-only surfaces, assign a
fresh ordinary project-scoped record, and record the expected current target
and reviewed build identities. Do not synthesize a browser event or invoke a
manual wake.

**Exercise:** Use only the already reviewed lifecycle/recovery and ordinary
autonomous final-review paths that the later host authorization permits.
Observe one reviewed lifecycle or recovery transition, its canonical
promotion-or-rollback result, supervisor/worker continuity, project-scoped
`WAITING_FOR_CHATGPT` handoff where naturally applicable, a pre-submit
`CHATGPT_NOT_IDLE` defer/retry where naturally encountered, then one natural
event-driven browser submit, official transport reattachment, and provider
continuity. A provider exhaustion branch must take the same logical
task/session to contract-approved local Qwen exactly once and retain the
canonical Codex thread/reset boundary.

**End:** Retain the post-cycle ledger and compare it to the start ledger. The
cycle passes only if every applicable observation is exact, bound to the same
project/current target and expected generation/build lineage, and the final
review/wake receipt is uniquely bound. Perform read-only final lifecycle,
transport, target/wake, provider-route, and core-acceptance readback before
allowing the next cycle.

### Per-cycle pass conditions

- Canonical reviewed binary/fingerprint and current/LKG transaction are exact;
  an interrupted operation is either provably recovered to the prior authority
  or fails closed without continuation.
- Fixed supervisor/worker and principal/session/generation observations retain
  continuity; negative/refusal checks preserve the valid backend.
- A `WAITING_FOR_CHATGPT` review handoff is project-scoped and exactly once.
- A busy defer is demonstrably pre-submit; after an ordinary natural retry,
  exactly one schema-4 `SENT` receipt has positive send time, receipt schema 1,
  exact message digest, current target digest, record ID, project, and session.
- Official Secure MCP is reattached/readable exactly once without CatDesk
  taking runtime ownership or creating a second tunnel runtime.
- Codex remains preferred while eligible; confirmed lasting exhaustion routes
  once to local Qwen, transient 429 stays retry/backoff, and no Codex retry
  occurs before an unambiguous persisted reset boundary.

## Immediate stop / fail-closed matrix

| Observation | Required response |
| --- | --- |
| Stale/missing/mismatched canonical hash pair, build fingerprint, LKG/current receipt, project, target, generation, session, provider route, or wake binding | Stop and invalidate the cycle; do not recapture current state as authority. |
| Lost/ambiguous wake ownership; duplicate inbox/state record; duplicate browser submit; post-submit ambiguity | Stop. Preserve durable ambiguity; never retry submission manually. |
| Target drift, login/profile, CAPTCHA, security, or `CHATGPT_NOT_IDLE` with any submit evidence | Stop except an exact pre-submit busy condition may remain deferred by the existing owner. It cannot be papered over or manually retried. |
| Recovery ambiguity, rollback failure, invalid current/LKG continuity, or worker/supervisor/pipe identity loss | Stop; use no script waterfall or version-coupled recovery substitute. |
| Duplicate/unowned Secure MCP/tunnel runtime or reattachment not independently observable | Stop; CatDesk must not restart, configure, or adopt the tunnel. |
| Qwen unavailable after lasting Codex exhaustion, Codex replay before reset, loss of canonical thread, or generic terminal error treated as credit exhaustion | Stop and escalate through existing fail-closed provider policy. |

Forbidden throughout: manual browser wake/submission; target/profile/config/owner
edits; generic shell/PowerShell/cmd recovery; ad-hoc daemon replacement; tunnel
restart/configuration/duplication; caller-selected lifecycle authority; and Git
publication. No failed cycle can be repaired by omitting or rewriting its
artifact.

## Aggregate predicate and start authority

After the independently reviewed protocol parameters are accepted, T-0155 can
pass only when every required serial cycle has complete immutable-attribution
ledgers, every applicable pass condition above holds, no stop condition occurs,
and an independently reviewed aggregate authority records the result. Source
tests, a GUI card, provider completion, a queue checkmark, or this runbook are
not that authority.

The exact next live action is **not** a soak: first complete the T-0301
operator bootstrap, then independently accept T-0223, literal T-0222/T-0139,
and T-0152. Only then may an explicitly authorized T-0155 execution ticket
adopt the independently accepted protocol parameters and begin cycle 1.

## Source decision and verification

No deterministic missing read-only measurement was proven. Existing wake,
lifecycle/recovery, core-acceptance, project/transport, and provider-routing
surfaces cover the future observations while preserving their current
fail-closed authority boundaries. No product source, test, runtime, or
protected-state file was changed by T-0305.

Documentation attribution for T-0305:

- `CATDESK_MILESTONES.md`
- `.catdesk/current_plan.md`
- `docs/orchestrator/review_bundles/T-0305_T0155_RESILIENCE_SOAK_READINESS_REVIEW_BUNDLE.md`

Verification to record for this documentation-only ticket:

- `cargo fmt --all -- --check` — PASS.
- `cargo test --workspace --all-targets --all-features core_host_acceptance_preflight`
  — PASS (9 tests).
- `cargo test --workspace --all-targets --all-features stable_wake_delivery`
  — PASS (13 tests).
- `cargo test --test recovery_powershell` — PASS (2 tests).
- `git diff --check` — PASS (only pre-existing working-copy line-ending
  warnings were emitted).

No browser wake, target mutation, host/supervisor activation, tunnel mutation,
external-project operation, signing/provenance work, or Git publication was
performed.

## Independent-review checklist

- Confirm that T-0305 is readiness-only and has not started T-0155.
- Confirm the proposed three-cycle cadence is labeled proposed, not accepted.
- Confirm every cycle needs exact project/target/build/generation/session/
  provider/wake attribution and each listed ambiguity stops the soak.
- Confirm T-0223 remains `OPERATOR_BOOTSTRAP_REQUIRED` and the critical order
  is preserved.
- Confirm no unrelated dirty-tree changes are attributed to this ticket.
