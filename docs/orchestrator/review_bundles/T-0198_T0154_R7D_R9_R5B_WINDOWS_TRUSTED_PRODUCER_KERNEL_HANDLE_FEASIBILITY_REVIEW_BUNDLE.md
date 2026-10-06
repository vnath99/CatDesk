# T-0198 R5B — Windows trusted-producer kernel-handle feasibility gate

## Starting point and scope

T-0197 is accepted only as a **fail-closed hardening slice**. Its production
worker still invokes `require_trusted_final_link_handoff()` and returns
`REVIEWED_BUILD_FINAL_LINK_HANDOFF_REQUIRED` before `open_built_output`,
candidate creation, or attestation construction. T-0193 built-output
provenance closure remains blocked.

R5B adds Windows-only, `cfg(test)` feasibility code. It does not enable a
candidate, attestation, promotion, reload, or a reviewed-build lifecycle. The
existing true first-authority regular-file swap remains intact as the
demonstration that a post-Cargo pathname open is not producer provenance.

## Existing trusted boundary

The only executable authority remains T-0195's `trusted_toolchain()`:
concrete Cargo and Rustc with fixed Program Files priority, otherwise the
current-token OS-profile Rustup resolver, and final concrete tools validated
by absolute path, opened-file SHA-256, bounded length, stable Windows file
identity, and version digest. Rustup shims, PATH, inherited Rustup/Cargo homes,
workspace text, shell lookup, and Cargo output prose are not executable
authority.

That boundary cannot determine an exact final linker. Current reviewed state
has no policy-attested linker identity, linker PID, output-handle value, or
one-attempt authenticated IPC. Cargo's CREATE_SUSPENDED → Job assignment →
resume containment proves a descendant belongs to the Job, but not that it
owns a selected file handle.

## R5B prototype and Windows observations

`ProducerHandleFeasibilityHandoff` is test-only and one-shot. It binds a
random nonce, expected PID, expected opened-file identity, and consumed state.
It rejects wrong nonce, wrong/non-Job PID, unproven owner handle, stable-object
mismatch, and replay with these fixed categories:

- `REVIEWED_BUILD_PRODUCER_HANDLE_NOT_CAPTURED`
- `REVIEWED_BUILD_PRODUCER_HANDLE_UNSAFE`
- `REVIEWED_BUILD_PRODUCER_IDENTITY_MISMATCH`
- `REVIEWED_BUILD_PRODUCER_PROCESS_UNTRUSTED`
- `REVIEWED_BUILD_PRODUCER_HANDOFF_REJECTED`
- `REVIEWED_BUILD_PRODUCER_HANDOFF_REPLAYED`

For a controlled test object only, `duplicate_file_handle_for_feasibility`
uses `DuplicateHandle(GetCurrentProcess, ..., DUPLICATE_SAME_ACCESS)` and
requires `GetFileType == FILE_TYPE_DISK`. It then compares the shared
handle-derived volume/file-index identity before accepting the duplicate. This
proves that a retained duplicate is the same kernel object; it is **not** a
mechanism for attributing a child process's handle. `BuildJob::contains_process`
uses `IsProcessInJob` for a controlled child and proves the precise separation:
Job membership is observable, but it is insufficient to prove that the member
owns this output file handle.

The controlled object and its parent are created through the accepted R7C
`ProtectedDirectoryGuard` and RootDirectory/no-follow regular-file primitive.
Their handle share is `FILE_SHARE_READ` only. While both the source and its
duplicated handle remain live, independent writable open, rename, same-name
same-length replacement, delete, and outside-sentinel replacement attempts
were denied. Evidence from the retained duplicate remained the original
20-byte SHA and stable identity; the outside sentinel was unchanged. This is
sharing evidence for a CatDesk-owned test object, not evidence that an
ordinary linker grants equivalent shares or hands CatDesk its file object.

