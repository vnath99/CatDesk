# T-0123-R1 — task-output attribution completion gate

## Defect reproduced

Before this change, a provider could complete without making task-attributable
changes. If independent verification passed and the repository-wide
authoritative diff was nonempty because of unrelated pre-existing dirt, the
controller marked the task verified and could write `completion.json` and emit
`independent_final_review`.

`structured_output_blocks_noop_provider_despite_unrelated_dirty_diff` is the
deterministic regression: a no-op provider, a passing verifier, and a nonempty
unrelated diff now return `QUEUED`, retain the task as `READY`, create no
completion artifact, and persist an `ABSENT` baseline before the provider turn.

## Approval and attribution design

- `AutonomousDevelopmentContractV1.completionArtifactIds` is the optional
  single-task authority.
- `AutonomousTaskSpecV1.completionArtifactIds` is the per-DAG-task authority.
- Both use serde `default` plus `skip_serializing_if = Vec::is_empty`; historical
  contracts with no field retain their serialized bytes and stable contract hash.
- DAG requirements materialize exactly to `AutonomousPlannedTaskV1`; planner
  metadata that adds, removes, or changes an artifact list is rejected before a
  provider launch.

For a nonempty approved set, the first launch of a logical task atomically
persists `artifacts/task-output-baselines.json`, bound to the session, task ID,
approved contract FNV hash, exact artifact set, and either `ABSENT` or an exact
SHA-256. The record is immutable for that task, so repair and restart reuse it;
another DAG task obtains its own record.

After verification/diff success but before `mark_task_completed`, every output
must safely exist and differ from its baseline: absent-to-created or
present-to-different-SHA-256. Failure records only the fixed non-secret event
`REQUIRED_OUTPUT_UNCHANGED_OR_MISSING`, returns the same task to the existing
bounded repair queue, and does not write `completion.json` or emit final review.

## Path-safety contract

Artifact IDs are bounded normalized workspace-relative file paths. Approval
rejects absolute paths, `.`/`..` and prefix components, normalization duplicates,
workspace/allowlist escapes, existing directory/special-file targets, and
symlink ambiguity. Absence remains valid at approval/baseline time. Capture and
post-verification recheck every existing path component, require a regular file,
apply the approved-path policy, and hash at most 16 MiB.

## Deterministic evidence

Tests added/extended:

- `completion_artifact_ids_are_optional_but_path_safe_and_exact`
- `structured_output_blocks_noop_provider_despite_unrelated_dirty_diff`
- `task_output_baselines_require_every_declared_artifact_to_change`
- `task_output_baseline_is_durable_and_immutable_per_task`
- existing `graph_materialization_corruption_escalates_before_provider_launch`,
  extended with completion-artifact metadata corruption

They cover legacy omission/hash compatibility, absent-to-created, unchanged
pre-existing rejection, existing-to-modified success, multiple outputs with one
missing, immutable same-task baseline reuse, a fresh baseline for a distinct DAG
task, durable reload, and planner graph artifact mismatch.

Executed locally:

```text
cargo fmt --check                         PASS
cargo clippy --all-targets --all-features -- -D warnings   PASS
cargo test                                PASS (559 passed, 18 ignored)
git diff --check                          PASS
```

## Changed files

- `src/delegated/autonomous_contract.rs`
- `src/delegated/autonomy_state.rs`
- `src/delegated/autonomous_controller.rs`
- `src/delegated/autonomy_supervisor.rs`
- `src/delegated/autonomous_qwen_tools.rs` (only constructor compatibility)
- this review bundle

The whole-workspace authoritative diff remains independent review evidence. It
is no longer task attribution when a contract declares structured required
outputs. No live provider, promotion/reload, wake, tunnel, Scheduler, browser,
external-project, protected-state, or Git-publication action occurred.
