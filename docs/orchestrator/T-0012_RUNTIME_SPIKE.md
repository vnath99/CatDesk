# T-0012 Runtime Spike

Date: 2026-07-24
Branch: orchestrator/t-0012-feasibility
Base commit: 3e4a512

## Scope

Evaluate whether CatDesk can act as the only file and shell execution authority while an OpenClaw/Ollama worker runtime provides planning, delegation, and model orchestration. The first pass was documentation-only because OpenClaw was not installed locally. On 2026-07-24, Node.js and OpenClaw were installed externally by the operator, so this note was continued with read-only local runtime checks.

## Boundaries Applied

- No OpenClaw onboarding, Gateway daemon install, Scheduled Task creation, provider setup, credential entry, native file/shell tool enablement, system PATH modification, Ollama config change, or CatDesk config change was made by this spike.
- Node.js and OpenClaw were already present on PATH before the continuation checks below.
- No repository secrets, environment variable values, private keys, or tokens were sent to Ollama, OpenClaw, or any model.
- Only localhost Ollama API calls were made, using short synthetic prompts with no private project content.
- CatDesk MCP behavior was exercised through the existing Rust MCP test module, which creates disposable temporary workspaces.
- A live CatDesk HTTP MCP probe was not run because the current TUI startup path requires interactive mode selection, an ngrok auth gate when no token is saved, and a random per-run MCP slug that is not exposed by health output.

## Local Runtime Discovery

OpenClaw initial pass:

- `Get-Command openclaw` returned no executable.
- Common user install locations were searched for `openclaw*`; no local OpenClaw binary or shim was found.

Node/npm initial pass:

- `Get-Command node` and `Get-Command npm` returned no system executable.
- Common locations under `%APPDATA%`, `%LOCALAPPDATA%`, Program Files, Scoop, Volta, and nvm-style paths did not reveal a separate user/system Node/npm install.
- Codex's bundled runtime does include an isolated Node executable at `C:\Users\Volap\.cache\codex-runtimes\codex-primary-runtime\dependencies\node\bin\node.exe`, reporting `v24.14.0`, and a bundled pnpm reporting `11.9.0`. These were not used to install OpenClaw.

Node/npm/OpenClaw continuation check after operator install:

- `node --version` reported `v24.18.0`.
- `npm --version` reported `11.16.0`.
- `where.exe node` resolved `C:\Program Files\nodejs\node.exe`.
- `where.exe npm` resolved both `C:\Program Files\nodejs\npm` and `C:\Program Files\nodejs\npm.cmd`.
- `openclaw --version` reported `OpenClaw 2026.7.1-2 (0790d9f)`.
- `where.exe openclaw` resolved both `C:\Users\Volap\AppData\Roaming\npm\openclaw` and `C:\Users\Volap\AppData\Roaming\npm\openclaw.cmd`.
- `openclaw config file` reported `~\.openclaw\openclaw.json`.
- `openclaw mcp status --verbose --json` reported path `C:\Users\Volap\.openclaw\openclaw.json` and an empty `servers` array.
- Setting process-only `OPENCLAW_CONFIG_PATH` and `OPENCLAW_STATE_DIR` made `openclaw config file` resolve to `~\OneDrive\Desktop\Projects\CatDesk\.tmp\openclaw-t0012\openclaw.json`; no config file was created, but OpenClaw did create disposable workspace-local SQLite state at `.tmp\openclaw-t0012\state\state\openclaw.sqlite`.

`openclaw doctor` result after operator install:

- Gateway mode is unset, so gateway start would be blocked until configured.
- Gateway auth is off or missing a token.
- No command owner is configured.
- State integrity reported missing OAuth/session state directories.
- Skills status showed 17 eligible, 27 missing requirements, 7 incompatible, 0 blocked by allowlist.
- Plugins status showed 32 loaded, 0 imported, 1 disabled, 0 errors.
- Gateway service is not installed.
- The command exited after reporting `GatewayCredentialsRequiredError: gateway status requires credentials before opening a websocket`.

