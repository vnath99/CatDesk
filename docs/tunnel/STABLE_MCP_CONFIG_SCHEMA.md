# Stable MCP Transport Configuration Schema

Ticket: T-0025A  
Status: proposed schema only; no runtime behavior changed

## Storage

Transport settings must live outside the repository. T-0025B must extend the
existing CatDesk user configuration at:

```text
%USERPROFILE%\.catdesk\config.toml
```

Do not introduce a second active config root in the first runtime phase. A
future optional migration ticket may evaluate moving CatDesk configuration to a
more conventional Windows user-level location, but that migration must include
compatibility, rollback, and secret-handling review.

Current CatDesk source uses:

```text
%USERPROFILE%\.catdesk\config.toml
```

T-0025B configuration plan:

1. Load the existing `~/.catdesk/config.toml`.
2. Preserve all current fields and defaults.
3. Add transport fields under new tables.
4. Use atomic writes when saving route/config changes.
5. Do not move, duplicate, or rewrite config into a new root.

Secrets and persistent routes must never be stored in a repository, `.catdesk/`
inside a project, review bundle, or raw log.

## Proposed TOML

```toml
[mcp]
bind_host = "127.0.0.1"
port = 3200
route_id = ""
display_full_url = false
require_auth_token = false

[tunnel]
mode = "managed_ephemeral_ngrok"
public_base_url = ""
ngrok_domain = ""
ngrok_config_path = ""
manage_process = true
remote_self_check = false

[security]
warn_on_public_no_auth = true
redact_connection_url = true
allow_one_time_reveal = true

[identity]
installation_id = ""
last_connection_fingerprint = ""
```

Allowed values for `tunnel.mode`:

```text
managed_ephemeral_ngrok
managed_stable_ngrok
external_tunnel
openai_secure_tunnel
```

## Field Semantics

`mcp.bind_host`

- Default: `127.0.0.1`
- T-0025 should keep localhost binding by default.
- Binding to `0.0.0.0` should be rejected for stable/external tunnel modes
  unless a later ticket adds explicit operator warnings and tests.

`mcp.port`

- Default: `3200`
- Valid range: `1024..=65535`
- Invalid, occupied, or unparseable ports fail clearly.

`mcp.route_id`

- Empty in legacy ephemeral mode is allowed and preserves current generated
  behavior.
- Required in managed stable and external modes.
- Generated once when absent and stable mode is selected.
- Must match a conservative URL-safe slug.
- Stored without leading slash and without `/mcp`.

`mcp.display_full_url`

- Default: `false`
- Does not affect copy/reveal commands.
- Standard logs remain redacted even when a deliberate reveal is allowed.

`mcp.require_auth_token`

- Default: `false` in T-0025B for compatibility with the current ChatGPT custom
  plugin, which uses No Authentication and does not expose a generic static
  bearer-token field.
- Do not make this an unconditional default until a compatible ChatGPT
  authentication path is proven.
- Stable public ngrok with No Authentication is development-only and must warn
  prominently.
- OAuth implementation remains out of scope for T-0025.
- OpenAI Secure MCP Tunnel remains the preferred future private connection path
  if locally proven.

`tunnel.public_base_url`

- Used in `external_tunnel` mode.
- Must be HTTPS.
- Must not include path, query, or fragment.
- Must not include credentials.

`tunnel.ngrok_domain`

- Used in `managed_stable_ngrok` mode.
- Must be an operator-owned or account-assigned ngrok HTTPS domain.
- Do not infer or print this from a token.

`tunnel.ngrok_config_path`

- Optional path to an operator-managed ngrok config for CLI/service examples.
- CatDesk must not read tokens from arbitrary paths unless explicitly needed
  and approved.

`tunnel.manage_process`

- `true` for managed modes.
- `false` for external tunnel mode.
- Runtime validation should reject contradictory combinations.

`tunnel.remote_self_check`

- Default: `false`.
- When true, CatDesk may perform a bounded remote reachability check without
  logging the route or token.

`security.redact_connection_url`

- Default: `true`.
- Should be treated as required for review bundles and standard logs.

## Validation Rules

Required validation tests:

- Missing new config preserves legacy behavior.
- All known tunnel modes parse.
- Unknown tunnel mode is rejected.
- Ports outside range are rejected.
- `public_base_url` must be HTTPS and origin-only.
- `ngrok_domain` must be a hostname, not a full URL with path.
- Route IDs reject traversal, percent encoding, whitespace, path separators,
  query, fragment, and punctuation outside the approved slug alphabet.
- Route ID generation is stable after first write.
- Route rotation writes a new route, marks restart required, and updates the
  fingerprint after restart.
- Config writes are atomic.
- Route file/config permissions are owner-restricted or emit a warning when
  permissions cannot be restricted.
- Standard log redaction removes host plus route when configured.

## Redacted Display Examples

```text
Tunnel mode: managed_stable_ngrok
Local MCP: RUNNING 127.0.0.1:3200
Public endpoint: CONNECTED
MCP URL: https://<stable-ngrok-domain>/<redacted>/mcp
Connection ID: 8f31c0e4a91b
```

```text
Tunnel mode: external_tunnel
Local MCP: RUNNING 127.0.0.1:3200
Public endpoint: OPERATOR-MANAGED
MCP URL: https://<external-host>/<redacted>/mcp
Connection ID: c188b7e099e2
```

## Review-Bundle Rules

Bundles may include:

- Redacted config examples.
- Config schema tests.
- Route fingerprints.
- Process ownership records.

Bundles must exclude:

- Full MCP URL.
- Route secret.
- ngrok authtoken.
- OpenAI API key.
- ChatGPT account information.
- Browser profiles.
- Raw endpoint screenshots.

## Installation Identity Schema

Later phases should expose a redacted read-only identity object:

```json
{
  "installationId": "stable-across-restarts",
  "serverInstanceId": "unique-per-process",
  "gitCommit": "commit-sha",
  "dirtyBuild": false,
  "binarySha256": "safe-binary-fingerprint",
  "startupTime": "RFC3339",
  "workspaceHash": "redacted-workspace-fingerprint",
  "transportMode": "managed_stable_ngrok",
  "connectionFingerprint": "12 hex chars"
}
```

`installationId` is generated once and persists across restarts. It is not a
secret. `serverInstanceId` changes on each process start. `gitCommit`,
`dirtyBuild`, and `binarySha256` or a safe binary fingerprint support stale
binary detection. `connectionFingerprint` supports plugin continuity without
revealing the full URL.
