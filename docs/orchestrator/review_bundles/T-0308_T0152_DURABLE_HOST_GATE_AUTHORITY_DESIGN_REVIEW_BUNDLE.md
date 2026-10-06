# T-0308 / T-0152 Durable Host-Gate Authority Design Review Bundle

## Scope, current gate truth, and classification

This ticket designs but does not implement an authority for the later live
T-0223, T-0222/T-0139, and aggregate T-0152 proofs identified by T-0303 and
T-0303-R1. It performs no host action, capture, bootstrap, signing, target
change, browser action, or source change.

| Gate | Current truth |
| --- | --- |
| T-0224 | Accepted by the exact current-target schema-4 `SENT`/receipt-schema-1 record and canonical inbox binding. |
| T-0223 | Source-ready; host-live proof is parked behind the externally reviewed-image bootstrap prerequisite. |
| T-0222/T-0139 | Source-ready; literal interactive-host proof is missing. |
| T-0152 | Unaccepted; the production preflight intentionally has no later-host-gate authority. |
| T-0155 | Unstarted and downstream of independently accepted T-0152. |

**Sole classification: `TRUST_MODEL_BLOCKER`.** The existing product has no
trusted writer or reviewer-authenticated receipt that can convert a concrete
later host observation into an independently reviewed durable acceptance
binding. This is not authority to add a local `accepted: true` field or infer
acceptance from a source test, provider session, GUI display, review bundle,
or legacy snapshot.

The one unresolved decision is: **which existing independent-review authority
will issue a fixed, verifier-authenticated receipt that binds one
product-derived host-observation digest, project/session/current-target
identity, and gate name?** No current source supplies such a receipt or a
verifier key/root. The canonical inbox value `next_action:
independent_final_review` requests/references independent review; it does not
attest that a reviewer approved a particular host observation.

## Existing authority inventory

| Primitive | Owner/writer and fixed storage | Binding, replay/atomicity, reader | Semantic adequacy |
| --- | --- | --- | --- |
| Canonical review inbox | Autonomous/delegated terminal handoff; workspace `.catdesk/autonomy/review-inbox.json`; schema 1, bounded canonical parser in `src/stable_wake_core.rs`. | Record id, CatDesk project id, session id, terminal state, next action, bounded reference, timestamp, unread flag. Duplicate conflict is refused; parser rejects reparse/unsafe/malformed input. Read by `canonical_inbox_records`. | Necessary terminal-review identity, but no current target, lifecycle/GUI measurements, reviewer approval, or host-proof schema. |
| Exact wake delivery | Stable wake owner writes schema-4 state under fixed workspace wake root; receipt schema 1 read by `StableWakeDelivery::read_exact_sent_evidence` in `src/stable_wake_delivery.rs`. | Requires exactly one canonical terminal-review record, `COMPLETED_VERIFIED`, `independent_final_review`, current protected target, one delivery, positive send time, message digest, and receipt version. State validation/conflict and operator-attention refusal are fail closed. | Exact authority for T-0224 only. It does not measure supervisor activation, literal GUI visibility, or aggregate acceptance. |
| Current target coherence | Project registry/effective-wake guarded pair read by `operator_read_designated_chat_target` and `workspace_readiness`; target is a canonical SHA-256, not a caller-provided URL. | Protected wake config parser/readback detects drift; T-0292 CAS preserves paired registry/wake equality on mutation. | Required revalidation predicate for every later record, not a host-acceptance record itself. |
| Supervisor lifecycle state/receipts | Product-owned fixed `C:\\ProgramData\\CatDesk\\ControlPlaneSupervisor` root, `ControlPlaneSupervisorStoreV1`, fixed current/LKG receipts, and lifecycle composition in `src/control_plane_supervisor.rs` / `src/supervisor_lifecycle.rs`. | Handle-pinned roots, no-follow relative reads, exact image/receipt binding, generation CAS, same-principal/session pipe admission, and temporary/fsync/atomic replacement. Transaction rollback restores exact receipt pairs. | Supplies product-derived T-0223 constituents. No immutable per-acceptance observation/reviewer binding exists. |
| Native GUI observability | `src/windows_gui.rs` reads only `read_fixed_core_host_acceptance_preflight`; its refresh replaces in-memory presentation. | No GUI path receives evaluator inputs or writes acceptance state; rendering says `READY` is not acceptance. | Correct read-only presentation. A window, taskbar item, or fixture alone must not be durable acceptance evidence. |
| Protected filesystem primitives | `ProtectedDirectoryGuard`, `read_optional_relative_regular`, and `write_unique_regular_for_atomic_replace` in `src/windows_protected_fs.rs`. | Pinned/no-follow ancestry, fixed component validation, unique temp, fsync, atomic replacement, and re-open validation. | Suitable storage mechanism, not a reviewer identity or writer authorization. |
| Legacy production-acceptance snapshots | Closed `catdesk_production_acceptance` tool and fixed `.catdesk/production-acceptance/pre.json`/`post.json`, schema 1 in `src/mcp.rs`. | Fixed script/action/hash boundary, bounded JSON, workspace containment, create-new temp, fsync and rename. Compare/pre/post fields bind build/lifecycle/listener continuity only. | Supporting continuity evidence only; cannot express T-0223 pipe/replacement/rollback, literal T-0222 proof, review binding, or T-0152 aggregate result. |
| Existing preflight evaluator | Argument-free `operator core-acceptance preflight` -> `read_fixed_core_host_acceptance_preflight` in `src/core_host_acceptance_preflight.rs`. | The pure ordered engine accepts exact project/session/target-bound inputs only; it exposes `READY`, `BLOCKED_BY_LIVE_ACCEPTANCE`, and `FAILED_DETERMINISTIC_PREREQUISITE`. Production reader intentionally supplies `Missing` for later gates. | Sole acceptance engine to retain. It is a reader, not a writer, and must not be duplicated. |

