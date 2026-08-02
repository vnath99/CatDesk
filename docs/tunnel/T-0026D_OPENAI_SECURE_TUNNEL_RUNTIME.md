# T-0026D OpenAI Secure MCP Tunnel Runtime

Status: implemented with fake-client runtime tests; live OpenAI tunnel proof is
operator-gated.

## Runtime Modes

`openai_secure_tunnel` supports two process ownership modes through
`[openai_tunnel].process_mode`.

`external` is the default. CatDesk does not discover the client, run doctor,
inspect `CONTROL_PLANE_API_KEY`, launch, or stop a tunnel process. If a loopback
admin URL is configured, CatDesk may probe `/readyz`; otherwise it reports
`CONFIGURED_UNVERIFIED`.

`managed` starts `tunnel-client run --profile <profile>` only after discovery,
credential-presence check, and doctor pass. CatDesk stores only the child handle
it started and stops only that owned child during shutdown.

## Credential Boundary

Managed mode checks only whether `CONTROL_PLANE_API_KEY` is present in the
CatDesk process environment. It does not read, print, persist, pass via
command-line argument, or log the key value. External mode does not inspect the
credential because the operator owns the external `tunnel-client` process.

CatDesk does not store tunnel IDs or runtime API keys. The profile is
operator-owned and initialized with official `tunnel-client` commands.

## Readiness

Before managed mode is accepted, CatDesk runs the official client doctor shape:

```text
tunnel-client doctor --profile <profile> --explain
```

The exact binary path and profile are operator-local values from
`%USERPROFILE%\.catdesk\config.toml`.

When `openai_tunnel.admin_ui_url` is configured, CatDesk checks the official
local health surfaces:

- `/healthz` for liveness;
- `/readyz` for readiness.

Only `/readyz` success can produce `CONNECTED_VERIFIED`.

## Failure Mapping

Failures are explicit and do not fall back to ngrok:

- missing or invalid client: `BLOCKED_MISSING_CLIENT`;
- missing profile name: `BLOCKED_MISSING_PROFILE`;
- missing runtime credential in the CatDesk process environment for managed
  mode:
  `BLOCKED_MISSING_CREDENTIAL`;
- doctor or process start failure: `FAILED`.

## Tests

Automated tests use fake `tunnel-client` scripts and synthetic environment
values only. They prove:

- missing client blocks without ngrok fallback;
- managed-mode missing credential blocks after client discovery;
- external mode launches no process and requires no CatDesk-held key;
- `/readyz` drives verified readiness;
- managed mode owns one child process;
- logs do not contain the synthetic credential value.

## Live Status

Live OpenAI tunnel testing has not run. The operator still must provide:

1. an official supported `tunnel-client`;
2. a profile initialized with a real tunnel ID and local CatDesk MCP target;
3. `CONTROL_PLANE_API_KEY` in the process environment;
4. Platform Tunnels Read + Use permission;
5. ChatGPT developer-mode tunnel selection.
