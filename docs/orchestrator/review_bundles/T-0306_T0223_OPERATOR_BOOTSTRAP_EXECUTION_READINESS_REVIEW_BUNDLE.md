# T-0306 / T-0223 Operator Bootstrap Execution Readiness Review Bundle

## Classification

**`TRUSTED_CANDIDATE_PREREQUISITE_MISSING`**.

No exact current T-0299-capable reviewed candidate and no matching protected
promotion authorization are available. Therefore no promotion `Execute` step,
raw daemon reload, or substitute operator command is stated or authorized by
this ticket. This is not a deterministic source defect: the current chain
fails closed exactly because its upstream durable authority is absent.

## Reconfirmed live boundary

- The live daemon is pre-T-0299; the three fixed stable-supervisor lifecycle
  tools are absent from live discovery.
- The host-observed legacy response to a `REVIEWED_BUILD_PREPARE` compatibility
  request was `buildPath is required`. It proves the old daemon recognizes
  only containment/hash-oriented legacy reload semantics, not current reviewed
  candidate provenance.
- T-0301 recorded no `.catdesk/reviewed-build`, `.catdesk/promotion-control`,
  or `target/reviewed-builds` state. T-0306 readback confirms all three remain
  absent. `.catdesk/reviewed-source-snapshots` exists but has no matching
  committed T-0299 candidate chain.
- The T-0304-R3 isolated release build remains release-equivalent verification
  evidence only. It is not a reviewed image, candidate, attestation, promotion
  authorization, deployment authority, or permissible bootstrap input.

## Authority graph and availability matrix

| Edge | Current authority supplier | What it binds | Availability now |
| --- | --- | --- | --- |
| Approved task outputs -> independent review authority | `AutonomousSupervisorV1::resolve_reviewed_promotion_review_authority` in `src/delegated/autonomy_supervisor.rs` | One CatDesk completed-verified, unread=false independent-final-review record; contract hash, exact completion artifacts and baseline/current hashes; produces a source-snapshot expectation. | Historical accepted T-0299 review does not by itself supply a current committed candidate chain; no usable current authority is proven. |
| Review authority -> immutable source snapshot | `src/reviewed_source_snapshot.rs` and `create_or_validate_reviewed_source_snapshot`/`validate_committed_snapshot` | Session/project/contract/task/artifact observations and immutable source bytes. | Snapshot root exists, but no matching T-0299 candidate chain is present. |
| Snapshot + review -> reviewed build attempt | `catdesk_reviewed_build` PREPARE/CONFIRM/RESULT in `src/delegated/autonomy_supervisor.rs`; `src/reviewed_build.rs` | Closed fixed Cargo/Rustc policy, tool identities, one attempt/claim/owner, and candidate path `target/reviewed-builds/<attempt>/catdesk.exe`. | Not live on the old daemon; `.catdesk/reviewed-build` and candidate directory are absent. |
| Attempt/claim/result -> producer attestation | `validate_producer_attestation` in `src/reviewed_build.rs` | Exact candidate hash/identity/length, review digest, snapshot identities, policy and Cargo/Rustc fingerprints. | Absent: no control result/attestation or candidate exists. |
| Attestation + review -> protected promotion preflight | `prepare_reviewed_promotion` in `src/daemon_reload.rs` through `catdesk_reviewed_build_promotion` PREFLIGHT | Candidate path/hash, prior canonical hash, promotion script and trusted-PowerShell identities, review/snapshot/attestation/toolchain bindings, expiry, one confirmation token, authorization/transaction/claim IDs. | Absent: `.catdesk/promotion-control/preflight.json` cannot be created without the preceding chain. |
| Preflight -> one protected authorization/claim/worker | `confirm_reviewed_promotion` and `run_reviewed_promotion_worker` in `src/daemon_reload.rs` | Fresh token, full revalidation, create-once claim/owner, fixed worker arguments, bounded terminal result. | Absent; no valid preflight or candidate exists. |
| Authorization -> fixed promotion phase 1/execute | `scripts/promote-reviewed-catdesk-build.ps1` invoked only via `operator_facade::reviewed_promotion_invocation` | Workspace-contained regular candidate, exact authorization/transaction, candidate handoff, canonical pair change, and a no-choice invocation. Direct `-Execute` lacks authority and returns `REVIEWED_PROMOTION_AUTHORIZATION_REQUIRED`. | Script and recovery helper exist, but are not authority and must not be called directly. |
| Promotion -> canonical/LKG or rollback | Promotion transaction and `scripts/catdesk-release-recovery.ps1` | Prior backup/hash, schema-2 transaction authorization binding, canonical pair validation, exact restoration or `ROLLBACK_UNPROVEN_OPERATOR_ATTENTION`. | Recovery capability exists; no current promotion transaction is eligible. |
| Promoted runtime -> reconnect/discovery | Existing CatDesk MCP transport; current source returns `none-external-tunnel-untouched`. | Reconnect to the externally owned official transport, rediscovery of the three T-0299 lifecycle tools, then empty-input status/preflight. | Cannot occur until a reviewed candidate is independently produced and promoted. No tunnel action is authorized. |

