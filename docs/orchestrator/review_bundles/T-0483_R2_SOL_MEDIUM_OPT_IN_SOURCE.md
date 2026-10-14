# T-0483 R2 — Opt-in Codex Sol/medium source route

**Date:** 2026-10-14
**State:** source increment; no live Sol/medium worker turn or natural Wake accepted.

## Change

`src/delegated/codex_model_policy.rs` defines two exact managed profiles: existing `gpt-5.6-terra`/`high` and opt-in `gpt-6.1-sol`/`medium`. The contract still defaults to its serialized legacy Terra model; choosing Sol changes the contract hash, so no old session silently migrates. Unsupported model IDs remain invalid for managed autonomous contracts.

`src/delegated/codex_cli.rs` now passes the matching `model_reasoning_effort` override for either managed profile. `src/delegated/codex_app_server.rs` provides a profile-aware preflight and rejects unknown, mismatched, or downgraded authoritative thread metadata. `src/delegated/autonomy_runtime.rs` binds and later accounts for the model from the **persisted contract**. Its existing Terra-only helper remains for callers that have no contract. The controller compares stored app-server telemetry against the exact contract model/effort before Codex mutation; its old Terra escalation reason is preserved. Tests cover legacy identity, Sol acceptance, wrong effort, Terra downgrade, rejected binding without a session change, and CLI override construction.

## Verification and boundaries

- P0 review checkpoint `bc24000` was pushed and its [exact Windows CI](https://github.com/vnath99/CatDesk/actions/runs/38644625690) finished SUCCESS in Rust, Python Wake/advisor, and Independent WakeHost Rust.
- Local full `cargo test --locked --offline --bin catdesk -q` on an earlier R2 worktree revision exited 0: 1,058 passed, 26 ignored. A nested AppContainer denial printed during the run was an expected negative probe; it did not fail the outer suite.
- Final R2 source local `cargo test --locked --offline --bin catdesk delegated:: -q` passed: 416 passed, 16 ignored. `cargo fmt --check`, strict offline workspace Clippy over all targets/features, and `git diff --check` passed. Exact-commit Windows CI is still required before source acceptance.
- No installed WakeHost, signed CatDesk image, Secure MCP tunnel, browser authentication, old ambiguous Wake event, or hourly automation was changed.

## Remaining T-0483 gates

The project registration/bootstrap path still uses Terra/high by default. Sol is enabled only when an explicitly selected Sol contract has an exact matching Sol/medium Codex thread; a Terra canonical thread fails closed rather than being relabeled. A true Codex task must still execute with authoritative effective model/effort readback, produce a concrete deliverable and review record, then trigger one authentic `review_ready` through an approved installed WakeHost. That live cycle remains unproven. P0 dev.85 activation is separately held by the documented status-store authority mismatch and protected install approval.
