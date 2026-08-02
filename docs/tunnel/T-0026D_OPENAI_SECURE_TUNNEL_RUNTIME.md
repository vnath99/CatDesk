# T-0026D OpenAI Secure MCP Tunnel Runtime

Status: implemented with fake-client runtime tests; live OpenAI tunnel proof is
operator-gated.

## Runtime Modes

`openai_secure_tunnel` supports two process ownership modes through
`[openai_tunnel].process_mode`.

`external` is the default. CatDesk validates the configured official
`tunnel-client` and profile readiness, but it does not launch or stop a tunnel
process.

`managed` starts `tunnel-client run --profile <profile>` only after discovery,
credential-presence check, and doctor pass. CatDesk stores only the child handle
it started and stops only that owned child during shutdown.

## Credential Boundary

CatDesk checks only whether `CONTROL_PLANE_API_KEY` is present in the process
environment. It does not read, print, persist, pass via command-line argument,
or log the key value.

CatDesk does not store tunnel IDs or runtime API keys. The profile is
operator-owned and initialized with official `tunnel-client` commands.

## Readiness

Before either process mode is accepted, CatDesk runs the official client doctor
shape:

```text
tunnel-client doctor --profile <profile> --explain
```

The exact binary path and profile are operator-local values from
`%USERPROFILE%\.catdesk\config.toml`.

## Failure Mapping

Failures are explicit and do not fall back to ngrok:

- missing or invalid client: `BLOCKED_MISSING_CLIENT`;
- missing profile name: `BLOCKED_MISSING_PROFILE`;
- missing runtime credential in the process environment:
  `BLOCKED_MISSING_CREDENTIAL`;
- doctor or process start failure: `FAILED`.

## Tests

Automated tests use fake `tunnel-client` scripts and synthetic environment
values only. They prove:

- missing client blocks without ngrok fallback;
- missing credential blocks after client discovery;
- external mode launches no process;
- managed mode owns one child process;
- logs do not contain the synthetic credential value.

## Live Status

Live OpenAI tunnel testing has not run. The operator still must provide:

1. an official supported `tunnel-client`;
2. a profile initialized with a real tunnel ID and local CatDesk MCP target;
3. `CONTROL_PLANE_API_KEY` in the process environment;
4. Platform Tunnels Read + Use permission;
5. ChatGPT developer-mode tunnel selection.
