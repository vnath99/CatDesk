# T-0204 R5D-R2A — actual-host AppContainer secured-root proof

## Verdict: ENVIRONMENT_UNAVAILABLE (provider); CatDesk-host proof pending

This is not a positive isolation result and does not close T-0193/T-0154.
The production reviewed-build worker still calls
`require_trusted_final_link_handoff()?` before `open_built_output`, and that
gate still returns `REVIEWED_BUILD_FINAL_LINK_HANDOFF_REQUIRED`.  No candidate,
attestation, promotion, recovery, or worker lifecycle was enabled.

The provider run reached the genuine `CreateAppContainerProfile` path and
returned `REVIEWED_BUILD_APPCONTAINER_LAUNCH_UNAVAILABLE` before a lowbox was
created.  This is an environment availability result, not evidence that the
CatDesk host's already-observed lowbox capability is unsafe or safe.  The
ordinary host test path is now ready to perform the actual fixed helper and
hostile-operation matrix on a compatible CatDesk Windows host.

## Changes and exact fixed helper action

- `src/reviewed_build.rs` adds the ignored child-only test
  `appcontainer_fixed_helper_writes_only_fixed_output_child`.  It accepts no
  command, executable, path, or payload input.  Its sole action is a
  create-new, sync'd write of the fixed `catdesk-appcontainer-output.bin`
  child in its process current directory.
- The parent launches only the current test executable with the fixed ignored
  test selector, `CREATE_SUSPENDED`, and
  `PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES`; its current directory is the
  pinned prototype producer root.  The child is assigned to the existing
  kill-on-close `BuildJob` and checked with `IsProcessInJob` before resume.
- `src/reviewed_source_snapshot.rs` adds a Windows-test-only
  `ProtectedDirectoryGuard::create_child_with_security_descriptor`.  It passes
  an SDDL-converted descriptor as `OBJECT_ATTRIBUTES.SecurityDescriptor` to
  the existing exact-child `NtCreateFile` call under the pinned
  `RootDirectory`; it is not a post-create ACL hardening step and is absent
  from non-test production authority.

## Creation-time policy and retained objects

The test creates a fresh parent, pins it, then creates `producer-root` as one
exact child with its descriptor at creation.  The descriptor has:

- an `OWNER RIGHTS` deny for write/append/EA/attribute writes, directory child
  creation, delete, `WRITE_DAC`, and `WRITE_OWNER`;
- only read/control visibility for the ordinary current-user SID;
- the real AppContainer package SID returned by
  `CreateAppContainerProfile` (not a synthetic SID string) granted the fixed
  helper's write/create mapping; and
- a low mandatory label with no-write-up policy.

The pinned parent/root handles are opened without delete sharing.  The parent
checks the real child token's `TokenIsAppContainer` value and exact
`TokenAppContainerSid` equality before resume.  If the helper produces the
fixed child, CatDesk opens it only through `open_relative_regular_file` under
the retained root, then obtains length, SHA-256, and stable volume/FileId only
from that exact handle through `evidence_from_open_regular`.

The current code returns `REVIEWED_BUILD_APPCONTAINER_SECURITY_UNPROVEN` even
after those steps until a complete real handle-derived object-security readback
(owner, DACL including OWNER RIGHTS, mandatory label, token owner/integrity and
privilege state) is positively established.  Thus a helper write can never
become provenance authority by itself.

## Actual hostile matrix when lowbox launch succeeds

`appcontainer_interactive_attack_matrix` runs after the fixed output handle is
retained.  It requires the ordinary token to be denied for each of:

| Attempt | Required outcome |
| --- | --- |
| Writable open and same-object write route | denied |
| Extra child and subdirectory creation | denied |
| Output delete/rename and root rename | denied by ACL and/or retained no-delete handle |
| Same-name, same-length outside attacker replacement | denied |
| Reparse/symlink child insertion | denied or unavailable under token policy; never accepted |
| `WRITE_DAC` and `WRITE_OWNER` opens on root | denied |
| Outside sentinel | exact bytes unchanged before and after |
| Retained output evidence after attacks | exact SHA/length/stable FileId unchanged |

