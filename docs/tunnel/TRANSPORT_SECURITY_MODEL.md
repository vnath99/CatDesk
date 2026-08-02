# Transport Security Model

## Authority Boundary

CatDesk remains the only local execution authority. Transport modes carry MCP
JSON-RPC messages; they do not grant tools to tunnel providers.

The OpenAI Secure MCP tunnel-client forwards requests between OpenAI's tunnel
endpoint and CatDesk's loopback MCP server. It does not receive filesystem,
shell, Git, patch, delegated-run internals, or provider credentials from
CatDesk beyond the MCP messages the connector is authorized to send.

## Authentication and Exposure

Legacy public ngrok compatibility remains development-oriented. Route secrecy
is not authentication.

External tunnel mode is operator-owned. CatDesk reports whether local and
optional remote checks passed, but it does not own the external tunnel.

OpenAI Secure MCP relies on OpenAI's tunnel control plane, workspace/org
association, and runtime API key handling through the official client. CatDesk
does not invent a second bearer-token layer for this mode unless a compatible
ChatGPT authentication path is proven later.

## Secrets

CatDesk must not persist or log:

- OpenAI runtime API keys;
- ngrok authtokens;
- browser cookies;
- ChatGPT session data;
- full public MCP endpoints;
- persistent route values.

Status responses use fingerprints and redacted state summaries.

## Process Ownership

CatDesk-owned process:

- started by the current CatDesk process;
- stored as a child handle;
- stopped only through that handle during shutdown.

Externally owned process:

- started by the operator or another supervisor;
- never terminated by CatDesk;
- may be checked only through documented status/doctor interfaces.

CatDesk must not kill by PID alone after restart.

## Silent Fallback

Transport fallback is explicit configuration, not automatic recovery.

If `openai_secure_tunnel` fails, CatDesk reports a blocked, degraded, or failed
OpenAI tunnel state. It does not start ngrok.

## Remote Disclosure

The transport layer does not broaden delegated-run disclosure policy. Advisor
and remote-disclosure rules remain independent of MCP transport choice.

## Known Limitations

- Live OpenAI tunnel status requires operator credentials and Platform access.
- Manual ChatGPT connector setup remains outside CatDesk automation.
- OAuth support is out of scope.
- A production direct public endpoint posture remains out of scope.
