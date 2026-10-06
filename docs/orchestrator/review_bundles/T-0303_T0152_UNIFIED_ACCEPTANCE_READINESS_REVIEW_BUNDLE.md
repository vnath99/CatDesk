# T-0303 T-0152 Unified Acceptance Readiness Review Bundle

## Scope and current boundary

T-0303 is a repository-only reconstruction of the final integrated sweep. It
does not execute a live supervisor, GUI, browser, target, tunnel, recovery, or
soak operation.

| Gate | Current state | T-0303 result |
| --- | --- | --- |
| T-0224 natural delivery | Accepted | Retained as the one exact schema-4 `SENT` current-target receipt and acknowledged ordinary final-review record. |
| T-0223 stable supervisor | Source-ready, host-live parked | Preserved as `OPERATOR_BOOTSTRAP_REQUIRED` under T-0301. |
| T-0222/T-0139 native GUI | Source-ready, literal host proof missing | Retained as the T-0302 visible-window procedure, not accepted by this audit. |
| T-0152 integrated sweep | Unaccepted | Predicate and the remaining durable-evidence boundary are defined below. |
| T-0155 resilience soak | Unaccepted | Remains downstream of independently accepted T-0152. |

## Exact T-0152 predicate

T-0152 requires all of the following independent, current, and coherent
evidence. Deterministic readiness is a prerequisite only; it is never a
substitute for an acceptance record.

| Requirement | Exact evidence required | Existing read-only/source authority | Classification today |
| --- | --- | --- |
| T-0224 | One fresh CatDesk `COMPLETED_VERIFIED` independent-final-review record; canonical schema-4 `SENT`; positive `browser_sent_at_unix`; receipt schema 1; non-null 64-hex message digest; exact record/project/session binding; exact current project/effective-wake target digest | `StableWakeDelivery::read_exact_sent_evidence`, canonical inbox reader, guarded project/effective-wake readback | Accepted, independently evidenced |
| Transport/project coherence | Current project designated target and protected effective wake target read back coherently; GUI observability reader is non-stale | T-0292 `operator_read_designated_chat_target`, stable-wake readiness, T-0293 snapshot reader | Deterministic prerequisite; re-read during a later sweep |
| T-0223 activation and continuity | Exact reviewed lifecycle status/preflight/activation result; one fixed 127.0.0.1:3201 supervisor; exact 127.0.0.1:3200 worker registration and generation/CAS; OS-attested same-user/same-nonzero-session pre-decode pipe admission; supported worker replacement; refusal checks preserving the valid backend; reviewed present/absent receipt/task rollback and recovery result | Fixed T-0299 lifecycle tools and T-0270/T-0283 authorities after T-0301 bootstrap | Host-live proof missing |
| T-0222/T-0139 literal GUI | One visible `CatDesk Binagotchy` window/taskbar entry in an interactive session; designated-target authoritative readback; T-0294 card; both read-only refreshes; repeated launch foregrounds rather than duplicates; close does not stop daemon; headless/session-zero refuses GUI | T-0139 closed native mode, T-0292 controller, T-0293 evaluator, T-0294 presentation, public `catdesk.ps1 start`/`recover` composition | Host-visible proof missing |
| Recovery/daemon continuity | Current canonical binary/fingerprint identity, public lifecycle readback, one canonical listener/instance, retained rollback/recovery behavior, and external runtime non-ownership | Existing fixed `catdesk_production_acceptance` pre/post snapshot and comparison workflow; bounded lifecycle readers | Supporting evidence only; not a T-0223/T-0222/T-0152 proof |
| Aggregate T-0152 | Independently reviewed binding of the preceding live observations to the same CatDesk project/session/current target plus the sweep result | No approved production authority exists yet | Missing; cannot be inferred |

The prior T-0152-R1 reconciliation, checked task-queue state, source tests,
provider-session completion, GUI presentation, and documentation review
bundles are historical/supporting material only. None is a durable live-gate
binding.

## Existing evaluator and evidence-authority audit

`operator core-acceptance preflight` is the sole current read-only integrated
operator surface. It has no arguments and invokes
`read_fixed_core_host_acceptance_preflight`. Its pure evaluator has the exact
ordered state machine below:

| Bound inputs | Result |
| --- | --- |
| Any unavailable supervisor, target/wake mismatch, unavailable wake reader, or stale GUI observability | `FAILED_DETERMINISTIC_PREREQUISITE` |
| Exact T-0224 only | `READY`, next `T-0223` |
| Exact T-0224 + T-0223 only | `READY`, next `T-0222` |
| Exact T-0224 + T-0223 + T-0222 only | `READY`, next `T-0152` |
| All four exact, same-project/session/current-target bindings | `READY`, `ALL_CORE_LIVE_GATES_AUTHORITATIVELY_ACCEPTED` |
| Stale, null, wrong target, wrong project/session/record, source-only, or provider-only gate evidence | `BLOCKED_BY_LIVE_ACCEPTANCE` at the first invalid gate |

