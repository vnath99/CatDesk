# T-0025D OpenAI Secure MCP Tunnel Decision

## Decision State

`SUPPORTED_BUT_CLIENT_NOT_INSTALLED`

Official OpenAI documentation confirms Secure MCP Tunnel is a supported private
MCP connection path for developer-mode testing, but this workstation does not
currently expose a `tunnel-client` command on PATH. No tunnel client was
installed, no OpenAI credentials were entered, and no ChatGPT account or plugin
setting was changed during this phase.

## Official Evidence

Sources checked:

- `https://developers.openai.com/api/docs/guides/secure-mcp-tunnels`
- `https://developers.openai.com/api/docs/guides/tools-connectors-mcp`
- `https://developers.openai.com/plugins/deploy/connect-chatgpt`

Current documented shape:

- Secure MCP Tunnel uses a local `tunnel-client` inside the network that can
  reach the private MCP server.
- The client opens outbound HTTPS to OpenAI and forwards queued MCP JSON-RPC
  requests to a local stdio or HTTP MCP server.
- A `tunnel_id`, runtime API key, Platform tunnel permissions, and ChatGPT
  developer-mode access are required.
- ChatGPT developer-mode connection can select Tunnel and choose an available
  tunnel or enter a `tunnel_id`.
- The tunnel is appropriate for private developer-mode testing, but it does not
  replace a stable public HTTPS endpoint for public plugin submission.

## Local Discovery

Commands inspected, without printing secret values:

- `Get-Command tunnel-client`
- `Get-Command tunnel-client.exe`
- `Get-Command openai-tunnel`
- `Get-Command openai-tunnel.exe`
- environment-variable name presence checks only

Results:

- `tunnel-client`: not found on PATH
- `openai-tunnel`: not found on PATH
- `CONTROL_PLANE_API_KEY`: absent
- `OPENAI_TUNNEL_ID`: absent
- `TUNNEL_ID`: absent
- `OPENAI_API_KEY`: present by name only; value was not printed or used

## D1 Gate

T-0025D1 must not run until all of the following are approved and available:

1. Exact tunnel-client source and version.
2. Installation or execution destination.
3. Rollback procedure.
4. Non-secret `tunnel_id` handling plan.
5. Runtime API-key handling plan that keeps credentials out of CatDesk config,
   logs, review bundles, and Git.
6. Operator confirmation that the target ChatGPT workspace and Platform
   organization have the required tunnel permissions.

## CatDesk Posture

For this branch:

- Keep `openai_secure_tunnel` recognized but unavailable at runtime.
- Prefer `external_tunnel` for independently managed private tunnel clients
  after an operator starts and verifies them.
- Do not add a CatDesk-managed OpenAI tunnel process manager until D1 proves it
  is needed and safe.
- Do not fall back from OpenAI Secure MCP Tunnel to public ngrok.

## Next Action

Continue automated validation and the morning manual test packet using the
implemented legacy and external transport paths. Treat OpenAI Secure MCP Tunnel
as a documented future private path, not as implemented runtime support.
