# Morning Manual Plugin Validation

Use this checklist after reviewing the T-0025 branch. Do not paste the actual
MCP URL into this file, logs, or review bundles.

## Preconditions

- CatDesk is built from the `infra/stable-mcp-transport` branch.
- The desired transport mode is selected in `%USERPROFILE%\.catdesk\config.toml`.
- For `managed_ephemeral_ngrok`, the URL is expected to change across restarts.
- For `external_tunnel`, the external tunnel is already running and points to
  the local CatDesk MCP server.
- `managed_stable_ngrok` remains unavailable until the assigned-domain
  feasibility proof passes.
- `openai_secure_tunnel` remains unavailable until tunnel-client proof and
  account permissions are approved.

## Steps

1. Start CatDesk.
2. Confirm the local MCP server is running.
3. Confirm the selected public or private transport is connected.
4. In ChatGPT developer mode, create or select the CatDesk plugin connection.
5. Call `catdesk_instruction`.
6. Call `delegated_run_list`.
7. Stop CatDesk cleanly.
8. Restart CatDesk with the same selected transport.
9. Reuse the same ChatGPT plugin connection without recreating it.
10. Call `catdesk_instruction` again.
11. Call `delegated_run_list` again.
12. Run a harmless ChatGPT-supervised disposable smoke test only after the two
    read-only calls succeed.
13. Confirm no release, deployment, branch deletion, or main merge occurred
    during validation.

## Expected Results

- Legacy ephemeral mode: the plugin may need URL refresh after restart.
- External tunnel mode: the plugin should remain usable if the external tunnel
  endpoint and CatDesk persistent route are unchanged.
- Managed stable ngrok: not ready from this branch because T-0025C0 was blocked.
- OpenAI Secure MCP Tunnel: not ready from this branch because `tunnel-client`
  is not installed and the account/tunnel identity proof has not run.

## Failure Handling

Stop and record `PARTIAL` or `BLOCKED` if:

- the plugin cannot call `catdesk_instruction`;
- the plugin cannot call `delegated_run_list`;
- the endpoint changes unexpectedly in a mode that should be stable;
- CatDesk falls back to a different transport without an explicit error;
- credentials, route values, or full MCP URLs appear in logs or review bundles.
