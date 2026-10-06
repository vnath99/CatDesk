# T-0201 R5D-R1 — real AppContainer capability-root feasibility

## Verdict: NEGATIVE

T-0200's synthetic capability model was not OS-isolation proof. R5D-R1 added
a genuine Windows lowbox launch attempt, but this workspace returned exact
`REVIEWED_BUILD_APPCONTAINER_LAUNCH_UNAVAILABLE` before a fixed helper could
execute. There is therefore no real AppContainer producer root, no helper
output, no retained output handle, and no basis for candidate/attestation
authority. The production worker still stops at
`REVIEWED_BUILD_FINAL_LINK_HANDOFF_REQUIRED`.

## Genuine Windows APIs attempted

The private `cfg(test)` attempt creates a fresh named AppContainer profile by
`CreateAppContainerProfile`, receives the real AppContainer SID, constructs
`SECURITY_CAPABILITIES`, allocates and updates a real process attribute list
with `InitializeProcThreadAttributeList` and
`UpdateProcThreadAttribute(PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES)`, then
calls `CreateProcessW` for only the current CatDesk test executable and fixed
`--help` argv with `EXTENDED_STARTUPINFO_PRESENT | CREATE_SUSPENDED`.

No PATH, caller executable, shell, environment-selected identity, synthetic
SID, or arbitrary helper is used. If creation succeeded, the process would be
assigned to the existing kill-on-close `BuildJob` before `ResumeThread`,
checked with `IsProcessInJob`, and inspected through `OpenProcessToken` and
`GetTokenInformation(TokenIsAppContainer)` before execution. Handles, profile
SID, and profile are cleaned with `CloseHandle`, `FreeSid`, and
`DeleteAppContainerProfile` on every path.

On this host the attempt failed at profile/attribute/process launch and
returned the bounded non-secret launch category above. It did not reach token
inspection, Job assignment, helper write, root creation, or output acquisition.
This is a real API outcome, not an expected-access calculation.

## Required root boundary not established

A positive root would need atomic RootDirectory/no-follow creation beneath a
pinned R7C parent with its complete security descriptor supplied at creation.
The ordinary interactive SID must lack `FILE_WRITE_DATA`,
`FILE_APPEND_DATA`, `FILE_ADD_FILE`, `FILE_ADD_SUBDIRECTORY`, delete/rename,
`WRITE_DAC`, and `WRITE_OWNER`; neither OWNER RIGHTS nor
`SeTakeOwnershipPrivilege` may restore them. Parent `DELETE_CHILD` and rename
authority must also be closed by the descriptor and pinned no-delete-share
handles. Mandatory label/integrity, producer token user/owner SIDs,
TokenCapabilities, enabled privileges, root/output owner/DACL/SACL, and all
post-attack values must be read from real handles.

Because the real lowbox helper did not launch, R5D-R1 does not create an
ordinary root and harden it later, does not fake ACL evidence, and does not
claim hostile writable-open/create/replacement/rename/delete/reparse/DACL/
owner/takeover attacks were covered by an AppContainer boundary. Existing R7C
and R5B controlled-handle tests remain only supplemental evidence.

## Existing hostile and capability evidence retained

| Evidence | Actual result | Scope |
| --- | --- | --- |
| Real lowbox launch attempt | `REVIEWED_BUILD_APPCONTAINER_LAUNCH_UNAVAILABLE` | negative precondition |
| Normal token owner diagnostic | owner-reacquirable or unproven | rejects ordinary token as isolation root |
| Synthetic capability replay/SID tests | rejected | protocol model only, not AppContainer proof |
| R5B retained R7C object attacks | write/rename/delete/replacement denied | controlled same-process object only |
| T-0193 same-length pre-first-open swap | preserved and succeeds without gate | proves why production remains gated |
| R7C reparse tests | no-follow rejection where live creation is permitted | not an AppContainer root proof |

There are zero positive iterations, not 20. A later host run may execute the
same focused test and must demonstrate a real lowbox token, atomically
secured root/output ACL/owner/label evidence, all hostile operations, and 20
zero-miss retained-handle iterations before any positive verdict.

## Production-gate regression

`os_isolation_prototype_cannot_activate_before_the_final_link_handoff_gate`
continues to assert the test-only isolation code is absent from the production
worker before `open_built_output`; `REVIEWED_BUILD_FINAL_LINK_HANDOFF_REQUIRED`
still precedes all output/candidate/attestation authority.

## Next stronger architecture

The concrete blocker is inability to create/launch the required AppContainer
helper in this environment. The next design must use a separately provisioned
service/dedicated local account or VM boundary if a real lowbox cannot be made
available. It must own the producer namespace outside the ordinary user's
owner/ACL/privilege reach, then transfer the exact output kernel handle once
while isolation remains enforced. Do not substitute a same-user DACL, timing,
or a later pathname open.

## Verification

- Focused real AppContainer lowbox test — passed with the explicit negative
  launch category above.
- The focused pre-existing handle/replay adversarial test also passed. The
  shared test mutex now recovers from a prior panic rather than poisoning
  unrelated adversarial tests; this is test-harness-only.
- `cargo fmt --check` — passed.
- `cargo clippy --all-targets --all-features -- -D warnings` — passed.
- Ordinary `cargo test` — not passed: 646 passed, 1 failed, 20 ignored. The
  sole failure is the pre-existing T-0196 replay fixture before positive
  control because this provider's OS-token profile lacks profile-local Rustup
  and `trusted_toolchain()` returns `REVIEWED_BUILD_STATE_UNAVAILABLE`.
- `git diff --check` — recorded after this final bundle update; the workspace
  has a broad untracked set, so Git cannot provide a task-only textual diff.

## Changed files

- `src/reviewed_build.rs`
- `docs/orchestrator/review_bundles/T-0201_T0154_R7D_R9_R5D_R1_REAL_APPCONTAINER_CAPABILITY_ROOT_FEASIBILITY_REVIEW_BUNDLE.md`

No reviewed-build worker, Cargo build, promotion, reload/recovery, tunnel,
browser/wake/Scheduler, or external-project action was invoked.
