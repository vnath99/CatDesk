# T-0197 — trusted final-link output-object handoff escalation

## Required T-0193 evidence retained

The true first-authority seam is `built-output-first-authority` in
`open_built_output`. `built_output_first_authority_seam_precedes_every_child_acquisition`
proves it occurs after pinned `release` descent but before the first relative
open of `catdesk.exe`; no baseline child handle or child identity is acquired
before it.

`first_authority_same_length_regular_swap_exposes_unbound_cargo_output` keeps
the concrete 20-byte genuine `genuine-output-00000` and 20-byte attacker
`evil----output-00000`. Its hook successfully renames the genuine entry and
installs the same-name attacker before the first CatDesk child open. The real
`open_built_output`, `evidence_from_open_regular`, candidate create, and
handle-to-handle copy path then binds the attacker SHA to both output and
candidate. The backed-up genuine bytes and outside sentinel remain unchanged.
This is preserved evidence of the unclosed regular-file provenance race, not a
negative test weakened by an earlier baseline open.

`first_authority_reparse_child_is_rejected_when_supported` uses the same seam.
Where live file-symlink creation succeeds, the R7C RootDirectory/no-follow
child open rejects it; where the token cannot create it, the test makes no
claim of a live reparse substitution and relies only on the existing R7C
classification coverage.

## Trusted-producer boundary that is absent

T-0195's authority ends at exact Cargo and Rustc: fixed-policy absolute path,
opened regular non-reparse handle, SHA-256, length, Windows volume/file-index
identity, and version digest. `BuildJob` contains Cargo descendants, but it
does not identify the concrete final linker, linker process ID, linker output
handle, or a producer-to-CatDesk authenticated nonce/IPC handoff. Cargo then
exits before `open_built_output` begins.

The current policy cannot derive the final linker from Cargo prose, PATH,
workspace configuration, or inherited Rustup/Cargo homes. Rustc may use an
internal rust-lld, target-default link.exe, or configured linker; none is
presently policy-attested. An ordinary linker creates and closes its own
pathname output. Parent-side Job membership cannot recover a private child
handle after close, and global handle enumeration/DuplicateHandle would be
unauthenticated, racy, and not a producer-origin proof.

## Fail-closed decision

No secure narrow handoff was implemented. A correct next design must make an
attested linker/broker the policy-forced final-link executable, launch it in
the existing Job with an attempt-bound nonce, and require it to return one
inherited or otherwise unspoofable restrictive output-object handle before it
releases producer ownership. It must validate linker identity, Job membership,
attempt/nonce, one-handoff semantics, crash/no-handoff, same-object write and
delete sharing, and reparse/regular replacement before CatDesk hashes or
copies. Alternatively the final build root must be isolated under a distinct
identity (service/AppContainer-equivalent) until object handoff.

Neither a later handle-relative open, a pinned parent, output path, file length,
mtime, Cargo success, Cargo log, nor a post-close duplicate can satisfy that
continuous identity requirement. T-0193 therefore remains blocked.

## Immediate production fail-closed boundary

`run_reviewed_build_worker` now invokes
`require_trusted_final_link_handoff()` after the final Cargo/Rustc revalidation
and before `open_built_output`, candidate creation, candidate copy, or
attestation construction. The current implementation returns exact
`REVIEWED_BUILD_FINAL_LINK_HANDOFF_REQUIRED`; the terminal result path records
a failed/ambiguous build instead of accepting naked post-Cargo pathname output.
This is a refusal, not substitute provenance. The static
`reviewed_build_worker_fails_closed_before_naked_post_cargo_output_open` test
verifies gate ordering and the fixed failure category.

## Verification recorded

Focused tests passed:

- `built_output_first_authority_seam_precedes_every_child_acquisition`
- `first_authority_same_length_regular_swap_exposes_unbound_cargo_output`
- `first_authority_reparse_child_is_rejected_when_supported`

Formatting, strict all-target/all-feature clippy, and `git diff --check` also
passed. Full rust_full is not claimed: the ordinary T-0196 replay fixture is
still dependent on the provider sandbox's OS-token Rustup profile. No live
worker, promotion, reload, recovery, or external mutation was run.

## Status and changed files

T-0197 is **not complete** by design. The concrete stronger isolation/handoff
boundary above is required before any claim that attacker bytes cannot become
reviewed-build authority.

- `src/reviewed_build.rs` (preserved T-0193 first-authority source/tests)
- `docs/orchestrator/review_bundles/T-0197_T0154_R7D_R9_R6_FINAL_LINK_OUTPUT_OBJECT_HANDOFF_FEASIBILITY_REVIEW_BUNDLE.md`
- `docs/orchestrator/review_bundles/T-0197_T0154_R7D_R9_R6_TRUSTED_FINAL_LINK_OUTPUT_OBJECT_HANDOFF_ESCALATION_REVIEW_BUNDLE.md`
