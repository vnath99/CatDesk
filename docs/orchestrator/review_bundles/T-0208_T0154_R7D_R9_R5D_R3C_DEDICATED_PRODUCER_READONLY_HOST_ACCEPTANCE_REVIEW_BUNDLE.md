# T-0208 R5D-R3C — dedicated producer read-only host acceptance

## Real-host read-only result: NOT_PROVISIONED

The production fixed operator surface was invoked with only
`DedicatedProducerOperatorRequest::ReadOnlyPreflight`. Its bounded test-local
result was:

```
R5D-R3C read-only dedicated producer preflight: Ok(NotProvisioned)
```

No `ExecuteFixedPolicy` request was constructed or invoked. No service/account
was created, configured, deleted, started, or stopped; no SCM, ACL, security
policy, namespace, elevation, or system setting was mutated.

## Exact checked authority

The production path was
`dedicated_producer_windows_operator_surface(ReadOnlyPreflight)`, which
constructs `SystemWindowsDedicatedProducerOperations` only. Its compiled
fixed authorities are:

| Field | Fixed authority |
| --- | --- |
| Service | `CatDeskReviewedProducer` |
| Binary/mode | `C:\Program Files\CatDesk\CatDeskReviewedProducer.exe --catdesk-reviewed-producer-service` |
| Namespace | `C:\ProgramData\CatDesk\reviewed-producer` |
| Service SID policy | `RESTRICTED_SERVICE_SID` |
| Namespace-security intent | `OWNER_RIGHTS_DENY_INTERACTIVE_MUTATION_MANDATORY_LABEL` |

Read-only APIs used by that adapter are `OpenSCManagerW`, `OpenServiceW`,
`QueryServiceConfigW`, and `GetFileAttributesW`. The result means the fixed
service and fixed namespace were both absent from the adapter's viewpoint; it
is not a READY/isolation acceptance result. Any half-state, configuration
drift, or namespace reparse would instead remain fail-closed as
`PARTIAL`/`AMBIGUOUS` and could not be treated as READY.

## Input and provenance checks

`DedicatedProducerOperatorRequest` has only `ReadOnlyPreflight` and
`ExecuteFixedPolicy`; it carries no service/account/path/executable/command
line/SDDL/ACL/SCM/tunnel authority. The concrete operator function accepts
only that enum. The focused test asserts the source shape, fixed adapter
construction, real read-only API presence, and absence of string parameters on
the bounded surface.

The reviewed-build worker still calls
`require_trusted_final_link_handoff()?` before `open_built_output`; the result
remains `REVIEWED_BUILD_FINAL_LINK_HANDOFF_REQUIRED`. Preflight cannot
authorize final-link output, candidate, attestation, promotion, recovery, or
Secure MCP ownership.

## Minimal later administrator prerequisite

An approved administrator must run a separate fixed-policy execute surface
from a clean read-only `ABSENT` result. That later operation must provision the
fixed service/virtual-service SID and creation-time secured namespace, journal
every stage and rollback, then perform independent host-security acceptance:
real token/service-SID, descriptor/owner/OWNER RIGHTS/SACL/mandatory-label,
interactive-token hostile operations including parent `DELETE_CHILD` and
take-ownership recovery, Job/process identity, stable FileId, and retained
output-handle provenance. This ticket did not perform those steps.

## Verification

- Focused production read-only command:
  `cargo test concrete_windows_dedicated_adapter_is_read_only_by_default_and_has_no_input_surface -- --nocapture`
  — passed and printed `Ok(NotProvisioned)`.
- `cargo fmt --check` — passed.
- `cargo clippy --all-targets --all-features -- -D warnings` — passed.
- Ordinary `cargo test` — not passed: 657 passed, 1 failed, 21 ignored. The
  only failure remains the pre-existing T-0196 replay fixture before positive
  control because the provider OS-token profile cannot resolve trusted
  profile-local Rustup and returns `REVIEWED_BUILD_STATE_UNAVAILABLE`.
- `git diff --check` is recorded after this bundle update. The repository is
  broadly untracked, preventing a task-only Git textual diff.

## Changed files

- `src/reviewed_build.rs` — test-local bounded preflight outcome output only.
- `docs/orchestrator/review_bundles/T-0208_T0154_R7D_R9_R5D_R3C_DEDICATED_PRODUCER_READONLY_HOST_ACCEPTANCE_REVIEW_BUNDLE.md`

No live provisioning, isolation acceptance, reviewed-build worker, promotion,
recovery/reload, Secure MCP/tunnel lifecycle, browser wake/Scheduler,
external-project action, or Git publication was invoked.
