# T-0012 Recommendation

Date: 2026-07-25

## Decision

Pause OpenClaw integration before onboarding, provider setup, or any model worker run. CatDesk now has a constrained headless local-MCP mode, and OpenClaw can register and probe that endpoint through disposable config/state. The remaining gate is still proving OpenClaw's effective worker-visible tool list with native execution tools denied.

## Why

The current CatDesk safety layer is materially stronger after T-0011, and the existing MCP tests pass. Local Ollama/Qwen can emit structured tool calls. OpenClaw is now installed and can see a disposable CatDesk MCP server, but it is not onboarded and `openclaw doctor` reports that the Gateway is unconfigured and not installed as a service. OpenClaw docs and local help show native runtime/file-style tools, so the core requirement remains unproven: CatDesk must stay the only file/shell execution authority during worker turns.

## Headless MCP Result

Implemented a narrow CatDesk headless local-MCP mode for test and orchestration spikes.

Current behavior:

- Starts without TUI input.
- Binds only to `127.0.0.1` by default.
- Verifies after binding that the resolved listener address is loopback.
- Skips ngrok entirely.
- Uses a caller-provided local-only MCP path or prints the generated path to stdout in machine-readable JSON.
- Accepts workspace, port, mode, and tool mode as per-process CLI settings.
- Defaults omitted `--tool-mode` to read-only.
- Restricts `--mcp-path` to `/[A-Za-z0-9_-]+/mcp`.
- Requires the workspace to exist and be a directory.
- Requires an explicit `--config-path`.
- Exits cleanly when the process receives Ctrl+C or is stopped by the supervisor.
- Propagates unexpected axum server termination or errors.
- Does not change existing interactive behavior.
- Rejects browser/both modes for now so no browser/devtools authority is started.
- Treats multi-tools over an unauthenticated loopback endpoint as unresolved.

Probe command shape:

```powershell
catdesk --headless-mcp --host 127.0.0.1 --port 33200 --workspace C:\Temp\catdesk-disposable --mcp-path /t0012/mcp --mode computer --tool-mode read-only --config-path C:\Temp\catdesk-disposable-config\config.toml --no-ngrok
```

## OpenClaw Probe Result

With process-scoped `OPENCLAW_CONFIG_PATH` and `OPENCLAW_STATE_DIR`, OpenClaw registered and probed a read-only CatDesk MCP endpoint. The corrected include filter exposed exactly:

- `catdesk-t0012__catdesk_instruction`
- `catdesk-t0012__plan_read`
- `catdesk-t0012__read`
- `catdesk-t0012__search`

`openclaw config validate --json` returned `valid: true` with no warnings. This proves MCP registration and tool discovery, not model-worker safety.

## OpenClaw Closure Audit

On 2026-07-25, the approved closure pass configured a disposable OpenClaw profile using only process-scoped `OPENCLAW_CONFIG_PATH` and `OPENCLAW_STATE_DIR`. The validated policy used `tools.profile: "minimal"`, an absolute `tools.allow` list containing only the four CatDesk MCP tools above, and explicit denies for runtime, filesystem, web, UI/browser, automation, exec, applyPatch, elevated, and code-mode capability names. It also set native command surfaces off, `browser.enabled: false`, `tools.exec.mode: "deny"`, `tools.exec.applyPatch.enabled: false`, `tools.elevated.enabled: false`, and `tools.codeMode: false`.

This closure pass distinguishes two observations:

- MCP server discovery succeeded and returned exactly the four filtered CatDesk tools.
- Final effective worker-visible tool definitions could not be exposed by the installed OpenClaw CLI before a model turn. `openclaw agent --help`, `openclaw mcp`, `openclaw config`, and plugin inspection commands expose configured policy, MCP discovery, or plugin metadata, but not the resolved tool list that would be sent to an actual worker.

Therefore OpenClaw remains unresolved as a safe model-worker orchestrator. Read-only CatDesk MCP mode is the only approved OpenClaw test posture until local client authentication or an equivalent control is designed and the worker-visible tool list can be audited before any model request.

## Future OpenClaw Approval Packet

Before any persistent OpenClaw configuration or service setup, prepare and approve:

- Exact commands.
- Destination config and state directories.
- Whether the default `~\.openclaw\openclaw.json` is read or written.
- Whether Gateway mode/auth, providers, credentials, services, Scheduled Tasks, PATH, npm global prefix, or OpenClaw state are changed.
- Dry-run or read-only command output when available.
- Rollback command and manual cleanup path for any created disposable files.

The operator has already installed Node.js and OpenClaw. The old install candidates below are no longer the next step and remain listed only for provenance:

```powershell
& ([scriptblock]::Create((iwr -useb https://openclaw.ai/install.ps1))) -Tag latest -NoOnboard -DryRun
```

```powershell
iwr -useb https://openclaw.ai/install.ps1 | iex
```

```powershell
ollama launch openclaw
```

The first command was the only candidate that appeared suitable for a pre-install approval packet because it advertises dry-run behavior. The second and third are persistent install/onboarding paths and should not be run without explicit approval.

## Minimum Nonpersistent Test Plan

Run OpenClaw with process-scoped paths only:

```powershell
$env:OPENCLAW_CONFIG_PATH = "$PWD\.tmp\openclaw-t0012\openclaw.json"
$env:OPENCLAW_STATE_DIR = "$PWD\.tmp\openclaw-t0012\state"
```

The disposable config should:

- register CatDesk under `mcp.servers.catdesk`;
- expose only read-only CatDesk tools for this spike;
- use `tools.profile: "minimal"` and an absolute `tools.allow` list containing only the four server-qualified CatDesk MCP tool names;
- deny OpenClaw-native runtime, filesystem, web, UI/browser, automation, exec, applyPatch, elevated, and code-mode capability names;
- set `tools.exec.mode: "deny"`;
- set `tools.exec.applyPatch.enabled: false`;
- keep `tools.elevated.enabled: false`;
- keep code mode disabled;
- keep native command, plugin-management, debug, browser, web, and automation surfaces disabled where the installed schema supports explicit disabling.

## Required Runtime Audit Before Model Worker Run

Only after approved disposable configuration:

1. Run read-only diagnostics such as `openclaw --version`, `openclaw doctor`, and `openclaw mcp status --verbose --json`.
2. Configure a disposable OpenClaw config/state directory with `OPENCLAW_CONFIG_PATH` and `OPENCLAW_STATE_DIR`.
3. Register CatDesk as an MCP server only against a disposable CatDesk workspace.
4. Inspect the effective tool list sent to the worker.
5. Confirm native OpenClaw file/shell/network/browser/automation tools are absent or denied.
6. Confirm CatDesk tools are the only available file/shell execution path.
7. Send synthetic prompts only.
8. Verify malformed CatDesk tool results are rejected by glue/schema validation before model continuation.

## Stop Conditions

Treat the integration as infeasible until redesigned if:

- OpenClaw cannot disable or deny native file/shell/runtime tools for the worker.
- OpenClaw requires persistent global config or service changes for a disposable local test.
- Tool policy cannot be inspected before the model call.
- CatDesk cannot be started in a deterministic local-only MCP mode.
- The worker can reach secrets, environment values, private keys, or non-disposable repositories outside CatDesk-mediated tools.
