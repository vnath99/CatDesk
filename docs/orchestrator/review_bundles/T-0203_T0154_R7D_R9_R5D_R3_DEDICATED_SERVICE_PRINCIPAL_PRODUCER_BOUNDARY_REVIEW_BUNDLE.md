# T-0203 R5D-R3 — dedicated service principal producer boundary

## Verdict: PENDING / NEGATIVE

The only fixed read-only detector name is `CatDeskReviewedProducer`. It used
`OpenSCManagerW(SC_MANAGER_CONNECT)` then
`OpenServiceW(SERVICE_QUERY_STATUS | READ_CONTROL)` and returned
`REVIEWED_BUILD_DEDICATED_PRODUCER_NOT_PROVISIONED`. No service, account,
SCM configuration, privilege, or elevation was changed. The production worker
still returns `REVIEWED_BUILD_FINAL_LINK_HANDOFF_REQUIRED` before output,
candidate, or attestation authority.

## Closed future protocol

The test-only `DedicatedProducerHandoffV1` binds buildAttemptId, random nonce,
R7C snapshot-authority digest, fixed policy digest, service-SID digest, and
exact Job membership. It rejects nonce/SID/snapshot/policy/Job spoofing and
replay. A matching record remains
`REVIEWED_BUILD_PRODUCER_HANDLE_NOT_CAPTURED`; a service name or synthetic SID
is not an output handle.

A positive service boundary must use a pre-provisioned noninteractive service
or virtual service SID. It must own a descriptor-at-create root under pinned
handle-relative parents. The interactive user must lack write/add/delete/
rename/reparse, parent `DELETE_CHILD`, `WRITE_DAC`, `WRITE_OWNER`, OWNER
RIGHTS, owner recovery, and `SeTakeOwnershipPrivilege` recovery. Real service
token SID/owner/integrity/privileges, root/output owner+DACL+OWNER RIGHTS+
mandatory label, process/Job identity, and one-shot retained output handle
must all be inspected from real handles before positive authority exists.

## Actual versus pending attacks

| Evidence | Result |
| --- | --- |
| Fixed SCM detector | not provisioned |
| Wrong nonce/service SID/replay/handleless handoff | rejected/not captured |
| R5B retained-handle attacks | supplemental controlled-object evidence only |
| R9 same-length first-output replacement | preserved reproduction; gate required |
| Real service ACL/owner/reparse/write/delete/process/Job/output matrix | pending pre-provisioned identity |

The minimal operator prerequisite is an approved lifecycle that provisions a
distinct restricted service/virtual-service identity and isolated namespace
outside the interactive user's owner/ACL/privilege reach. If that cannot close
owner and delete-child recovery, use a separate account, service, or VM.

## Verification

- Focused dedicated detector/protocol tests — 2 passed.
- `cargo fmt --check` — passed.
- `cargo clippy --all-targets --all-features -- -D warnings` — passed.
- Ordinary `cargo test` — not passed: 649 passed, 1 failed, 20 ignored. The
  only failure remains the pre-existing T-0196 replay fixture before positive
  control because the provider OS-token profile has no profile-local Rustup;
  `trusted_toolchain()` returns `REVIEWED_BUILD_STATE_UNAVAILABLE`.
- `git diff --check` — recorded after this final bundle update; the broadly
  untracked workspace prevents a task-only Git textual diff.

## Changed files

- `src/reviewed_build.rs`
- `docs/orchestrator/review_bundles/T-0203_T0154_R7D_R9_R5D_R3_DEDICATED_SERVICE_PRINCIPAL_PRODUCER_BOUNDARY_REVIEW_BUNDLE.md`

No reviewed-build worker, service/account/SCM mutation, promotion,
reload/recovery, tunnel, wake/browser/Scheduler, or external-project action
was invoked.
