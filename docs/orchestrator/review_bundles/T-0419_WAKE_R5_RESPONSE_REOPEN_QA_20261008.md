# T-0419 — Wake Chrome-error source verification checkpoint (2026-10-08)

## Decision
**NOT READY FOR IMMUTABLE WAKE INSTALLATION.** This is a bounded source QA checkpoint, **not** an independent final-review approval and **not** a release attestation.

## Reviewed candidate
- Source branch `orchestrator/chatgpt-codex-autonomous-loop`; source fix `083404a` and follow-up `94fe507`.
- Current installed WakeHost `1.0.0-dev.84`, independent target generation 31, canonical `https://chatgpt.com/c/6ac6cbe8-6f0c-83ea-9f7d-13489d4d87f5`.
- Existing current manual Wake `manual-wake-mcp-1791459602150` was visibly submitted, but persisted receipt absent and sender status `SUBMIT_TARGET_DRIFT_CHROME_ERROR` / `SUBMITTING`; no repeat/resubmit was attempted.
- User explicitly disabled hourly deadman. No recurring fallback should be restored; future manual Wake tests must follow reviewed installation.

## Defects and closed fixes
1. Source `083404a` corrected post-submit Chrome-error fallback from tab-switching-capable `cdp.open()` to existing same-CDP/tab `cdp.get()`. Navigation still requires only the fixed canonical URL, no Send/USER resubmission, and post-navigation exact target/new document/unique USER receipt checks.
2. Source audit discovered second same-tab variable regression: `reopen_response_target()` had assigned `navigate = getattr(cdp, "get", None)` but called/checks undefined `opener`. Commit `94fe507` changes both references to `navigate`, preserving expected fail-closed `RESPONSE_REOPEN_UNAVAILABLE` and `RESPONSE_REOPEN_FAILED` errors.
3. Added direct regression `test_response_reopen_uses_same_cdp_tab_and_fails_closed_when_missing`: asserts only fixed current-tab `get()` is invoked; alternative `open()` raises; missing/failing `get()` both reject. This covers a previously unexercised branch instead of mocking it away.

## Verification evidence
- `pytest -k response_reopen -q`: PASS 1 test.
- `pytest -k receipt -q`: PASS 21 selected tests project-wide.
- `pytest tests/test_stable_wake_browser_adapter.py -q`: 5 PASS, 1 SKIP (previously known Windows symlink-permission fixture).
- `pytest tests/test_wake_profile_login.py -q`: 3 PASS.
- `pytest tests/test_wake_smoke_state.py -q`: 10 PASS.
- `cargo fmt --all -- --check`: PASS.
- `cargo clippy --locked --offline --all-targets --all-features -- -D warnings`: PASS.
- `git diff --check`: PASS before source commit.
- Local/remote feature branch HEAD matched before new source work and source changes were committed/pushed.
- `pytest tests/test_wake_bridge.py --collect-only -vv`: collected 115 test cases, including direct regression.
- Full `pytest -q` and full bridge run **not accepted**: the CatDesk command bridge returned `INVALID_ARGUMENT` or disconnected on long runs. These are transport/runtime blockers, not evidence of test success/failure.
- Broad Rust `verify_project` had previously returned `cargo test` exit 101; exact fixture classification and full-suite acceptance remain unresolved.

## Security and release boundaries
- No external secure tunnel modification, reviewed-build retry, canonical release promotion, WakeHost installation/restart, or duplicate USER wake.
- Preserve the six untracked files: five historical diagnostics/forensics and redundant untracked `pyproject.toml`, deletion of which is blocked under guarded configuration. Only tracked `pytest.ini` is part of official checkout.
- Source checkpoint is **not an independently accepted Wake installation**. Do not promote based solely on focused tests or this note.

## Next bounded actions
1. Obtain deterministic full Python test proof in approved, bounded nonoverlapping batches; isolate long-run hang and any failing fixture with final commands/receipts. Obtain full Rust failure classification.
2. Independent source review of exact source-current Git HEAD, review both Chrome-error receipt and response-reopen paths, CI/test scope and file-change boundaries.
3. Approved immutable Wake package build/install only after review and test gates. Then **exactly one** fresh generation-31 manual Wake test, requiring durable current-generation `EXACT_USER_MESSAGE_APPENDED`, no duplicate USER send, terminal SENT and timer COMPLETE.
4. Continue separate T-0419 `ring 0.17.14` protected Cargo build diagnosis; no protected retry unless cause and review authority justified.

OPERATOR ACTION: NONE. Continuation through direct current ChatGPT conversation; hourly deadman disabled.
