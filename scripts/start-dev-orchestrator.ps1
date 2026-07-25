param(
    [string]$Workspace = (Get-Location).Path,
    [string]$ConfigPath = (Join-Path (Get-Location).Path (".tmp\catdesk-dev-orchestrator\config-{0}.toml" -f $PID)),
    [string]$ListenHost = "127.0.0.1",
    [int]$Port = 38766,
    [string]$McpPath = "/catdesk-dev/mcp",
    [string]$AuthToken
)

$ErrorActionPreference = "Stop"

$workspacePath = Resolve-Path -LiteralPath $Workspace
$configDir = Split-Path -Parent $ConfigPath
New-Item -ItemType Directory -Force -Path $configDir | Out-Null

if ([string]::IsNullOrWhiteSpace($AuthToken)) {
    throw "AuthToken is required. Generate a short-lived token and pass it with -AuthToken."
}

try {
    $env:CATDESK_MCP_AUTH_TOKEN = $AuthToken
    $env:CATDESK_DELEGATED_DEV = "1"
    cargo run -- `
        --headless-mcp `
        --workspace $workspacePath.Path `
        --config-path $ConfigPath `
        --host $ListenHost `
        --port $Port `
        --mcp-path $McpPath `
        --tool-mode supervisor-only
} finally {
    Remove-Item Env:\CATDESK_MCP_AUTH_TOKEN -ErrorAction SilentlyContinue
}
