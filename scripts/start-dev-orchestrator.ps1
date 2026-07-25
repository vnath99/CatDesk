param(
    [string]$Workspace = (Get-Location).Path,
    [string]$ConfigPath = (Join-Path (Get-Location).Path ".tmp\catdesk-dev-orchestrator\config.json"),
    [string]$Host = "127.0.0.1",
    [int]$Port = 38766,
    [string]$McpPath = "/catdesk-dev/mcp"
)

$ErrorActionPreference = "Stop"

$workspacePath = Resolve-Path -LiteralPath $Workspace
$configDir = Split-Path -Parent $ConfigPath
New-Item -ItemType Directory -Force -Path $configDir | Out-Null
if (!(Test-Path -LiteralPath $ConfigPath)) {
    "{}" | Set-Content -LiteralPath $ConfigPath -Encoding UTF8
}

$env:CATDESK_DELEGATED_DEV = "1"
cargo run -- `
    --headless-mcp `
    --workspace $workspacePath.Path `
    --config-path $ConfigPath `
    --host $Host `
    --port $Port `
    --mcp-path $McpPath `
    --tool-mode multi-tools
