# T-0392 Recovery/Release Invariant Review R3

## Classification

`RECOVERY_RELEASE_INVARIANT_REVIEW_R3_NOT_READY`

This is a no-implementation-change review. The corrected stale Wake-version
fixture is present, but the required current full verification is not green,
so this review does not approve the snapshot for final authority.

## Bounded findings

| Area | Current source finding |
| --- | --- |
| Release candidate isolation | `scripts/provision-catdesk-release.ps1` builds only under `target\\catdesk-release-candidate` with `--target-dir`, reports `CanonicalMutated=$false`, and returns `REVIEWED_PROMOTION_REQUIRED`. The directly relevant `reviewed_build` regression asserts those boundaries and rejects a direct canonical build/manifest write shape. |
| Legacy LKG classification | `scripts/catdesk-release-recovery.ps1` accepts only `reviewed_promotion` as rollback authority. Legacy `operational_verified` slots are refused as authority; a well-formed legacy-only escrow is classified `LKG_AUTHORITY_MISSING`, while malformed or ambiguous state remains `LKG_AUTHORITY_AMBIGUOUS_OR_DAMAGED`. Reviewed promotion requires `CANONICAL_HANDOFF_PROVEN` and matching canonical hash/path evidence. |
| Corrected Wake version fixture | `wake/Cargo.toml` currently declares `1.0.0-dev.17`; `wake/install.ps1` uses the same value. `tests/recovery_powershell.rs` now derives the expected installer declaration from `wake/Cargo.toml`, instead of preserving a stale literal version. |
| Current-generation anti-replay | `wake_protocol_client` computes the cutoff as the maximum of activation and the exact current target-history timestamp. Publication rejects older records with `WAKE_HISTORICAL_EVENT_RETAINED`; absent current-target history fails closed with `WAKE_TARGET_HISTORY_INVALID`. |
| Same-target network recovery | `scripts/wake_bridge.py` permits bounded pre-submit reload/reopen only after a positive network-error classification, reopens only `self.url` (the exact canonical target), and retries a browser session only for `BROWSER_NETWORK_ERROR` before the first browser write. The focused Python test covers the exact-target reopen shape. |

## Verification evidence

- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: completed successfully.
- `cargo fmt --all -- --check`: failed on pre-existing formatting drift in
  `wake/examples/hash_catdesk_release.rs`, `wake/examples/queue_readback.rs`,
  and `wake/src/store.rs`. Those files were outside this review's permitted
  implementation-edit scope and were left unchanged.
- `cargo test --workspace --all-targets --all-features`: not green. The bounded
  run reported failure of
  `stable_wake_owner_mode::tests::independent_target_staging_is_exact_and_preflight_is_non_authoritative`.
- Focused reproduction of that test failed deterministically at
  `src/stable_wake_owner_mode.rs:517`: the fixture supplies a newly generated,
  nonexistent temp path to `catdesk_wake::store::Store::open`, while
  `Store::open` canonicalizes its root after checking child directories and
  returns `CONFIG_ROOT_UNAVAILABLE`. Therefore the test does not reach its
  intended staging/preflight assertions.
- `git diff --check` completed without a whitespace error. `git status` showed
  the reviewed implementation as inherited dirty work, including untracked
  recovery/Wake files and modified `scripts/wake_bridge.py`; no cleanup,
  reset, staging, or implementation mutation was performed.

## Residual risks and next safe step

The candidate's reviewed source boundaries are present, but the full Rust
baseline remains blocked by the formatting drift and the deterministic stale
Wake fixture-root failure above. A separately authorized implementation repair
must first establish the intended fixture root before `Store::open`, and the
pre-existing formatting drift must be resolved by its owning work, before a
fresh full verification review can be approved. This task intentionally makes
neither change.

No release provisioning/promotion/recovery action, Wake send or target change,
browser action, Secure MCP/tunnel action, daemon/service action, Git
publication, or external-project mutation occurred. This bundle is the sole
T-0392 R3-attributable workspace mutation.

## Independent final review request

Request independent final review of this fail-closed evidence record. It must
not be treated as approval of the current source snapshot until the stated
verification blockers are independently repaired and rechecked.
