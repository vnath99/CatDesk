# T-0478 R2 — Isolated development worker readiness audit (design, not authority)

2026-10-11 UTC. Follows T0478 R1 fixed source-only `status/build/verify` facade. No production or developer worker was started.

## Inspected current implementation

1. `src/main.rs::parse_headless_mcp_options` already supports an explicit `--headless-mcp` with loopback-only `--host`, numeric `--port`, `--workspace`, `--config-path`, `--mode`, `--tool-mode` and a process-supplied bearer auth token (>=24 nonwhitespace). The normal production listener is 127.0.0.1:3200; do not reuse it for development. It refuses browser-enabled mode. The parser *requires* a config path explicitly so tests never default to the normal user config. Source tests already exist for these invariants.
2. `src/main.rs::run_headless_mcp` uses `AppState::new_headless` with that explicit config, builds the MCP router with process-scoped bearer auth, binds a loopback listener and does **not** itself launch OpenAI's Secure MCP tunnel, an external tunnel client, or the normal production native-daemon runner. Passing `--port 0` lets the OS allocate a non-conflicting loopback port in source, but needs an explicit acceptance test.
3. `src/state.rs::AppState::new_headless` delegates to `from_config_path_with_archive(..., false)` which suppresses the normal startup mascot archive and uses the explicit configuration path; `user_home_dir/app_config_path` otherwise points to HOME/USERPROFILE and must never be used by an isolated launcher.
4. `src/delegated/autonomy_runtime.rs` and related MCP handlers may still derive `.catdesk/autonomy`, `.catdesk/projects`, and other writable worker state from `workspace_root`. A developer workspace MUST be a separate verified source fixture, **not** the real CatDesk working directory; otherwise read-only presentation mode alone does not prevent a delegated worker from mutating real task/registry data.
5. Some MCP tools use `catdesk_wake::runtime::default_root()` (derived from current user's LOCALAPPDATA) or other global state **independently of workspace**. Even a separate `--workspace` and `--config-path` are NOT sufficient to prove the dev instance cannot access the installed production WakeHost or global accounts. Do not permit untrusted tool dispatch merely based on the generic `--tool-mode read-only` switch until full tool catalog enforcement is audited.

## Required R2 isolation mechanism before any `run` command

- Introduce an explicit closed `--catdesk-isolated-dev-worker` mode with source-verified own state and a *strict allowed tool list* (ideally status/health/fixture-only read operations, no generic run_command/write/delegation, no Wake, no Github, no tunnel, no review/receipt/install/signer operations). Deny all other RPC methods at the router boundary. Do not simply trust tool-mode string labels.
- Derive dev workspace/config/port/auth token from a fresh self-owned dev root, prevent symlinks/junctions and reject intersection with the production checkout's `.catdesk`, %%LOCALAPPDATA%% CatDeskWake and Program Files/ProgramData roots.
- Require loopback ephemeral port, no external tunnel ownership or registration, no profile/browser startup, no service/TaskScheduler activity. Save process PID/port only inside the dev state root with a controlled lock and make status/restart/stop apply only to that exact child/identity.
- Add adversarial Rust + Windows PowerShell CI coverage: sandbox refuses global independent Wake, config roots and arbitrary paths; no mutating/privileged MCP methods, token never printed; production loopback3200 and official runtime stay untouched; stale PID and conflicting lock fail closed; crash restart and bounded shutdown validated.
- Only after these gates pass may `scripts/catdesk-dev-lane.ps1` acquire a `run`/`stop` operation. Current R1 remains explicitly `isolatedRuntimeReady=false`.

## Production Recovery remains a separate block

Stable supervisor root unavailable, startup definition COM read failed and ownership policy unproven; frontdoor3201 was not verified. Existing CatDesk source typed `catdesk.ps1 diagnose` exists but old serving command gateway rejected the invocation. The recommended remedy is future versioned, reviewed production worker activation, not a headless development process impersonating the production tunnel.

Manual Wake on Chat51 generation32 has exact `EXACT_USER_MESSAGE_APPENDED` receipt, SENT and timer COMPLETE; natural automatic acceptance remains separate. Keep the installed WakeHost and official Secure MCP tunnel untouched. The signed T0366 pending epoch2 is historical maintenance, not a dev-loop prerequisite.
