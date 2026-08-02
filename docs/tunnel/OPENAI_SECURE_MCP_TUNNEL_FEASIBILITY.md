# OpenAI Secure MCP Tunnel Feasibility

Ticket: T-0025A, refreshed in T-0025D0
Status: feasibility analysis only; no runtime behavior changed
Date: 2026-08-01

## Sources Checked

Official OpenAI documentation:

- https://developers.openai.com/api/docs/guides/secure-mcp-tunnels
- https://developers.openai.com/api/docs/guides/tools-connectors-mcp
- https://developers.openai.com/plugins/deploy/connect-chatgpt
- https://developers.openai.com/plugins/build/mcp-server

## T-0025D0 Official Findings

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

The current guide says `tunnel-client` is obtained from Platform tunnel settings
or the latest public `openai/tunnel-client` release. It also documents
`tunnel-client help quickstart`, `tunnel-client init`, `tunnel-client doctor`,
and `tunnel-client run` workflows. For CatDesk, the HTTP MCP path would use
`--mcp-server-url` rather than `--mcp-command`.

Network requirements are narrow but still account-gated: the host running
`tunnel-client` needs outbound HTTPS to OpenAI and local reachability to the
private MCP server. The private MCP server address stays private and is used
only from the `tunnel-client` side of the boundary.

The documented local health surfaces are `/healthz`, `/readyz`, `/metrics`, and
a loopback-only admin UI at `/ui`. The guide says raw HTTP logging is disabled
by default and support exports are redacted.

The ChatGPT plugin testing docs say developer mode can connect an MCP server
through either:

- a public endpoint URL ending in `/mcp`; or
- Secure MCP Tunnel by selecting Tunnel and choosing or entering a `tunnel_id`.

The plugin server deployment docs distinguish private developer-mode testing
from public plugin submission. They state that public submission still requires
a stable publicly reachable HTTPS MCP endpoint and that Secure MCP Tunnel alone
does not satisfy public submission requirements.

## Feasibility Classification

Classification for CatDesk T-0025D0:

```text
SUPPORTED_DOCS_ONLY_ACCOUNT_GATED
```

Reasoning:

- The official workflow exists and is documented.
- It is a strong fit for keeping CatDesk private during developer-mode testing.
- It requires external account/workspace permissions, a `tunnel_id`, and a
  runtime API key for `tunnel-client`.
- It requires a tunnel-client runtime that T-0025D0 did not install or execute.
- It does not replace the stable public HTTPS endpoint requirement for public
  plugin submission.
- It can remain a preferred future private connection path, but runtime
  implementation must stay blocked until an operator-approved D1 proof obtains
  or identifies the required tunnel identity and client installation source.

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

Before implementing `openai_secure_tunnel`, T-0025D1 must verify:

1. Official `tunnel-client` installation source and version.
2. Windows support and local execution behavior.
3. Required environment variables, profile files, and secret storage behavior.
4. Exact secret locations and redaction requirements.
5. Whether CatDesk's local HTTP MCP endpoint is accepted directly through
   `--mcp-server-url`.
6. Whether streamable HTTP behavior and long-running tool calls work.
7. How ChatGPT developer mode selects the tunnel by list or `tunnel_id`.
8. Whether the tunnel identity remains stable across CatDesk restarts.
9. Whether stale tunnel-client processes can be detected without killing
   unrelated processes.
10. A disposable proof that `catdesk_instruction` and `delegated_run_list`
    work through the tunnel without exposing a public URL.
11. Whether `tunnel-client doctor --profile <name> --explain` gives enough
    structured state for CatDesk-facing diagnostics.

## Proposed Product Posture

For T-0025B/C:

- Include `openai_secure_tunnel` as a recognized but disabled mode.
- If selected before T-0025D proof, fail with a clear message:

```text
OpenAI Secure MCP Tunnel is documented but not yet locally validated by
CatDesk. Run the T-0025D tunnel-client proof before enabling this mode.
```

For T-0025D:

- Treat `tunnel-client` as operator-owned unless an explicit CatDesk-managed
  lifecycle is separately approved after D1.
- Keep all credentials out of the repository and review bundles.
- Do not request, print, store, or infer a runtime API key during documentation
  feasibility.

## Decision

Secure MCP Tunnel is viable and remains the preferred future private connection
path for ChatGPT developer-mode use, but it is not ready for runtime
implementation in this branch without account-level tunnel access and a local
client proof. The next practical phases are:

1. Persistent route and user-level config.
2. External tunnel mode.
3. Managed stable ngrok only after an assigned-domain feasibility pass.
4. Secure MCP Tunnel D1 proof only after operator-approved tunnel-client and
   credential handling are available.
