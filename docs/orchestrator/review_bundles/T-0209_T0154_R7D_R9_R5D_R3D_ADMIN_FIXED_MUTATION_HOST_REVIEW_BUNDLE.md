# T-0209 R5D-R3D — administrator fixed mutation host

## Verdict

Implemented the separately scoped Windows fixed-policy mutation host and kept
ordinary/autonomous construction read-only. No live provisioning, service
operation, SCM/ACL/security-policy mutation, elevation, reviewed-build worker,
promotion, recovery, or tunnel operation was invoked by this task.

T-0208's real-host result remains **NOT_PROVISIONED**. This ticket does not
claim READY or dedicated-producer isolation acceptance. The reviewed-build
worker remains fail closed at `REVIEWED_BUILD_FINAL_LINK_HANDOFF_REQUIRED`.

## Fixed authority and host boundary

`DedicatedProducerAdministratorMutationHost` has no caller input. Its sole
operation is `execute_dedicated_producer_fixed_policy_as_administrator()`,
which internally selects only `ExecuteFixedPolicy`. The ordinary
`dedicated_producer_windows_operator_surface` rejects that request with
`REVIEWED_BUILD_DEDICATED_PRODUCER_MUTATION_HOST_UNAVAILABLE`; it can perform
only `ReadOnlyPreflight`.

All mutation authority remains compiled policy:

| Authority | Fixed value |
| --- | --- |
| Service | `CatDeskReviewedProducer` |
| Binary and mode | `C:\Program Files\CatDesk\CatDeskReviewedProducer.exe --catdesk-reviewed-producer-service` |
| Namespace | `C:\ProgramData\CatDesk\reviewed-producer` |
| Service-SID policy | `RESTRICTED_SERVICE_SID` |
| Namespace policy | `OWNER_RIGHTS_DENY_INTERACTIVE_MUTATION_MANDATORY_LABEL` |

There is no public string/path/account/credential/executable/command-line/
SDDL/ACL/SCM/tunnel parameter. The service binary must be a canonical regular,
non-reparse exact fixed file before service creation is attempted.

## Real Windows operations behind the seam

`SystemWindowsDedicatedProducerOperations` now contains the concrete fixed
operations used only by the non-test administrator host:

1. Query the process elevation token with `OpenProcessToken` and
   `GetTokenInformation(TokenElevation)`; non-admin is
   `REVIEWED_BUILD_DEDICATED_PRODUCER_ADMIN_REQUIRED`.
2. Require clean `ABSENT` inspection, then use fixed `OpenSCManagerW` /
   `CreateServiceW`; the service is demand-started and this ticket never calls
   `StartServiceW`.
3. Apply only `SERVICE_SID_TYPE_RESTRICTED` through
   `ChangeServiceConfig2W`, then prove the exact product-owned
   `NT SERVICE\CatDeskReviewedProducer` SID resolves.
4. Create the exact namespace with `CreateDirectoryW` and a descriptor supplied
   in `SECURITY_ATTRIBUTES` at creation time, never by a later DACL rewrite.
   The fixed descriptor uses `SY` ownership, denies `IU` and `OWNER RIGHTS`
   mutation/owner rights, gives the restricted service only its fixed
   directory/file-creation rights, gives the current CatDesk token read/control
   only, and adds high-integrity no-write mandatory-label policy. Conversion or
   post-create owner/DACL/label inspection ambiguity fails closed. Read-only
   preflight also uses `QueryServiceConfig2W` to require the restricted service
   SID policy and `GetFileSecurityW` to reject unreadable owner/DACL/mandatory
   label state.
5. Persist the fixed policy digest plus each completed stage in the fixed
   durable journal. The backend journals `ServiceCreated`,
   `ServiceSidRestricted`, `NamespaceCreated`, and
   `NamespaceSecurityApplied`; failure rolls those stages back in reverse and
   clears the journal only after rollback. A failed rollback returns
   `REVIEWED_BUILD_DEDICATED_PRODUCER_ROLLBACK_UNPROVEN`.

The pending result is explicitly not security acceptance: later host work must
still test real service token/SID, owner+DACL+OWNER RIGHTS+mandatory label,
ordinary interactive write/delete/rename/reparse/parent-`DELETE_CHILD` and
take-ownership recovery, stable object identity, Job/process evidence, and
retained output-handle provenance.

## Deterministic coverage

| Test | Evidence |
| --- | --- |
| `dedicated_producer_execution_requires_admin_exact_plan_and_proven_rollback` | administrator gate, forged fixed-plan refusal, rollback and rollback-unproven cases |
| `windows_dedicated_backend_orders_only_fixed_service_sid_namespace_stages` | exact fixed four-stage order and durable journal stages |
| `windows_dedicated_backend_refuses_hostile_state_admin_bypass_and_rolls_back_in_reverse` | ambiguous-state refusal, non-admin refusal, reverse rollback, cleared durable journal |
| `administrator_fixed_mutation_host_is_no_input_and_unreachable_from_unit_tests` | no-input host shape, ordinary-surface execute refusal, test build's fixed execute entry returns unavailable, real primitive shape exists |
| `dedicated_producer_provisioning_model_cannot_authorize_reviewed_build_or_mcp_ownership` | final-link gate precedes output acquisition; no `StartServiceW` path |

Fakes implement the same operation seam. They are the only backend used by
mutation-state tests; no test invokes the production mutation host.

## Later administrator-run boundary (not performed)

An approved elevated Windows administrator must explicitly invoke the
zero-input `execute_dedicated_producer_fixed_policy_as_administrator` host
from an approved operator integration. It must proceed only when the fixed
read-only preflight is clean `ABSENT`; `PARTIAL`, `AMBIGUOUS`, reparse, drift,
non-admin, or recovery uncertainty is refused. A subsequent independent
host-security acceptance is mandatory before any later reviewed-build
integration can consider a producer boundary. This function is intentionally
not wired to autonomous execution or test execution.

## Verification

- `cargo fmt --check` — passed.
- `cargo test dedicated_producer -- --nocapture` — 6 passed.
- `cargo test administrator_fixed_mutation_host -- --nocapture` — 1 passed.
- `cargo clippy --all-targets --all-features -- -D warnings` — passed.
- Full `cargo test` — 658 passed, 1 failed, 21 ignored. The sole failure is
  the pre-existing T-0196 replay test's provider-token Rustup profile failure
  (`REVIEWED_BUILD_STATE_UNAVAILABLE`); it was not changed here.
- `git diff --check` — passed (the workspace reports unrelated CRLF warnings).

## Changed files

- `src/reviewed_build.rs`
- `docs/orchestrator/review_bundles/T-0209_T0154_R7D_R9_R5D_R3D_ADMIN_FIXED_MUTATION_HOST_REVIEW_BUNDLE.md`

No live provisioning or isolation acceptance is claimed.
