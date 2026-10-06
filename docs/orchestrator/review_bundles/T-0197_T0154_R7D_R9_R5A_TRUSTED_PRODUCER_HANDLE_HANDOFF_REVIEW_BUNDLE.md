# T-0197 R5A — trusted producer handle-handoff boundary

## Required-output baseline

At this task's start, `src/reviewed_build.rs` was present and the exact
required R5A bundle was absent. Earlier R6-named T-0197 feasibility and
escalation notes are historical diagnostics only; they are not substituted for
this required artifact.

## Preserved first-authority evidence

`open_built_output` pins `target/release` and fires the test-only
`built-output-first-authority` seam immediately before its first
`open_relative_regular_file(..., "catdesk.exe", ...)`. No CatDesk file handle,
metadata identity, hash, or byte read of the child occurs before that seam.
`built_output_first_authority_seam_precedes_every_child_acquisition` keeps this
ordering under source regression.

The accepted T-0193 reproduction remains deliberately unchanged:
`first_authority_same_length_regular_swap_exposes_unbound_cargo_output`
replaces 20-byte `genuine-output-00000` with the distinct same-length
20-byte `evil----output-00000` after `release` is pinned and before the first
child open. The shared no-follow primitive rejects a live reparse child when
the Windows token permits creating one; lack of symlink privilege is recorded
only as simulated R7C classification coverage. This is evidence of the
unresolved regular-file producer-provenance race, not a claim that a pinned
parent proves producer origin.

## Trusted-chain and final-link assessment

T-0195 attests final concrete Cargo and Rustc only: exact absolute regular
non-reparse object, handle-derived SHA-256, bounded length, stable Windows
volume/file-index identity, and bounded version evidence. Its deterministic
selector prefers the exact Program Files pair and otherwise obtains only the
current-token OS-profile-local Rustup resolver under `.cargo\\bin`, with a
cleared, fixed environment. Rustup is discovery-only; Cargo and Rustc under
one exact `.rustup\\toolchains/<toolchain>/bin` root are the persisted and
launched tools.

That authority does **not** identify a final linker. Cargo/Rustc can use an
internal `rust-lld`, target-default `link.exe`, or a configured linker. The
present policy contains no attested linker path/evidence, linker PID, output
handle, attempt nonce, or authenticated producer-to-CatDesk IPC. The existing
kill-on-close Job proves descendant containment, not which descendant created
the final pathname entry.

An ordinary linker owns and closes its output handle. After it closes, a
RootDirectory/no-follow reopen can classify the current object but cannot bind
it to that producer. Process-handle enumeration or post-close
`DuplicateHandle` would be global, racy, unauthenticated, and cannot prove
the chosen object was the final link output. FILE_SHARE_WRITE/DELETE closure
after a pathname reopen also moves the demonstrated race rather than closing
it; same-object write/delete sharing must be part of a restrictive producer
handoff.

## Minimal fail-closed behavior

No narrow continuous final-link handoff exists in the present Cargo/Rustc/Job
architecture. `run_reviewed_build_worker` therefore calls
`require_trusted_final_link_handoff()` after exact Cargo/Rustc post-build
revalidation and before `open_built_output`, candidate-parent creation, or
attestation construction. It returns the fixed
`REVIEWED_BUILD_FINAL_LINK_HANDOFF_REQUIRED` error, so the normal terminal
failure path cannot turn a naked post-Cargo pathname object into built,
candidate, or producer-attestation authority.

`reviewed_build_worker_fails_closed_before_all_post_cargo_output_authority`
now source-checks that this gate precedes all three boundaries and checks the
fixed fail-closed error. The gate does not claim to bind Cargo's already
created file, and it does not remove the T-0193 reproduction.

## Required stronger design boundary

T-0193 cannot be completed until a separate reviewed execution protocol:

1. policy-select and attest one concrete final linker/broker;
2. force final linking through it while it is inside the existing Job;
3. bind one attempt nonce and a one-time authenticated handoff;
4. transfer an inherited or equivalently unspoofable restrictive handle for
   the exact output object before producer ownership ends; and
5. prove linker identity, Job membership, wrong PID/nonce/handle rejection,
   duplicate/replay rejection, crash/no-handoff failure, reparse/regular
   replacement rejection, and no write/delete sharing before CatDesk hashes
   or copies that handle.

An isolated build identity/root with equivalent object-handoff guarantees is
the alternative. Pathname, Cargo exit success, linker prose, mtime, length,
or a hidden earlier open are not acceptable substitutes.

## Verification

Executed in this workspace:

- `cargo fmt --check` — passed.
- Focused `cargo test` for
  `reviewed_build_worker_fails_closed_before_all_post_cargo_output_authority`,
  `built_output_first_authority_seam_precedes_every_child_acquisition`,
  `first_authority_same_length_regular_swap_exposes_unbound_cargo_output`, and
  `first_authority_reparse_child_is_rejected_when_supported` — each passed.
- `cargo clippy --all-targets --all-features -- -D warnings` — passed.
- `git diff --check` — passed; the repository has a pre-existing broad
  untracked working set, so Git cannot present a task-only textual diff for
  these untracked artifacts.
- Ordinary `cargo test` — not passed: 636 passed, 1 failed, 19 ignored. The
  sole failure is the accepted ordinary T-0196 replay fixture before its
  positive control: the provider sandbox's OS-token profile has no
  profile-local Rustup installation and `trusted_toolchain()` correctly
  returned `REVIEWED_BUILD_STATE_UNAVAILABLE`. This R5A task does not alter
  T-0195 profile authority or re-ignore that test.

## Changed artifacts

- `src/reviewed_build.rs`
- `docs/orchestrator/review_bundles/T-0197_T0154_R7D_R9_R5A_TRUSTED_PRODUCER_HANDLE_HANDOFF_REVIEW_BUNDLE.md`

## Status

T-0197 does **not** claim T-0193 closure. The source fails closed pending the
stronger producer-object handoff or isolation design described above. No live
reviewed-build worker, promotion, daemon lifecycle action, or external-system
mutation was invoked.
