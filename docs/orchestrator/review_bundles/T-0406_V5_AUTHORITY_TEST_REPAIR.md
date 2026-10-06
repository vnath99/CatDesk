# T-0406 V5 Authority Test Repair Review

## Classification

`V5_AUTHORITY_TEST_REPAIR_DEFECT`

The reviewed V5 assertion repair is present and passes.  The two independent-
Wake fixture tests remain unable to open their disposable `catdesk_wake::Store`
roots on this Windows worker, so the required zero-failure workspace test result
was not achieved.  This record is fail closed and does not approve reviewed-
source authority.

## Bounded repair inspection

1. `reviewed_build::tests::reviewed_build_worker_crosses_local_output_policy_before_all_post_cargo_authority`
   now asserts the exact V5 policy string
   `CATDESK_REVIEWED_BUILD_POLICY_V5_OFFLINE_VALIDATED_CARGO_CACHE`, replacing
   the stale V4 `LOCAL_USER_PINNED_OUTPUT` expectation.  Its exact test passed.
2. `persisted_completed_review_wake_candidates_with_root` provides the
   internal optional `independent_wake_root` seam; normal production callers
   retain `None` and therefore the fixed default-root policy.  The regression
   supplies its own fixture root rather than consulting the live default Wake
   root, but the fixture Store open fails before owner-selection assertions can
   execute.
3. `independent_target_staging_is_exact_and_preflight_is_non_authoritative`
   uses a PID-and-nanosecond unique temporary root and cleans it after use.  It
   nevertheless fails at `Store::open` with `CONFIG_ROOT_UNAVAILABLE` before
   staging/preflight assertions execute.

No production root selection, Wake runtime state, target, delivery journal,
tunnel, reviewed-build Store, daemon, release/LKG state, or Git history was
changed.  An attempted test-only alternate fixture location was rejected by the
worker sandbox (system temporary directory write denied) and was fully reverted;
no such change remains in source.

## Exact verification evidence

| Profile | Result |
| --- | --- |
| Exact V5 assertion test | passed |
| Exact completed-review independent-Wake test | failed: `CONFIG_ROOT_UNAVAILABLE` at `src/delegated/autonomy_runtime.rs:3650` |
| Exact independent-target staging test | failed: `CONFIG_ROOT_UNAVAILABLE` at `src/stable_wake_owner_mode.rs:518` |
| `cargo test --workspace --all-targets --all-features` | failed: 950 passed, 2 failed, 21 ignored, 0 measured |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | passed |

The full test run also emitted Cargo's bounded host warning, `could not
canonicalize path C:\\Users\\Volap`.  The two failing tests use disposable
paths under that user temporary-root hierarchy.  A test-only move to the
OS-derived `C:\\Windows\\Temp` was denied with Windows error 5 while creating
the fixture directory.  These observations establish a worker filesystem
containment/canonicalization blocker; they do not justify weakening the
production Wake Store's absolute-path, non-reparse, or canonical-root checks.

## Attribution and next action

The dirty worktree predates this session.  The only intended task-attributable
source-tree output is this review bundle.  A separately authorized follow-up
must supply a writable, canonicalizable test-fixture root (or an equivalently
safe test-only seam) and re-run the exact two fixtures plus the full workspace
profile before V5 authority can be approved.
