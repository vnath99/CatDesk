# T-0029 Live Secure MCP Requirements Snapshot

Captured: 2026-08-07

## Official sources

- OpenAI Help Center: Developer mode and MCP apps in ChatGPT
  (`help.openai.com/en/articles/12584461-developer-mode-apps-and-full-mcp-connectors-in-chatgpt-beta`)
- OpenAI official tunnel-client repository and release history
  (`github.com/openai/tunnel-client`)

The Help Center documentation states that Secure MCP Tunnel is the supported
connection path for an MCP server on a private network or developer machine.
ChatGPT developer-mode and connector availability remain workspace and account
dependent.

## Installed official client

- Location: `%USERPROFILE%\\.catdesk\\tools\\tunnel-client\\current\\tunnel-client.exe`
- Version: `0.0.10+105e17a79a36e4e5c897fd698ed2b8dbf935b144`
- The public GitHub release history identifies `v0.0.10` as the current stable
  release. An installer notice for `v0.0.11` refers to a development release,
  so no update was performed.
- Verified native commands: `runtimes connect`, `runtimes status`,
  `runtimes stop`, `runtimes rm`, `doctor`, and `health`.

## Runtime requirements

The client requires a tunnel identifier and a runtime API-key reference. The
documented secret-safe form is an environment reference such as
`env:CONTROL_PLANE_API_KEY`; the key must not appear in CatDesk configuration,
argv, Git, logs, or review bundles.

For the T-0029 run, CatDesk must remain loopback-only and use
`openai_secure_tunnel` without an ngrok fallback. Readiness requires a healthy
local MCP target plus runtime `/healthz` and `/readyz` success, followed by a
successful control-plane poll.

## Current preflight result

- Runtime API-key presence: absent.
- Tunnel identifier presence: absent.
- Local runtime aliases: none.
- Client doctor: failed only because the tunnel ID is missing.

Account-side tunnel creation, workspace association, and the runtime API key
are therefore an operator gate. T-0029 must resume after those values are
supplied locally without being disclosed to CatDesk or committed to this
repository.
