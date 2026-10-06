# T-0213 R5D-R3H — fixed-policy service image deployment and service entry

## Verdict

Implemented the missing fixed-policy image-deployment stage and the exact
closed `--catdesk-reviewed-producer-service` process entry. This task did not
invoke the administrator command, copy an image, write Program Files, create
or configure an SCM service, modify any account/ACL/security policy, or start
a service. The reviewed-build production worker remains fail closed: a
provisioned-but-unaccepted boundary returns
`REVIEWED_BUILD_FINAL_LINK_HANDOFF_REQUIRED`; an absent boundary returns
`REVIEWED_BUILD_DEDICATED_PRODUCER_NOT_PROVISIONED`.

## Smallest missing seams found

T-0205/T-0209 already owned a fixed service binary destination and journaled
service/SID/namespace stages, T-0210 exposed only a zero-parameter
administrator command, and T-0211 defined the bound request/peer/retained-file
handle model. They did not (1) deploy a reviewed CatDesk image to the fixed
service image before SCM creation, or (2) dispatch the compiled service mode
into a closed runtime. T-0213 adds only those two seams.

## Fixed deployment authority

| Authority | Product-owned value |
| --- | --- |
| Reviewed source image | `C:\Program Files\CatDesk\CatDesk.exe` |
| Deployed service image | `C:\Program Files\CatDesk\CatDeskReviewedProducer.exe` |
| Service | `CatDeskReviewedProducer` |
| Service mode | `--catdesk-reviewed-producer-service` |
| Namespace | `C:\ProgramData\CatDesk\reviewed-producer` |
| Policy generation | `CATDESK_DEDICATED_PRODUCER_V3_FIXED_REVIEWED_IMAGE_DEPLOYMENT_BINDING` |

No public API accepts an image, destination, service/account name, command,
environment, SDDL, ACL, SCM option, or path. The Windows mutation adapter uses
only its compiled policy: it validates the fixed source as a canonical regular
non-reparse file, validates the fixed destination parent, refuses an existing
destination, invokes `CopyFileW` with `fail_if_exists`, then revalidates the
fixed destination before fixed `CreateServiceW` configuration. A source that
is absent returns `REVIEWED_BUILD_DEDICATED_PRODUCER_IMAGE_MISSING`; other
copy/image faults return
`REVIEWED_BUILD_DEDICATED_PRODUCER_IMAGE_DEPLOYMENT_FAILED`.

The durable policy digest now hashes the compiled policy generation together
with the exact service name, reviewed source image, destination image, service
mode, namespace, and service-SID policy. This prevents an old or mixed image
plan from comparing equal merely because it carries the same broad version
label. The deterministic exact-plan test independently rejects a forged
reviewed source image before any fake deployment stage can run.

`ImageDeployed` is a durable, first provisioning stage. Failure after it rolls
back in reverse, deleting only the compiled fixed destination through the
existing administrator-only rollback seam. A pre-existing image without the
matching complete service/namespace state is `PARTIAL`, not an idempotent
success.

## Closed service entry and T-0211 boundary

`parse_dedicated_producer_service_args` accepts exactly one token:

```text
--catdesk-reviewed-producer-service
```

Any extra token, option, duplicate, path, command, pipe, or environment text
is rejected. `main` dispatches it before the administrator provisioning mode
and before helper/worker parsing. The production entry verifies its own exact
fixed deployed image and then reaches `DedicatedProducerServiceRuntimeV1`.
That runtime has no general command or request parser; its only future input
is the already-reviewed T-0211 authenticated execution request and retained
output-handle protocol. It deliberately returns
`REVIEWED_BUILD_DEDICATED_PRODUCER_SERVICE_IPC_UNAVAILABLE` pending T-0212
live provisioned-host acceptance; it cannot run Cargo, open built output, or
create candidate/attestation authority.

The flag is absent from MCP and autonomous command surfaces. Tests compile the
service entry without a deployed service and never call live SCM/ACL/image
operations.

## Bounded stage outcomes

The fixed lifecycle now distinguishes non-secret errors:

