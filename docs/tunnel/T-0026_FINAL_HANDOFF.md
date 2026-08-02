# T-0026 Final Transport Handoff

## Overall Result

Ready for operator tunnel setup. CatDesk-side OpenAI Secure MCP support is
automated-test complete, but live OpenAI tunnel validation remains
operator-gated.

## Implemented

- Remote-sync and baseline audit.
- Truthful external transport health.
- Local MCP initialize and tools-list self-check.
- Optional bounded remote MCP self-check.
- Read-only `catdesk_transport_status`.
- Official OpenAI `tunnel-client` discovery and explicit user-level install
  support.
- `openai_secure_tunnel` runtime mode with external and managed process
  ownership.
- Credential presence check through process environment only for managed mode.
- External OpenAI mode does not require a CatDesk-held runtime API key.
- `/readyz`-based OpenAI tunnel readiness when a loopback admin URL is
  configured.
- No credential persistence or logging.
- No fallback from OpenAI Secure MCP to ngrok.
- MCP request-size hardening.
- Setup doctor transport reporting.
- Read-only Secure MCP setup wizard.
- Windows setup, operations, troubleshooting, rollback, and security docs.

## Live-Unverified

- OpenAI Secure MCP live tunnel because no tunnel ID, runtime API key, Platform
  permissions, or ChatGPT workspace association was supplied to Codex.
- ChatGPT connector continuity through Secure MCP Tunnel.
- Optional MCP Inspector run against an active local CatDesk instance.

## Verification

Completed after the T-0026F documentation/setup-doctor pass:

```text
cargo fmt --check
PASS

cargo clippy --all-targets --all-features -- -D warnings
PASS

cargo test
PASS: 329 passed, 9 ignored

cargo build --release
PASS

scripts/setup-doctor.ps1
PASS: redacted transport fields reported; tunnel-client absent; runtime
credential absent; exact Qwen model present.

scripts/setup-secure-mcp.ps1
PASS: emitted read-only redacted operator plan.
```

No live OpenAI Secure MCP tunnel test was run because Codex was not supplied a
tunnel ID, runtime API key, OpenAI Platform permissions, or ChatGPT workspace
association.

## Manual Operator Checklist

1. Create or select an OpenAI Secure MCP tunnel in Platform.
2. Ensure the runtime API key has Tunnels Read and Use permissions.
3. Associate the tunnel with the intended ChatGPT workspace or organization.
4. Install or locate the official `tunnel-client`.
5. Create the official client profile.
6. Set `CONTROL_PLANE_API_KEY` only for the process that runs the client.
7. Configure CatDesk `openai_secure_tunnel` mode.
8. Start external or managed tunnel mode.
9. In ChatGPT, call:
   - `catdesk_instruction`
   - `catdesk_transport_status`
   - `delegated_run_list`
10. Restart CatDesk and verify plugin continuity.
11. Run the harmless delegated Qwen smoke test.

## Rollback

Set:

```toml
[tunnel]
mode = "managed_ephemeral_ngrok"
```

or return to:

```text
infra/stable-mcp-transport
```
