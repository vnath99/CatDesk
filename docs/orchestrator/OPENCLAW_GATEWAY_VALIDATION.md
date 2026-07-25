# OpenClaw Gateway Validation

Date: 2026-07-25

Ticket: T-0013A

Branch: `orchestrator/v1-coding-sprint`

Pinned OpenClaw: `OpenClaw 2026.7.1-2 (0790d9f)`

Status: FAIL_CLOSED

## Summary

T-0013A proved that CatDesk can start and authenticate to a disposable loopback OpenClaw Gateway, invoke documented Gateway RPC methods, receive structured event frames, and track event sequence values without running a model worker.

It did not prove the required pre-model tool-policy invariant. `tools.effective` returned no CatDesk MCP tools and repeatedly reported `mcp-not-yet-connected`:

```text
MCP servers "catdesk-t0013a" are configured but not connected for this session yet. MCP tools will appear here after an agent run discovers them.
```

Because the approved CatDesk MCP tools cannot be confirmed in the final effective worker-visible tool list before a model turn, the ticket fails closed. Do not proceed to T-0014 on the OpenClaw thin-adapter path until OpenClaw exposes a supported pre-model MCP catalog warm-up method or an equivalent control.

## Boundaries Held

- No model worker was run.
- No Qwen or other provider was invoked.
- No provider credentials were entered or configured.
- No OpenClaw Gateway service was installed.
- No persistent onboarding was performed.
- No global PATH, shell profile, registry setting, service, Ollama config, or existing CatDesk config was modified.
- OpenClaw used process-scoped `OPENCLAW_CONFIG_PATH` and `OPENCLAW_STATE_DIR`.
- Gateway bind was loopback-only at `127.0.0.1:18791`.
- Gateway auth used a disposable process-scoped token; evidence redacts the token.
- CatDesk headless MCP ran read-only on loopback at `127.0.0.1:33413`.
- No commit, push, PR, merge, release, or T-0014 work was performed.

## Disposable Runtime

CatDesk headless MCP:

```powershell
target\debug\catdesk.exe --headless-mcp --host 127.0.0.1 --port 33413 --workspace .tmp\t0013a-live\catdesk-workspace --mcp-path /t0013a/mcp --mode computer --tool-mode read-only --config-path .tmp\t0013a-live\catdesk-config\config.toml --no-ngrok
```

OpenClaw Gateway:

```powershell
$env:OPENCLAW_CONFIG_PATH = ".tmp\t0013a-live\openclaw\openclaw.json"
$env:OPENCLAW_STATE_DIR = ".tmp\t0013a-live\openclaw\state"
$env:OPENCLAW_GATEWAY_TOKEN = "[REDACTED]"
openclaw.cmd gateway run --bind loopback --auth token --port 18791 --tailscale off --ws-log compact
```

The Gateway status evidence reported:

- CLI version: `2026.7.1-2`
- Config path: `.tmp\t0013a-live\openclaw\openclaw.json`
- Config valid: true
- Gateway bind mode: `loopback`
- Gateway bind host: `127.0.0.1`
- Gateway port: `18791`
- Service runtime: stopped/missing, confirming no service install was used
- Listener: `node.exe` on `127.0.0.1:18791` only during the foreground validation

## Minimal Probe Code

Added experimental helper:

- `scripts/openclaw_gateway_probe.mjs`

The helper uses Node 24's built-in `WebSocket`; it adds no package dependency. It supports:

- live authenticated Gateway connect;
- protocol version check through `hello-ok.protocol`;
- method discovery through `hello-ok.features.methods`;
- documented RPC invocation;
- event collection;
- event sequence monotonicity check;
- redacted evidence output;
- fail-closed tool-policy validation;
- offline fixture replay mode.

Added offline fixture:

- `tests/fixtures/openclaw_gateway/t0013a_minimal_gateway.jsonl`

The fixture represents a successful minimal protocol trace: `connect.challenge`, `hello-ok`, monotonic events, and an effective tool list containing only the four approved CatDesk MCP tools.

## Commands Run

Setup and validation:

```powershell
openclaw --version
openclaw gateway --help
openclaw gateway start --help
openclaw gateway run --help
openclaw gateway call --help
openclaw gateway health --help
openclaw gateway probe --help
openclaw config validate --json
node scripts\openclaw_gateway_probe.mjs --fixture tests\fixtures\openclaw_gateway\t0013a_minimal_gateway.jsonl --out .tmp\t0013a-evidence\fixture-probe-result.json
openclaw.cmd mcp probe catdesk-t0013a --json
openclaw.cmd gateway health --port 18791 --json
openclaw.cmd gateway status --json --no-probe
node scripts\openclaw_gateway_probe.mjs --out .tmp\t0013a-evidence\live-gateway-probe-result.json
```

Process cleanup:

```powershell
Get-NetTCPConnection -LocalPort 18791 -State Listen
Stop-Process -Id <listener-pid> -Force
Stop-Process -Id <catdesk-pid> -Force
Remove-Item Env:\OPENCLAW_GATEWAY_TOKEN
Remove-Item Env:\OPENCLAW_GATEWAY_URL
Remove-Item Env:\OPENCLAW_GATEWAY_AGENT_ID
Remove-Item Env:\OPENCLAW_GATEWAY_SESSION_KEY
```

## Evidence Files

Raw disposable evidence, not intended for commit:

- `.tmp\t0013a-evidence\openclaw-config-validate.json`
- `.tmp\t0013a-evidence\catdesk-headless.stdout.txt`
- `.tmp\t0013a-evidence\catdesk-headless.stderr.txt`
- `.tmp\t0013a-evidence\openclaw-mcp-probe.json`
- `.tmp\t0013a-evidence\openclaw-gateway-health.json`
- `.tmp\t0013a-evidence\openclaw-gateway-status.json`
- `.tmp\t0013a-evidence\openclaw-gateway.stdout.txt`
- `.tmp\t0013a-evidence\openclaw-gateway.stderr.txt`
- `.tmp\t0013a-evidence\fixture-probe-result.json`
- `.tmp\t0013a-evidence\live-gateway-probe-result.json`
- `.tmp\t0013a-evidence\live-gateway-probe-console.txt`
- `.tmp\t0013a-evidence\live-command-log.txt`

Secrets were redacted in the probe output. The disposable token was supplied only through the process environment and was removed after the run.

## MCP Discovery Versus Effective Tools

MCP discovery succeeded:

```json
{
  "servers": {
    "catdesk-t0013a": {
      "launch": "http://127.0.0.1:33413/t0013a/mcp",
      "tools": 4,
      "filteredTools": 4,
      "resources": true
    }
  },
  "tools": [
    "catdesk-t0013a__catdesk_instruction",
    "catdesk-t0013a__plan_read",
    "catdesk-t0013a__read",
    "catdesk-t0013a__search"
  ],
  "diagnostics": []
}
```

This is not sufficient for worker safety. Final effective tools failed:

```json
{
  "agentId": "catdesk-t0013a-audit",
  "profile": "minimal",
  "groups": [],
  "notices": [
    {
      "id": "mcp-not-yet-connected",
      "severity": "info",
      "message": "MCP servers \"catdesk-t0013a\" are configured but not connected for this session yet. MCP tools will appear here after an agent run discovers them."
    }
  ]
}
```

Four retry attempts, spaced by 3 seconds, all returned the same notice and zero effective tool groups.

## RPC Support Matrix

| RPC | Result | Notes |
| --- | --- | --- |
| `tools.catalog` | OK | Returned core/plugin catalog groups. This is not final effective policy. |
| `tools.effective` | FAIL_CLOSED | Callable, but omitted all approved CatDesk MCP tools and returned `mcp-not-yet-connected`. |
| `sessions.create` | OK | Created/adopted `agent:catdesk-t0013a-audit:t0013a`; no model run started. |
| `sessions.resolve` | OK | Direct call succeeded, but method was not advertised in `hello.features.methods`. |
| `sessions.get` | OK | Direct call succeeded, but method was not advertised in `hello.features.methods`. |
| `sessions.subscribe` | OK | Returned subscribed true. |
| `sessions.messages.subscribe` | OK | Returned subscribed true. |
| `agent.wait` | OK | Nonexistent run with `timeoutMs: 0` returned timeout; no worker was started. |
| `chat.abort` | OK | Returned no active run. |
| `sessions.abort` | OK | Returned no active run. |
| `tasks.list` | OK | Returned zero tasks. |
| `tasks.get` | DOCUMENTED_APPLICATION_ERROR | Method exists; nonexistent task returned `task not found`. |
| `tasks.cancel` | OK | Nonexistent task returned found/cancelled false. |
| `models.list` | OK | Returned configured model catalog metadata. |
| `agents.list` | OK | Returned one disposable agent. |
| `audit.list` | OK | Returned zero events. |

## Events

The live probe observed structured Gateway event frames:

- `connect.challenge`
- `health`

Observed event sequence values:

```json
[1]
```

The observed sequence was monotonic. The small event count is expected because no model worker or tool invocation was run.

## Prohibited Tool Visibility

Final `tools.effective` exposed no prohibited native tools, but it also exposed no approved CatDesk tools. Therefore the safety gate still fails.

The broader `tools.catalog` response included core and plugin catalog entries such as file, runtime, web, UI, automation, and plugin tools. This catalog is an inventory view, not the final effective worker-visible tool list. The validation does not infer safety from `tools.catalog`.

One notable startup observation: Gateway logged that several runtime plugins were auto-enabled without writing config, including browser, canvas, file-transfer, ollama, phone-control, and talk-voice. Because final `tools.effective` remained empty, these did not become worker-visible in this run, but future adapter work must continue to validate effective policy rather than trust config intent.

## Fail-Closed Decision

T-0013A fails closed for this reason:

```text
tools.effective missing approved tools:
catdesk-t0013a__catdesk_instruction,
catdesk-t0013a__plan_read,
catdesk-t0013a__read,
catdesk-t0013a__search
```

OpenClaw explicitly reported that the MCP tools appear only after an agent run discovers them. That violates the T-0013A requirement to warm and verify CatDesk MCP tools before any model turn.

## Recommendation

Do not proceed to T-0014 on the OpenClaw direct/thin-adapter path yet.

Recommended architecture review decision:

1. Prefer D: implement a minimal CatDesk-controlled worker loop, unless OpenClaw can provide a documented pre-model MCP catalog warm-up RPC or a supported Gateway-side session preparation method.
2. Keep B as conditional only if OpenClaw adds or documents a safe pre-model warm-up path that lets CatDesk verify `tools.effective` contains exactly the approved MCP tools before a model request.
3. Do not relax the boundary by running a model turn merely to warm the catalog.

## Roadmap Impact

- T-0013A adds a useful offline protocol fixture and disposable probe helper.
- T-0014 remains blocked for OpenClaw-backed execution.
- Next viable design work should either:
  - ask OpenClaw for/locate a documented pre-model MCP warm-up API; or
  - pivot to a CatDesk-owned minimal worker loop that consumes the T-0013 protocol and calls CatDesk MCP tools directly under CatDesk policy.