## Conditional minimum evidence model

The following model is a design contract, not an approved writer or schema in
the current binary. It is the smallest proposed representation if the missing
reviewer receipt authority is separately accepted.

### Fixed ledger

`CoreHostGateLedgerV1` would live only at the compiled root
`C:\\ProgramData\\CatDesk\\CoreHostGateEvidence\\ledger.v1.json`. There is no
workspace, caller-selected, target-selected, or filename argument. Its top
level requires exactly:

```text
schemaVersion = 1
projectId = "catdesk"
t0224SessionId, t0224RecordId, currentTargetSha256
ledgerRevision (positive monotonic u64)
t0223?, t0222?, t0152?
```

Every present gate entry requires a canonical `observationId` computed from
canonical product-derived bytes; positive capture and approval times; the same
project/session/T-0224-record/current-target tuple; a fixed host principal and
nonzero interactive-session identity where relevant; a product-measured build
identity and lifecycle generation where relevant; and a verifier-authenticated
approval receipt. There is no free-form prose, `accepted` boolean,
caller-supplied hash, arbitrary path, or arbitrary command field.

| Entry | Required objective observations | Required approval binding |
| --- | --- | --- |
| `t0223` | Fixed lifecycle status/preflight/activation outcome; exact current/LKG receipt/image identity; one loopback 3201 supervisor and fixed 3200 worker registration; generation/CAS before/after; OS-attested same-principal/nonzero-session pipe result; supported replacement, refusal-preserves-backend, and rollback/recovery result. | Receipt binds gate `T0223`, `observationId`, exact T-0224 session/record/target, current build/generation and capture time. |
| `t0222` | Product-derived interactive nonzero session; exact native window/process/class identity; authoritative designated-target readback; readiness-card/read-only-refresh result; foreground-not-duplicate, close-preserves-daemon, and headless/session-zero refusal results. | Receipt binds gate `T0222`, the exact T-0223 observation id plus same T-0224 tuple, GUI process/session and capture time. A window-only observation is incomplete. |
| `t0152` | Re-read current registry/effective-wake target, exact referenced finalized T-0223/T-0222 observations, canonical T-0224 receipt, fixed production-continuity comparison, and no conflicting ledger state. | Receipt binds gate `T0152`, all three referenced observation ids, same project/session/target, final build/generation and sweep time. |

No entry may be interpreted as acceptance until its approval receipt verifies.
Missing, malformed, unrecognized, partial, duplicate-conflicting, stale,
wrong-target, wrong-session, or wrong-generation input is a reader result of
`Missing`/invalid evidence, never an implicit pass.

## Required writer and reviewer boundary

The proposed writer has two deliberately separate phases:

1. A future closed-world capture action has one fixed operation per gate and
   accepts no evidence, path, hash, target, command, or `accepted` argument.
   It re-reads the fixed lifecycle/GUI/project/wake authorities and stores only
   product-derived *pending* bytes. It refuses if prior gates are absent, the
   current target/session changes during capture, the lifecycle proof is
   partial, the GUI process is not in the observed interactive session, or a
   pending/final record conflicts.
