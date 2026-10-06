# T-0193 — built-output first-authority acquisition provenance

## Timeline audit

Old `open_built_output` pinned `release`, opened `catdesk.exe`, collected its
stable identity, and only then fired the advertised child pre-open hook before
opening the child a second time. That baseline open was an authority-bearing
acquisition before the claimed adversarial boundary and could not prove a true
post-Cargo/pre-first-open race.

The replacement timeline is now:

1. Cargo exits; its target root remains pinned from `build_target_guard`.
2. `open_built_output` pins the `release` directory.
3. Test-only `built-output-first-authority` fires.
4. The first CatDesk child authority touch is the shared RootDirectory/no-follow
   `open_relative_regular_file(..., "catdesk.exe", ...)`.
5. Only the opened handle reaches `evidence_from_open_regular`, candidate copy,
   and attestation construction.

`built_output_first_authority_seam_precedes_every_child_acquisition` statically
checks that this seam occurs before the first child open and that no baseline
identity/file acquisition remains before it.

## Deterministic result: unresolved provenance defect

`first_authority_same_length_regular_swap_exposes_unbound_cargo_output` starts
with genuine 20-byte `genuine-output-00000`. At the true first-authority seam
it successfully renames that entry and installs the different, same-length
20-byte `evil----output-00000`. The real `open_built_output`,
`evidence_from_open_regular`, `candidate_parent_for_create`,
`create_candidate_file`, and `copy_open_regular_files` paths then accept the
attacker SHA as both built and candidate evidence. The genuine backup and an
independent outside sentinel remain unchanged.

This proves a real provenance defect: a pinned parent plus a first
handle-relative/no-follow child open rejects reparse traversal but cannot prove
that a same-name regular object was produced by the completed Cargo process.
No hidden baseline open, pathname/mtime/length comparison, or Cargo success
status can create that missing object binding. A secure repair requires a
separate trusted build-lifecycle output-object handoff/commit protocol, which
is broader than a narrow child-open refactor. No unsafe workaround was added.

`first_authority_reparse_child_is_rejected_when_supported` attempts a real
file symlink at the same seam. When the token permits creation, the shared
no-follow open must reject it; where the token refuses symlink creation, the
test records only that live attack was unavailable and relies on existing R7C
no-follow classification coverage rather than claiming a live reparse result.

## Verification and status

The same-length regular swap reproduction and conditional reparse test passed
locally. The static seam-order regression was repaired after an initial slice
calculation error and must be rerun with the focused suite. T-0196's ordinary
replay test remains environment-dependent on this provider's sandbox profile;
no toolchain policy was changed for this ticket.

T-0193 is **not complete**: the required guarantee that an attacker cannot
become built/candidate authority is false under the demonstrated true
first-authority race. This bundle preserves the reproduction rather than
claiming closure.

## Changed files

- `src/reviewed_build.rs`
- `docs/orchestrator/review_bundles/T-0193_T0154_R7D_R9_R5_BUILT_OUTPUT_FIRST_AUTHORITY_ACQUISITION_PROVENANCE_REVIEW_BUNDLE.md`
