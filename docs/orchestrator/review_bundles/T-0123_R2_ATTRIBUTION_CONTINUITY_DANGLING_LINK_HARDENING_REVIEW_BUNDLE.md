# T-0123 R2 / T-0134: attribution continuity and dangling-link hardening

## Defects closed

The required-output authority now treats only an exact component `NotFound` as absence. Each path component is classified with `symlink_metadata`; a dangling target or intermediate link/reparse point is unsafe, as are non-directory intermediates, special final objects, permission failures, metadata failures, and oversized files. Approval containment, normalized workspace-relative IDs, allowed-path validation, regular-file-only outputs, and the 16 MiB hash cap remain unchanged.

The prior baseline design could recapture a missing baseline for a task that had already launched. This ticket adds an immutable, contract-hash/task/artifact-set-bound launch-authority marker separate from the exact observation baseline.

## Durable baseline state machine

| State | Launch behavior |
| --- | --- |
| no baseline, no marker, task never launched | capture baseline, then atomically write marker, then provider may launch |
| exact baseline, no marker | recover only by writing the matching marker; no provider had yet launched |
| exact baseline and matching marker | reuse exactly for repair/restart |
| marker with missing/corrupt/mismatched baseline | fail closed before provider launch; never recapture |
| marker/baseline binding mismatch | fail closed before provider launch |

The baseline write precedes the marker write. A crash between them is recoverable only from the original validated baseline; a marker cannot exist before baseline creation in normal operation. Once a provider turn is prepared, either the marker or the existing persisted current-task/turn evidence prohibits recapture. A fresh DAG task has a distinct task ID and receives its own baseline/marker.

## Ordering and zero-launch behavior

Attribution remains before `mark_task_completed`; attribution failure retains the task for repair and prevents completion artifacts, final review, and review-inbox completion emission. Lost prior-launch authority returns an error before `prepare_turn` and provider start; the deterministic test holds `provider_turn_count` at one and the queue task at `READY`.

## Tests

- Required output classification: genuine absence, special directory, dangling final and intermediate Windows link/reparse paths, and source-linked reparse classification when this host denies symlink creation with Windows error 1314.
- Immutable continuity: deleted baseline after a prior launch yields zero duplicate launch; missing-marker partial write restores only the original baseline; binding drift and existing immutable baseline tests remain covered.
- Existing output tests retain absent-to-created, changed-existing, multi-artifact, fresh DAG task, restart/repair, no-op provider with unrelated dirty diff, and post-verification ordering coverage.

## Verification

| Gate | Result |
| --- | --- |
| Focused attribution, lost-baseline, crash-marker, and output-classification tests | PASS |
| `cargo fmt --all -- --check` | PASS |
| `cargo clippy --all-targets --all-features -- -D warnings` | PASS |
| `cargo test --all-targets --all-features --no-fail-fast` | PASS (836 unit tests plus integration suites) |
| `cargo build --all-targets --all-features` | PASS |
| `git diff --check` | PASS |
| Separate project `rust_full` / `verify_project` command | Not configured/discoverable; not inferred |

## Attribution and boundaries

T-0134-attributable hunks are the task-output baseline-authority record/store validation in `src/delegated/autonomy_state.rs`; the pre-launch marker/baseline continuity gate, component classification seam, and focused regressions in `src/delegated/autonomous_controller.rs`; and this bundle. Both source files already contain unrelated dirty-worktree changes, which were neither cleaned nor attributed to this ticket. No live provider, promotion/reload/wake/tunnel/Scheduler/external-project/protected-state/Git-publication action occurred.

Request ChatGPT independent final review after the final verification run. Controller green is not independent acceptance.
