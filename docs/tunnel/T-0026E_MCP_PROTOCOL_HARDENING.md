# T-0026E MCP Protocol Hardening

## Scope

T-0026E keeps CatDesk transport behavior unchanged and hardens the MCP
server surface used by every transport mode.

Implemented corrections:

- MCP POST bodies larger than 2 MiB are rejected before JSON parsing.
- CatDesk's internal MCP self-check header does not increment the visible
  remote request counter.
- CatDesk's internal MCP self-check header does not mark a remote client as
  connected.
- The long-running managed OpenAI tunnel-client child discards stdout and
  stderr by default so ordinary runtime does not retain tunnel diagnostics.

No delegated worker, Qwen, DeepSeek, ngrok, provider, job, patch, journal, or
authentication behavior was changed.

## Request Size Limit

The MCP server now rejects oversized JSON-RPC POST bodies with:

```text
HTTP 413 Payload Too Large
JSON-RPC code -32600
```

The error contains only a bounded generic message. It does not echo request
content.

## Self-Check Accounting

CatDesk's local transport self-check uses an internal request header. Requests
carrying that header still exercise the MCP protocol, but they are not counted
as remote plugin activity and do not set the remote-connected UI state.

This prevents local readiness probes from being mistaken for ChatGPT or tunnel
traffic.

## Inspector Status

Automated protocol tests cover the local MCP initialize/tools-list cycle and
representative call behavior. A live MCP Inspector run remains optional because
it can require a Node-based temporary tool invocation and an active local
CatDesk server. No global npm package was installed in this pass.

## Verification

Focused tests added:

- `post_mcp_rejects_oversized_request_before_parsing`
- `self_check_post_does_not_mark_remote_activity`

Full verification is recorded in the final T-0026 handoff and external review
bundle.
