# OpenAI Secure MCP Tunnel Setup on Windows

## Purpose

This guide describes the operator steps needed after CatDesk has been built
with `openai_secure_tunnel` support.

CatDesk does not create OpenAI tunnels, automate ChatGPT login, store runtime
API keys, install Windows services, or modify firewall/router policy.

## Prerequisites

- A CatDesk release binary built from this branch.
- An OpenAI Platform account with Secure MCP Tunnel access.
- A tunnel created or selected in the OpenAI Platform tunnel settings.
- Tunnel Read and Use permission for the runtime API key.
- The tunnel associated with the ChatGPT workspace or organization that will
  use the CatDesk connector.
- The official OpenAI `tunnel-client` from the `openai/tunnel-client` release
  channel or Platform tunnel settings.

References checked during implementation:

- OpenAI Secure MCP Tunnel guide
- Official `openai/tunnel-client` repository
- Official `tunnel-client` configuration and end-user guide

## Install or Discover the Client

Preferred user-level destination:

```text
%USERPROFILE%\.catdesk\tools\tunnel-client\
```

CatDesk discovery order:

```text
1. Explicit configured client path
2. PATH
3. User-level CatDesk tools directory
```

CatDesk does not modify system PATH.

## Read-Only Setup Wizard

Run the CatDesk setup wizard to generate a redacted operator checklist:

```powershell
.\scripts\setup-secure-mcp.ps1 `
  -TransportMode openai_secure_tunnel `
  -ProfileName "<profile-name>"
```

The wizard is read-only. It does not install software, initialize a profile,
start the tunnel client, automate ChatGPT, or store credentials.

## Create the Official Profile

Use the official client help for exact syntax on your installed version:

```powershell
tunnel-client --help
tunnel-client init --help
tunnel-client doctor --help
tunnel-client run --help
```

Then create an official profile with operator-provided values:

```powershell
$env:CONTROL_PLANE_API_KEY = "<runtime-api-key>"
tunnel-client init --profile "<profile-name>"
tunnel-client doctor --profile "<profile-name>" --explain
```

Do not put the runtime API key in CatDesk config, command-line arguments, logs,
review bundles, or shell profile files.

## CatDesk Configuration

Edit the existing user-level CatDesk config:

```text
%USERPROFILE%\.catdesk\config.toml
```

Example:

```toml
[tunnel]
mode = "openai_secure_tunnel"

[openai_tunnel]
client_path = "C:\\Users\\<YOU>\\.catdesk\\tools\\tunnel-client\\tunnel-client.exe"
profile_name = "catdesk-local"
process_mode = "external"
admin_ui_url = ""
```

Use `process_mode = "external"` first. In this mode, the operator starts and
owns the `tunnel-client` process. CatDesk will not stop it.

Managed mode is available for later controlled use:

```toml
process_mode = "managed"
```

Managed mode starts only the verified configured client and stops only the child
process handle that CatDesk created in the current process.

## Start Order

External mode:

```powershell
$env:CONTROL_PLANE_API_KEY = "<runtime-api-key>"
tunnel-client run --profile "<profile-name>"
```

In another terminal:

```powershell
target\release\catdesk.exe
```

Managed mode:

```powershell
$env:CONTROL_PLANE_API_KEY = "<runtime-api-key>"
target\release\catdesk.exe
```

## ChatGPT Connector

In ChatGPT, select the tunnel-backed connector according to the OpenAI product
UI. Do not paste CatDesk route secrets into review artifacts.

Initial read-only checks:

```text
catdesk_instruction
catdesk_transport_status
delegated_run_list
```

## Live Validation Status

This branch is automated-test complete for CatDesk-side behavior. A live OpenAI
Secure MCP tunnel run remains operator-gated because it requires tunnel ID,
runtime API key, Platform permissions, and ChatGPT workspace association.
