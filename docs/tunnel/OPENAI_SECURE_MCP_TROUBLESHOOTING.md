# OpenAI Secure MCP Troubleshooting

## BLOCKED_MISSING_CLIENT

CatDesk could not find a supported official `tunnel-client`.

Actions:

```powershell
tunnel-client --help
tunnel-client --version
```

If it is not installed, obtain it from OpenAI Platform tunnel settings or the
official `openai/tunnel-client` release channel. Prefer the user-level CatDesk
tools directory.

## BLOCKED_MISSING_PROFILE

The configured profile name is blank or unavailable.

Actions:

```powershell
tunnel-client init --help
tunnel-client doctor --profile "<profile-name>" --explain
```

Create the profile through official client commands only.

## BLOCKED_MISSING_CREDENTIAL

`CONTROL_PLANE_API_KEY` was not present for the process that needs to run
`tunnel-client`.

Actions:

```powershell
$env:CONTROL_PLANE_API_KEY = "<runtime-api-key>"
```

Do not store this key in CatDesk config, repo files, review bundles, or shell
profiles unless you have a separate approved secret-management procedure.

## LOCAL_MCP_UNAVAILABLE

CatDesk's own loopback MCP self-check failed.

Actions:

- Confirm CatDesk is running.
- Confirm the configured local host is loopback.
- Confirm the configured route is the active CatDesk route.
- Run CatDesk's transport health refresh.

Do not start a public fallback tunnel.

## DEGRADED or DISCONNECTED

Local MCP may be healthy, but remote tunnel reachability is not proven.

Actions:

- Run `tunnel-client doctor` with the configured profile.
- Check OpenAI Platform tunnel permissions.
- Confirm the tunnel is associated with the intended ChatGPT workspace or
  organization.
- Confirm ChatGPT uses the tunnel connector, not an old public endpoint.

## Managed Process Did Not Start

Actions:

- Verify the configured client path.
- Verify the binary fingerprint did not change unexpectedly.
- Verify the profile name is configured.
- Verify the runtime key is present only in the process environment.

CatDesk does not kill unrelated `tunnel-client` processes. If another process
owns the tunnel, use external mode or stop it manually outside CatDesk.

## Route or Endpoint Appears in Output

Stop and treat this as a release-blocking leak. Review:

- normal logs;
- review bundles;
- crash output;
- Git diff;
- staged files.

CatDesk status tools should return fingerprints, not full routes or full
public endpoints.
