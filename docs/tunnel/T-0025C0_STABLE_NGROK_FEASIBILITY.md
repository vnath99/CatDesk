# T-0025C0 Stable ngrok Feasibility

## Outcome

`BLOCKED_BY_ACCOUNT_CONFIGURATION`

The installed ngrok runtime is usable, but the account-assigned development
domain could not be discovered or selected noninteractively during this phase.
The ngrok API client reported that an API key is required for reserved-domain
discovery. No API key was entered, printed, stored, or requested.

## Runtime Evidence

- ngrok version: `ngrok version 3.39.9-msix-stable`
- ngrok config check: valid
- API discovery: blocked because no ngrok API key is configured
- test port: `3217`
- active CatDesk processes before live probe: none observed
- active ngrok processes before live probe: none observed
- active ngrok processes after live probe: none observed
- owned ngrok children stopped: yes
- Windows service installation: not attempted
- token, route, and full endpoint disclosure: none

## Default Endpoint Control

A disposable local HTTP server was started on `127.0.0.1:3217` with `/health`
and `/instance` endpoints. Three owned `ngrok http 3217` starts were tested
without an assigned `--url` value.

Result:

- classification: `FAILED_DOMAIN_REUSE`
- restart count: `3`
- unique host fingerprints: `3`
- start 1 host fingerprint: `332d15aecccbafbd`
- start 2 host fingerprint: `d8a5f0e2deb20cfb`
- start 3 host fingerprint: `50f6b999df8ccbae`
- public `/health` result through each selected endpoint: `ok`

This proves the default ngrok endpoint behavior in this environment is
ephemeral for this test and is not sufficient for stable CatDesk MCP transport.

## Process Ownership

The live probe started only its own disposable local server and ngrok child
processes. It stopped only those owned children. It did not stop, reuse, or
take ownership of any pre-existing CatDesk or ngrok process.

## C0 Decision

Managed stable ngrok runtime support must remain unavailable until the operator
provides or confirms the assigned development domain through an approved,
non-secret configuration path or a later feasibility pass can discover it
without account interaction.

T-0025C may continue with fake-process tests, local abstractions, and external
tunnel behavior, but it must not claim live managed-stable-ngrok support from
this C0 result.

## Limitations

- No ngrok API key was configured for reserved-domain discovery.
- No account dashboard or browser automation was used.
- No ChatGPT plugin or account setting was changed.
- No full public endpoint, route slug, token, or credential is recorded here.