| Stage | Outcome |
| --- | --- |
| reviewed image absent | `REVIEWED_BUILD_DEDICATED_PRODUCER_IMAGE_MISSING` |
| image validation/copy/destination refusal | `REVIEWED_BUILD_DEDICATED_PRODUCER_IMAGE_DEPLOYMENT_FAILED` |
| service create/configuration | `REVIEWED_BUILD_DEDICATED_PRODUCER_SERVICE_CONFIGURATION_FAILED` |
| restricted service SID | `REVIEWED_BUILD_DEDICATED_PRODUCER_SERVICE_SID_FAILED` |
| namespace or security verification | `REVIEWED_BUILD_DEDICATED_PRODUCER_NAMESPACE_SECURITY_FAILED` |
| reverse rollback uncertainty | `REVIEWED_BUILD_DEDICATED_PRODUCER_ROLLBACK_UNPROVEN` |
| ambiguous/drift/refusal | `REVIEWED_BUILD_DEDICATED_PRODUCER_PROVISIONING_REFUSED` |

## Deterministic coverage

| Test | Evidence |
| --- | --- |
| `dedicated_producer_service_entry_is_exact_fixed_and_non_authoritative` | exact one-token service mode; hostile/mixed input rejection; no MCP/autonomous surface; runtime returns fixed IPC-unavailable and does not open output or spawn a command |
| `dedicated_producer_deployment_stages_return_bounded_redacted_outcomes` | fake-backed failure of every image/service/SID/namespace stage returns its bounded category and clears the durable journal |
| `windows_dedicated_backend_orders_only_fixed_service_sid_namespace_stages` | fixed order now starts `ImageDeployed`, then service/SID/namespace/security, with durable journal identity |
| `windows_dedicated_backend_refuses_hostile_state_admin_bypass_and_rolls_back_in_reverse` | non-admin/ambiguous refusal and reverse rollback includes the image stage |
| `dedicated_producer_execution_capability_rejects_spoofs_replay_and_path_swaps` | T-0211 retained-handle/nonce/SID/replay and same-name swap proof remains intact |
| `dedicated_producer_provisioning_model_cannot_authorize_reviewed_build_or_mcp_ownership` | final-link gate remains before output acquisition |

## T-0212 live acceptance sequence (not performed)

1. An approved Windows administrator runs only the exact zero-parameter
   T-0210 provisioning command from the fixed reviewed CatDesk image.
2. It must observe clean `ABSENT`, deploy the fixed reviewed image, apply the
   exact service/SID/creation-time namespace policy, and prove journaled
   completion. Any partial, existing image, reparse, drift, non-admin, or
   rollback ambiguity is refused.
3. A separately approved host acceptance starts the fixed service mode and
   proves actual service token/SID, pipe ACL/peer identity, Job membership,
   namespace security, interactive-token mutation denial, and the one-shot
   T-0211 retained output-handle transfer.
4. Only after that acceptance can a later integration ticket replace the
   final-link handoff gate. This ticket does not do so.

## Verification

- Focused `cargo test dedicated_producer_ -- --nocapture` — 10 passed.
- Focused `cargo test windows_dedicated_backend -- --nocapture` — 2 passed.
- `cargo fmt --check` — passed.
- `cargo clippy --all-targets --all-features -- -D warnings` — passed.
- Final full `cargo test` — 662 passed, 1 failed, 21 ignored. The sole failure
  is the known provider profile-dependent T-0196 replay test, which returned
  `REVIEWED_BUILD_STATE_UNAVAILABLE`. An earlier full-suite occurrence of the
  existing `adversarial_copy_evidence_and_replay_stay_bound_to_open_handles`
  hook race passed on its focused rerun and on the final full run. No T-0213
  focused deployment/service test failed.
- `git diff --check` — passed; the workspace reported pre-existing CRLF
  conversion warnings.

## Changed files

- `src/reviewed_build.rs`
- `src/main.rs`
- `docs/orchestrator/review_bundles/T-0213_T0154_R7D_R9_R5D_R3H_FIXED_POLICY_DEPLOYMENT_SERVICE_ENTRY_REVIEW_BUNDLE.md`

No live provisioning, elevation, Program Files mutation, service/account/SCM/
ACL/security-policy mutation, service start, reviewed-build worker, promotion,
recovery, browser/Scheduler, external-project, Secure MCP/tunnel, or Git
publication action was performed.
