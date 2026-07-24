# T-0012 Runtime Spike Review Log

Date: 2026-07-24
Branch: orchestrator/t-0012-feasibility

## Chat Summary

User approved the T-0012 runtime spike under a restricted scope:

- no OpenClaw, Node.js, npm package, system package, or persistent dependency installation without a separate approval packet;
- first search for existing OpenClaw, Node, and npm installations outside PATH;
- prefer temporary, isolated, reversible tests;
- do not change global PATH, shell profiles, registry, services, Ollama config, or existing CatDesk config;
- use only disposable repositories/workspaces for experimental calls;
- do not send secrets, credential values, tokens, or private keys to a model;
- keep CatDesk as the only intended file/shell authority;
- documentation-only changes approved;
- run fmt, clippy, and tests after edits;
- do not commit, push, open a PR, or merge.

## Commands Run

Baseline:

```powershell
git remote -v
git status --short
git switch -c orchestrator/t-0012-feasibility
git log -1 --oneline
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

Runtime discovery:

```powershell
Get-Command openclaw -ErrorAction SilentlyContinue
Get-Command ollama -ErrorAction SilentlyContinue
Get-Command node -ErrorAction SilentlyContinue
Get-Command npm -ErrorAction SilentlyContinue
Get-ChildItem -Path C:\Users\Volap -Recurse -Filter openclaw* -ErrorAction SilentlyContinue
& 'C:\Users\Volap\AppData\Local\Programs\Ollama\ollama.exe' --version
& 'C:\Users\Volap\AppData\Local\Programs\Ollama\ollama.exe' list
& 'C:\Users\Volap\AppData\Local\Programs\Ollama\ollama.exe' show qwen3.5:9b
& 'C:\Users\Volap\.cache\codex-runtimes\codex-primary-runtime\dependencies\node\bin\node.exe' --version
& 'C:\Users\Volap\.cache\codex-runtimes\codex-primary-runtime\dependencies\bin\fallback\pnpm.cmd' --version
```

Ollama probes:

```powershell
curl.exe -s http://127.0.0.1:11434/api/chat -H "Content-Type: application/json" --data-binary @-
```

Synthetic prompt bodies only:

- basic local completion marker;
- fake `get_project_status` tool schema;
- valid fake tool result continuation;
- malformed fake tool result continuation.

Source inspection:

```powershell
rg -n "PORT|WORKSPACE_ROOT|/mcp|health|bind|serve|listen|Router|post_mcp" src\main.rs src\server.rs src\mcp.rs
Get-Content src\server.rs | Select-Object -First 180
Get-Content src\main.rs | Select-Object -Skip 830 -First 70
Get-Content src\main.rs | Select-Object -Skip 2410 -First 80
Get-Content src\server.rs | Select-Object -Skip 1000 -First 110
rg -n "CATDESK|mcp_path|default_mcp|MCP_PATH|ToolMode|tool_mode" src\main.rs src\state.rs src\mcp.rs
Get-Content src\server.rs | Select-Object -Skip 225 -First 70
Get-Content src\state.rs | Select-Object -Skip 560 -First 70
Get-Content src\state.rs | Select-Object -Skip 110 -First 80
rg -n "fn generate_mcp_slug|mcp_slug" src\state.rs
```

MCP disposable-workspace tests:

```powershell
cargo test mcp::tests -- --nocapture
```

Final verification:

```powershell
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
git status --short
```

## Findings

- Initially, OpenClaw was not installed locally.
- Initially, system Node/npm were not available locally through PATH or common install locations.
- Codex has a bundled Node/pnpm runtime, but it was not used for dependency installation.
- Ollama is installed and `qwen3.5:9b` is available with tool-call capability.
- Qwen emitted structured tool calls and continued from valid tool results.
- Qwen did not reliably escalate malformed tool output; schema validation must be deterministic outside the model.
- CatDesk MCP tests passed.
- Live HTTP CatDesk MCP probing should wait for a headless local-only mode because the current TUI startup path, ngrok auth gate, and hidden random MCP slug make automation unsafe under the approved boundaries.

## Continuation After Operator Install

The operator later installed Node.js and OpenClaw. The following read-only checks were run on 2026-07-24 without onboarding OpenClaw, installing the Gateway daemon, creating Scheduled Tasks, adding providers, entering credentials, modifying PATH, or enabling native OpenClaw file/shell tools:

```powershell
node --version
npm --version
where.exe node
where.exe npm
openclaw --version
where.exe openclaw
openclaw doctor
openclaw config file
openclaw mcp status --verbose --json
$env:OPENCLAW_CONFIG_PATH = (Join-Path (Resolve-Path '.') '.tmp\openclaw-t0012\openclaw.json'); $env:OPENCLAW_STATE_DIR = (Join-Path (Resolve-Path '.') '.tmp\openclaw-t0012\state'); openclaw config file
```

Observed versions and paths:

- Node.js: `v24.18.0` at `C:\Program Files\nodejs\node.exe`.
- npm: `11.16.0` at `C:\Program Files\nodejs\npm` and `C:\Program Files\nodejs\npm.cmd`.
- OpenClaw: `OpenClaw 2026.7.1-2 (0790d9f)` at `C:\Users\Volap\AppData\Roaming\npm\openclaw` and `C:\Users\Volap\AppData\Roaming\npm\openclaw.cmd`.
- Default OpenClaw config path: `~\.openclaw\openclaw.json`.
- OpenClaw MCP registry: empty `servers` array at `C:\Users\Volap\.openclaw\openclaw.json`.
- Disposable env path check: `OPENCLAW_CONFIG_PATH` redirected config lookup to `~\OneDrive\Desktop\Projects\CatDesk\.tmp\openclaw-t0012\openclaw.json`; no config file was created, but OpenClaw initialized SQLite state under `.tmp\openclaw-t0012\state\state\openclaw.sqlite`.

`openclaw doctor` reported Gateway mode/auth/command owner/state are unconfigured, Gateway service is not installed, plugins loaded without errors, and status failed with `GatewayCredentialsRequiredError` because gateway credentials are not configured.

Minimum nonpersistent test posture:

- Use process-scoped `OPENCLAW_CONFIG_PATH` and `OPENCLAW_STATE_DIR`.
- Register CatDesk under `mcp.servers.catdesk` only after a deterministic local-only CatDesk MCP endpoint exists.
- Use `tools.profile: "minimal"` plus `tools.alsoAllow: ["bundle-mcp"]`.
- Deny OpenClaw-native `group:runtime`, `group:fs`, `group:web`, `group:ui`, and `group:automation`.
- Set `tools.exec.mode: "deny"`, `tools.exec.applyPatch.enabled: false`, `tools.elevated.enabled: false`, and code mode disabled.
- Do not use `tools.deny: ["bundle-mcp"]`, because that disables configured MCP servers.

## Headless MCP Implementation Continuation

After review, CatDesk received a constrained experimental `--headless-mcp` startup mode to unblock local orchestration probes without using the TUI, ngrok, browser/devtools, or the default CatDesk config.

Code changes:

- `.gitignore`: ignores `/.tmp` disposable probe state.
- `src/main.rs`: adds headless option parsing, loopback-only host validation, fixed `--mcp-path` support, required `--config-path`, JSON readiness output, and a headless axum server runner.
- `src/state.rs`: adds `AppState::new_headless` and skips startup mascot archiving for headless/disposable state.

Focused tests:

```powershell
cargo test headless -- --nocapture
```

Result:

- 4 passed.
- Covered parsing a valid headless command, rejecting non-loopback host, rejecting missing disposable config path, and rejecting browser/both mode.

Live CatDesk headless probe:

```powershell
cargo build
target\debug\catdesk.exe --headless-mcp --host 127.0.0.1 --port 33312 --workspace .tmp\catdesk-headless-t0012\workspace --mcp-path /t0012-headless/mcp --mode computer --tool-mode read-only --config-path .tmp\catdesk-headless-t0012\config\config.toml --no-ngrok
curl.exe -s http://127.0.0.1:33312/
curl.exe -s http://127.0.0.1:33312/t0012-headless/mcp -H 'Content-Type: application/json' --data-binary '@.tmp\catdesk-headless-t0012\initialize.json'
curl.exe -s http://127.0.0.1:33312/t0012-headless/mcp -H 'Content-Type: application/json' --data-binary '@.tmp\catdesk-headless-t0012\tools-list.json'
curl.exe -s http://127.0.0.1:33312/t0012-headless/mcp -H 'Content-Type: application/json' --data-binary '@.tmp\catdesk-headless-t0012\read-hello.json'
```

Result:

- Health endpoint returned CatDesk status for the disposable workspace.
- MCP `initialize` succeeded.
- MCP `tools/list` exposed only read-only CatDesk tools.
- MCP `read` returned disposable `hello.txt` content.
- Read-only probing did not create `.catdesk` in the disposable workspace.
- A disposable CatDesk config file was created at the explicit `.tmp\catdesk-headless-t0012\config\config.toml` path.
- The headless CatDesk process was stopped after probing.

OpenClaw disposable MCP probe:

```powershell
$env:OPENCLAW_CONFIG_PATH = (Join-Path (Resolve-Path '.') '.tmp\openclaw-headless-t0012\openclaw.json')
$env:OPENCLAW_STATE_DIR = (Join-Path (Resolve-Path '.') '.tmp\openclaw-headless-t0012\state')
openclaw mcp add catdesk-t0012 --url http://127.0.0.1:33313/t0012-openclaw/mcp --transport streamable-http --include catdesk_instruction,read,search,plan_read --timeout 10 --connect-timeout 5
openclaw mcp tools catdesk-t0012 --include "catdesk_instruction,read,search,plan_read"
openclaw mcp status --verbose --json
openclaw mcp probe catdesk-t0012 --json
openclaw config validate --json
```

Result:

- OpenClaw saved the server only to `.tmp\openclaw-headless-t0012\openclaw.json`.
- The first unquoted include filter collapsed to one bad string and exposed zero tools.
- Quoting the include CSV corrected the filter.
- `openclaw mcp probe` reported exactly four tools:
  - `catdesk-t0012__catdesk_instruction`
  - `catdesk-t0012__plan_read`
  - `catdesk-t0012__read`
  - `catdesk-t0012__search`
- `openclaw config validate --json` returned `valid: true` with no warnings.
- No OpenClaw onboarding, Gateway service, provider setup, credential entry, default OpenClaw config mutation, or model worker run was performed.

## Files Included In Review Bundle

- `.gitignore`
- `src/main.rs`
- `src/state.rs`
- `docs/orchestrator/T-0012_RUNTIME_SPIKE.md`
- `docs/orchestrator/T-0012_RUNTIME_COMPARISON.md`
- `docs/orchestrator/T-0012_RECOMMENDATION.md`
- `docs/orchestrator/tickets/T-0012.md`
- `review-bundles/T-0012-runtime-spike-log.md`

## Verification Results

- `cargo fmt --check`: passed
- `cargo clippy --all-targets --all-features -- -D warnings`: passed
- `cargo test`: passed, 114 tests

## Recommendation

Do not onboard or persistently configure OpenClaw yet. The next gate is effective worker tool-list inspection with OpenClaw-native runtime/file/web/browser/automation tools denied and CatDesk MCP explicitly allowed. Do not run a model worker until that list is inspectable and clean.
