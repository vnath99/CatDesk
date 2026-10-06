# T-0190 — actual output/candidate adversarial matrix

## Boundary map before change

| Hook | Production helper | Pinned/open before the hook | attempted mutable entry |
| --- | --- | --- | --- |
| `built-output-release-descent` | `open_built_output` | target guard | `release` |
| `built-output-child-open` | `open_built_output` | target/release and then exact child handle | `release/catdesk.exe` |
| candidate target/builds/attempt | `candidate_parent_for_create` | preceding directory components | next/current directory entry |
| `candidate-child-create` | `create_candidate_file` | exact attempt parent | candidate child |
| `candidate-copy` | `copy_open_regular_files` | source and destination file handles | source/destination names |
| `candidate-evidence` | `evidence_from_open_regular` | destination file handle | candidate name |
| `candidate-replay-open` | `open_candidate_for_replay` | candidate parent, then exact child handle | candidate name |

The T-0189 hook existed but the general test counted calls rather than carrying
out a replacement.  T-0190 uses the same test-only hook with a serialized
context and records actual `rename`/write outcomes.  Hooks compile to inert
calls outside tests.

## Actual adversarial tests

| Test | Seam/mutation | Outcome | Production path |
| --- | --- | --- | --- |
| `adversarial_built_output_child_swap_is_denied_or_never_measured` | rename genuine output, install same-name attacker output | exact opened child denies swap; attacker hash is never measured | `open_built_output`, `evidence_from_open_regular` |
| `adversarial_candidate_parent_seams_deny_rebinding_of_pinned_components` | rename each pinned target/reviewed-builds/attempt directory | Windows denies rebinding or helper fails | `candidate_parent_for_create` |
| `candidate_create_seam_refuses_same_name_substitution_without_overwrite` | install same-name sentinel before create-new | create fails and sentinel bytes remain | `create_candidate_file` |
| `adversarial_copy_evidence_and_replay_stay_bound_to_open_handles` | rename/write source and candidate names at copy, evidence, replay hooks | opened handles retain genuine bytes/evidence; outside sentinel unchanged | copy/evidence/replay helpers |

Each test has an independent outside sentinel with byte-for-byte post-state
assertion.  These are real Windows file operations, not source-text or
same-object-only hook-count checks.  The suite does not claim a live reparse
substitution where token privileges do not permit one; the existing R7C
no-follow/reparse classification primitive remains the fail-closed path.

## Product fixes exposed by the matrix

The pre-open child hook showed a same-name output swap could be measured.  The
exact child is now acquired before the test seam, so the handle is retained
through evidence and Windows sharing denies later entry replacement.

The copy test then exposed same-object mutation because normal relative files
allowed `FILE_SHARE_WRITE`.  `create_relative_regular_file` and
`open_relative_regular_file` now grant only `FILE_SHARE_READ`, while retaining
read/write access on the CatDesk-owned handle itself.  This prevents a caller
from mutating the opened source/destination during copy/hash and preserves
create-new/no-follow/RootDirectory behavior.

## Preserved scope

T-0186 control state, T-0187 static pathname regression, R7C snapshot
authority, trusted tools/process Job containment, and R6 promotion rules are
unchanged.  Promotion mirror/global cleanup and live worker actions are out of
scope and were not invoked.

## Changed files

- `src/reviewed_build.rs`
- `src/reviewed_source_snapshot.rs`
- `docs/orchestrator/review_bundles/T-0190_T0154_R7D_R9_R2_ACTUAL_OUTPUT_CANDIDATE_ADVERSARIAL_MATRIX_REVIEW_BUNDLE.md`

## Verification

Focused adversarial command completed:

- `cargo test adversarial_ -- --nocapture` — 3 passed.

Full verification:

- `cargo fmt --check` — passed;
- `cargo clippy --all-targets --all-features -- -D warnings` — passed after
  correcting a test-only clippy finding;
- `cargo test` — 629 passed, 18 ignored, plus two recovery fixtures passed;
- `cargo test adversarial_ -- --nocapture` — 3 passed after the final fix; and
- `git diff --check` — passed.

The configured release build was not restarted in this bounded regression pass;
the preceding task's release attempts timed out while compiling without a
compiler diagnostic, so this bundle does not call it passed.
