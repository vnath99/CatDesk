# T-0027 Final Handoff

## Result

T-0027 is ready for operator-provided OpenAI tunnel ID and runtime key.

CatDesk now has a user-level official tunnel-client installer, native runtime alias integration, continuous monitoring, setup/status scripts, and automated fake-client coverage. A live OpenAI control-plane connection was not attempted because this pass did not receive a tunnel ID or runtime API key and was not authorized to create account resources.

## Architecture

```text
ChatGPT / OpenAI supported MCP surface
    |
OpenAI Secure MCP Tunnel control plane
    |
official tunnel-client runtime alias
    |
loopback CatDesk MCP HTTP endpoint
    |
CatDesk MCP tools and delegated worker
```

CatDesk supervises integration health. The official client owns the long-lived runtime process in `official_runtime` mode.

## Supported Modes

```text
official_runtime
external_foreground
legacy_direct_managed
```

`official_runtime` is preferred and defaults to leaving the official runtime alive when CatDesk exits. `external_foreground` is operator-owned. `legacy_direct_managed` exists for compatibility/debugging only.

## Security Boundary

- No runtime API key is persisted or logged.
- Runtime connect uses `env:CONTROL_PLANE_API_KEY`.
- No full local MCP URL, persistent route, tunnel ID, or API key is printed in normal logs or review artifacts.
- OpenAI Secure MCP never falls back to ngrok.
- CatDesk does not create tunnels, API keys, ChatGPT connectors, services, firewall rules, or account permissions.
- Setup scripts use explicit mutating modes; default behavior is plan/status-only.

## Operator Commands

Plan and install the official client:

```powershell
.\scripts\install-openai-tunnel-client.ps1 -Plan
.\scripts\install-openai-tunnel-client.ps1 -Install
```

Inspect setup status:

```powershell
.\scripts\setup-doctor.ps1 -TransportMode openai_secure_tunnel
.\scripts\setup-secure-mcp.ps1 -Plan
.\scripts\setup-secure-mcp.ps1 -Status
```

Connect only when the operator has a tunnel ID and a runtime key in process scope:

```powershell
$env:CONTROL_PLANE_API_KEY = "<runtime-api-key>"
.\scripts\setup-secure-mcp.ps1 -ConnectRuntime -TunnelId "<tunnel-id>"
```

The key should be removed from the shell environment when the session is complete.

## Verification Summary

Focused verification completed:

```text
cargo test openai_tunnel -- --nocapture
cargo test openai_secure_tunnel -- --nocapture
scripts/install-openai-tunnel-client.ps1 -Status
scripts/setup-secure-mcp.ps1 -Plan
scripts/setup-secure-mcp.ps1 -Status
scripts/setup-doctor.ps1 -TransportMode openai_secure_tunnel
```

Full verification evidence is recorded in the external T-0027 review bundle.

## Manual Remaining Steps

1. Create or select an OpenAI Secure MCP tunnel in Platform.
2. Create a runtime API key with the required tunnel permissions.
3. Associate the tunnel with the intended ChatGPT workspace or organization.
4. Run the setup wizard with `CONTROL_PLANE_API_KEY` present only in process scope.
5. Verify the official runtime reaches ready status.
6. Select the tunnel-backed connector in ChatGPT.
7. Call read-only tools first:
   - `catdesk_instruction`
   - `catdesk_transport_status`
   - `delegated_run_list`
8. Restart CatDesk and confirm the same connector remains usable.
9. Run the harmless delegated Qwen smoke test.

## Known Limitations

- Live OpenAI tunnel status is unverified until the operator supplies tunnel credentials and Platform access.
- The TUI does not yet expose every runtime action key from the long-term design; scripts and `catdesk_transport_status` provide the current operator surface.
- Secure prompting for runtime keys remains future work; process-scoped environment input is the supported path in this branch.
- `legacy_direct_managed` is compatibility-only and should not be used as the unattended default.
