# T-0405 Current Source V5 Authority Review

## Classification

`CURRENT_SOURCE_V5_AUTHORITY_DEFECT`

The bounded source review confirms the requested V5 and lifecycle invariants below, but the authoritative full Rust test profile is not green.  This record is therefore **not** approval to freeze the current workspace as reviewed-source authority.  No product, protected-state, runtime, Wake, tunnel, release, or Git-history mutation was made to address the failing tests.

## Reviewed-build V5 boundary

`src/reviewed_build.rs` sets the current live policy to
`CATDESK_REVIEWED_BUILD_POLICY_V5_OFFLINE_VALIDATED_CARGO_CACHE`.  Its fixed
arguments are `build --release --locked --offline`; the corresponding V5
environment policy requires a cleared inherited environment, trusted-profile
Rustup discovery, per-attempt temporary material, validated registry/index and
crate archives, offline execution, and immediate pinned-output measurement.

`fixed_policy()` is used by live-attempt validation.  Thus active PREPARE,
CONFIRM/RESULT, attestation, and current retry work require the exact V5 policy
metadata and digest.

Historical terminal validation is separately closed: `known_historical_build_policy`
accepts only exact V3, V4, or current V5 version/argument/environment-policy
combinations and their fixed digests.  `validate_historical_terminal_attempt`
continues to bind schema, attempt and review IDs, review-authority digest,
attempt digest, immutable source snapshot/manifest, and trusted Cargo and Rustc
binary identities.  The terminal-failed supersession path uses that historical
validator; live attempts retain strict `validate_attempt`/V5 validation.  Unknown
or altered historical policy metadata is not accepted.

## Serving and source-state observations

Read-only process and listener inspection found PID `41328` executing
`target-verify/t0402-v5-controller/release/catdesk.exe` and owning the loopback
listener at `127.0.0.1:3200`.  The artifact measured:

* SHA-256: `b8e3bd78de9e5e7ecd30f39070aaf96457a0b939e152730d439383cdffb20cbb`
* length: `27100672` bytes

The older isolated artifact remains a distinct process/file observation:
`.catdesk/verification-targets/autonomy-release/release/catdesk.exe`, SHA-256
`8afcfb2c99ca1eba8482a685e5d3243fee3d670a676277291bec258275d22bbe`, length
`27063296` bytes; it did not own the observed listener.  A literal scan of the
optimized serving PE did not find the V5 policy string, so this review does not
mistake that absence for byte-to-source provenance.  The normal reviewed-source
snapshot/freeze boundary remains required to bind current source to any later
reviewed-build authority.

## Launch, recovery, and tunnel boundary

The T-0403 source retains external official-runtime ownership.  The stack path
performs bounded runtime-status/readiness observation and requires the official
runtime to be running, healthy, ready, and paired with local MCP readiness; it
does not take ownership of starting or managing that external runtime.

Reviewed promotion writes `CANONICAL_HANDOFF_PROVEN` only after exact canonical
artifact/listener handoff.  Recovery accepts reviewed LKG only from that
promotion authority and treats legacy `operational_verified` material as
non-authoritative.  Generic reload, temporary controller bootstrap, isolated
build evidence, and review records cannot mint LKG authority.

## Wake dev.50 source-only review

The source is at Wake version `1.0.0-dev.50`.  Its submitting-state path permits
only bounded reconciliation, preserves the original target/event binding, and
records attention rather than replaying, clearing, downgrading, or retargeting
an ambiguous submission.  The installed T-0403 delivery record remains
`SUBMITTING` with `SUBMISSION_RECONCILIATION_REQUIRED`; no Wake runtime,
journal, target, or browser action occurred in this task.  Consequently dev.50
is source evidence only and does not alter the live ambiguous delivery.

## Verification evidence

| Profile | Result |
| --- | --- |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | passed |
| Focused `test-secure-mcp-route-validation.ps1`, `test-start-catdesk-stack.ps1`, `test-catdesk-lifecycle.ps1`, `test-promote-reviewed-catdesk-build.ps1` | passed |
| `cargo test --workspace --all-targets --all-features` | failed: 949 passed, 3 failed, 21 ignored |
| `git diff --check` before this artifact | passed (line-ending warnings only) |

The three exact Rust failures were:

1. `reviewed_build::tests::reviewed_build_worker_crosses_local_output_policy_before_all_post_cargo_authority` at `src/reviewed_build.rs:8476`, asserting the old `LOCAL_USER_PINNED_OUTPUT` V4 policy label despite the reviewed current V5 policy.
2. `delegated::autonomy_runtime::tests::completed_autonomous_review_rehydrates_only_for_independent_wake_owner`, which received `Validation("CONFIG_ROOT_UNAVAILABLE")` while creating independent-owner candidates.
3. `stable_wake_owner_mode::tests::independent_target_staging_is_exact_and_preflight_is_non_authoritative`, whose fixture Store creation returned `CONFIG_ROOT_UNAVAILABLE`.

The workspace was already materially dirty and contains broad staged/untracked
and modified work.  This session preserved it.  The only intended task output
is this review artifact; build and test outputs are non-source verification
artifacts.

## Prohibited-action audit

No protected reviewed-build Store access or mutation, PREPARE/CONFIRM/RESULT,
release or LKG action, daemon reload, tunnel operation, Wake installation or
delivery, target change, browser action, Git branch/history/publication action,
credential access, or external-project mutation occurred.

## Required next step

Keep this review fail closed.  A separately authorized repair/review must make
the full Rust test profile green and capture a clean authoritative diff before
the current source is frozen as reviewed-source authority.
