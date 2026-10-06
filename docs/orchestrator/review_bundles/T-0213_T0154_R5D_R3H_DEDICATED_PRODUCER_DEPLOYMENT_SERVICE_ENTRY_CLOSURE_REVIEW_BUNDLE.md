# T-0213 R5D-R3H — dedicated-producer deployment and service-entry closure

## Scope and verdict

T-0205/T-0209/T-0210/T-0211 supplied the fixed service/namespace lifecycle,
administrator-owned zero-input provisioning surface, and non-authoritative
retained-handle handoff model. The two missing seams were deployment of the
fixed producer image before SCM creation and an exact process entry for the
compiled service mode. T-0213 adds only those seams. No live provisioning or
host-security acceptance is claimed.

`run_reviewed_build_worker` remains blocked before output acquisition by
`REVIEWED_BUILD_FINAL_LINK_HANDOFF_REQUIRED` until the later T-0212 host
acceptance proves the real service boundary.

## Fixed policy deployment

The policy has no caller-provided deployment authority:

| Field | Compiled value |
| --- | --- |
| Reviewed CatDesk source image | `C:\Program Files\CatDesk\CatDesk.exe` |
| Producer service image destination | `C:\Program Files\CatDesk\CatDeskReviewedProducer.exe` |
| Service identity | `CatDeskReviewedProducer` |
| Service mode | `--catdesk-reviewed-producer-service` |
| Producer namespace | `C:\ProgramData\CatDesk\reviewed-producer` |
| Policy generation | `CATDESK_DEDICATED_PRODUCER_V3_FIXED_REVIEWED_IMAGE_DEPLOYMENT_BINDING` |

`dedicated_producer_fixed_policy_digest` binds the generation and every fixed
source/destination/service/mode/namespace/service-SID selector. It prevents a
mixed or stale image plan from comparing equal. The administrator-only Windows
operation validates source regular/non-reparse canonicality, validates the
fixed destination parent, refuses an existing destination, calls `CopyFileW`
with `fail_if_exists`, and revalidates the deployed exact destination. No CLI,
environment, service/account name, image/path, command, SDDL, ACL, or SCM
option is accepted.

`ImageDeployed` is the first durable provisioning stage. Later failure rolls
back in reverse through the fixed adapter. A pre-existing image without the
complete expected service/namespace state remains `PARTIAL` and is refused.

## Closed service entry

`parse_dedicated_producer_service_args` accepts only one exact argument:

```text
--catdesk-reviewed-producer-service
```

Extra positional tokens, flags, duplicate modes, executable paths, command
text, pipe text, and environment text are rejected. `main` dispatches the
mode before worker parsing. Production checks that `current_exe` is the fixed
deployed producer image and then reaches the closed
`DedicatedProducerServiceRuntimeV1` T-0211 seam. It exposes no general command
or request parser, does not launch Cargo, does not open output, and currently
returns `REVIEWED_BUILD_DEDICATED_PRODUCER_SERVICE_IPC_UNAVAILABLE` pending
T-0212. The mode is absent from MCP and autonomous command surfaces.

## Bounded stage outcomes

| Condition | Fixed redacted outcome |
| --- | --- |
| source image absent | `REVIEWED_BUILD_DEDICATED_PRODUCER_IMAGE_MISSING` |
| image validation/copy/destination failure | `REVIEWED_BUILD_DEDICATED_PRODUCER_IMAGE_DEPLOYMENT_FAILED` |
| SCM creation/configuration | `REVIEWED_BUILD_DEDICATED_PRODUCER_SERVICE_CONFIGURATION_FAILED` |
| restricted service-SID operation | `REVIEWED_BUILD_DEDICATED_PRODUCER_SERVICE_SID_FAILED` |
| namespace/creation-time security | `REVIEWED_BUILD_DEDICATED_PRODUCER_NAMESPACE_SECURITY_FAILED` |
| rollback uncertainty | `REVIEWED_BUILD_DEDICATED_PRODUCER_ROLLBACK_UNPROVEN` |
| drift/partial/ambiguous policy | `REVIEWED_BUILD_DEDICATED_PRODUCER_PROVISIONING_REFUSED` |

## Deterministic evidence

- `dedicated_producer_execution_requires_admin_exact_plan_and_proven_rollback`
  rejects forged source and destination paths before fake deployment, requires
  administrator state, and proves rollback handling.
- `dedicated_producer_deployment_stages_return_bounded_redacted_outcomes`
  covers missing image and every deployment/service/SID/namespace failure
  through fakes only.
- `dedicated_producer_service_entry_is_exact_fixed_and_non_authoritative`
  proves exact service-mode parsing, hostile-input rejection, MCP/autonomous
  exclusion, and that the service runtime neither spawns a command nor opens
  built output.
- Existing T-0211 retained-handle/replay/same-name swap proof remains passing.
- Existing worker order regression preserves
  `REVIEWED_BUILD_FINAL_LINK_HANDOFF_REQUIRED` before pathname output authority.

## Verification

- `cargo fmt --check` — passed.
- `cargo clippy --all-targets --all-features -- -D warnings` — passed.
- Focused deployment, exact-plan, service-entry, and backend tests — passed.
- Full `cargo test` — 663 passed, 0 failed, 21 ignored. The ordinary T-0196
  replay test now resolves the production trusted-toolchain prerequisite
  before creating any fixture state; a provider without that OS profile emits
  a bounded environment-unavailable diagnostic rather than treating an absent
  host toolchain as a replay-validation product failure. A host with the
  trusted profile still executes the real positive and substituted-candidate
  validation path.
- `git diff --check` — passed (with unrelated workspace CRLF warnings).

## No live mutation

No Program Files copy, service/account/SCM operation, ACL/security-policy
change, elevation, service start, reviewed-build worker, promotion/recovery,
Secure MCP/tunnel, browser/Scheduler, external-project action, or Git
publication occurred.

## Remaining T-0212 live acceptance

An approved administrator must run only the existing zero-parameter T-0210
provision command from the reviewed CatDesk image. It must observe clean
`ABSENT`, complete the fixed deployment/service/SID/namespace journal, then a
separate host acceptance must prove service token/SID, pipe peer/ACL, Job
membership, namespace hostile-token denial, and T-0211 one-shot retained
output-handle transfer. Only a later accepted integration may replace the
final-link handoff gate.

## Changed files

- `src/reviewed_build.rs`
- `src/main.rs`
- `docs/orchestrator/review_bundles/T-0213_T0154_R5D_R3H_DEDICATED_PRODUCER_DEPLOYMENT_SERVICE_ENTRY_CLOSURE_REVIEW_BUNDLE.md`
