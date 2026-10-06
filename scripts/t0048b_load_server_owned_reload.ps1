[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"
$workspace = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$buildPath = Join-Path $workspace ".catdesk\t0048b-server-owned-reload\debug\catdesk.exe"
$restartHelper = Join-Path $workspace "scripts\restart_catdesk_daemon.ps1"

if (-not (Test-Path -LiteralPath $buildPath -PathType Leaf)) {
    throw "T-0048B server-owned reload build not found: $buildPath"
}
if (-not (Test-Path -LiteralPath $restartHelper -PathType Leaf)) {
    throw "CatDesk restart helper not found."
}

if ([string]::IsNullOrWhiteSpace($env:CATDESK_CODEX_CLI_EXECUTABLE) -or
    -not (Test-Path -LiteralPath $env:CATDESK_CODEX_CLI_EXECUTABLE -PathType Leaf)) {
    $npmRoot = npm root -g
    if ([string]::IsNullOrWhiteSpace($npmRoot)) {
        throw "Unable to determine the global npm root for Codex discovery."
    }
    $codexExe = Get-ChildItem -Path $npmRoot -Recurse -Filter codex.exe -File -ErrorAction SilentlyContinue |
        Where-Object { $_.FullName -match '@openai' } |
        Select-Object -First 1 -ExpandProperty FullName
    if (-not $codexExe) {
        throw "Could not locate the native OpenAI Codex executable."
    }
    $env:CATDESK_CODEX_CLI_EXECUTABLE = $codexExe
}

Remove-Item Env:CATDESK_CODEX_HOME -ErrorAction SilentlyContinue
Remove-Item Env:CODEX_HOME -ErrorAction SilentlyContinue

& $env:CATDESK_CODEX_CLI_EXECUTABLE login status
if ($LASTEXITCODE -ne 0) {
    throw "Current-user Codex authentication check failed."
}

& $restartHelper -OldPid 0 -BuildPath $buildPath -Workspace $workspace -McpPort 3200 -Execute
if ($LASTEXITCODE -ne 0) {
    throw "CatDesk T-0048B bootstrap restart handoff failed."
}

Write-Host "T-0048B server-owned reload handoff launched. External Secure MCP tunnel untouched."
