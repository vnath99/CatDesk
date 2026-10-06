# T-0207 R5D-R3B — actual Windows provisioning backend and operator surface

## Verdict: concrete read-only Windows adapter added; autonomous execution performed no provisioning

T-0206 was inert because `WindowsDedicatedProducerProvisioningBackend` was
only a generic adapter over test fakes: no concrete operations object was
constructed, and no first-class operator construction existed. T-0207 adds
`SystemWindowsDedicatedProducerOperations` and the bounded
`dedicated_producer_windows_operator_surface` construction point in
`src/reviewed_build.rs`.

The autonomous test invokes only `ReadOnlyPreflight`. It performs actual
read-only Windows SCM/namespace inspection and does not create, configure,
start, stop, delete, or otherwise mutate a service/account/ACL/root. The
concrete adapter currently refuses `ExecuteFixedPolicy` with the bounded
administrator/action-required category until a separately approved
administrator host binds the fixed mutation primitive and completes the
required real security acceptance. No live provisioning or isolation success
is claimed by this ticket.

## Fixed authorities and concrete read-only operations

All values remain compiled policy, never operator input:

| Authority | Fixed value |
| --- | --- |
| Service | `CatDeskReviewedProducer` |
| Binary/mode | `C:\Program Files\CatDesk\CatDeskReviewedProducer.exe --catdesk-reviewed-producer-service` |
| Namespace | `C:\ProgramData\CatDesk\reviewed-producer` |
| Service SID policy | `RESTRICTED_SERVICE_SID` |
| Namespace intent | `OWNER_RIGHTS_DENY_INTERACTIVE_MUTATION_MANDATORY_LABEL` |

The concrete preflight uses `OpenSCManagerW`, `OpenServiceW`, and
`QueryServiceConfigW` with read-only access to inspect the exact fixed service
and requires the exact fixed binary/mode string. It uses `GetFileAttributesW`
only on the fixed namespace and rejects a reparse point, missing half-state,
or any mismatched configuration as `PARTIAL`/`AMBIGUOUS`.

The first-class request enum has exactly two values:
`ReadOnlyPreflight` and `ExecuteFixedPolicy`; there are no fields for service
names, accounts, credentials, executable/path, SDDL/ACL, SCM configuration,
privileges, or tunnel controls. The concrete construction point defaults to
read-only execution. Its execute methods deliberately fail closed unless an
explicit future administrator-owned fixed mutation primitive is bound; this
prevents this autonomous ticket from accidentally provisioning the host.

## Journal, ambiguity, and acceptance boundary

The product backend journals only the fixed order `ServiceCreated` →
`ServiceSidRestricted` → `NamespaceCreated` → `NamespaceSecurityApplied` and
rolls it back in reverse. Existing partial/ambiguous state, configuration
drift, namespace reparse, or possible interactive-token recovery is refused.
The later live operator must additionally prove from real handles that the
interactive token lacks write/append/add-file/add-subdirectory/delete/rename/
reparse, parent `DELETE_CHILD`, `WRITE_DAC`, `WRITE_OWNER`, OWNER RIGHTS and
take-ownership recovery. Service token/SID, DACL/SACL/mandatory label,
process/Job, stable FileId, and retained output handle remain independent host
acceptance evidence; none is synthesized by this adapter.

`run_reviewed_build_worker` remains blocked by
`REVIEWED_BUILD_FINAL_LINK_HANDOFF_REQUIRED` before output/candidate/
attestation/promotion/recovery authority.

## Tests

| Test | Result |
| --- | --- |
| `concrete_windows_dedicated_adapter_is_read_only_by_default_and_has_no_input_surface` | actual read-only preflight and compile/shape checks; no provisioning |
| `windows_dedicated_backend_orders_only_fixed_service_sid_namespace_stages` | fixed policy and journal ordering via fake |
| `windows_dedicated_backend_refuses_hostile_state_admin_bypass_and_rolls_back_in_reverse` | ambiguity, administrator gate, failure rollback/recovery via fake |
| T-0205 dedicated preflight tests | idempotency, hostile input/state refusal, fixed policy, non-authority |

## Later administrator-run sequence (not performed)

1. Run the exact read-only preflight and proceed only from a clean `ABSENT`
   state.
2. An approved administrator binds the fixed Windows create/configure/service
   SID/creation-time namespace security operations; no parameterized authority
   is permitted.
3. Persist and inspect the reverse rollback journal after every stage.
4. Run host security acceptance for real service identity/root/output and the
   complete interactive-token hostile-operation matrix.
5. Only a later reviewed integration ticket may consume a positive result for
   output-handle provenance.

## Verification

- Focused `cargo test concrete_windows_dedicated_adapter -- --nocapture` — 1
  passed; it performed only read-only SCM/namespace inspection.
- `cargo fmt --check` — passed.
- `cargo clippy --all-targets --all-features -- -D warnings` — passed.
- Ordinary `cargo test` — not passed: 657 passed, 1 failed, 21 ignored. The
  only failure remains the pre-existing T-0196 replay fixture before positive
  control because this provider OS-token profile cannot resolve trusted
  profile-local Rustup and returns `REVIEWED_BUILD_STATE_UNAVAILABLE`.
- `git diff --check` is recorded after this bundle update; the repository is
  broadly untracked, preventing a task-only Git textual diff.

## Changed files

- `src/reviewed_build.rs`
- `docs/orchestrator/review_bundles/T-0207_T0154_R7D_R9_R5D_R3B_ACTUAL_WINDOWS_PROVISIONING_BACKEND_OPERATOR_SURFACE_REVIEW_BUNDLE.md`

No live service/account/SCM/system-policy/ACL/elevation mutation, reviewed
build worker, promotion, recovery/reload, Secure MCP/tunnel lifecycle, browser
wake/Scheduler, external-project action, or Git publication was invoked.
