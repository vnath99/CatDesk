# OpenAI Secure MCP Operations

## Transport Modes

CatDesk supports:

- `managed_ephemeral_ngrok`
- `managed_stable_ngrok`
- `external_tunnel`
- `openai_secure_tunnel`

OpenAI Secure MCP never falls back to ngrok. A failed OpenAI tunnel remains a
blocked or failed OpenAI tunnel state until the operator changes configuration.

## Health States

CatDesk reports structured transport health:

```text
DISABLED
CONFIGURED_UNVERIFIED
LOCAL_READY
CONNECTING
CONNECTED_VERIFIED
DEGRADED
DISCONNECTED
BLOCKED_MISSING_CLIENT
BLOCKED_MISSING_PROFILE
BLOCKED_MISSING_CREDENTIAL
BLOCKED_MISSING_TUNNEL_ID
FAILED
```

`LOCAL_READY` means the loopback MCP server passed CatDesk's own initialize and
tools-list self-check. It does not prove that ChatGPT can reach the server
through OpenAI's tunnel.

`CONNECTED_VERIFIED` requires an actual remote MCP self-check or tunnel-mediated
proof.

## Startup Doctor

Run:

```powershell
.\scripts\setup-doctor.ps1 `
  -Model "qwen3.6:35b-a3b" `
  -TransportMode "openai_secure_tunnel" `
  -OpenAiTunnelProfile "<profile-name>"
```

The doctor reports:

- CatDesk binary discovery
- Rust and Git discovery
- Ollama and selected model presence
- loopback MCP posture
- tunnel-client discovery
- whether `CONTROL_PLANE_API_KEY` is present

It redacts profile names and never prints credential values.

## Setup Wizard

Run:

```powershell
.\scripts\setup-secure-mcp.ps1 -TransportMode openai_secure_tunnel
```

The wizard emits operator steps and command templates with placeholders. It is
safe to run repeatedly because it performs no mutations.

## Normal External-Mode Procedure

1. Start the official `tunnel-client` with the operator-owned profile.
2. Start CatDesk.
3. Confirm `catdesk_transport_status` reports a truthful health state.
4. In ChatGPT, call read-only tools first.
5. Start delegated runs only after the connector is confirmed.

## Normal Managed-Mode Procedure

1. Export the runtime API key only for the CatDesk process.
2. Start CatDesk.
3. CatDesk runs `tunnel-client doctor`.
4. CatDesk starts one verified tunnel-client child process.
5. CatDesk stops only that child process during shutdown.

## Restart Behavior

Persistent MCP route rotation remains restart-required:

```text
generate pending route
restart required
controlled restart
pending route becomes active
old route invalid
```

OpenAI Secure MCP profile setup is also operator-controlled. CatDesk does not
modify OpenAI account configuration.

## Rollback

Change:

```toml
[tunnel]
mode = "managed_ephemeral_ngrok"
```

or return to the previous branch:

```text
infra/stable-mcp-transport
```

No source deletion is required.
