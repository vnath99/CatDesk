# T-0478 R3B3 R1 — Retained fixture-root lease and owned-child lifecycle proof

Date: 2026-10-11. Canonical CatDesk Chat52: https://chatgpt.com/c/6acba3bf-e740-8331-8896-8a25fefdbf16.

## Verified preceding work

- The previous R3B2 isolated in-memory fixture configuration change reached commit `fe90d69`; its initial Windows CI run `38156865303` failed at Rustfmt, and the narrowly scoped correction commit `6f07231a7ff996f0f077dc52d284805c7456e7c8` was independently verified in GitHub Windows CI `38157083333`, **all three jobs SUCCESS**.
- The generation-33 Chat52 independent WakeHost test `manual-wake-mcp-1791732042693` already achieved `SENT/EXACT_USER_MESSAGE_APPENDED/COMPLETE`. Current manual event `manual-wake-mcp-1791737671996` reached the correct chat and was `SUBMISSION_ACCEPTED` during the active response, but its exact terminal receipt had not yet appeared at turn start. Do not duplicate while it is in flight. This is manual delivery only, not natural acceptance.
- The hourly ChatGPT deadman remains disabled. The official Secure MCP tunnel and independent production WakeHost image are outside this disposable source-worker change.

## R3B3 root lifetime boundary

- Before R3B3, the fixture validated an OS temp direct-child root at startup, but `mcp::fixture_read_data` independently reacquired that root pathname per tool request. A cooperating or malicious same-user process could attempt to rename or replace the root between requests.
- `run_headless_mcp` now acquires a `windows_protected_fs::PinnedDirectory` **only** after the existing `--isolated-dev-fixture` scope check and retains its no-`FILE_SHARE_DELETE` directory handle for the entire listener future. The helper uses `FILE_FLAG_OPEN_REPARSE_POINT`, rejects reparse directories and checks retained identity via `assert_stable`. Any failure stops fixture startup. Normal production/headless routes have no new guard.
- The real Windows-only subprocess HTTP test attempts to rename the fixture root to a separate test-owned temp name **while** source-current test child serves read/search. Windows refuses the rename, confirming the lease is actually held during serving. After the bounded test child kill+wait, the test can rename the released root and clean it up. Test never kills or moves an unowned process or directory.
- Existing fixture MCP still permits **only** bearer-authenticated read/search with confined plain root-level files; in-memory fixture config and single-link no-follow read protections remain unchanged.

## Local verification and limitations

- Local Windows strict all-target Clippy `-D warnings` PASS, real subprocess fixture integration 1/1 PASS after the root-lease and cleanup-test changes, full `cargo test --locked --offline --bin catdesk` exit0 before the final cleanup-test refinement, `git diff --check` exit0. `cargo fmt --all` executed successfully; installed gateway sporadically returned INVALID_ARGUMENT for `cargo fmt --check`, so independent CI's exact formatting pass is required for this new commit.
- This prevents a simple same-user rename/root swap **while the server is alive**. It does not prove the original source data is trustworthy, protect against all ancestor-path or volume-level attacks, make mutable fixture files immutable, or grant authority to read real project files or execute code. Do not claim a general security sandbox.
- Remaining R3B: establish authenticated/signed fixture source snapshot and immutable manifest or equivalent reviewed provenance, constrain file replacement/content changes with identity+hash revalidation, supply an owner-bound fixture launcher with bounded PID/port allocation, disposal, safe restart, and crash recovery, and test junction/reparse/path races. Keep unrelated production stable-supervisor recovery and signed pending T0366 epoch2 maintenance separate.

## Publication/acceptance

- Commit only scoped `src/main.rs`, `tests/isolated_dev_fixture.rs`, this report, and current milestone/command/handoff docs to existing feature branch. Preserve the seven historical untracked paths. After push, independently verify new GitHub Windows Rust, Python Wake/advisor, and Independent WakeHost Rust CI jobs before calling R3B3 accepted.
