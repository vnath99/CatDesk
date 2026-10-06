# T-0046 CatDesk trust-anchor reload
#
# Run from:
# <USER_PROFILE>\OneDrive\Desktop\Projects\CatDesk-codex-loop
#
# Loads the already-built debug CatDesk binary containing:
# - explicit gpt-5.6-terra -> model_reasoning_effort=high launch pin
# - oversized Codex JSONL omission instead of terminal failure
# - provider-terminal queue recovery
# - restart recovery for stale WORKER_RUNNING state
#
# This is a bootstrap reload only. T-0046 will then simplify normal
# operation so these manual environment steps are not routine.
#
# No credential files are read or printed.

$ErrorActionPreference = "Stop"

$workspace = (Get-Location).Path
$debugCatDesk = Join-Path $workspace "target\debug\catdesk.exe"
$restartScript = Join-Path $workspace "scripts\restart_catdesk_daemon.ps1"

if (-not (Test-Path -LiteralPath $debugCatDesk -PathType Leaf)) {
    throw "Patched debug CatDesk binary not found: $debugCatDesk"
}
if (-not (Test-Path -LiteralPath $restartScript -PathType Leaf)) {
    throw "Restart helper not found: $restartScript"
}

# Bootstrap with the already-authenticated per-user Codex context.
$env:CATDESK_CODEX_HOME = Join-Path $HOME ".codex"
if (-not (Test-Path -LiteralPath $env:CATDESK_CODEX_HOME -PathType Container)) {
    throw "Authenticated Codex home was not found."
}

# Resolve the real native Codex executable, not a PowerShell/CMD shim.
$npmRoot = npm root -g
if ([string]::IsNullOrWhiteSpace($npmRoot)) {
    throw "Unable to determine global npm root."
}

$codexExe = Get-ChildItem -Path $npmRoot `
    -Recurse `
    -Filter codex.exe `
    -File `
    -ErrorAction SilentlyContinue |
    Where-Object { $_.FullName -match '@openai' } |
    Select-Object -First 1 -ExpandProperty FullName

if (-not $codexExe) {
    throw "Could not locate the native OpenAI Codex executable."
}
$env:CATDESK_CODEX_CLI_EXECUTABLE = $codexExe

# Prove the exact binary is authenticated using the same context.
$priorCodexHome = $env:CODEX_HOME
try {
    $env:CODEX_HOME = $env:CATDESK_CODEX_HOME
    Write-Host "Direct Codex:"
    & $env:CATDESK_CODEX_CLI_EXECUTABLE --version
    & $env:CATDESK_CODEX_CLI_EXECUTABLE login status
    if ($LASTEXITCODE -ne 0) {
        throw "Direct Codex authentication check failed."
    }
}
finally {
    if ($null -eq $priorCodexHome) {
        Remove-Item Env:CODEX_HOME -ErrorAction SilentlyContinue
    } else {
        $env:CODEX_HOME = $priorCodexHome
    }
}

Write-Host ""
Write-Host "Reloading CatDesk from patched debug build..."
Write-Host "The helper will auto-resolve the current CatDesk PID from MCP port 3200."
Write-Host ""

& $restartScript `
    -OldPid 0 `
    -BuildPath $debugCatDesk `
    -Workspace $workspace `
    -McpPort 3200 `
    -Execute

if ($LASTEXITCODE -ne 0) {
    throw "CatDesk restart handoff failed."
}

Write-Host ""
Write-Host "Restart handoff launched."
Write-Host "Wait about 5-15 seconds for local MCP and the existing Secure MCP tunnel to reconnect."
Write-Host ""
Write-Host "Then return to ChatGPT and say:"
Write-Host "  T-0046 patched daemon loaded"
