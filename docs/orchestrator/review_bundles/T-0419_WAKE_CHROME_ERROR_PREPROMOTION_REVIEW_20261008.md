# T-0419 Wake Chrome-error repair — bounded source review (2026-10-08)

## Authority and scope
- Workspace branch: `orchestrator/chatgpt-codex-autonomous-loop`; inspected HEAD `e8bbbc2`.
- Candidate source fix: commit `6f590ee` (`scripts/wake_bridge.py` plus regression tests). The current installed independent WakeHost remains `dev.84`, generation 31, and is **not** proven to contain this source fix.
- No browser submission, event replay, protected build, installation, daemon reload, or Git publication was performed in this review.

## Static behavioral review
- After the Send boundary, only `SUBMIT_TARGET_DRIFT_CHROME_ERROR` can invoke `reopen_submit_receipt_target`; unrelated host/conversation drift fails closed.
- Recovery navigates only to the immutable `self.url` through the existing CDP object. It does not type, click Send, or issue a second user action.
- A successful reopen merely resumes proof; it is **not** a delivery receipt. Acceptance still requires exact target, cleared composer, exact turn anchor, and a server-backed Stop/assistant signal.
- The durable receipt path marks the current JS document, reloads, requires the marker to disappear in a loaded exact-target document, and then separately verifies the uniquely latest exact USER digest.
- Follow-up review question: post-submit reopen uses `cdp.open(self.url)`, while pre-submit readiness recovery deliberately favors `cdp.get(self.url)` to avoid a new/switching tab. Existing mocks prove fixed URL and stale-document rejection, but do **not** independently prove tab/context continuity for the real SeleniumBase CDP implementation. Before promotion, confirm this method's real same-tab semantics or add an explicit context-continuity regression. Do not replace it blindly without fixture and integration coverage.

## Fresh verification evidence
- `pytest --collect-only -q`: 209 collected (73 advisor, 6 stable adapter, 115 bridge, 2 browser cleanup, 3 profile, 10 smoke).
- `pytest tests/advisors/test_deepseek_web_advisor.py -q`: 73 PASS.
- `pytest tests/test_stable_wake_browser_adapter.py -q`: 5 PASS, 1 SKIP (symlink privilege fixture).
- `pytest tests/test_wake_profile_login.py -q`: 3 PASS.
- `pytest tests/test_wake_smoke_state.py -q`: 10 PASS.
- Bridge partition runs: `-k chrome` 6 PASS; `-k target` 15 PASS; `-k response` 11 PASS; `-k submission` 7 PASS; `-k reconciliation` 2 PASS; `-k cleanup` 3 PASS; `-k receipt` 20 PASS; `-k page` 14 PASS. These subsets overlap and **must not** be summed as unique tests.
- Full `verify_project` and full `pytest tests/test_wake_bridge.py -q` exceeded the tool's execution window; this is **NOT** a passing full-suite result. Dedicated browser-cleanup file invocation was rejected by the command gateway and is also not claimed verified.
- The project currently has two equivalent untracked root pytest configurations (`pyproject.toml` and `pytest.ini`). Guarded deletion preview for `pyproject.toml` was rejected by platform safety checks; neither file was silently removed.

## Gates and next actions
1. Partition the remaining bridge tests into bounded nonoverlapping selections; obtain full Rust/Python acceptance without interpreting a timeout as success.
2. Resolve the `cdp.open` versus same-tab `cdp.get` semantic question with bounded deterministic coverage.
3. Consolidate root pytest configuration using an approved guarded file operation, preserving the working `pythonpath = .` behavior.
4. Obtain independent immutable source review and reviewed-build attestation; only then install/restart Wake and run exactly one **fresh** generation-31 event with exact appended USER receipt proof.
5. Do not replay ambiguous `manual-wake-mcp-1791459602150` or treat the generation-30 receipt as a generation-31 success.

## Canonical direct-chat reconciliation — 2026-10-08 12:xx EDT
- User disabled the sole enabled hourly CatDesk deadman; all older CatDesk hourly tasks were already disabled. Canonical project/Wake target generation 31 was independently checked and left unchanged.
- Prior task changes consisted only of dirty docs/tests and *untracked* review and pytest configuration files. Confirmed no new source commit, reviewed build, Wake package installation or event replay by that task. Source HEAD at initial reconciliation remained e8bbbc2.
- **Corrected a real safety gap before installation:** post-submit `reopen_submit_receipt_target` was using `cdp.open(self.url)` even though pre-submit `retry_page_readiness` in this same project warns that `open()` may switch/new tab and uses `get()`. The new candidate uses `cdp.get(self.url)` on the same CDP object, failing closed if method missing, checking exact target and fresh JS marker, and never retyping or clicking Send. Modified mock tests make `open()` raise if touched and assert the single exact `get()` destination plus other-host/chat, missing-get, stale-document rejections.
- Re-run `pytest tests/test_wake_bridge.py -k receipt -q`: 20 PASS. Re-run `pytest tests/test_stable_wake_browser_adapter.py -q`: 5 PASS, 1 SKIP (symlink fixture). `pytest --collect-only -q` 209 collected. The full bridge command exceeded the bounded tool window again; not accepted as full PASS.
- The two root pytest configs are equivalent but redundant. `pytest.ini` is the selected root config to be versioned; `pyproject.toml` was left untracked rather than bypass the `destructive_delete_enabled` deletion guard. In this local workspace pytest.ini takes precedence. Production clone receives only the reviewed tracked config.
- Any source/code commit here is a **candidate** only: no independent review finalized, no current installed Wake swap, no production acceptance, no protected-build promotion. Full Python/Rust verification and package review remain mandatory gates.
