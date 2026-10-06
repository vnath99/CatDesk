# T-0219 Qwen3.8 Reliability Live Canary

## Run Classification

This run is a **Qwen3.8 live canary** for the CatDesk reliability/progress gate.

- **Exact model:** `qwen3.8:27b` (provider: `ollama`)
- **Task ID:** `T-0219-LIVE-CANARY`
- **Objective:** Validate the live Qwen3.8 reliability/progress gate by creating exactly one new acceptance report.

## CatDesk Sequence Followed

1. **Reconnaissance (bounded):** Attempted a minimal `read` of the target report path to confirm it did not already exist (file-not-found, as expected).
2. **Patch preview:** Created a single-operation `patch.preview` that adds only `docs/orchestrator/review_bundles/T-0219_QWEN38_RELIABILITY_LIVE_CANARY.md`.
3. **Patch apply:** Applied the exact previewed patch via `patch.apply`.
4. **Verification:** Ran `verify.run` under the `rust_full` profile.
5. **Authoritative diff:** Captured `diff.actual` for the allowed report path.
6. **Completion gate:** CatDesk rejected the premature completion claim because delegated verification had not passed; the run failed closed after its bounded corrective-turn budget.

## Scope Compliance

- Only the single allowed report file was created.
- No other files were edited.
- No forbidden paths (`.git`, `.catdesk`, `src`, `scripts`, `Cargo.toml`, `Cargo.lock`) were touched.
- No shell, Git push, merge, or external effects were performed.
- Pre-existing dirty-workspace changes are not claimed as this run's work.

## Acceptance

- Verification: **failed in the live delegated run because `cargo test` hit the old 30-second per-command timeout**; independent host verification passed when given 120 seconds, with the full test suite taking roughly a minute.
- CatDesk source fix: Qwen3.8 `verify.run` now enforces a 120-second minimum even if the model requests 30 seconds, preventing this false-negative repair loop while remaining bounded at 300 seconds.
- Authoritative diff: **captured** (see `diff.actual` result).
