# T-0400R1 V3/V4 Transition Compile Review

## Classification

`T0400R1_V3_V4_TRANSITION_COMPILE_NOT_READY`

The reviewed policy-transition implementation is present and has the intended
closed acceptance boundary, but this evidence-only session cannot report a
green contract verification set: `CARGO_FMT` and `CARGO_TEST` failed for
pre-existing workspace/host conditions described below. No product source or
protected/runtime state was changed to work around either condition.

## Transition-boundary readback

`src/reviewed_build.rs` defines only these accepted historical policy forms in
`known_historical_build_policy`:

- current `CATDESK_REVIEWED_BUILD_POLICY_V4_LOCAL_USER_PINNED_OUTPUT`, with
  the exact SHA-256 of the current fixed environment-policy string; or
- legacy `CATDESK_REVIEWED_BUILD_POLICY_V3_RUSTUP_PROFILE_RESOLVER`, with the
  exact fixed legacy environment-policy SHA-256.

Before that version switch, the helper requires the exact fixed
`["build", "--release", "--locked"]` argv, its canonical JSON SHA-256, and a
policy SHA-256 derived from the exact version string. All other versions,
altered argv, and altered policy/environment hashes return false. The focused
`historical_policy_acceptance_is_closed_to_exact_v3_or_current_v4` regression
covers a valid V3 form, a tampered V3 environment hash, an unknown version,
and a tampered current argv.

`validate_historical_terminal_attempt` retains the historical evidence
bindings: attempt schema, opaque attempt and confirmation identifiers,
review-record presence, lowercase SHA-256 review authority, immutable attempt
digest, the closed V3/V4 policy predicate, a revalidated committed snapshot,
and exact snapshot ID/authority/manifest digest equality. It also reopens and
revalidates the absolute Cargo and Rustc binaries and requires equality with
the persisted tool evidence. `digest_attempt` serializes every attempt field
other than its digest, thereby binding the review authority, expected snapshot,
policy, and tool evidence together.

The terminal-failed supersession branch in `prepare_reviewed_build` calls
`validate_historical_terminal_attempt` before `terminal_failure_audit` and
retry publication. In contrast, `confirm_reviewed_build` and producer
attestation call `validate_attempt`, which requires `attempt.policy ==
fixed_policy()` and therefore preserves strict current V4 validation for live
attempts.

## Contract-profile evidence (2026-09-22)

- `cargo fmt --all -- --check`: failed. Rustfmt reported extensive existing
  formatting differences across untracked/dirty workspace files (including
  examples, Wake files, and pre-existing source); no formatting was applied.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`:
  passed (exit 0), after the non-fatal `could not canonicalize path
  C:\\Users\\Volap` warning.
- `cargo test --workspace --all-targets --all-features`: failed before tests
  ran because Cargo could not remove
  `target\\debug\\catdesk.exe` (`Access is denied`, Windows OS error 5).
  This session did not alter or unlock that executable.
- `cargo build --release --locked`: passed (exit 0), finishing the release
  profile in 5m27s and freshly compiling the ordinary
  `target\\release\\catdesk.exe` from the current source. It was not run,
  reloaded, promoted, or otherwise used as runtime authority.
- `git diff --check`: passed (exit 0). Git emitted only existing CRLF
  conversion warnings.
- `git status --short`: confirms a broad inherited dirty/untracked worktree;
  it was neither cleaned nor modified. This review bundle is the sole file
  created by this session.

## Boundary audit

No reviewed-build PREPARE/CONFIRM/RESULT action, protected-store mutation,
daemon/recovery/release action, Wake/target operation, Secure MCP/tunnel
operation, Git publication, credential access, or external-project mutation
occurred. Independent final review remains required, and the failed required
profiles must be resolved by their responsible owner before a green transition
compile record can be issued.