This proves the evaluator itself neither relaxes ordering nor needs a second
engine. However, the production adapter obtains only T-0224 from the canonical
inbox plus schema-4 receipt. It intentionally supplies `Missing` for T-0223,
T-0222, and T-0152. Thus, after later host observations exist, the currently
supported production preflight cannot classify their acceptance or reach
`ALL_CORE_LIVE_GATES_AUTHORITATIVELY_ACCEPTED`.

That limitation is intentional fail-closed behavior, not a defect safely
repairable in T-0303: accepted project history defines no durable writer or
read-only reader for the three later independent host proofs. Creating one
here would introduce a new trusted host-evidence authority/store without its
own accepted criteria. The older `catdesk_production_acceptance` capture paths
are not reused as an alternate engine: they persist bounded pre/post lifecycle
snapshots, but their fixed gate schema does not express T-0223's stable
supervisor proof, literal T-0222 GUI proof, or aggregate T-0152 result.

## Fail-closed matrix

| Condition | Required result | Why it cannot pass T-0152 |
| --- | --- | --- |
| Provider completion or review bundle only | Not acceptance evidence | No exact host proof/binding |
| Source test or GUI fixture/render only | Not acceptance evidence | Fixture and presentation have no production evidence authority |
| Stale/null/multiple wake candidate or null receipt field | `BLOCKED_BY_LIVE_ACCEPTANCE` | T-0224 receipt proof is incomplete/ambiguous |
| Current registry/effective-wake divergence or stale observability | `FAILED_DETERMINISTIC_PREREQUISITE` | Current transport/project prerequisite is not coherent |
| T-0223 or T-0222 proof for another project, session, record, or target | `BLOCKED_BY_LIVE_ACCEPTANCE` | Exact binding fails |
| Partial T-0223 runtime observation or a GUI window without daemon/refresh/single-instance proof | Not a complete gate proof | The required independent host procedure is incomplete |
| Legacy production-acceptance snapshots alone | Supporting continuity evidence only | Their schema cannot close the newer live gates |
| Any missing later host-gate authority | `READY` only for the next gate or remains missing | The production evaluator deliberately refuses inference |

## Future host sequence (not executed)

1. **Bootstrap boundary:** the operator deploys the independently reviewed
   T-0299 release through the approved fixed reviewed-image procedure, then
   reconnects through the official existing transport. No direct workspace
   executable, shell copy, or legacy reload self-blessing is permitted.
2. **Read-only T-0223 preflight:** rediscover the three fixed lifecycle tools;
   call only `catdesk_stable_supervisor_status` and
   `catdesk_stable_supervisor_preflight` with `{}`. Park any non-ready or
   `ELEVATION_REQUIRED` outcome exactly.
3. **T-0223 host proof:** only when separately authorized, call confirmed
   activation and perform the exact fixed 3201/3200, pipe, generation,
   replacement, refusal, and rollback/recovery observations listed above.
4. **T-0222/T-0139 host proof:** use the public interactive lifecycle
   `catdesk.ps1 start` or `recover`; perform the literal visible-window,
   authoritative readback, read-only refresh, singleton, close/daemon, and
   headless-refusal observations from T-0302. Do not Apply a target as proof.
5. **Read-only sweep first:** re-read designated target/effective wake
   coherence and invoke `operator core-acceptance preflight`. It can confirm
   deterministic prerequisites and must remain non-accepting until a reviewed
   durable host-gate authority exists.
6. **Integrated verification second:** use the existing fixed
   production-acceptance pre/post comparison only for its bounded canonical
   release/lifecycle/listener/recovery/external-runtime continuity checks. It
   is supplemental and does not replace the live-gate authority.
7. **T-0152 decision:** obtain an independently reviewed fixed durable binding
   for the completed T-0223/T-0222/aggregate observations, then use the same
   existing evaluator for the all-gates classification. Only after independent
   T-0152 acceptance may T-0155 resilience soak begin.

## Source decision, changes, and residual risk

No product source change is made. The single named residual is
`T0152_DURABLE_HOST_GATE_AUTHORITY_UNDEFINED`: later host proofs have no
approved durable representation for the existing read-only evaluator. This is
not authority to write arbitrary state, reuse an unrelated snapshot, or infer
acceptance. A later specifically authorized design must define the smallest
fixed evidence authority and independent review conditions before any source
implementation.

Attributable T-0303 files:

- `CATDESK_MILESTONES.md`
- `.catdesk/current_plan.md`
- this bundle

## Verification and prohibited actions

| Check | Result |
| --- | --- |
| `cargo test core_host_acceptance_preflight --all-targets --all-features` | Passed: 9 focused fail-closed evaluator tests. |
| `cargo test windows_gui --all-targets --all-features` | Passed: 19 GUI/controller/read-only tests. |
| `cargo fmt --all -- --check` | Passed. |
| `git diff --check` | Passed; accumulated pre-existing CRLF notices were non-fatal. |

No browser wake/profile inspection, target/config/registry mutation, daemon
replacement, supervisor activation, ProgramData/Scheduler/service mutation,
Secure MCP/tunnel action, external-project action, signing/provenance/
dedicated-producer work, unrestricted shell, Git staging/commit/publication,
or T-0155 soak occurred.

**Status: READY_FOR_INDEPENDENT_REVIEW.**
