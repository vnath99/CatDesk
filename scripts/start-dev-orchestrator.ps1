param(
    [string]$Workspace = (Get-Location).Path,
    [string]$ConfigPath = (Join-Path (Get-Location).Path (".tmp\catdesk-dev-orchestrator\config-{0}.toml" -f $PID)),
    [string]$ListenHost = "127.0.0.1",
    [int]$Port = 38766,
    [string]$McpPath = "/catdesk-dev/mcp",
    [string]$AuthToken = ("catdesk-dev-" + [guid]::NewGuid().ToString("N"))
)

$ErrorActionPreference = "Stop"

$workspacePath = Resolve-Path -LiteralPath $Workspace
$configDir = Split-Path -Parent $ConfigPath
New-Item -ItemType Directory -Force -Path $configDir | Out-Null

$env:CATDESK_DELEGATED_DEV = "1"
cargo run -- `
    --headless-mcp `
    --workspace $workspacePath.Path `
    --config-path $ConfigPath `
    --host $ListenHost `
    --port $Port `
    --mcp-path $McpPath `
    --tool-mode supervisor-only `
    --auth-token $AuthToken