## Exact absent prerequisite and accepted future mechanism

The missing upstream artifact is one **current independently reviewed
T-0299-capable candidate chain**: a valid CatDesk review authority, matching
committed reviewed-source snapshot, one fixed-worker build attempt/claim/result,
the immutable producer attestation for its fixed candidate path/hash, and the
fresh protected promotion preflight/authorization derived from that exact
chain. The individual files are intentionally insufficient in isolation.

The accepted future mechanism is the existing approved reviewed-image release
procedure, not the old daemon's reload API and not a generic build/copy/launch.
It must produce that candidate through the closed reviewed-build worker and
attestation path. Only then can the current promotion surface issue its
time-limited preflight and confirmation authority. T-0306 neither creates nor
selects any part of that chain and does not reopen signing, provenance, or
dedicated-producer work.

## Promotion and recovery semantics once the prerequisite exists

This is a capability map, not an execution instruction for the current state:

1. The future reviewed runtime can use the first-class reviewed-build
   PREPARE/CONFIRM/RESULT surface to establish the immutable candidate chain.
2. The first-class promotion PREFLIGHT revalidates review, snapshot,
   attestation, candidate, canonical binary, promotion script, and trusted
   PowerShell identity, then yields a short-lived opaque confirmation token.
3. CONFIRM revalidates those bindings, writes the protected authorization and
   create-once claim, and schedules only the fixed-purpose worker. Accepted
   public outcomes are scheduling/pending/completed or bounded ambiguous
   failure; secrets and arbitrary paths/commands are not exposed.
4. The worker invokes the fixed promotion script with the Rust-issued
   authorization ID and transaction ID. The script proves phase-1 handoff,
   protects the prior canonical pair, records a transaction, swaps binary plus
   manifest, proves handback, records reviewed promotion/LKG only after proof,
   or restores the exact prior pair.
5. Interrupted, stale, mismatched, missing-authorization, or rollback-ambiguous
   state fails closed. `ROLLBACK_UNPROVEN_OPERATOR_ATTENTION` is an attention
   state, never a reason to run a script waterfall.
6. Only after `PROMOTION_COMPLETED` and ordinary transport reconnect may the
   three T-0299 tools be rediscovered. The next host ticket calls only
   `catdesk_stable_supervisor_status` and
   `catdesk_stable_supervisor_preflight` with `{}`; activation remains outside
   T-0306 and requires separate authority.

## Why no current operator command is emitted

`BOOTSTRAP_OPERATOR_COMMAND_READY` requires the current candidate identity and
protected authorization source. Both are absent. Naming `target/release`, the
T-0304-R3 verification target, a manually built image, a raw legacy reload,
SCM/Task Scheduler, or copy/launch command would self-bless bytes and violate
the accepted provenance boundary. The correct current operator boundary is to
make the approved reviewed-image release mechanism produce the missing chain;
there is no safe substitute command in this repository state.

## Source decision, attribution, and prohibited-action audit

No source change is warranted. The observed gap is missing upstream durable
authority plus old-daemon capability age, not a defect in the reviewed-build or
promotion integration. T-0306 attribution is documentation only:

- `CATDESK_MILESTONES.md`
- `.catdesk/current_plan.md`
- `docs/orchestrator/review_bundles/T-0306_T0223_OPERATOR_BOOTSTRAP_EXECUTION_READINESS_REVIEW_BUNDLE.md`

No candidate was minted; no reviewed build, promotion, authorization, daemon
reload, supervisor activation, recovery, browser/wake/target/profile action,
Secure MCP/tunnel mutation, signing/provenance/dedicated-producer work,
external-project operation, or Git publication occurred.

## Verification and independent-review checklist

Documentation-only verification results:

- `cargo fmt --all -- --check` — PASS.
- `cargo test --workspace --all-targets --all-features reviewed_build` — PASS
  (66 passed; 3 fixed-host tests ignored).
- `cargo test --workspace --all-targets --all-features reviewed_promotion` —
  PASS (3 tests).
- `cargo test --workspace --all-targets --all-features stable_supervisor` —
  PASS (7 tests).
- `git diff --check` — PASS (only pre-existing working-copy line-ending
  warnings were emitted).

Independent review should confirm:

- `TRUSTED_CANDIDATE_PREREQUISITE_MISSING` is the sole classification;
- the absent candidate/attestation/preflight/authorization records are not
  replaced with historical or verification-only evidence;
- promotion/recovery remains authorization- and rollback-bound;
- T-0305 is accepted readiness-only, T-0155 remains unstarted, and the order
  **T-0223 -> T-0222/T-0139 -> T-0152 -> T-0155** is unchanged;
- no unrelated dirty-worktree changes are attributed.
