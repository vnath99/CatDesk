# T-0027A Operations Baseline

Status: implemented in `infra/openai-secure-mcp-operations`.

## Scope

T-0027 starts from the reviewed OpenAI Secure MCP Tunnel checkpoint and adds the operator-side operations layer:

- official `openai/tunnel-client` capability detection;
- verified user-level installer;
- official runtime alias integration;
- continuous status monitoring;
- redacted setup/status scripts.

It does not create Platform tunnels, API keys, ChatGPT connectors, services, firewall rules, or fallback transports. Qwen, DeepSeek, delegated execution, and audio-project behavior are intentionally unchanged.

## Official Surface

The current official OpenAI Secure MCP Tunnel documentation describes an outbound-only `tunnel-client` that runs inside the network that can reach the MCP server and forwards queued MCP JSON-RPC work to that private server.

The current `openai/tunnel-client` command surface includes native runtime supervision:

```text
tunnel-client runtimes connect
tunnel-client runtimes status --json
tunnel-client runtimes stop
tunnel-client runtimes rm
```

CatDesk therefore treats `runtimes connect/status/stop/rm` as the preferred unattended runtime path. The foreground `run --profile` mode remains compatibility-only as `legacy_direct_managed`.

## Baseline Before T-0027

Client discovery existed, but ordinary CatDesk startup did not install or update the client. OpenAI Secure MCP mode could discover an operator-provided client and validate `/readyz`, but it primarily modeled external or direct child-process ownership.

Setup scripts existed but were mostly read-only. The operator still had to locate the right archive, verify hashes manually, place the binary, decide the runtime command, and infer whether the tunnel was ready.

## T-0027 Additions

### Capability Model

CatDesk now parses bounded help output into a versioned capability structure:

```text
supports_profiles
supports_http_mcp
supports_doctor
supports_health_command
supports_runtime_connect
supports_runtime_status_json
supports_runtime_stop
supports_runtime_remove
supports_admin_ui
supports_health_url_file
```

Support is derived from actual help output rather than version strings alone. Unsupported or prerelease clients are blocked with `BLOCKED_UNSUPPORTED_CLIENT`.

### Installer

`scripts/install-openai-tunnel-client.ps1` supports:

```text
-Plan
-Install
-Update
-Rollback
-Status
```

The installer resolves the latest stable official GitHub release, ignores drafts/prereleases, selects the current Windows architecture asset, downloads `SHA256SUMS.txt`, verifies the selected archive hash, extracts safely, validates the binary and runtime command surface, then atomically activates:

```text
%USERPROFILE%\.catdesk\tools\tunnel-client\current\tunnel-client.exe
```

It does not modify `PATH`, require administrator rights, install services, or persist credentials.

### Runtime Integration

`openai_secure_tunnel` now supports process modes:

```text
official_runtime
external_foreground
legacy_direct_managed
```

`official_runtime` is the default. It reconciles an existing runtime alias before connecting so CatDesk does not start duplicate tunnel runtimes after restart.

### Monitor

The monitor polls native runtime JSON status, `/readyz` when an admin URL is known, and the local CatDesk MCP self-check. It debounces transient failures, reports truthful states, performs bounded recovery only when the local MCP endpoint is healthy and the operator supplied runtime environment variables, and never falls back to ngrok.

### Setup Wizard

`scripts/setup-secure-mcp.ps1` supports explicit safe modes:

```text
plan
install
configure
connect
start_all
status
repair
stop
rollback
```

Default mode is `plan`. The runtime API key is referenced only as `env:CONTROL_PLANE_API_KEY`; the script does not store or print it.

## Local Installation Evidence

The official stable client installed during T-0027 was:

```text
version: 0.0.10+105e17a79a36e4e5c897fd698ed2b8dbf935b144
architecture: windows-amd64
archive SHA-256: 5e64a056f1d96786da0a6f8db1da5f5f4a03fd19a90d951a25cf2ca8d9093d00
binary SHA-256: d893d8127eee35070d265c1be29bfe008f8d9fcb476e7febf56c8fdc6c0615c8
```

No installed executable is stored in Git or in review bundles.

## Remaining Operator Gate

Live OpenAI tunnel connection remains operator-gated because it requires:

- an OpenAI Platform tunnel ID;
- a runtime API key with the required tunnel permissions;
- ChatGPT connector selection in the user account/workspace.

Automated tests use fake clients and fake runtime responses for credentialed paths.
