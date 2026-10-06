# T-0211 R5D-R3F — dedicated producer execution integration

## Verdict

Implemented the closed dedicated-producer execution capability and transferred
output-handle acceptance model without invoking a service, provisioning, or a
reviewed build. T-0208's fixed host state remains **NOT_PROVISIONED**. The
reviewed-build worker now performs only the fixed read-only boundary preflight:
on this host it returns `REVIEWED_BUILD_DEDICATED_PRODUCER_NOT_PROVISIONED`;
if the fixed service is present but only pending host acceptance it still
returns `REVIEWED_BUILD_FINAL_LINK_HANDOFF_REQUIRED` before any pathname output
open, candidate, or attestation authority.

## Closed protocol model

`DedicatedProducerExecutionRequestV1` binds exactly one reviewed-build attempt
to a UUID-v4 128-bit nonce, R7C snapshot authority and manifest digests, fixed
build-policy digest, and the exact T-0195 trusted Cargo/Rustc evidence. It has
no service/path/pipe/executable/command/environment input.

`DedicatedProducerPeerEvidenceV1` requires the fixed service-SID digest,
observed process and token identities, fixed pipe ACL digest, isolated-root
stable identity, exact Job membership, and owner/ACL verification. A future
Windows named-pipe/handle-transfer adapter must populate these only from real
service/process/token/pipe/root handles before execution. The model rejects a
missing or mismatched field and expects the parent to retain the nonce it
issued; no peer-provided nonce is authoritative.

`DedicatedProducerTransferredOutputV1` owns an already-open file object. Its
parent acceptance function recomputes SHA-256, byte length, and stable FileId
only through `evidence_from_open_regular` on that handle, compares the result
to the claimed handle evidence, then consumes the one-shot record. It accepts
no path and does not call `open_built_output`. A future accepted implementation
may feed this retained file only into the existing `copy_open_regular_files`
handle-to-handle candidate flow; it may not re-discover output by pathname.

## Fail-closed execution boundary

`require_trusted_final_link_handoff` now inspects only the compiled fixed
service/namespace policy through the existing read-only Windows adapter:

| Fixed state | Worker result |
| --- | --- |
| `ABSENT` | `REVIEWED_BUILD_DEDICATED_PRODUCER_NOT_PROVISIONED` |
| exact service/namespace but no completed host acceptance | `REVIEWED_BUILD_FINAL_LINK_HANDOFF_REQUIRED` |
| partial, reparse, ambiguous, or interactive-recovery state | `REVIEWED_BUILD_DEDICATED_PRODUCER_PROVISIONING_REFUSED` |

The gate remains before `open_built_output`, candidate creation/copy, and
attestation construction. No live IPC server, service start, Cargo execution,
or SCM/account/ACL mutation is called by this ticket.

## Deterministic evidence

| Test | Evidence |
| --- | --- |
| `dedicated_producer_execution_capability_rejects_spoofs_replay_and_path_swaps` | nonce replay, Cargo evidence drift, SID/token/Job/owner-ACL spoof rejection; same-name equal-length output replacement after retained-handle creation; retained evidence remains genuine; outside sentinel unchanged |
| `reviewed_build_worker_fails_closed_before_all_post_cargo_output_authority` | live read-only host preflight reports fixed NOT_PROVISIONED and the gate remains ordered before all post-Cargo pathname/candidate/attestation authority |
| Existing `dedicated_producer_handoff_rejects_spoof_replay_and_handleless_service_claims` | service/SID assertion cannot replace a real handle |
| Existing R9 candidate/replay and reparse tests | handle-relative candidate/copy/evidence/replay and no-follow behavior remain separate and unchanged |

The tests use ordinary temporary files/fakes only. They never construct a
production service host, call the administrator mutation command, run a live
reviewed-build service, modify an account/service/SCM/ACL, or alter an outside
sentinel.

## Remaining administrator and host-acceptance gate

Before a future integration can enable this protocol, an approved administrator
must perform T-0210's one-token fixed provisioning command from clean `ABSENT`
state. A separate host-security acceptance must prove the service token/SID,
pipe peer/ACL, creation-time isolated-root descriptor and owner/OWNER RIGHTS/
mandatory label, ordinary-token hostile attack matrix, Job/process identity,
fixed trusted tools, protected snapshot materialization, retained final output
handle, and at least the required repeated host iterations. Until then the
worker remains fail closed and cannot derive candidate or attestation authority.

## Verification

- Focused `cargo test dedicated_producer_execution_capability -- --nocapture`
  — passed.
- Focused `cargo test reviewed_build_worker_fails_closed_before_all_post_cargo_output_authority -- --nocapture` — passed.
- `cargo fmt --check` and strict all-target/all-feature `cargo clippy` —
  passed.
- Full `cargo test` — 660 passed, 1 failed, 21 ignored. The sole failure is
  the pre-existing T-0196 replay test's provider-token Rustup failure
  (`REVIEWED_BUILD_STATE_UNAVAILABLE`); it is unrelated to this ticket.
- `git diff --check` — passed (the workspace emitted unrelated CRLF warnings).

## Changed files

- `src/reviewed_build.rs`
- `docs/orchestrator/review_bundles/T-0211_T0154_R5D_R3F_DEDICATED_PRODUCER_EXECUTION_INTEGRATION_REVIEW_BUNDLE.md`

No live provisioning, elevation, service/account/SCM/ACL/security mutation,
reviewed-build service execution, promotion, recovery/reload, Git publication,
browser/Scheduler/external-project change, or Secure MCP tunnel action occurred.