A successful hostile operation, changed retained evidence, or changed outside
sentinel returns `REVIEWED_BUILD_APPCONTAINER_SECURITY_UNPROVEN`.  The provider
did not reach this matrix because lowbox launch was unavailable; it therefore
does not claim live reparse, owner-rights, privilege, or ACL success/denial.

## Process, replay, and production-gate regression

The ordinary static test
`appcontainer_fixed_helper_and_secured_root_remain_test_only_non_authority`
asserts that the helper is ignored outside the lowbox harness, the prototype is
`cfg(all(test, windows))`, and neither prototype function is reachable before
the final-link gate or output acquisition in `run_reviewed_build_worker`.
Existing one-shot capability/replay and final-link-gate tests remain unchanged.

If one complete host iteration reaches the matrix without ambiguity, the same
ordinary fixed test path can be repeated by a separate host acceptance run for
20 consecutive iterations.  No repetition was claimed or simulated here.
Any real host failure in owner rights, `WRITE_DAC`, `WRITE_OWNER`, parent
delete-child, reparse, output identity, or handle capture must be recorded as
negative and should use the T-0203 dedicated-service-principal contingency.

## Verification

- Focused `cargo test appcontainer_ -- --nocapture` — 3 passed, 1 intentionally
  ignored fixed helper; both real lowbox tests returned the explicit provider
  `REVIEWED_BUILD_APPCONTAINER_LAUNCH_UNAVAILABLE` category.
- Explicit fixed helper invocation with `--ignored` — passed; generated test
  output was removed immediately from the workspace after that isolated check.
- `cargo fmt --check` — passed.
- `cargo clippy --all-targets --all-features -- -D warnings` — passed.
- Ordinary `cargo test` — not passed: 650 passed, 1 failed, 21 ignored.  The
  only failure remains the pre-existing T-0196 replay fixture before its
  positive control because this provider OS-token profile cannot resolve the
  fixed profile-local Rustup policy; it returns
  `REVIEWED_BUILD_STATE_UNAVAILABLE`.
- `git diff --check` is recorded after this bundle update.  The repository is
  broadly untracked, so Git cannot provide a task-only textual diff.

## Changed files

- `src/reviewed_build.rs`
- `src/reviewed_source_snapshot.rs`
- `docs/orchestrator/review_bundles/T-0204_T0154_R7D_R9_R5D_R2A_ACTUAL_HOST_APPCONTAINER_SECURED_ROOT_PROOF_REVIEW_BUNDLE.md`

No reviewed-build worker, Cargo build, service/account provisioning, promotion,
reload/recovery, tunnel, browser/wake/Scheduler, or external-project action was
invoked.

## Independent CatDesk-host acceptance — 2026-08-21

The provider verdict above was followed by an independent run on the actual
CatDesk Windows host using the fixed bounded command:
`cargo test appcontainer_ -- --nocapture`.

The host reached the genuine lowbox/helper path, but both child helper attempts
failed at the fixed create-new output operation with Windows error 5
(`Access is denied`). The two parent proof paths therefore reported:

- `R5D-R1 AppContainer feasibility result: REVIEWED_BUILD_APPCONTAINER_OUTPUT_UNPROVEN`
- `R5D-R2 secured-root host proof result: REVIEWED_BUILD_APPCONTAINER_OUTPUT_UNPROVEN`

This converts the T-0204 host-feasibility conclusion from provider-only
`ENVIRONMENT_UNAVAILABLE` to **NEGATIVE on the actual CatDesk host**. The
creation-time namespace policy does not currently permit the fixed AppContainer
producer to create its intended output, so no positive producer authority or
20-iteration claim exists. The production reviewed-build path remains
fail-closed at `REVIEWED_BUILD_FINAL_LINK_HANDOFF_REQUIRED`.

Per the predeclared T-0204 decision rule, the next architecture path is the
reviewed T-0203 dedicated restricted service/virtual-service principal
contingency. This acceptance did not provision a service/account, mutate SCM,
reload/recover CatDesk, or touch the external Secure MCP tunnel.