Ollama:

- `Get-Command ollama` found `C:\Users\Volap\AppData\Local\Programs\Ollama\ollama.exe`.
- `ollama --version` reported `ollama version is 0.24.0`.
- `ollama list` showed multiple installed local models, including `qwen3.5:9b`.
- `ollama show qwen3.5:9b` reported a Qwen 3.5 9.7B Q4_K_M model with 262144 context length and capabilities including completion, vision, tools, and thinking.

## Ollama/Qwen Probe

Endpoint: `http://127.0.0.1:11434/api/chat`

Model: `qwen3.5:9b`

Findings:

- Basic chat completion succeeded with the expected marker text.
- Tool-call emission succeeded: given a fake tool schema named `get_project_status`, Qwen emitted a structured tool call with JSON arguments.
- Tool-result continuation succeeded with valid JSON tool output.
- Malformed tool output was not reliably escalated. When given a malformed tool result string and instructed to escalate, Qwen rationalized the malformed string instead of rejecting it.

Implication:

Worker prompts may ask for strict escalation, but the runtime must not rely on the model alone to validate tool output. CatDesk/OpenClaw glue should validate required tool-result schemas before the worker sees them, and malformed CatDesk tool results should become deterministic orchestration failures.

## CatDesk MCP Probe

Executed:

```powershell
cargo test mcp::tests -- --nocapture
```

Result:

- 38 MCP tests passed.
- Coverage included JSON-RPC lifecycle, tool listing, read-only mode, no `.catdesk` initialization from read-only tools, plan enforcement, shell chaining rejection, delete dry-run and confirmation flow, verification, Git status warnings, file write/edit/delete widget payloads, project memory, task queue, prompt templates, repo map generation, and run-command behavior.

Initial live HTTP probe status:

- The first pass did not run a live HTTP probe because startup required TUI input, ngrok setup, and a random in-memory MCP slug.
- That was not a CatDesk MCP correctness failure; it was a testability/operability gap for delegated orchestration spikes.

Headless MCP continuation:

- Added an experimental `--headless-mcp` startup path for local orchestration probes.
- The headless path binds only to a loopback host; non-loopback hosts such as `0.0.0.0` are rejected.
- The headless path requires `--config-path`, so tests do not read or write the normal CatDesk config.
- The headless path skips TUI startup, ngrok setup, browser/devtools startup, and startup mascot archiving.
- The headless path accepts `--workspace`, `--port`, `--mcp-path`, `--mode computer`, and `--tool-mode`.
- Browser and both modes are rejected for headless mode in this first implementation because they would start an additional browser/devtools authority.
- The server prints one JSON readiness line with `status`, `workspace`, `config_path`, and `mcp_url`.

Live headless probe:

- Started `target\debug\catdesk.exe --headless-mcp --host 127.0.0.1 --port 33312 --workspace .tmp\catdesk-headless-t0012\workspace --mcp-path /t0012-headless/mcp --mode computer --tool-mode read-only --config-path .tmp\catdesk-headless-t0012\config\config.toml --no-ngrok`.
- `GET /` returned health JSON with mode `Computer`, tool mode `read-only`, and the disposable workspace path.
- JSON-RPC `initialize` succeeded.
- JSON-RPC `tools/list` returned only read-only CatDesk tools.
- JSON-RPC `tools/call` for `read` successfully read `hello.txt` from the disposable workspace.
- No `.catdesk` directory was created in the disposable workspace.
- A disposable config file was created under the explicitly supplied `.tmp\catdesk-headless-t0012\config\config.toml` path during MCP usage.
- The process was stopped after the probe.

OpenClaw disposable MCP probe:

- Started a second read-only headless CatDesk endpoint at `http://127.0.0.1:33313/t0012-openclaw/mcp`.
- Set process-scoped `OPENCLAW_CONFIG_PATH` and `OPENCLAW_STATE_DIR` to `.tmp\openclaw-headless-t0012`.
- Ran `openclaw mcp add catdesk-t0012 --url http://127.0.0.1:33313/t0012-openclaw/mcp --transport streamable-http --include catdesk_instruction,read,search,plan_read --timeout 10 --connect-timeout 5`.
- The first unquoted PowerShell include attempt produced one bad include string and exposed zero tools; this was corrected with `openclaw mcp tools catdesk-t0012 --include "catdesk_instruction,read,search,plan_read"`.
- `openclaw mcp status --verbose --json` reported the disposable server as configured, enabled, and ok.
- `openclaw mcp probe catdesk-t0012 --json` reported four tools: `catdesk-t0012__catdesk_instruction`, `catdesk-t0012__plan_read`, `catdesk-t0012__read`, and `catdesk-t0012__search`.
- `openclaw config validate --json` reported `valid: true` with no warnings.
- No OpenClaw onboarding, Gateway service, provider setup, credential entry, default OpenClaw config mutation, or model worker run was performed.

## OpenClaw Feasibility

OpenClaw is now installed and basic read-only CLI checks work, but it has not been onboarded and no Gateway service is installed. `openclaw doctor` confirms the gateway is intentionally unconfigured.

Relevant official docs reviewed:

- https://docs.openclaw.ai/install
- https://docs.openclaw.ai/install/installer
- https://docs.openclaw.ai/install/uninstall
- https://docs.openclaw.ai/install/node
- https://docs.openclaw.ai/cli/mcp
- https://docs.openclaw.ai/tools
- https://docs.ollama.com/integrations/openclaw

Important OpenClaw constraints observed from docs:

- OpenClaw has native runtime/file/shell-style tools in its own tool catalog.
- OpenClaw MCP configuration supports server-level `toolFilter` include/exclude lists for MCP servers, but this filters MCP-server tools, not necessarily OpenClaw's own native authority.
- OpenClaw tool policy can remove tools before the model call, but this must be verified on an installed runtime before treating CatDesk as the only execution authority.
- OpenClaw's MCP server mode (`openclaw mcp serve`) exposes OpenClaw-routed conversations to an MCP client; OpenClaw's MCP client configuration manages outbound MCP servers.
- OpenClaw honors `OPENCLAW_CONFIG_PATH` and `OPENCLAW_STATE_DIR`, which is enough to route test config/state into a disposable workspace path without touching the default `~\.openclaw\openclaw.json`.
- Even read-only config-path checks may initialize SQLite state under `OPENCLAW_STATE_DIR`; use a disposable path and clean it up explicitly after the approved test.
- OpenClaw-managed MCP servers are exposed under the `bundle-mcp` plugin id. `tools.deny: ["bundle-mcp"]` disables them, so this must not be used for the CatDesk MCP registration test.
- A minimal CatDesk-MCP test posture should use `tools.profile: "minimal"` plus `tools.alsoAllow: ["bundle-mcp"]`, with `tools.deny` covering at least `group:runtime`, `group:fs`, `group:web`, `group:ui`, and `group:automation`.
- Host shell execution should also be blocked with `tools.exec.mode: "deny"`, and `tools.exec.applyPatch.enabled: false` should be set because `apply_patch` is separately enabled by default.
- OpenClaw code mode should remain disabled for this spike because code mode exposes `exec` and `wait` as the model-facing surface.

## Minimum Nonpersistent OpenClaw Test Configuration

Do not write this into the default OpenClaw config. The next test should run only with process-scoped environment variables:

```powershell
$env:OPENCLAW_CONFIG_PATH = "$PWD\.tmp\openclaw-t0012\openclaw.json"
$env:OPENCLAW_STATE_DIR = "$PWD\.tmp\openclaw-t0012\state"
```

Candidate disposable config shape, to create only after approval:

