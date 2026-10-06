[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$workspace = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$sourceExe = Join-Path $workspace '.catdesk\t0048b-server-owned-reload\debug\catdesk.exe'
$targetDir = Join-Path $workspace '.catdesk\t0048b-native-acceptance\debug'
$targetExe = Join-Path $targetDir 'catdesk.exe'
$configPath = Join-Path $HOME '.catdesk\config.toml'

if (-not (Test-Path -LiteralPath $sourceExe -PathType Leaf)) {
    throw 'Verified T-0048B source executable is missing.'
}
if (-not (Test-Path -LiteralPath $configPath -PathType Leaf)) {
    throw 'CatDesk config is unavailable.'
}

New-Item -ItemType Directory -Force -Path $targetDir | Out-Null
Copy-Item -LiteralPath $sourceExe -Destination $targetExe -Force
$sha256 = (Get-FileHash -LiteralPath $targetExe -Algorithm SHA256).Hash.ToLowerInvariant()

# Resolve the local MCP route without printing or persisting it anywhere else.
$configText = Get-Content -LiteralPath $configPath -Raw
$routeMatch = [regex]::Match($configText, '(?m)^\s*route_id\s*=\s*"([^"]+)"\s*$')
if (-not $routeMatch.Success) {
    throw 'CatDesk MCP route id was not found in the local config.'
}
$routeId = $routeMatch.Groups[1].Value
if ([string]::IsNullOrWhiteSpace($routeId)) {
    throw 'CatDesk MCP route id is empty.'
}
$endpoint = "http://127.0.0.1:3200/$routeId/mcp"

function Invoke-CatDeskReloadTool {
    param([hashtable]$Arguments)
    $body = @{
        jsonrpc = '2.0'
        id = [guid]::NewGuid().ToString('N')
        method = 'tools/call'
        params = @{
            name = 'catdesk_daemon_reload'
            arguments = $Arguments
        }
    } | ConvertTo-Json -Depth 8 -Compress

    $response = Invoke-RestMethod -Method Post -Uri $endpoint -ContentType 'application/json' -Body $body -TimeoutSec 15
    if ($null -ne $response.error) {
        throw "Native reload MCP call failed: $($response.error.message)"
    }
    if ($null -eq $response.result -or $null -eq $response.result.structuredContent) {
        throw 'Native reload MCP call returned no structuredContent.'
    }
    return $response.result.structuredContent
}

$dry = Invoke-CatDeskReloadTool -Arguments @{
    buildPath = $targetExe
    expectedSha256 = $sha256
    dryRun = $true
}

if ($dry.dryRun -ne $true -or [string]::IsNullOrWhiteSpace([string]$dry.confirmationToken)) {
    throw 'Native reload dry-run did not return a valid confirmation.'
}
if ([string]$dry.expectedSha256 -ne $sha256) {
    throw 'Native reload dry-run hash did not match the prepared replacement.'
}

$execute = Invoke-CatDeskReloadTool -Arguments @{
    buildPath = $targetExe
    expectedSha256 = $sha256
    dryRun = $false
    confirmationToken = [string]$dry.confirmationToken
}

if ($execute.accepted -ne $true) {
    throw 'Native reload execute was not accepted.'
}

Write-Host 'T0048B_NATIVE_RELOAD_EXECUTE_ACCEPTED'
Write-Host 'ROUTE_REDACTED=true'
Write-Host 'CONFIRMATION_TOKEN_REDACTED=true'
Write-Host 'EXTERNAL_TUNNEL_ACTION=none'
