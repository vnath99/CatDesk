# T-0408 current-source authority final review

## Classification

`CURRENT_SOURCE_ELIGIBLE_FOR_REVIEWED_SOURCE_AUTHORITY_FREEZE`

This is a source-review classification only.  It does not create a reviewed-source
snapshot or mutate the protected reviewed-build Store, release/LKG state, daemon,
Wake runtime, tunnel, target, or credentials.

## Reviewed source boundaries

- `src/reviewed_build.rs` retains current policy
  `CATDESK_REVIEWED_BUILD_POLICY_V5_OFFLINE_VALIDATED_CARGO_CACHE`: live attempts
  use the fixed offline, locked, isolated Cargo environment.  V3 and V4 remain
  accepted only as exact historical terminal-attempt policies; current attempts
  retain strict V5 validation.
- Production Wake storage remains `Store::open(default_root())`, where
  `default_root()` is the fixed `%LOCALAPPDATA%\\CatDeskWake` authority.  The
  scoped store constructor is feature-gated as `test-support` and used only by
  fixture tests beneath an explicitly supplied canonical fixture parent.
- The T-0403 launch/tunnel source boundary remains unchanged: the official tunnel
  runtime is externally owned, and reviewed LKG authority is minted only by the
  reviewed-promotion canonical-handoff path, not generic daemon reload.
- Wake remains source-only at the reviewed dev.50 manifest version; this review
  performed no package installation, activation, or delivery action.

## Targeted regression evidence

The three prior regressions passed under the ordinary test profile:

1. `reviewed_build::tests::reviewed_build_worker_crosses_local_output_policy_before_all_post_cargo_authority`
2. `delegated::autonomy_runtime::tests::completed_autonomous_review_rehydrates_only_for_independent_wake_owner`
3. `stable_wake_owner_mode::tests::independent_target_staging_is_exact_and_preflight_is_non_authoritative`

The full profile initially exposed one additional stale fixture assertion in
`tests/recovery_powershell.rs`: it expected a removed literal version assignment
in `wake/install.ps1`.  The installer correctly reads and validates its package
version from `wake/Cargo.toml`; the test was narrowed to assert that
manifest-bound mechanism instead.  No production installer, Store, runtime, or
authority code changed.

`bounded_local_control_probe` is explicitly ignored in ordinary `cargo test`
because it requires inherited MCP credentials and restarts the installed
WakeHost.  Its test remains available to an authorized operator through Cargo's
standard `-- --ignored` selection, but was not run here.

## Verification

- Targeted regressions: passed.
- `cargo test --workspace --all-targets --all-features`: passed.  Main suite:
  952 passed, 0 failed, 21 ignored; `local_mcp_wake_control` reported its one
  live-mutating probe as ignored; all remaining target and integration suites
  passed, including the corrected four-test PowerShell recovery suite.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: passed.
- `git diff --check`: passed after this bundle was created.

## Scope and attribution

Attributable edits in this session are the stale PowerShell installer regression
assertion and this review bundle.  Existing dirty worktree changes were preserved.
No live Wake, tunnel, protected reviewed-build state, release/LKG, daemon, Git
history, credentials, or external project was accessed or mutated.

