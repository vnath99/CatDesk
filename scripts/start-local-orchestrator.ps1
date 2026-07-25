param(
    [string]$Workspace = (Get-Location).Path,
    [string]$ConfigPath = (Join-Path (Get-Location).Path ".tmp\catdesk-local-orchestrator\config.json"),
    [string]$Host = "127.0.0.1",
    [int]$Port = 38765,
    [string]$McpPath = "/catdesk/mcp"
)

$ErrorActionPreference = "Stop"

$workspacePath = Resolve-Path -LiteralPath $Workspace
$configDir = Split-Path -Parent $ConfigPath
New-Item -ItemType Directory -Force -Path $configDir | Out-Null
if (!(Test-Path -LiteralPath $ConfigPath)) {
    "{}" | Set-Content -LiteralPath $ConfigPath -Encoding UTF8
}

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

& $catdeskPath `
    --headless-mcp `
    --workspace $workspacePath.Path `
    --config-path $ConfigPath `
    --host $Host `
    --port $Port `
    --mcp-path $McpPath `
    --tool-mode multi-tools
