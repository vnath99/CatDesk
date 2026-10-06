# T-0047D Override-Free Current-User Codex Acceptance

Status: `HOST_ACCEPTANCE_PASSED__LIVE_DAEMON_ENV_MIGRATION_IN_PROGRESS`.

## Repair Applied

Normal host startup now resolves a direct Codex executable from the current
user's `PATH` without requiring `CATDESK_CODEX_CLI_EXECUTABLE`. On Windows,
when the normal npm entry is the `codex.cmd`/`.bat`/`.ps1` launcher, CatDesk
does not parse or execute that launcher. It rejects the adjacent extensionless
npm shim and considers only bounded package-local native executable locations
for the known Windows platform packages (with the prior package-local layout
retained for supported older installations).

`CATDESK_CODEX_CLI_EXECUTABLE` remains an advanced recovery override and has
strict precedence only when it names an existing direct executable. Shell
launchers fail closed rather than falling back to a different binary.

Normal app-server children explicitly remove `CATDESK_CODEX_HOME`,
`CATDESK_CODEX_CLI_EXECUTABLE`, `CODEX_HOME`, and API-key environment names.
They retain ordinary current-user environment inheritance (including PATH and
Windows user-profile variables). An explicit, validated `CATDESK_CODEX_HOME`
is still passed as `CODEX_HOME` only for advanced recovery. No configuration,
history, or credential contents are read, copied, or persisted.

## Deterministic Evidence

- `override_free_windows_npm_launcher_resolves_native_package_without_reading_shims`
  proves override-free resolution through the current bounded npm-native path,
  even with an extensionless shim present, without consulting launcher
  contents.
- `direct_recovery_override_has_precedence_over_current_user_path` proves
  explicit recovery precedence.
- `shell_launcher_override_is_rejected_without_falling_back_or_reading_config`
  proves a `.cmd` override fails closed even when PATH offers another binary.
- `override_free_launch_clears_recovery_and_api_variables_but_keeps_current_user_context`
  proves child-environment hygiene without erasing normal user context.
- No nested `codex exec` or `codex app-server` process was started by this
  coding worker.

## Host Acceptance

Host acceptance passed after the source repair. The first operator-local
read-only override-free probe intentionally removed `CATDESK_CODEX_HOME`,
`CATDESK_CODEX_CLI_EXECUTABLE`, `CODEX_HOME`, and API-key environment names
from that process and exposed a Windows discovery bug: an adjacent
extensionless npm `codex` shim was selected and failed to launch with Win32
error 193. T-0047D-R1 corrected the resolver so Windows accepts only a direct
native executable and otherwise uses the bounded known npm package-native
`.exe` layout without reading or executing launcher contents.

The exact same ignored host probe was rerun after repair and printed
`OVERRIDE_FREE_CURRENT_USER_CODEX_OK`: production current-user executable
discovery succeeded with recovery/API variables absent, the supported
app-server initialized, and metadata-only canonical-thread preparation passed
the exact-CWD, direct-input/idle, `gpt-5.6-terra` / `high` gate. The probe
accessed no credential/auth/config contents.

Native successor-daemon launch is additionally hardened to remove the six
historical recovery/API environment names from replacement CatDesk processes,
so routine daemon operation migrates onto the ordinary current-user Codex
context rather than perpetuating bootstrap overrides.

## Verification

- `cargo fmt --check`: PASS.
- `cargo test autonomy_runtime`: deterministic resolver coverage PASS; the
  ignored host-only override-free live probe separately passed with
  `OVERRIDE_FREE_CURRENT_USER_CODEX_OK`.
- `cargo test codex_app_server`: override-free child-environment and
  Terra/High metadata tests PASS; operator-local probes remain ignored by the
  ordinary suite.
- `cargo test daemon_reload::tests`: 7 passed, including successor-daemon
  recovery/API environment scrubbing and non-inheritable Windows MCP listener
  coverage.
- `cargo clippy --all-targets --all-features -- -D warnings`: PASS.
- Latest `cargo test --no-fail-fast`: 438 passed, 0 failed, 11 ignored.
- Latest `git diff --check`: PASS (only existing CRLF conversion warnings on
  the dirty working tree).

CatDesk independently performs daemon reload, host acceptance, verification,
and diff capture. Live daemon environment migration is complete only after a
successor started by the new scrub-aware reload helper has been verified.
