param(
    [string]$Workspace = (Get-Location).Path,
    [string]$ConfigPath = (Join-Path (Get-Location).Path (".tmp\catdesk-local-orchestrator\config-{0}.toml" -f $PID)),
    [string]$ListenHost = "127.0.0.1",
    [int]$Port = 38765,
    [string]$McpPath = "/catdesk/mcp",
    [string]$AuthToken
)

$ErrorActionPreference = "Stop"

$workspacePath = Resolve-Path -LiteralPath $Workspace
$configDir = Split-Path -Parent $ConfigPath
New-Item -ItemType Directory -Force -Path $configDir | Out-Null

$catdesk = Get-Command catdesk -ErrorAction SilentlyContinue
if ($null -eq $catdesk) {
    $candidate = Join-Path (Get-Location).Path "target\release\catdesk.exe"
    if (Test-Path -LiteralPath $candidate) {
        $catdeskPath = $candidate
    } else {
        throw "catdesk executable was not found on PATH or at target\release\catdesk.exe"
    }
} else {
    $catdeskPath = $catdesk.Source
}

if ([string]::IsNullOrWhiteSpace($AuthToken)) {
    $AuthToken = "catdesk-local-" + [guid]::NewGuid().ToString("N")
}

$env:CATDESK_MCP_AUTH_TOKEN = $AuthToken

& $catdeskPath `
    --headless-mcp `
    --workspace $workspacePath.Path `
    --config-path $ConfigPath `
    --host $ListenHost `
    --port $Port `
    --mcp-path $McpPath `
    --tool-mode supervisor-only
