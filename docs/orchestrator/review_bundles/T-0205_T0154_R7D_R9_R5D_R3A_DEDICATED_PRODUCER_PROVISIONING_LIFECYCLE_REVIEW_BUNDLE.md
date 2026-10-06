# T-0205 R5D-R3A — dedicated producer provisioning lifecycle

## Verdict: lifecycle model implemented; no live provisioning or isolation success claimed

T-0203's fixed SCM detector still reports the policy service as not
provisioned, and T-0204's real CatDesk-host AppContainer trial was negative
because the isolated helper received Windows `Access is denied` when creating
its fixed output.  This ticket adds the narrow, fixed-policy maintainer
lifecycle model required before a later administrator-gated host provisioning
acceptance.  It performs no live service/account/SCM/system-policy/ACL/
elevation mutation in this provider or in routine CatDesk lifecycle code.

`run_reviewed_build_worker` remains unchanged at the authority boundary:
`require_trusted_final_link_handoff()?` returns
`REVIEWED_BUILD_FINAL_LINK_HANDOFF_REQUIRED` before output, candidate,
attestation, promotion, or recovery authority.

## Fixed policy

The model has no caller-selected identity or pathname. Its sole policy is:

| Field | Fixed value |
| --- | --- |
| Service / virtual-service identity | `CatDeskReviewedProducer` |
| Future service executable | `C:\Program Files\CatDesk\CatDeskReviewedProducer.exe` |
| Producer namespace | `C:\ProgramData\CatDesk\reviewed-producer` |
| Policy generation | `CATDESK_DEDICATED_PRODUCER_V1`, SHA-256-bound |

The later creation-time root and inherited child descriptors must deny the
ordinary interactive token `FILE_WRITE_DATA`, `FILE_APPEND_DATA`,
`FILE_ADD_FILE`, `FILE_ADD_SUBDIRECTORY`, `DELETE`, parent `DELETE_CHILD`,
rename/reparse, `WRITE_DAC`, `WRITE_OWNER`, OWNER RIGHTS recovery, and
take-ownership recovery.  A later live acceptance must inspect—not synthesize
from this model—the service token/SID/owner/integrity/privileges, root/output
owner+DACL+OWNER RIGHTS+mandatory label, exact Job/process membership, stable
FileId, and one-shot retained output handle.

## State machine and administrator boundary

`dedicated_producer_read_only_preflight` is the default entry point. It calls
only a backend `inspect_fixed_state` seam and returns a fixed read-only plan:

```
ABSENT -> ReadOnlyAbsent(fixed policy)
EXACT_PROVISIONED_PENDING_LIVE_ACCEPTANCE -> ReadOnlyExistingPendingAcceptance
PARTIAL / AMBIGUOUS / INTERACTIVE_RECOVERY_POSSIBLE -> REFUSED
```

`execute_dedicated_producer_fixed_plan` accepts only an unchanged
`ReadOnlyAbsent` plan whose policy equals the compiled policy and whose backend
administrator gate returns true. Its sole possible success is
`ReadOnlyExistingPendingAcceptance`; it cannot produce reviewed-build
authority. Provision failure requires a rollback and a second exact inspection
proving `ABSENT`; rollback failure or a non-absent result returns
`REVIEWED_BUILD_DEDICATED_PRODUCER_ROLLBACK_UNPROVEN`.

There is intentionally no production Windows backend for service creation,
configuration, start, account mutation, ACL mutation, or elevation. The only
backend is a deterministic test fake. The source-level regression also rejects
the relevant SCM mutation API names in `reviewed_build.rs` and confirms the
prototype cannot occur before the final-link gate.

## Deterministic tests

| Test | Evidence |
| --- | --- |
| `dedicated_producer_read_only_plan_is_fixed_and_idempotent` | repeated preflight, fixed identity/path/policy, full denied-rights list, zero mutation calls |
| `dedicated_producer_preflight_refuses_ambiguous_partial_and_recoverable_state` | partial, ambiguous, and interactive owner-recovery states fail closed |
| `dedicated_producer_execution_requires_admin_exact_plan_and_proven_rollback` | administrator gate, forged executable path rejection, failed-provision rollback, rollback ambiguity |
| `dedicated_producer_provisioning_model_cannot_authorize_reviewed_build_or_mcp_ownership` | final-link gate ordering; no prototype worker call; no SCM mutation API in this source |
| Existing T-0203 detector/handoff tests | no pre-provisioned service; nonce/SID/replay/handleless claims remain rejected |

The model does not modify routine lifecycle, daemon service ownership, or the
external Secure MCP tunnel. It creates no executable, service, account,
namespace, DACL, SDDL, SCM command, or IPC endpoint in normal execution.

## Later operator-gated live sequence (not performed)

1. An approved administrator invokes a separate fixed-policy execute surface
   after read-only preflight reports only `ABSENT`.
2. That surface creates the fixed service/virtual-service identity and the
   namespace descriptor at creation under pinned handle-relative parents;
   it records exact service configuration and fails closed on any pre-existing
   partial/ambiguous state.
3. A distinct host-security acceptance verifies ordinary-token inability to
   mutate/replace/delete/reparse/take ownership, including parent
   `DELETE_CHILD`, OWNER RIGHTS, and `SeTakeOwnershipPrivilege` routes.
4. Only after a separate reviewed integration ticket proves the exact service
   process/Job, trusted T-0195 toolchain, one-shot handoff, retained output
   handle, and 20 host iterations may production consume such authority.

## Verification

- Focused `cargo test dedicated_producer_ -- --nocapture` — 6 passed.
- `cargo fmt --check` — passed.
- `cargo clippy --all-targets --all-features -- -D warnings` — passed.
- Ordinary `cargo test` — not passed: 654 passed, 1 failed, 21 ignored. The
  only failure remains the pre-existing T-0196 replay fixture before its
  positive control: this provider OS-token profile cannot resolve its trusted
  profile-local Rustup policy and returns
  `REVIEWED_BUILD_STATE_UNAVAILABLE`.
- `git diff --check` is recorded after this bundle update. The workspace is
  broadly untracked, so Git cannot provide a task-only textual diff.

## Changed files

- `src/reviewed_build.rs`
- `docs/orchestrator/review_bundles/T-0205_T0154_R7D_R9_R5D_R3A_DEDICATED_PRODUCER_PROVISIONING_LIFECYCLE_REVIEW_BUNDLE.md`

No live service/account/SCM/system-policy/ACL/elevation mutation, reviewed
build worker, promotion, recovery/reload, Secure MCP/tunnel lifecycle, browser
wake/Scheduler, external-project change, or Git publication was invoked.
