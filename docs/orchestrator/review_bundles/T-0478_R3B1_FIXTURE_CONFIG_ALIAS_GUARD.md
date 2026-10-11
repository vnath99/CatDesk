# T-0478 R3B1 — Fixture config alias check and Chat52 handoff
Date: 2026-10-11. Canonical Chat52: https://chatgpt.com/c/6acba3bf-e740-8331-8896-8a25fefdbf16

## Verified baseline
- Existing CatDesk project registry and independent Wake target were updated together through the designated paired `autonomy_project_registry_bind` CAS: both are Chat52 SHA-256 `d82593bf8a7eb82936cd63d1ed52f77243c46afa3f9aa57ca2d9a642a31b73ce`, independent Wake generation 33. Host dev.84 RUNNING, queue zero; old Chat51 manual event `manual-wake-mcp-1791693864808` remains RESPONSE_SEQUENCE_DRIFT, without an event-specific SENT receipt. Do not replay it or claim natural acceptance.
- Independent GitHub Actions run `38147692024` for prior T0478 R3A source commit `bedfe05` is now terminal SUCCESS, all three Windows Rust, Python Wake/advisor and Independent WakeHost Rust jobs.
- The user-requested Chat51 hourly deadman `6acaaebce5b08191859800991962180d` and all older CatDesk hourly automations are independently confirmed disabled. Do not reactivate.
- Existing seven unrelated historical untracked files untouched. Production stable supervisor ROOT_UNAVAILABLE, STARTUP_POLICY_UNPROVEN, STARTUP_DEFINITION_READ_FAILED and frontDoor3201 not probed: recovery is still incomplete. Do not rotate pending signed T0366, mutate protected product state or external Secure MCP runtime.

## R3B1 source-only delta
- `src/main.rs::validate_isolated_fixture_scope` originally checked `config.toml` via `symlink_metadata`, requiring an existing ordinary file or a missing file, but did not reject a hardlinked regular file. Fixture file reads had R3A pinned parent/no-follow and link-count-1 checks; config startup did not.
- The source guard now checks the pre-existing config metadata on Windows for `FILE_ATTRIBUTE_REPARSE_POINT == 0` and `number_of_links() == 1`. An additional Windows regression creates an external temp config and hardlinks it as fixture `config.toml`, then requires scope validation to fail before removing only test-owned entries.
- This is a narrow preflight check only: it does **not** pin `config.toml` through later normal headless config creation/reading, solve TOCTOU against malicious same-user replacement after preflight, guarantee fixture-copy provenance, or create a coding worker. Further R3B hardening must bind root/config handles at startup and enforce stable identity throughout fixture use.
- Verification at handoff: `cargo fmt --all -- --check` passed. Dedicated `verify_project` timed out without a result and focused `cargo test` through the installed MCP command gateway returned INVALID_ARGUMENT; Clippy invocation through that gateway was refused. This source must NOT be treated as independently passing tests until proper local or GitHub CI succeeds.

## Next safe actions
Run focused Windows test `isolated_dev_fixture_rejects_production_and_outside_config` and full fmt/Clippy/test in GitHub Actions; if any fails, correct only observed regressions. Continue R3B separate stages: verified snapshot copy/provenance, fixed pinned root/config identity and adversarial replacement tests, then owner-bound PID/port/stop lifecycle. Check no competing writer; publish only scoped verified source/docs; use manual Wake only after exact stale Chat51 event reconciliation and target generation-33 verification.
