# T-0400R2 V3/V4 Transition Isolated Review

## Classification

`T0400R2_V3_V4_TRANSITION_ISOLATED_READY`

The narrow reviewed-build transition readback is consistent with the approved
closed V3/V4 historical-policy boundary, and every profile required by this
R2 contract passed. This classification is limited to the stated isolated
review/build evidence; it grants no reload, promotion, or runtime authority.

## Closed historical-policy boundary

`known_historical_build_policy` first requires the exact fixed Cargo argv
`["build", "--release", "--locked"]`, its canonical JSON SHA-256, and a
policy SHA-256 derived from the exact version string. It then accepts exactly
one of two version/hash pairs:

- current V4 `CATDESK_REVIEWED_BUILD_POLICY_V4_LOCAL_USER_PINNED_OUTPUT` with
  `sha256(ENVIRONMENT_POLICY)`; or
- legacy V3 `CATDESK_REVIEWED_BUILD_POLICY_V3_RUSTUP_PROFILE_RESOLVER` with
  `LEGACY_BUILD_ENVIRONMENT_POLICY_V3_SHA256`.

Any other version, argv, policy hash, or environment-policy hash is rejected.
The focused
`historical_policy_acceptance_is_closed_to_exact_v3_or_current_v4` test covers
valid V3 plus tampered V3 environment data, an unknown version, and tampered
V4 argv rejection.

## Validation and supersession path

`validate_historical_terminal_attempt` retains checks for the attempt schema,
valid opaque attempt and confirmation IDs, bounded review-record field,
lowercase SHA-256 review authority, full immutable attempt digest, and the
closed policy predicate. It revalidates the committed immutable snapshot and
requires exact snapshot ID, authority digest, and manifest digest equality. It
also reattests the persisted absolute Cargo and Rustc binaries and requires
each complete tool-evidence record to match. The attempt digest is derived
from the serialized attempt with only its digest field cleared, binding the
review authority, snapshot, policy, and tool evidence.

For a terminal-failed supersession with a different binding,
`prepare_reviewed_build` invokes `validate_historical_terminal_attempt` before
`terminal_failure_audit` and retry publication. Live paths remain strict:
`validate_attempt`, called by CONFIRM and producer-attestation validation,
requires `attempt.policy == fixed_policy()` and therefore accepts only current
V4 policy metadata.

## Inherited T-0400R1 evidence (not repaired here)

- The generic `cargo fmt --all -- --check` profile was blocked by broad,
  unrelated pre-existing formatting drift.
- The generic all-target test profile was blocked before tests ran because
  Windows denied replacement of the serving `target\\debug\\catdesk.exe`.

Neither condition was retried, repaired, unlocked, or otherwise mutated by
this R2 isolated task.

## R2 verification evidence (2026-09-22)

- `cargo clippy --workspace --all-targets --all-features -- -D warnings`:
  passed (exit 0). The only output was the pre-existing non-fatal
  `could not canonicalize path C:\\Users\\Volap` warning.
- `cargo build --release --locked --target-dir
  .catdesk/verification-targets/autonomy-release` (the exact
  `CARGO_BUILD_RELEASE_ISOLATED` profile): passed (exit 0), completing in
  5m22s.
- Fresh isolated candidate, measured without execution or mutation:
  `.catdesk/verification-targets/autonomy-release/release/catdesk.exe`;
  length `27063296` bytes; SHA-256
  `8afcfb2c99ca1eba8482a685e5d3243fee3d670a676277291bec258275d22bbe`.
- `git diff --check`: passed (exit 0). Existing CRLF conversion warnings were
  emitted but no whitespace error was reported.

## Attribution and prohibited-action audit

The workspace was already broadly dirty, including untracked
`src/reviewed_build.rs`; it was preserved. This file is the only source-tree
artifact created by this R2 session. No product source edit, reviewed-build
control-store action, candidate execution, daemon/recovery/release action,
Wake/target/profile operation, Secure MCP/tunnel operation, Git publication,
credential access, or external-project mutation occurred.