```json
{
  "tools": {
    "profile": "minimal",
    "alsoAllow": ["bundle-mcp"],
    "deny": ["group:runtime", "group:fs", "group:web", "group:ui", "group:automation"],
    "exec": {
      "mode": "deny",
      "applyPatch": {
        "enabled": false
      }
    },
    "elevated": {
      "enabled": false
    },
    "codeMode": {
      "enabled": false
    },
    "toolSearch": false
  },
  "mcp": {
    "servers": {
      "catdesk": {
        "url": "http://127.0.0.1:33200/t0012/mcp",
        "transport": "streamable-http",
        "toolFilter": {
          "include": ["catdesk_instruction", "list_files", "read", "search", "run_command"],
          "exclude": []
        }
      }
    }
  }
}
```

The exact CatDesk URL and tool list depend on adding or otherwise exposing a deterministic local-only CatDesk MCP endpoint first. The important OpenClaw property is that configured MCP remains allowed through `bundle-mcp` while OpenClaw-native runtime/file/browser/web/automation tools are denied.

Read-only validation steps for the disposable config should be:

1. `openclaw config validate`
2. `openclaw mcp status --verbose --json`
3. `openclaw mcp doctor catdesk --json`
4. `openclaw mcp probe catdesk --json` only after a disposable CatDesk MCP server is already running on `127.0.0.1`.
5. Inspect the effective worker tool list before any model/provider run.

Stop for approval before creating that disposable config, probing the live CatDesk MCP server, onboarding OpenClaw, starting or installing the Gateway, adding providers, entering credentials, or enabling any native OpenClaw file/shell tools.

## Command History

Repository and baseline:

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

Ollama local model probes:

```powershell
curl.exe -s http://127.0.0.1:11434/api/chat -H "Content-Type: application/json" --data-binary @-
```

The posted JSON bodies used synthetic prompts only:

- basic marker completion
- fake `get_project_status` tool-call schema
- valid fake JSON tool result continuation
- malformed fake tool result continuation

CatDesk source inspection and MCP tests:

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
cargo test mcp::tests -- --nocapture
```

Headless CatDesk and OpenClaw disposable MCP probe:

```powershell
cargo test headless -- --nocapture
cargo build
target\debug\catdesk.exe --headless-mcp --host 127.0.0.1 --port 33312 --workspace .tmp\catdesk-headless-t0012\workspace --mcp-path /t0012-headless/mcp --mode computer --tool-mode read-only --config-path .tmp\catdesk-headless-t0012\config\config.toml --no-ngrok
curl.exe -s http://127.0.0.1:33312/
curl.exe -s http://127.0.0.1:33312/t0012-headless/mcp -H 'Content-Type: application/json' --data-binary '@.tmp\catdesk-headless-t0012\initialize.json'
curl.exe -s http://127.0.0.1:33312/t0012-headless/mcp -H 'Content-Type: application/json' --data-binary '@.tmp\catdesk-headless-t0012\tools-list.json'
curl.exe -s http://127.0.0.1:33312/t0012-headless/mcp -H 'Content-Type: application/json' --data-binary '@.tmp\catdesk-headless-t0012\read-hello.json'
openclaw mcp add catdesk-t0012 --url http://127.0.0.1:33313/t0012-openclaw/mcp --transport streamable-http --include catdesk_instruction,read,search,plan_read --timeout 10 --connect-timeout 5
openclaw mcp tools catdesk-t0012 --include "catdesk_instruction,read,search,plan_read"
openclaw mcp status --verbose --json
openclaw mcp probe catdesk-t0012 --json
openclaw config validate --json
```

## Recommendation

Do not onboard or persistently configure OpenClaw yet as part of this spike. The CatDesk-local headless MCP test mode now exists for read-only local probes and should remain constrained:

- binds only to `127.0.0.1`;
- uses a caller-provided or printed local-only MCP path;
- skips ngrok entirely;
- supports per-process config/state directories;
- can start in read-only or multi-tools mode explicitly;
- exits cleanly after a test window or on stdin/HTTP shutdown.

The next spike should verify OpenClaw's effective worker tool list with native runtime/file/web/browser/automation tools denied and CatDesk MCP explicitly allowed. Do not run a model worker until that effective tool list is inspectable and clean.
