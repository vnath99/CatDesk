# T-0424 — Shared Turn Timer Review

## Scope
Implement one shared CatDesk turn-budget mechanism for automatic Wake-event timing and ordinary ChatGPT Web work without duplicating timer math or breaking installed Wake dev.67 timer-file compatibility.

Changed for T-0424:
- `wake/src/store.rs`
- `wake/src/runtime.rs`
- `src/mcp.rs`

No install, promotion, reload, wake-target mutation, tunnel mutation, Git cleanup, commit, push, or publication is part of this task.

## Design
- The existing `TurnTimer` serialized schema is unchanged. No origin/version field was added because dev.67 deserializes with `deny_unknown_fields`.
- Existing `turns/` storage and exact project / target-generation / target-digest binding are reused.
- Ordinary-work timers use the reserved `manual-turn-` ID namespace.
- `start_manual_turn_timer`:
  - accepts only reserved valid IDs and valid project IDs;
  - requires the exact current configured target;
  - refuses queue/archive/delivery authority collision under that ID;
  - is restart-idempotent for the same active bound timer;
  - refuses restarting a completed timer.
- `complete_manual_turn_timer`:
  - applies only to reserved manual IDs;
  - requires the exact current target;
  - refuses queue/archive/delivery authority collision;
  - is idempotent once complete.
- Existing Wake receipt-bound paths `record_exact_receipt_timer`, `set_timer_response_state`, and `complete_timer` are unchanged. A Wake-event timer cannot be completed through the manual path.
- The existing Wake `turn_timer_status` projection is now public and remains the single implementation of:
  - 18-minute soft checkpoint;
  - 20-minute hard deadline;
  - elapsed/remaining calculations;
  - completed-timer freeze semantics.
- New root MCP tool: `catdesk_turn_timer`.
  - `START` accepts no timer ID and generates a reserved handle server-side.
  - `STATUS` requires an exact timer ID and can inspect manual or Wake-event timers.
  - `STOP` requires an exact timer ID and can complete only manual timers.
  - No caller-selected duration, path, target URL, target generation, target digest, executable, browser profile, or credential input exists.
  - Output is bounded to timer identity/origin, state, start time, elapsed/remaining seconds, and checkpoint/deadline booleans.

## Focused verification
- Root compile through `cargo test manual_turn_timer`: PASS.
- MCP timer regressions: `cargo test shared_turn_timer`: 2 passed / 0 failed.
- Wake manual timer regressions: `cargo test -p catdesk-wake manual_turn_timer`: 2 passed / 0 failed.
- Wake manual STOP isolation: `cargo test -p catdesk-wake manual_stop`: 1 passed / 0 failed.
- Shared Wake timer family: `cargo test -p catdesk-wake turn_timer`: 7 passed / 0 failed, including existing 18/20-minute projection behavior.
- Historical dev.67 JSON compatibility: `cargo test -p catdesk-wake dev67_turn_timer`: 1 passed / 0 failed.
- Broader MCP family: `cargo test mcp`: PASS.
- Root `cargo clippy`: PASS.
- Exact contract `cargo clippy --all-targets --all-features -- -D warnings`: PASS (`.catdesk/logs/codex-t0424-strict-clippy-20260925.log`).
- Generic `cargo fmt --check` and zero-filter full `cargo test` calls were rejected by the current generic MCP command-policy wrapper before execution; they are not test failures. Formal contract verification owns the approved CARGO_FMT/CARGO_TEST profiles.

## Inherited dirty-worktree note
The repository was already intentionally dirty before T-0424. In particular, `src/mcp.rs` contains substantial prior-session changes, and current source also contains a pre-existing verifier failure-diagnostics improvement. T-0424 does not claim authorship of those unrelated changes. Review T-0424 against the specific additions above and the direct-work baseline captured at claim time.

## Acceptance posture
Focused behavior, compatibility, compilation, MCP-family, Wake-timer, and clippy gates are green. Formal CatDesk contract verification remains authoritative for CARGO_FMT, full CARGO_TEST, CARGO_CLIPPY, authoritative diff, and final review.

## Verification prerequisite resolution — Codex, 2026-09-26 UTC

The previous WAITING_FOR_CHATGPT checkpoint is stateVersion 11, not 7. Exact root `cargo test` output was captured locally without changing the serving daemon or its command policy. It isolated one failure: `reviewed_build::tests::offline_worker_policy_never_inherits_ambient_cargo_home`. The test searched an arbitrary 6000-byte source window and missed environment clearing after command setup moved into a helper. A separately attributed prerequisite repair now uses bounded worker/helper regions and behaviorally verifies that command configuration clears a preconfigured sentinel and replaces ambient Cargo-home with the isolated path. No reviewed-build production behavior changed.

Formatting provenance is recorded separately in `CODEX_20260925_VERIFICATION_PREREQUISITE_PROVENANCE.md`. The exact contract gate is `cargo fmt --check`; root-only `cargo fmt` normalized eight files (two historical root examples, five root source files, one recovery test). This is current-source maintenance under the broader Wake-maturity goal, not timer implementation or T-0424 authorship of inherited work. The 53 additional Wake files found by the broader `--all` audit were left byte-identical. Saved before/after hashes and a formatter-only patch are retained under `.catdesk/logs/codex-verification-prerequisite-20260925/`; every normalized file was confirmed identical to formatter output from its saved baseline.

Post-repair verification:
- Exact root `cargo fmt --check`: PASS.
- Exact root `cargo test`: PASS, including main CatDesk 961 passed / 22 ignored and all downstream binary/integration suites; recovery PowerShell fixtures 4/4. Intentionally failing child-process fixtures remain expected parent-test evidence, not suite failures.
- Root `cargo clippy`: PASS.
- Root `git diff --check`: PASS (existing line-ending notices only).
- Logs: `.catdesk/logs/codex-t0424-full-tests-20260925-01.log` (original failure), `codex-t0424-full-tests-20260925-02.log` (complete passing run), `codex-t0424-fmt-before-20260925.log`, and `codex-t0424-clippy-20260925.log`.

The failed full-suite result is explained by this stale source-as-data test; there is no demonstrated concurrency defect. Formal finalization must still rerun the contract gates. After completion, preserve this bundle unchanged because its hash is review authority. Observe the ordinary final-review event through installed dev.67 into Chat34 generation 17 before acknowledging it. Full-source build/promotion review remains separate from this timer feature review.
