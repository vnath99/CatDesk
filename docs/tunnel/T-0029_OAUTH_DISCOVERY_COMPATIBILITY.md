# T-0029 OAuth Discovery Compatibility

## Scope

This compatibility behavior is limited to the OpenAI Secure MCP transport's
local OAuth Protected Resource Metadata (PRM) probe. It does not add OAuth,
an authorization server, bearer-token validation, credential storage, or a
`WWW-Authenticate` challenge.

## Investigation Result

The installed official `tunnel-client` v0.0.10 exposes no documented
configuration switch that disables HTTP OAuth discovery for an HTTP MCP
target. Its `sample_mcp_remote_no_auth` profile is configuration-equivalent to
the OAuth-friendly HTTP profile. The sample states that a no-auth server can
reach ready when every PRM candidate returns `404`, but the v0.0.10 discovery
implementation treats an empty `404` response body as a terminal error before
its documented `404` fallback is reached.

The same official source includes a discovery test for a successful PRM JSON
document that contains a `resource` value and omits `authorization_servers`.
RFC 9728 makes `resource` required and `authorization_servers` optional. This
is the smallest client-compatible representation of an MCP resource that does
not advertise an OAuth authorization server.

## Implemented Behavior

For the already-configured MCP path `/<route>/mcp`, add only this GET route:

```text
/.well-known/oauth-protected-resource/<route>/mcp
```

It returns:

```http
200 OK
Content-Type: application/json
```

```json
{
  "resource": "<the local MCP endpoint URL configured for this CatDesk process>"
}
```

The route deliberately omits `authorization_servers`, does not emit
`WWW-Authenticate`, and does not inspect or require an `Authorization` header.
The host-root PRM route remains absent. Query-bearing discovery requests are
rejected so the endpoint remains an exact, bounded probe surface.

The local resource URL is only consumed by the loopback tunnel client; the
OpenAI Secure MCP transport retains the public HTTPS boundary. The response
does not expose a route in normal CatDesk logs, review artifacts, or MCP tool
output.

The handler reads only the in-memory bind host, port, and MCP path. It does
not write configuration or persist any credential. The normal MCP endpoint
continues to apply its existing authentication behavior independently.

## Preserved Behavior

- MCP `POST`, `GET`, and `DELETE` behavior at `/<route>/mcp` is unchanged.
- Loopback binding and existing MCP authorization behavior are unchanged.
- No OAuth credential, provider, token, or authorization server is created.
- `openai_secure_tunnel` does not fall back to ngrok.

## Test Plan

- exact PRM GET returns JSON containing only the configured local resource;
- query-bearing discovery requests do not resolve;
- the endpoint accepts no authorization requirement;
- authenticated MCP `initialize` and `tools/list` remain unchanged;
- no OAuth metadata advertises an authorization server and the handler has no
  persistence path;
- the official client reached `/healthz` and `/readyz` with HTTP `200` after
  the local check, while `/api/oauth` reported no error and zero authorization
  servers.