Reparse cannot be installed at that name while delete/replacement is denied.
The retained-handle test therefore records denial rather than claiming a live
post-producer reparse replacement. The preserved T-0193 conditional live
reparse test and R7C no-follow classification coverage remain the applicable
reparse evidence.

## Producer capture verdict: NEGATIVE

R5B cannot safely inspect and attribute a final linker handle from the current
Cargo/Rustc/Job design. A post-hoc process-handle scan and `DuplicateHandle`
would lack an authenticated linker PID and a specific output-handle
capability; it could select an unrelated Job member/handle and would not prove
that it is the final trusted output. Treating Job membership or a pathname as
the missing ownership fact would be unsafe, so real observations are required
to fail with `REVIEWED_BUILD_PRODUCER_HANDLE_UNSAFE` rather than guess.

`host_trusted_producer_kernel_handle_feasibility_probe_20_iterations` is an
ignored, directly runnable host-context probe. It calls the real T-0195
resolver, confirms nonempty Cargo/Rustc evidence, and records 20/20 deliberate
capture misses because no attested final-link producer-to-handle binding exists.
It does not claim a compiler capture or POSITIVE feasibility. It is run with:

```text
cargo test host_trusted_producer_kernel_handle_feasibility_probe_20_iterations -- --ignored --nocapture
```

The next acceptable design is still an attested policy-forced linker/broker in
the Job, with authenticated nonce/one-shot IPC and an inherited restrictive
output handle transferred before the producer releases ownership; alternatively
an isolated build identity/root must prevent same-user mutation through that
handoff. Only then can producer PID, exact output-handle ownership, access,
and sharing semantics be meaningfully verified.

## Source/order regression and tests

`producer_handle_feasibility_cannot_bypass_the_final_link_handoff_gate`
asserts that the production final-link gate remains before
`open_built_output` and that the test-only duplicate primitive is not present
in the worker's pre-output authority path.

| Test | Evidence | Result |
| --- | --- | --- |
| `producer_handle_feasibility_state_rejects_spoofed_and_replayed_handoffs` | nonce, PID, non-Job, unproven-handle, identity mismatch, one-shot replay, exact duplicate identity | passed |
| `producer_handle_feasibility_observes_job_membership_but_not_file_ownership` | controlled suspended child assigned to exact Job, `IsProcessInJob` | passed; no ownership inference |
| `producer_handle_feasibility_duplicate_stays_bound_when_path_attacks_are_denied` | writable open, rename, same-name replacement, delete, outside replacement, retained handle evidence | passed; denied, outside sentinel unchanged |
| `first_authority_same_length_regular_swap_exposes_unbound_cargo_output` | preserved T-0193 first-boundary 20-byte same-name swap | passed; demonstrates why production remains gated |

## Verification

- `cargo fmt --check` — passed.
- Focused R5B feasibility tests and the preserved first-authority reproduction
  — passed.
- `cargo clippy --all-targets --all-features -- -D warnings` — passed.
- Ordinary `cargo test` — not passed: 640 passed, 1 failed, 20 ignored. The
  only failure is the pre-existing ordinary T-0196 replay fixture before its
  positive control: this provider's OS-token profile has no profile-local
  Rustup installation and `trusted_toolchain()` returns
  `REVIEWED_BUILD_STATE_UNAVAILABLE`. No R5B test failed and no cross-profile,
  PATH, or caller-environment fallback was introduced.
- `git diff --check` — recorded after this bundle update. The workspace has a
  broad pre-existing untracked set, so Git cannot produce a task-only textual
  diff for the untracked source/artifacts.

## Changed files

- `src/reviewed_build.rs`
- `docs/orchestrator/review_bundles/T-0198_T0154_R7D_R9_R5B_WINDOWS_TRUSTED_PRODUCER_KERNEL_HANDLE_FEASIBILITY_REVIEW_BUNDLE.md`

No live reviewed-build worker, promotion, reload/recovery, tunnel, browser,
Scheduler, or external project was invoked.
