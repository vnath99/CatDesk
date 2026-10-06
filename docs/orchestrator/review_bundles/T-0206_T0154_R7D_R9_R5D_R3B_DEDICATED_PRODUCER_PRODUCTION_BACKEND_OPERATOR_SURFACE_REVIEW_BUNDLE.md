# T-0206 R5D-R3B — dedicated producer backend and operator surface

## Verdict: fixed-policy backend model implemented; no live provisioning or isolation acceptance claimed

T-0203 established that the fixed `CatDeskReviewedProducer` service is not
provisioned. T-0204's actual CatDesk-host AppContainer helper experiment was
negative (`Access is denied` during fixed output creation). T-0205 supplied a
read-only lifecycle model. This ticket adds the production-shaped, injectable
Windows provisioning backend and its zero-input operator request surface. No
real SCM, service, account, ACL, SDDL, root, privilege, or elevation mutation
was performed by this provider or its tests.

The reviewed-build worker remains unchanged: it stops at
`REVIEWED_BUILD_FINAL_LINK_HANDOFF_REQUIRED` before `open_built_output`, so the
backend, its journal, and fakes cannot authorize a candidate, attestation,
promotion, recovery, or successful reviewed build.

## Product-owned fixed policy

| Authority | Compiled fixed value |
| --- | --- |
| Service identity | `CatDeskReviewedProducer` |
| Service binary | `C:\Program Files\CatDesk\CatDeskReviewedProducer.exe` |
| Service mode | `--catdesk-reviewed-producer-service` |
| Namespace | `C:\ProgramData\CatDesk\reviewed-producer` |
| Service-SID policy | `RESTRICTED_SERVICE_SID` |
| Namespace intent | `OWNER_RIGHTS_DENY_INTERACTIVE_MUTATION_MANDATORY_LABEL` |
| Policy generation | `CATDESK_DEDICATED_PRODUCER_V1`, SHA-256-bound |

No public request has fields for service/account name, executable, command
line, credentials, namespace, SDDL/ACL, SCM option, token privilege, or tunnel
control. `DedicatedProducerOperatorRequest` has only `ReadOnlyPreflight` and
`ExecuteFixedPolicy`.

## Backend, journal, and rollback

`WindowsDedicatedProducerProvisioningBackend<O>` is the product backend over
the narrow `DedicatedProducerWindowsOperations` adapter. The adapter receives
only the compiled policy. A future administrator-approved Windows adapter must
implement the following journaled sequence:

1. inspect exact existing fixed state;
2. create exact fixed service;
3. restrict its exact service SID;
4. create the exact product-owned namespace;
5. apply exact namespace security at creation; and
6. prove the interactive token cannot regain namespace authority.

The journal records `ServiceCreated`, `ServiceSidRestricted`,
`NamespaceCreated`, and `NamespaceSecurityApplied` only after each operation
succeeds. A later failure rolls stages back in reverse order. Existing partial,
ambiguous, or interactive-owner-recovery state is refused. Failed provisioning
requires rollback and a second fixed inspection proving `ABSENT`; otherwise the
bounded result is `REVIEWED_BUILD_DEDICATED_PRODUCER_ROLLBACK_UNPROVEN`.

The actual host acceptance must still prove denial of interactive write/append/
add-file/add-subdirectory/delete/rename/reparse, parent `DELETE_CHILD`,
`WRITE_DAC`, `WRITE_OWNER`, OWNER RIGHTS recovery, and take-ownership recovery.
It must derive service token, object DACL/SACL/mandatory label, Job/process,
FileId, and retained output-handle evidence from real handles—not this state
machine.

## Deterministic non-mutating tests

| Test | Evidence |
| --- | --- |
| `windows_dedicated_backend_orders_only_fixed_service_sid_namespace_stages` | read-only outcome, explicit execute stage order, fixed policy supplied at every seam, journal order |
| `windows_dedicated_backend_refuses_hostile_state_admin_bypass_and_rolls_back_in_reverse` | ambiguous-state refusal, administrator gate, namespace-stage failure, reverse rollback, restored absent state |
| T-0205 preflight tests | fixed plan/idempotency, forged executable rejection, partial/ambiguous/owner-recovery refusal |
| `dedicated_producer_provisioning_model_cannot_authorize_reviewed_build_or_mcp_ownership` | final-link gate precedes output and no SCM mutation API is present in reviewed-build source |

The fake adapter records calls but never opens SCM, creates a service/account,
writes an ACL, changes a namespace, elevates, or controls the external Secure
MCP tunnel. Routine CatDesk lifecycle has no construction/call site for this
backend.

## Later administrator-run sequence (not performed)

1. Run the read-only fixed preflight; proceed only from exact `ABSENT`.
2. An approved administrator separately binds a real Windows operations
   adapter to the fixed execute request; no arbitrary arguments are accepted.
3. Reinspect fixed SCM/service-SID/namespace configuration and journal each
   exact operation; roll back or stop on every ambiguity.
4. Run independent host security acceptance against real service token and
   root/output handles, including hostile interactive-token attack matrix and
   retained-handle evidence.
5. A later reviewed integration ticket may consume that result only after the
   complete service boundary and continuous output provenance have passed.

## Verification

- Focused `cargo test windows_dedicated_backend -- --nocapture` — 2 passed.
- `cargo fmt --check` — passed.
- `cargo clippy --all-targets --all-features -- -D warnings` — passed.
- Ordinary `cargo test` — not passed: 656 passed, 1 failed, 21 ignored. The
  only failure remains the pre-existing T-0196 replay fixture before its
  positive control because this provider's OS-token profile cannot resolve its
  trusted profile-local Rustup policy and returns
  `REVIEWED_BUILD_STATE_UNAVAILABLE`.
- `git diff --check` is recorded after this bundle update. The repository is
  broadly untracked, preventing a task-only Git textual diff.

## Changed files

- `src/reviewed_build.rs`
- `docs/orchestrator/review_bundles/T-0206_T0154_R7D_R9_R5D_R3B_DEDICATED_PRODUCER_PRODUCTION_BACKEND_OPERATOR_SURFACE_REVIEW_BUNDLE.md`

No reviewed-build worker, live service/account/SCM/ACL/namespace mutation,
elevation, promotion, recovery/reload, Secure MCP/tunnel lifecycle, browser
wake/Scheduler, external-project action, or Git publication was invoked.
