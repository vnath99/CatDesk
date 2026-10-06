# T-0189 — output/candidate deterministic race seams

## Production seam audit

T-0187 leaves the following exact authority transitions in
`src/reviewed_build.rs`:

| Boundary | Production function |
| --- | --- |
| Pinned target/release descendant and built child | `build_target_guard`, `open_built_output` |
| Candidate chain | `candidate_parent_for_create`, `descend_or_create` |
| Candidate child | `create_candidate_file` / shared `create_relative_regular_file` |
| Exact handle copy | `copy_open_regular_files` |
| Exact handle evidence | `evidence_from_open_regular` / `opened_regular_identity` |
| Candidate replay child | `open_candidate_for_replay` |

Before this ticket there were no T-0187-specific deterministic hooks around
those transitions.  The control-state hook and snapshot cleanup hooks are not
used by this path.

## Test-only hooks

`OUTPUT_CANDIDATE_HOOK` is compiled only in test builds.  Production builds
receive an inert zero-authority call site.  The hook fires immediately before:

- release descent and built `catdesk.exe` relative open;
- `target`, `reviewed-builds`, and attempt-parent descent/create;
- candidate `catdesk.exe` create-new;
- exact-handle copy start;
- exact-handle candidate evidence; and
- replay relative open.

The hook has no caller-facing input and is serialized only between the two
test fixtures; it cannot participate in production authority decisions.

## Deterministic exercised matrix

`output_candidate_test_seams_exercise_exact_relative_authority_helpers` builds
a temporary release child, opens it through `open_built_output`, creates the
candidate via `candidate_parent_for_create`/`create_candidate_file`, streams
the exact opened source to exact opened destination, derives both evidence
records from their handles, and reopens replay through
`open_candidate_for_replay`.  It asserts every production hook boundary ran
and that replay evidence exactly matches the candidate handle evidence.

`candidate_create_seam_refuses_same_name_substitution_without_overwrite`
uses the final pre-create hook to install an attacker-controlled same-name
child.  The shared create-new relative operation fails; the sentinel bytes are
then read back unchanged.  This proves no truncation/overwrite path exists for
an existing candidate child.

| Seam | Injection/result | Outside mutation |
| --- | --- | --- |
| Built release/open | Exact relative-open hook executes; opened evidence comes from the returned handle | None |
| Candidate parent components | Each descent/create seam executes below pinned root | None |
| Candidate create | Same-name sentinel blocks create-new and survives unchanged | None |
| Copy start | Copy hook executes after both handles exist; copy is handle-to-handle | None |
| Evidence | Evidence hook executes on the destination handle; SHA/length/identity are handle-derived | None |
| Replay | Replay hook executes before relative open; replay bytes are remeasured from opened child | None |

Windows sharing can deny a rename/reparse substitution while a parent or child
handle is pinned; that denial is fail-closed evidence.  This ticket does not
weaken sharing flags to make a replacement succeed.  Non-Windows production
mutation remains fail closed because the shared R7C primitive has no equivalent
authority fallback.

## Minimal product correction

The seam test exposed that `create_relative_regular_file` opened a create-new
destination with write/attributes access but not read access.  T-0187 requires
post-copy evidence from that *same* handle.  The shared R7C primitive now asks
for `FILE_READ_DATA` in addition to its existing write/attributes/synchronize
access.  It does not change parent resolution, no-follow behavior, create-new
semantics, or pathname authority.

## Preserved invariants

The T-0187 static `output_and_candidate_authority_stay_handle_relative` test
remains in place.  T-0186 control-state, R7C snapshot containment, trusted
tool identity, fixed Cargo policy, suspended-process Job ownership, and
promotion/recovery behavior were not changed.  Promotion mirror and global
cleanup are outside T-0189.

## Changed files

- `src/reviewed_build.rs`
- `src/reviewed_source_snapshot.rs`
- `docs/orchestrator/review_bundles/T-0189_T0154_R7D_R9_R1_OUTPUT_CANDIDATE_RACE_PROOF_CLOSURE_REVIEW_BUNDLE.md`

## Verification

Focused checks completed:

- `cargo fmt --check`
- `cargo test reviewed_build -- --nocapture` — 13 passed

Full verification completed after this bundle:

- `cargo fmt --check` — passed;
- `cargo clippy --all-targets --all-features -- -D warnings` — passed;
- `cargo test` — 626 passed, 18 ignored, plus two recovery fixtures passed.

The release build was not started again in this narrow regression pass because
the immediately preceding release attempts were bounded out at 120 seconds
without a compiler diagnostic; this bundle does not represent that command as
passing.
