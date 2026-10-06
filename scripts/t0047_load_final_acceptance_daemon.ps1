[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"
$workspace = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$buildPath = Join-Path $workspace ".catdesk\t0047-final-target\debug\catdesk.exe"
$restartHelper = Join-Path $workspace "scripts\restart_catdesk_daemon.ps1"

if (-not (Test-Path -LiteralPath $buildPath -PathType Leaf)) {
    throw "T-0047 final acceptance build not found: $buildPath"
}
if (-not (Test-Path -LiteralPath $restartHelper -PathType Leaf)) {
    throw "CatDesk restart helper not found."
}

# Resolve only the native OpenAI Codex executable locally. No auth/config file is opened.
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

# Normal operation must inherit the current Windows user's Codex context.
# These are only CatDesk/Codex-home overrides; credential files are not touched.
Remove-Item Env:CATDESK_CODEX_HOME -ErrorAction SilentlyContinue
Remove-Item Env:CODEX_HOME -ErrorAction SilentlyContinue

Write-Host "Checking current-user Codex authentication..."
& $env:CATDESK_CODEX_CLI_EXECUTABLE --version
& $env:CATDESK_CODEX_CLI_EXECUTABLE login status
if ($LASTEXITCODE -ne 0) {
    throw "Current-user Codex authentication check failed."
}

Write-Host ""
Write-Host "Loading T-0047 canonical-thread / Terra-High acceptance daemon..."
& $restartHelper `
    -OldPid 0 `
    -BuildPath $buildPath `
    -Workspace $workspace `
    -McpPort 3200 `
    -Execute

if ($LASTEXITCODE -ne 0) {
    throw "CatDesk restart handoff failed."
}

Write-Host ""
Write-Host "T-0047 restart handoff launched. Leave the external Secure MCP tunnel running."
Write-Host "Return to the existing ChatGPT conversation after about 5-15 seconds and send:"
Write-Host "  T-0047 final acceptance daemon loaded"
