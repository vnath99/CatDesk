# T-0402R1 Reviewed-Build Offline Cache Repair

## Classification

`REVIEWED_BUILD_OFFLINE_CACHE_REPAIR_BLOCKED_HOST_LOCK`

The bounded source/test repair is implemented and focused tests plus strict
Clippy pass. The required isolated release build could not complete because
Windows denied replacement of the fixed isolated output. This session did not
unlock, replace, execute, reload, or otherwise mutate that artifact.

## Root cause carried from T-0401

T-0401 reproduced exit 101 with the reviewed worker's empty generation-local
Cargo home. Cargo attempted to resolve `index.crates.io`, could not resolve its
hostname, and failed while obtaining locked dependency `axum`. The prior
implementation had no protected local index/cache closure and did not require
offline resolution.

## Repair

`src/reviewed_build.rs` now defines current policy
`CATDESK_REVIEWED_BUILD_POLICY_V5_OFFLINE_VALIDATED_CARGO_CACHE`, whose fixed
argv is exactly `build --release --locked --offline` and whose environment
policy identity includes validated registry index/crate archives and offline
execution.

Before Cargo starts, the worker:

1. Reads the materialized `Cargo.lock` through a protected source guard and
   accepts only bounded registry packages with safe names/versions and a
   SHA-256 checksum.
2. Derives the current user profile through the existing OS API, opens only
   `.cargo/registry/index` and `.cargo/registry/cache` under handle-bound,
   reparse-refusing guards, and selects one exact shared registry identity.
3. Creates a fresh generation-local protected `cargo-home`; it copies only
   `config.json`, the needed sparse-index cache entries, and the matching
   `.crate` archives. It never copies ambient `registry/src`.
4. Rehashes every copied archive against the lockfile checksum and fails closed
   on absent, oversized, unsafe, non-regular, reparse, ambiguous, or checksum-
   mismatched input. Cargo receives only that local home after `env_clear()`.

The worker therefore does not inherit mutable ambient `CARGO_HOME` and has no
network fallback: `--offline` is a fixed policy argument.

## Policy and retry compatibility

Live validation still requires exact equality with the new V5 `fixed_policy`.
Historical terminal validation remains closed to precisely:

- V3 with its original argv and exact environment-policy digest;
- V4 with its original argv and exact V4 environment-policy digest; or
- the exact current V5 policy.

Unknown versions, modified argv, or modified policy/environment digests remain
non-retryable. The terminal-failed supersession path still calls
`validate_historical_terminal_attempt`; live CONFIRM and attestation paths
continue to call strict `validate_attempt`.

## Focused regression evidence

- `offline_cache_seeding_copies_only_checked_archive_and_index_entry`: passed.
- `offline_cache_seed_refuses_missing_or_wrong_checksum_archive`: passed.
- `offline_cache_seed_refuses_non_directory_index_component`: passed.
- `offline_worker_policy_never_inherits_ambient_cargo_home`: passed.
- `historical_policy_acceptance_is_closed_to_exact_v3_v4_or_current_v5`:
  passed.

## Required profile evidence

- `cargo clippy --workspace --all-targets --all-features -- -D warnings`:
  passed (exit 0; only the pre-existing non-fatal path canonicalization
  warning).
- Exact `CARGO_BUILD_RELEASE_ISOLATED` command
  `cargo build --release --locked --target-dir
  .catdesk/verification-targets/autonomy-release`: failed after compilation
  when Cargo could not remove
  `.catdesk/verification-targets/autonomy-release/release/catdesk.exe`:
  `Access is denied. (os error 5)`.
- Fresh bounded retry after the independent-verification report reached the
  same fixed output replacement and failed with the same Windows `Access is
  denied. (os error 5)` result. No source, process, or output-path workaround
  was applied; this confirms the remaining unmet profile is an external
  host-lock condition rather than a retryable source diagnostic.
- `git diff --check`: recorded after this bundle.

## Boundary audit

No live reviewed-build PREPARE/CONFIRM/RESULT operation, protected control-store
edit, Wake/target action, daemon/recovery/release action, Secure MCP/tunnel
action, Git publication, credential access, or external-project mutation
occurred. The remaining safe next step is independent source review followed
by host-owner resolution of the fixed isolated-output lock before rebuilding.
