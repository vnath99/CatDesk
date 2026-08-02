# T-0025C Transport Implementation

## Outcome

`PARTIAL`

T-0025C implemented the branch-safe runtime abstraction needed for external
tunnel mode and preserved the legacy managed ephemeral ngrok path. Managed
stable ngrok remains explicitly unavailable because T-0025C0 did not prove an
account-assigned stable development domain.

## Implemented

- Legacy `managed_ephemeral_ngrok` remains the default when transport fields are
  absent.
- Legacy mode still calls the existing ngrok SDK startup path.
- `external_tunnel` starts only the local CatDesk MCP server.
- `external_tunnel` does not launch, stop, or take ownership of ngrok.
- `external_tunnel` requires an HTTPS origin-only `public_base_url`.
- Stable nonlegacy modes use the persisted `mcp.route_id` as the runtime MCP
  route.
- External mode computes and stores a redacted connection fingerprint.
- External mode logs only a redacted MCP URL and the public development endpoint
  warning.
- `managed_stable_ngrok` returns an explicit unavailable error and does not
  silently fall back to ephemeral ngrok.

## Not Implemented

- Managed stable ngrok process launch with an assigned domain.
- Dynamic route remounting.
- Windows service installation.
- OpenAI Secure MCP Tunnel runtime support.
- ChatGPT plugin updates or account automation.

## C0 Dependency

The stable ngrok feasibility gate ended as
`BLOCKED_BY_ACCOUNT_CONFIGURATION`. The default endpoint control showed
`FAILED_DOMAIN_REUSE`, and assigned-domain discovery required an ngrok API key
that was not entered, printed, stored, or requested.

Until a later feasibility pass proves the assigned domain, CatDesk must keep
`managed_stable_ngrok` behind an explicit unavailable state.

## Verification Focus

Tests cover:

- external mode loading with persistent route identity;
- external mode startup without ngrok ownership;
- redacted external URL logging;
- public no-auth warning;
- managed stable startup failure without ephemeral fallback;
- endpoint normalization and route-case-preserving fingerprints;
- existing tunnel configuration and route-rotation invariants.