2. A future fixed finalizer may commit a ledger entry only after it reads a
   verifier-authenticated approval receipt from a separately accepted,
   product-owned fixed source. That receipt must be signature- or
   independently-authenticated by a pre-provisioned verifier root; it cannot
   be substituted by an operator assertion or arbitrary uploaded JSON. It
   must bind the canonical pending observation digest and be revalidated
   against current project, canonical inbox T-0224 record/session, effective
   wake target, build, generation, and gate order at commit time.

The existing review inbox cannot fill phase 2. It identifies the session and
review request, but it contains no approval verdict, observation digest,
verifier identity/key, signature, or immutable approval sequence. This is the
single `TRUST_MODEL_BLOCKER`; the ticket deliberately does not choose a new
signer, remote service, human-file writer, or privileged local assertion.

## Conditional storage, atomicity, and reader semantics

If the approval decision is later accepted, the ledger implementation must use
the existing `ProtectedDirectoryGuard` and relative regular-file helpers from
`src/windows_protected_fs.rs` rooted from the compiled ProgramData anchor.
The product-owned writer must create/verify the fixed child chain with the
same protected ACL/owner assumptions as the supervisor installation root; the
reader is read-only and never creates the root.

- Canonical serialization and `schemaVersion == 1`; reject unknown/missing
  fields, noncanonical hashes/ids, oversized records, reparse points, and
  corrupt JSON.
- Write a unique relative temporary file, fsync it, atomically replace only
  `ledger.v1.json`, then reopen and validate it. Pending bytes are never read
  as gate authority. Crash before replace leaves prior ledger authoritative;
  crash after replace is recovered by strict reread. Temp debris is inert.
- The same observation plus identical approval is idempotent. A different
  observation/approval for an already-finalized gate, a lower ledger revision,
  a same revision with different canonical bytes, or T-0222/T-0152 before its
  predecessor is refused before replacement.
- T-0223 is finalized before T-0222; T-0222 before T-0152. Any target,
  session, T-0224 record, build, generation, host-session, or prior-observation
  drift invalidates the pending capture and refuses finalization.
- Ledger absence, stale timestamp, corrupted state, approval expiry/replay,
  conflict, or failed reread maps to later-gate `Missing`/invalid evidence and
  therefore `BLOCKED_BY_LIVE_ACCEPTANCE`, never to `READY`/accepted.

`read_fixed_core_host_acceptance_preflight` remains the sole engine. A future
`read_fixed_core_host_gate_evidence` validates the entire ledger and each
receipt first, re-reads current target/wake and the canonical exact T-0224
record, then converts only fully valid entries to its existing bound-gate
inputs. It must reject wrong project/session/T-0224 record/target/build/
generation, partial runtime proof, GUI window-only proof, duplicate/conflict,
stale/replayed approval, and invalid reviewer identity before conversion.
The pure evaluator still determines ordering and the final
`ALL_CORE_LIVE_GATES_AUTHORITATIVELY_ACCEPTED` result; no ledger reader may
declare it independently.

## Surface constraints and non-chosen designs

Read access remains the existing argument-free `operator core-acceptance
preflight` and its read-only GUI card/refresh. A conditional future capture
surface would expose three named fixed operations with empty input only; a
conditional future finalize surface would poll one fixed protected approval
source and take no approval bytes. Both must be unavailable until the missing
review authority exists.

Rejected designs:

- Reusing a review-bundle, provider session, source test, GUI rendering, queue
  state, or `independent_final_review` string as approval: none binds an exact
  host observation to an independent verifier.
- Reusing production-acceptance pre/post JSON as a ledger: its fixed schema is
  intentionally narrower and cannot represent the later gates.
- An MCP/CLI capture accepting prose, a boolean, a file/path, a hash, a
  command, a screenshot, a target, or a reviewer response: it creates
  caller-selected evidence authority.
- A second evaluator or browser/review engine: it would weaken the accepted
  T-0293/T-0294 single-reader boundary.

## Hostile/fail-closed implementation test matrix

