# T-0042 App-Server Authentication / Continuity Repair Review Bundle

Status: `BLOCKED_INTERACTIVE_OPERATOR_LOGIN_OR_CONTEXT_SELECTION`.

## Outcome

T-0042 identified and repaired the CatDesk-side context-handoff gap without
reading, exporting, or persisting authentication material.  The local machine
does not currently expose an already-authorized Codex CLI context to the
CatDesk worker/app-server process, so the required authenticated live proof
cannot be completed safely.  No thread was created, no project/thread mapping
was written, and no daemon restart was performed.

The remaining action is an operator-controlled, supported Codex login or
selection of an existing authorized Codex config root.  It is a genuine
interactive account-context gate; CatDesk must not bypass it by scraping
browser/session data or by accepting a token, key, cookie, or cloud fallback.

## Supported Audit Evidence

- The normal PATH `codex` command resolved to an npm PowerShell shim, while
  `CATDESK_CODEX_CLI_EXECUTABLE` was present and identified an existing direct
  `.exe`; both reported `codex-cli 0.146.1`.
- The direct worker executable is distinct from the PATH shim.  Its supported
  `codex login status` response was `Not logged in`.
- `CODEX_HOME` and `XDG_CONFIG_HOME` were absent from the worker probe
  environment.  The default user config directory was present, but its
  contents were not inspected.
- Supported `codex app-server --help` documented `--stdio`; generated public
  app-server schema confirmed `initialize`, `thread/list`, `thread/start`,
  `thread/read`, `turn/start`, and `account/rateLimits/read`.
- A sequential, supported direct `codex app-server --stdio` probe using the
  direct worker executable produced: `initialize=SUCCESS`, exact-CWD
  `thread/list=SUCCESS; count=0; nextCursor=null`, and
  `account/rateLimits/read=ERROR: codex account authentication required to
  read rate limits`.  Only method names, counts, and the supported error text
  were retained; no response payloads or credential material were printed or
  persisted.

This establishes that the blocking condition is account context, not protocol
startup, executable selection, workspace normalization, or a stale candidate.

## Minimal Local Handoff

`CATDESK_CODEX_HOME` is now an optional operator-local directory setting.
When supplied, CatDesk validates only that it is a directory and passes that
opaque path to directly launched Codex CLI/app-server children as the
documented `CODEX_HOME` environment variable.

- `src/delegated/codex_cli.rs` applies the context to discovery and worker
  turn children, while retaining explicit removal of API-key environment
  variables.
- `src/delegated/codex_app_server.rs` adds
  `CodexAppServerLaunchConfigV1`, a direct-executable-only
  `codex app-server --stdio` launch builder that performs the same path-only
  handoff and removes API-key variables.
- The relaunch preflight scripts validate only the presence of the configured
  directory and redact both its path and contents.

The handoff neither opens the config directory nor makes it MCP-selectable.
It cannot manufacture an account session: absent an operator-provided
authorized context, Codex correctly reports the authentication gate above.

## Thread, Registry, And Continuity Safety

Canonical workspace:

`<USER_PROFILE>\OneDrive\Desktop\Projects\CatDesk-codex-loop`

The authenticated-read prerequisite did not pass.  Therefore:

- no existing canonical thread could be accepted;
- no `thread/start` replacement was attempted (T-0041's one-attempt limit is
  preserved until list/read authentication succeeds);
- no `thread/read`, reconnect/list durability check, or direct-input
  continuity turn was attempted;
- `autonomy_project_registry_bind` was not called and no mapping exists;
- the healthy daemon was not restarted merely to expose a write surface with
  no valid evidence.

This retains the required exact-workspace, direct-input, and single-writer
invariants.  A future authenticated run must re-list exact-CWD candidates;
only an `exact-unowned-direct-input` candidate may be bound and read back.

## Verification Performed

- `cargo fmt --check`: PASS.
- `cargo test codex_app_server -- --nocapture`: PASS (6 tests).
- `cargo test codex_cli -- --nocapture`: PASS (12 tests).
- `cargo test autonomy_supervisor::tests::project_registry_binding_is_canonical_evidence_gated_and_durable -- --nocapture`: PASS.
- `cargo clippy --all-targets --all-features -- -D warnings`: PASS.
- `cargo test --no-fail-fast`: completed with `399` passed, `7` failed, and
  `9` ignored.  The known environment-sensitive failures are unchanged:
  three advisor tests could not start their configured advisor program, the
  related cancellation expectation then failed, and three Windows
  process-tree cancellation tests received `ERROR: Access denied`.
- `git diff --check`: PASS (Git emitted existing line-ending warnings only).

No commit, branch change, push, merge, PR, release, billing operation, API-key
fallback, or browser/private-backend access occurred.

## Required Operator Follow-Up

In the operator session that launches CatDesk, either complete the supported
Codex CLI login or set `CATDESK_CODEX_HOME` to the already-authorized Codex
context directory (path only; do not send its contents to CatDesk).  Then
relaunch CatDesk through the existing local relaunch handoff and rerun the
supported `thread/list` and `account/rateLimits/read` probe.  No further
manual prompt relay is needed once those reads authenticate.
