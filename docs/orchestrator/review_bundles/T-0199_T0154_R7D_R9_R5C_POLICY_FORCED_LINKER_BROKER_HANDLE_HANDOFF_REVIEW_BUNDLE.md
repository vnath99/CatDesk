# T-0199 R5C — policy-forced linker broker handle-handoff prototype

## Verdict: NEGATIVE

The production worker remains fail closed with
`REVIEWED_BUILD_FINAL_LINK_HANDOFF_REQUIRED`. T-0197/R5A remains a
hardening-only slice and T-0193 first-output provenance remains blocked. R5C
does not add a public helper argument, candidate path, attestation, promotion,
or a live reviewed-build invocation.

## Linker discovery

T-0195 attests concrete Cargo and Rustc, but it does not policy-select a final
linker. Rustc may choose internal `rust-lld`, a target-default linker, or a
configuration-selected linker. No one has a persisted policy path, SHA,
stable identity, or version evidence.

`policy_forced_final_linker_for_feasibility` returns exact
`REVIEWED_BUILD_LINKER_UNAVAILABLE`. It refuses PATH, Cargo stdout/prose,
workspace settings, caller environment, shim identity, and global discovery.
That is a concrete producer-identity failure, not an inference that a Job
descendant is the linker.

## Test-only internal broker protocol

`LinkerBrokerCapabilityV1` and `LinkerBrokerHandoffV1` are internal
`cfg(test)` protocol types; no parser or production call site exposes a broker
mode. The parent-created capability binds session ID, build-attempt ID, parent
PID, broker PID, random nonce, independent expected output token, broker
identity, and one-shot state. A record additionally carries schema version,
linker PID/identity, exact-Job membership, and stable output identity.

Wrong or malformed nonce, PID, broker identity, Job state, output token,
duplicate/replay, empty linker identity, or empty output identity is rejected
with `REVIEWED_BUILD_BROKER_HANDOFF_REJECTED` or
`REVIEWED_BUILD_PRODUCER_HANDOFF_REPLAYED`. A valid-looking record still
returns `REVIEWED_BUILD_PRODUCER_HANDLE_NOT_CAPTURED`: no exact final-linker
kernel handle was transferred and no policy forced/attested that linker.

## Windows handle/Job result

R5B's controlled R7C RootDirectory/no-follow object uses `FILE_SHARE_READ`
only. `DuplicateHandle(..., DUPLICATE_SAME_ACCESS)` plus `GetFileType` and
stable volume/file-index comparison shows a retained duplicate remains the
same test object. Writable open, rename/delete, same-name replacement, and
outside-entry replacement were denied while the duplicate was alive; the
outside sentinel was unchanged.

`IsProcessInJob` proves exact controlled-child Job membership but cannot prove
which file handle that child owns. Cargo/linker supplies neither an attested
linker PID nor a nonce-authenticated output-handle capability. A post-hoc
handle scan/duplicate could select any Job handle, so it is not authority.
No claim is made that ordinary linker share flags match the controlled object.

## Adversarial matrix

| Boundary | Attack/failure | Result |
| --- | --- | --- |
| Broker record | wrong nonce, spoofed PID, non-Job linker, malformed record | rejected |
| Broker record | valid-looking record with no exact handle | not-captured |
| Broker record | replay | rejected |
| Linker identity | no policy-forced linker | linker-unavailable |
| Retained object | writable open, rename/delete, same-name/outside replacement | denied; exact handle evidence unchanged |
| R9 output boundary | same-length regular replacement before first child open | preserved T-0193 reproduction; production gate remains required |
| Reparse | live creation where permitted, otherwise R7C classification seam | no-follow rejection; no false live claim |
| Crash/capture miss | no authenticated transferred handle | fail closed; no candidate/attestation |

There is no 20-run positive evidence. The ignored R5B host probe records
deliberate 0/20 captures because producer identity/output-handle ownership is
unavailable. Any miss or ambiguity remains negative.

## Static boundary and next design

`producer_handle_feasibility_cannot_bypass_the_final_link_handoff_gate` keeps
the production gate before `open_built_output` and asserts test-only duplicate
code is absent from the production pre-output path. No pathname open/hash/
identity becomes broker provenance authority.

The required next design is OS-enforced producer isolation: a policy-attested
linker/broker as the only forced final linker under a restricted token or
AppContainer-equivalent isolated root, creating or inheriting a
non-write/non-delete-share output object and transferring it once over
parent-created authenticated IPC before producer release.

## Verification

- `cargo fmt --check` — passed.
- `cargo test linker_ -- --nocapture` — 2 passed.
- `cargo test producer_handle_feasibility -- --nocapture` — 4 passed.
- `cargo clippy --all-targets --all-features -- -D warnings` — passed.
- Ordinary `cargo test` — not passed: 642 passed, 1 failed, 20 ignored. The
  sole failure remains the pre-existing ordinary T-0196 replay fixture before
  its positive control: the provider OS-token profile has no profile-local
  Rustup installation and `trusted_toolchain()` returns
  `REVIEWED_BUILD_STATE_UNAVAILABLE`. R5C does not weaken that authority.
- `git diff --check` — recorded after this final bundle update; the workspace
  has a broad untracked set, so Git cannot produce a task-only textual diff.

## Changed files

- `src/reviewed_build.rs`
- `docs/orchestrator/review_bundles/T-0199_T0154_R7D_R9_R5C_POLICY_FORCED_LINKER_BROKER_HANDLE_HANDOFF_REVIEW_BUNDLE.md`

No live reviewed-build worker, promotion, reload/recovery, wake/browser,
Scheduler, tunnel, or external-project action was invoked.
