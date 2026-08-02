# T-0026C OpenAI tunnel-client Discovery

Status: implemented with deterministic fake-client tests; live credentialed
OpenAI tunnel testing remains operator-owned.

## Official Source Basis

OpenAI Secure MCP Tunnel uses a customer-run `tunnel-client` process inside the
network that can reach the private MCP server. The client opens outbound HTTPS
to OpenAI, long-polls for tunnel work, forwards JSON-RPC to the local MCP
server, and returns responses through the tunnel. The operator must provide the
tunnel identity, a runtime API key, and Platform tunnel permissions.

The supported setup guidance is:

- use Platform tunnel settings or the latest public `openai/tunnel-client`
  release;
- use `tunnel-client help quickstart` as the first CLI discovery surface;
- use `tunnel-client init --profile <name> --tunnel-id <id>
  --mcp-server-url <local MCP URL>` for HTTP MCP profiles when supported;
- use `tunnel-client doctor --profile <name> --explain` for readiness;
- keep runtime API keys outside CatDesk configuration.

## CatDesk Policy

CatDesk does not download or execute `tunnel-client` during ordinary startup.
Discovery and installation are explicit operator actions.

Discovery precedence:

1. explicit configured path;
2. `PATH`;
3. `%USERPROFILE%\.catdesk\tools\tunnel-client\`;
4. documented known local paths, if added by a later phase.

Validation requires:

- expected executable name;
- existing file;
- not installed inside the controlled repository;
- bounded `--version` command;
- bounded `help quickstart` command;
- capability parsing for HTTP MCP support, doctor support, admin UI hints, and
  profile operations.

Install/update support is intentionally explicit:

- latest release metadata is resolved from the official OpenAI GitHub release
  API;
- Windows artifacts are selected dynamically;
- a SHA-256 checksum is required for automatic verification when official
  metadata supplies one;
- if no official checksum is available, CatDesk records a warning and requires
  operator confirmation before install;
- extraction uses a staging directory and preserves a previous binary for
  rollback;
- CatDesk does not modify global PATH, install a service, or store credentials.

## Config Section

Stored in `%USERPROFILE%\.catdesk\config.toml`:

```toml
[openai_tunnel]
client_path = "C:\\Users\\<user>\\.catdesk\\tools\\tunnel-client\\tunnel-client.exe"
profile_name = "catdesk-local"
process_mode = "external"
admin_ui_url = "http://127.0.0.1:<port>/ui"
```

Do not store:

- `CONTROL_PLANE_API_KEY`;
- OpenAI API keys;
- ChatGPT session data;
- browser cookies;
- tunnel runtime secrets.

## Verification

Automated tests use fake clients and local ZIP fixtures only. They verify
discovery precedence, invalid executables, unsupported output, release asset
selection, checksum failure, archive rollback, successful atomic-style install,
startup no-download policy, and absence of credential persistence.

Live setup remains unverified until the operator supplies:

1. a supported `tunnel-client` binary;
2. a tunnel ID from Platform tunnel settings;
3. a runtime API key in the environment;
4. Tunnels Read + Use permission;
5. a profile initialized with the official CLI syntax shown by the installed
   binary.
