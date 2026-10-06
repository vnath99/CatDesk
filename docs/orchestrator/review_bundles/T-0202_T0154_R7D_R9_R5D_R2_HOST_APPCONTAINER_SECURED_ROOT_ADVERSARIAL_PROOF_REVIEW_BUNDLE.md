# T-0202 R5D-R2 — host AppContainer secured-root adversarial proof

## Verdict: NEGATIVE

The real `appcontainer_secured_root_host_proof_stops_before_any_output_authority`
test ran in this provider and returned
`REVIEWED_BUILD_APPCONTAINER_LAUNCH_UNAVAILABLE`. It reaches the genuine
CreateAppContainerProfile / security-capabilities / suspended CreateProcessW
path from T-0201, but no lowbox helper executes. No root is therefore created,
no helper output is produced, and no output handle, candidate, or attestation
authority is accepted. The production worker continues to stop at
`REVIEWED_BUILD_FINAL_LINK_HANDOFF_REQUIRED` before `open_built_output`.

## Host-focused test path

The test is ordinary Windows test membership, not an ignored synthetic token
test. It uses only the current CatDesk test executable with fixed `--help`
argv, a fresh real AppContainer profile/SID, real
`PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES`, and `CREATE_SUSPENDED`. Were
creation successful, the exact process would be assigned to the existing
kill-on-close Job, checked by `IsProcessInJob`, inspected with
`OpenProcessToken`/`TokenIsAppContainer`, then resumed. It has no caller path,
shell, PATH, workspace executable, or arbitrary helper input.

The experiment must still be extended on a compatible host with a fixed helper
action and atomically created RootDirectory/no-follow producer root whose full
security descriptor is present *at creation*. The producer root must grant
only package/capability mutation rights and CatDesk's bounded read/handle
rights; the ordinary token must lack write/add/delete/rename, `WRITE_DAC`,
`WRITE_OWNER`, `DELETE_CHILD`, and an owner/OWNER RIGHTS/
`SeTakeOwnershipPrivilege` recovery route. Parent/root handles must remain
pinned without delete sharing.

## Security and attack matrix

| Required check | R5D-R2 provider result |
| --- | --- |
| Real AppContainer SID and security-capabilities launch | attempted; launch unavailable |
| Helper token user/owner/capabilities/integrity/privileges | not reached |
| Atomic security-descriptor root create | not reached; no ordinary-root fallback |
| Root/output owner/DACL/OWNER RIGHTS/mandatory label from handles | not reached |
| Retained relative no-follow output handle, SHA/length/identity | not reached |
| Writable open/same-object write/create child/subdirectory | not executed; no root |
| Same-name replacement/root or child rename/delete/parent DELETE_CHILD | not executed; no root |
| DACL rewrite/WRITE_OWNER/take ownership/privilege recovery | not executed; no root |
| Symlink/junction/reparse/outside redirection | not executed; no root |
| Post-attack token/object/retained-handle identity comparison | not reached |
| 20 consecutive positive iterations | zero; not eligible |

The existing R5B/R5C controlled-handle and broker tests, plus R7C no-follow
tests, remain supplemental only. They do not stand in for a real AppContainer
root hostile-operation result.

## Production and test-harness regression

The host-focused test source-checks that the final-link gate precedes
`open_built_output`. It remains test-only and non-authoritative. The shared
output-candidate test lock was changed to recover from a poisoned prior test
panic, preventing one flaky adversarial assertion from hiding independent
attack results; production filesystem or provenance behavior was not changed.

## Next stronger boundary

The exact provider blocker is lowbox helper launch. A host with the required
AppContainer capability must run the ordinary test after implementing the
descriptor-at-create producer root and fixed helper write. If AppContainer
continues to be unavailable or any ordinary-token DACL/owner/delete-child/
privilege bypass succeeds, the correct architecture is a separate service or
dedicated local-account/VM producer namespace outside the interactive user's
owner and privilege reach, with one retained output handle transferred while
that boundary is still active.

## Verification

- Focused R5D-R2 ordinary host proof — passed with explicit negative launch
  category.
- `cargo fmt --check` — passed.
- `cargo clippy --all-targets --all-features -- -D warnings` — passed.
- Ordinary `cargo test` — not passed: 647 passed, 1 failed, 20 ignored. The
  one failure is the pre-existing T-0196 replay fixture before positive
  control because the provider OS-token profile has no profile-local Rustup
  and `trusted_toolchain()` returns `REVIEWED_BUILD_STATE_UNAVAILABLE`.
- `git diff --check` — recorded after this final bundle update. The workspace
  is broadly untracked, preventing a task-only Git textual diff.

## Changed files

- `src/reviewed_build.rs`
- `docs/orchestrator/review_bundles/T-0202_T0154_R7D_R9_R5D_R2_HOST_APPCONTAINER_SECURED_ROOT_ADVERSARIAL_PROOF_REVIEW_BUNDLE.md`

No reviewed-build worker, Cargo build, promotion, recovery/reload, tunnel,
browser/wake/Scheduler, or external-project action was invoked.
