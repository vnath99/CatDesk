# T-0478 R3B2 — Disposable fixture in-memory configuration

Date: 2026-10-11. Canonical Chat52: https://chatgpt.com/c/6acba3bf-e740-8331-8896-8a25fefdbf16.

## Grounded starting point

- R3B1 repair commit `9f6d0bc2b4adeb7d0f6ffaf03184033fef1c4132` has independent GitHub Windows Actions run `38151893173` **SUCCESS all three jobs** (Rust, Python Wake/advisor, independent WakeHost). Local tracked branch and origin were synchronized before this change.
- Independent WakeHost gen33 targeted Chat52 with exact SHA `d82593bf8a7eb82936cd63d1ed52f77243c46afa3f9aa57ca2d9a642a31b73ce`. Manual diagnostic `manual-wake-mcp-1791732042693` ended SENT with an EXACT_USER_MESSAGE_APPENDED receipt and event timer COMPLETE. This proves a manual delivery, **not** natural automatic Wake. All old hourly deadmen are disabled.
- Security finding: `src/main.rs::run_headless_mcp` called `validate_isolated_fixture_scope` (R3B1 no-follow/single-link preflight), then `AppState::new_headless`. That constructor calls `AppConfig::load_or_initialize_from_path_with_warnings`, which uses `path.exists`, `fs::read_to_string(path)`, and potentially config backup/save. Therefore fixture config data/behavior could be changed by pathname replacement after preflight, despite R3B1 handle-read proof.

## Source-only R3B2 changes

- `AppState::new_isolated_fixture` creates fresh in-memory `AppConfig::default`, initializes a unique transient installation ID, and shares the existing common state-initialization path without calling `load_or_initialize_from_path_with_warnings` or reading/writing `config.toml`.
- `AppState::from_prepared_config` refactors the existing constructor's body without changing the normal production/headless config-file-loading route. The new `fixture_config_in_memory` flag causes general `app_config()`, `persist_state()`, and `persist_state_with_log()` to refuse disk load/write for an isolated fixture.
- `run_headless_mcp` selects the new constructor **only** when `--isolated-dev-fixture` has passed existing strict temp-root/loopback/port-zero/auth validation. Other headless modes retain the normal constructor. The isolated router still exposes only bearer-authenticated read/search via POST /dev/mcp.
- New unit test verifies no config is created; both persistence paths fail closed; an existing malformed config file is ignored and unchanged. Real Windows HTTP fixture process test also verifies `config.toml` remains absent before and after serving requests.
- **Local verification:** strict `cargo clippy --locked --offline --workspace --all-targets --all-features -- -D warnings` PASS, focused in-memory test 1/1 PASS, real fixture HTTP integration 1/1 PASS, full `cargo test --locked --offline --bin catdesk` exit0, `git diff --check` exit0. A later `cargo fmt --all -- --check` invocation intermittently returned gateway INVALID_ARGUMENT (an earlier pre-test invocation passed before the additional regression was inserted), so independent Windows CI formatting result is required for this exact final source.
- **No** protected signing, Program Files, official Secure MCP, independent WakeHost, startup tasks, other projects, or production configuration files were mutated.

## Independent CI correction

- First R3B2 source commit `fe90d69365b2253abe10c6dcb4ae7716997c0290`, Windows CI run `38156865303`, had a genuine Rust `cargo fmt --check` failure for only four fragments in the newly added in-memory fixture unit test. Fmt failure occurred before CI Rust tests; it does not invalidate the prior passing local compilation, but CI has not yet accepted this exact source.
- Applied exactly the Rustfmt changes shown in that job's log (constructor call wrapping and two multi-line assertions). No fixture runtime behavior changed. Follow-up CI must establish fmt/Clippy/tests all terminal SUCCESS.

## Security limits / subsequent R3B

This removes the fixture config file's **startup parse/rewrite race**, not hostile same-user replacement of the **fixture root directory** or access to mutable fixture source files. It is still only an ephemeral read/search server, not a sandboxed autonomous coding agent. Next tasks: fixture root identity retained for the process lifetime or equivalent no-follow path chain, trusted source-copy manifest and immutable snapshot provenance, file content hashes, and owner-validated bounded PID/port/stop lifecycle with adversarial tests. Finish Windows CI before claiming R3B2 complete. Production stable supervisor recovery remains separate and incomplete; signed T0366 pending epoch2 remains untouched.
