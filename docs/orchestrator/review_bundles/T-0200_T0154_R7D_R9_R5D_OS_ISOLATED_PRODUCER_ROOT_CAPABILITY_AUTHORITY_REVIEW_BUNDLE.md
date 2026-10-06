# T-0200 R5D — OS-isolated producer-root capability authority

## Verdict: NEGATIVE

T-0197/R5A's production `REVIEWED_BUILD_FINAL_LINK_HANDOFF_REQUIRED` gate is
preserved. T-0198/R5B and T-0199/R5C remain negative: neither a post-hoc Job
member nor a broker record without a transferred producer-owned handle is
provenance authority. This ticket adds no production output/candidate/
attestation activation and launches no reviewed-build worker.

## Required Windows security invariant

For a positive isolated producer namespace, the normal interactive token must
have none of `FILE_WRITE_DATA`, `FILE_ADD_FILE`, delete/rename authority,
`WRITE_DAC`, or `WRITE_OWNER` on the root or its descendants. Its SID must not
own the namespace, and it must not be able to reassert ownership through
`SeTakeOwnershipPrivilege` or rewrite its DACL. The producer needs only an
OS-enforced distinct restricted/AppContainer-or-equivalent identity, exact
write capability for its isolated root, and read access to immutable snapshot
and trusted tool inputs. The root must retain integrity and mandatory-label
semantics that prevent the normal token from re-acquiring mutation authority.

An ordinary same-user DACL is insufficient: the owner can normally alter the
DACL/owner subject to token privileges. Integrity labels are not a replacement
for an owner-separated token/capability boundary. A positive result would need
to inspect the producer token user/owner SIDs, AppContainer/restricted state,
capabilities, integrity level, enabled privileges including
`SeTakeOwnershipPrivilege`, and the root/child DACL/SACL before and after all
attacks.

## Implemented test-only diagnostic

`current_token_isolation_owner_separated` uses `OpenProcessToken`,
`GetTokenInformation(TokenUser, TokenOwner, TokenIsAppContainer)`, and
`EqualSid`. A non-AppContainer current token whose owner SID equals its user
SID returns exact `REVIEWED_BUILD_ISOLATION_OWNER_REACQUIRABLE`. Any other
state remains `REVIEWED_BUILD_ISOLATION_UNPROVEN`: a different owner or an
AppContainer bit alone cannot establish the required DACL, integrity,
privilege, capability, or confined-output invariant.

`IsolatedProducerCapabilityV1` is private `cfg(test)` state only. It binds an
attempt, a producer-SID digest distinct from a namespace-owner-SID digest, a
random nonce, and one-shot consumption. It rejects wrong/spoofed SID, owner
alias, nonce, and replay as `REVIEWED_BUILD_ISOLATION_CAPABILITY_REJECTED`.
It is not an actual restricted token, does not accept caller identity input,
and has no production worker call path.

The diagnostic reaches a negative result before it is safe to materialize a
snapshot into a purported producer root or launch trusted Cargo/Rustc. Doing
either under the normal interactive owner would test timing and ordinary DACL
behavior, not OS isolation. Therefore there is no real compiler build,
final-output pathname acquisition, or retained-output-handle claim in R5D.

## Adversarial evidence and preserved boundaries

| Case | Actual evidence | Result |
| --- | --- | --- |
| Normal token owner equals user | Token user/owner SID comparison | rejected as owner-reacquirable |
| AppContainer/alternate owner alone | token diagnostic | unproven without DACL/SACL/integrity/privilege proof |
| Wrong producer SID, wrong nonce, owner alias, replay | private capability state test | rejected |
| Output path replacement, writable open, rename/delete, outside substitution | R5B retained R7C handle test | denied for controlled object only |
| Same-length pre-first-open output swap | preserved T-0193 test | demonstrates unbound Cargo output; product stays gated |
| Broker/linker PID/handle/replay | R5B/R5C tests | rejected/not-captured/linker-unavailable |
| Reparse | preserved R7C/T-0193 conditional live no-follow test | rejected when live creation permitted; otherwise classification seam |

No claim is made that the controlled R7C file-share test proves an ordinary
interactive token cannot invoke ACL, owner, privilege, or root replacement
operations against a normal-user-owned producer root. That missing proof is
the negative feasibility blocker.

## Static regression and next stronger architecture

`os_isolation_prototype_cannot_activate_before_the_final_link_handoff_gate`
proves the test-only isolation model is absent from the production worker's
pre-output authority path and the final-link gate remains before
`open_built_output`.

The next acceptable architecture is a real restricted-token/AppContainer or
stronger service identity created before Cargo execution, with a namespace
owned outside the normal interactive token, constrained capability-only write
access for the producer, validated DACL/SACL/integrity/privilege state, and a
retained exact output handle acquired while that boundary remains enforced.
Cargo/Rustc descendants must be assigned to the existing kill-on-close Job
before execution, and all output evidence/candidate copying must consume the
retained handle only. A later integration ticket must prove this on at least
20 host iterations before removing the production gate.

## Verification

- Focused isolation tests — passed.
- `cargo fmt --check` — passed.
- `cargo clippy --all-targets --all-features -- -D warnings` — passed.
- Ordinary `cargo test` — not passed: 645 passed, 1 failed, 20 ignored. The
  only failure is the pre-existing T-0196 replay fixture before positive
  control because the provider OS-token profile has no profile-local Rustup;
  `trusted_toolchain()` returns `REVIEWED_BUILD_STATE_UNAVAILABLE`. No R5D
  isolation policy fallback was introduced.
- `git diff --check` — recorded after this final bundle update. The workspace
  has a broad untracked set, so Git cannot provide a task-only textual diff.

## Changed files

- `src/reviewed_build.rs`
- `docs/orchestrator/review_bundles/T-0200_T0154_R7D_R9_R5D_OS_ISOLATED_PRODUCER_ROOT_CAPABILITY_AUTHORITY_REVIEW_BUNDLE.md`

No live worker, promotion, reload/recovery, Secure MCP/tunnel action,
browser/wake/Scheduler action, or external-project mutation was invoked.
