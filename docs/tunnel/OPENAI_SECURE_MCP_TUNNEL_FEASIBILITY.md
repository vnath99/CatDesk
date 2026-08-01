# OpenAI Secure MCP Tunnel Feasibility

Ticket: T-0025A  
Status: feasibility analysis only; no runtime behavior changed  
Date: 2026-08-01

## Sources Checked

Official OpenAI documentation:

- https://developers.openai.com/api/docs/guides/secure-mcp-tunnels
- https://developers.openai.com/plugins/deploy/connect-chatgpt
- https://developers.openai.com/plugins/build/mcp-server

## Current Official Shape

OpenAI documents Secure MCP Tunnel as an outbound-only tunnel for private MCP
servers. A local `tunnel-client` runs inside the network that can reach the MCP
server, polls OpenAI over HTTPS, forwards JSON-RPC requests to the private MCP
server, and returns responses through the same tunnel.

The documented setup requires:

- A `tunnel_id` from Platform tunnel settings.
- A runtime API key for `tunnel-client`.
- An MCP server reachable by the tunnel client over stdio or HTTP.
- Platform tunnel permissions for read/manage/use depending on the action.
- ChatGPT developer-mode access when using it from ChatGPT.
- Tunnel association with the relevant Platform organization or ChatGPT
  workspace.

The ChatGPT plugin testing docs say developer mode can connect an MCP server
through either:

- a public endpoint URL ending in `/mcp`; or
- Secure MCP Tunnel by selecting Tunnel and choosing or entering a `tunnel_id`.

The plugin server deployment docs distinguish private developer-mode testing
from public plugin submission. They state that public submission still requires
a stable publicly reachable HTTPS MCP endpoint and that Secure MCP Tunnel alone
does not satisfy public submission requirements.

## Feasibility Classification

Classification for CatDesk T-0025A:

```text
SUPPORTED_BUT_MANUAL_ONLY_PENDING_LOCAL_PROOF
```

Reasoning:

- The official workflow exists and is documented.
- It is a strong fit for keeping CatDesk private during developer-mode testing.
- It requires external account/workspace permissions and a tunnel-client runtime
  that T-0025A did not install or execute.
- It does not replace the stable public HTTPS endpoint requirement for public
  plugin submission.
- It should not block managed stable ngrok or external tunnel delivery.

## CatDesk Fit

Potential advantages:

- CatDesk can keep the MCP server bound to localhost.
- No public ngrok URL is needed for private developer-mode testing.
- The tunnel client, rather than CatDesk, owns the outbound OpenAI connection.
- The local route can remain CatDesk-controlled, while any application-layer
  authentication must be proven compatible with the actual ChatGPT connection
  path before being required.

Integration concerns:

- CatDesk must not store OpenAI API keys or Platform tunnel credentials.
- CatDesk must not assume the user's ChatGPT workspace can list or use tunnels.
- CatDesk must not automatically install or launch `tunnel-client` without a
  reviewed command, source, version, destination, and rollback procedure.
- CatDesk must still preserve MCP authentication, route redaction, workspace
  containment, approval gates, and delegated verification semantics.

## T-0025D Proof Requirements

Before implementing `openai_secure_tunnel`, T-0025D must verify:

1. Official `tunnel-client` installation source and version.
2. Windows support and local execution behavior.
3. Required environment variables or config files.
4. Exact secret locations and redaction requirements.
5. Whether CatDesk's HTTP MCP endpoint is accepted directly.
6. Whether streamable HTTP behavior and long-running tool calls work.
7. How ChatGPT developer mode selects the tunnel.
8. Whether the tunnel identity remains stable across CatDesk restarts.
9. Whether stale tunnel-client processes can be detected without killing
   unrelated processes.
10. A disposable proof that `catdesk_instruction` and `delegated_run_list`
    work through the tunnel without exposing a public URL.

## Proposed Product Posture

For T-0025B/C:

- Include `openai_secure_tunnel` as a recognized but disabled mode.
- If selected before T-0025D proof, fail with a clear message:

```text
OpenAI Secure MCP Tunnel is documented but not yet locally validated by
CatDesk. Run the T-0025D tunnel-client proof before enabling this mode.
```

For T-0025D:

- Treat the tunnel-client as operator-owned unless an explicit CatDesk-managed
  lifecycle is separately approved.
- Keep all credentials out of the repository and review bundles.

## Decision

Secure MCP Tunnel is viable as a later gated spike, but not as the first stable
transport implementation. The next practical implementation phases should focus
on:

1. Persistent route and user-level config.
2. Managed stable ngrok.
3. External tunnel mode.
4. Secure MCP Tunnel proof and decision.