| Case | Required result |
| --- | --- |
| Missing ledger/pending state | Later gate remains `Missing`; preflight names the next live gate. |
| Unknown/corrupt schema, oversized bytes, reparse root, or torn temporary | Read failure/invalid evidence; no acceptance and no repair write from reader. |
| Stale approval, replayed lower revision, or rollback of ledger | Refuse/fail closed; prior valid record is not silently replaced. |
| Wrong project, session, T-0224 record, target, host session, build, or generation | Reject before conversion to bound gate input. |
| T-0222 before T-0223 or T-0152 before T-0222 | Monotonic-order refusal; ledger unchanged. |
| Duplicate identical finalized entry | Idempotent no-op after reread. |
| Duplicate/conflicting observation or approval | Refuse conflict; ledger unchanged. |
| Partial T-0223 3201/3200/pipe/generation/replacement/rollback proof | Pending capture refused; no final entry. |
| GUI window-only, fixture/render-only, wrong session, or no refresh/singleton/headless check | Pending T-0222 capture refused. |
| Source/provider/review-bundle-only or legacy production snapshot alone | Cannot reach capture/finalize; evaluator remains non-accepting. |
| Crash after temp fsync before replace or after replace before response | Pending is inert or ledger rereads strictly; no duplicate acceptance. |
| Current target drift between observation and commit | Finalizer refuses before atomic replace. |
| Missing, malformed, wrong-key, wrong-digest, expired, or duplicate verifier receipt | Finalization refuses; no local assertion fallback. |

## Future work decision

There is no implementation ticket while the one approval-receipt trust decision
is unresolved. The next safe bounded work item is a **trust-decision-only
review**: identify an already accepted independent verifier root/receipt
transport capable of issuing the fixed observation-bound approval receipt, or
explicitly authorize one. It must not mint a key, create a signature, touch
the host, or implement a writer.

Only after that decision may a bounded implementation ticket modify the
following expected surfaces:

- new `src/core_host_gate_evidence.rs` for strict schemas, protected ledger,
  fixed capture/finalize transaction, and read-only validation;
- `src/core_host_acceptance_preflight.rs` to consume only validated bound
  later-gate inputs while retaining the pure evaluator;
- `src/operator_facade.rs` and `src/mcp.rs` for closed no-argument read/capture
  routes, if the accepted authority requires them;
- `src/control_plane_supervisor.rs` / `src/supervisor_lifecycle.rs` and
  `src/windows_gui.rs` only for fixed product-derived observation seams;
- focused module, preflight, MCP-schema, protected-filesystem, lifecycle, and
  GUI hostile tests. Migration is absence-only: no ledger means existing
  `Missing`, and no legacy snapshot conversion is permitted.

## Attribution, verification, and prohibited-action audit

T-0308 attribution is documentation only:

- `CATDESK_MILESTONES.md`
- `.catdesk/current_plan.md`
- `docs/orchestrator/review_bundles/T-0308_T0152_DURABLE_HOST_GATE_AUTHORITY_DESIGN_REVIEW_BUNDLE.md`

The accumulated dirty worktree is preserved; unrelated changes are not
attributed. Documentation-only verification results:

- `cargo fmt --all -- --check` - PASS (only the existing
  `could not canonicalize path C:\\Users\\Volap` warning was emitted).
- `cargo test --workspace --all-targets --all-features core_host_acceptance_preflight`
  - PASS: 9 focused evaluator/read-only/fail-closed tests; 878 unrelated unit
  tests filtered.
- `cargo test --workspace --all-targets --all-features stable_wake_delivery`
  - PASS: 13 focused durable receipt/idempotency/target-drift tests in each
  relevant binary target; unrelated tests filtered.
- `cargo test --workspace --all-targets --all-features windows_protected_fs`
  - PASS: 5 focused pinned-root/rename/caller-unselectable-temp tests in each
  relevant binary target; unrelated tests filtered.
- `cargo test --workspace --all-targets --all-features production_acceptance`
  - PASS: 5 fixed-input/bounded-output/atomic-snapshot tests; 882 unrelated
  unit tests filtered.
- `cargo test --workspace --all-targets --all-features windows_gui` - PASS:
  19 read-only presentation/refresh/non-upgrade tests; 868 unrelated unit
  tests filtered.
- `git diff --check` - PASS; only accumulated working-copy line-ending
  warnings were emitted.

No product source, host evidence ledger, ProgramData state,
supervisor, GUI, browser/wake, target, tunnel, signing/provenance,
dedicated-producer, external project, or Git publication action occurred.

Independent review should confirm the single blocker, reject a local editable
acceptance assertion, preserve the existing evaluator as sole acceptance
engine, and retain the active order **T-0223 -> T-0222/T-0139 -> T-0152 ->
T-0155**.
