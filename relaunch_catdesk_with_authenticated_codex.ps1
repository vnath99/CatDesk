# CatDesk authenticated Codex relaunch helper
# Run this from PowerShell in:
# <USER_PROFILE>\OneDrive\Desktop\Projects\CatDesk-codex-loop
#
# Purpose:
# 1) Use the authenticated per-user Codex home.
# 2) Locate the real OpenAI Codex executable.
# 3) Verify that executable sees the authenticated ChatGPT login.
# 4) Verify CatDesk's required environment variables are present.
# 5) Identify the CatDesk process listening on local MCP port 3200.
# 6) Run the CatDesk restart preflight.
# 7) Relaunch CatDesk from THIS PowerShell environment so the variables
#    propagate to the new CatDesk process.
#
# This script does not read or print Codex credential contents.

$ErrorActionPreference = "Stop"

Write-Host ""
Write-Host "=== CatDesk authenticated Codex relaunch ==="
Write-Host ""

# ------------------------------------------------------------
# 1. Use the authenticated global/per-user Codex home
# ------------------------------------------------------------

$env:CATDESK_CODEX_HOME = Join-Path $HOME ".codex"

if (-not (Test-Path -LiteralPath $env:CATDESK_CODEX_HOME -PathType Container)) {
    throw "Expected Codex home was not found: $env:CATDESK_CODEX_HOME"
}

Write-Host "[OK] CATDESK_CODEX_HOME exists."

# ------------------------------------------------------------
# 2. Locate the real Codex .exe installed by the npm package
#    rather than using codex.ps1 / codex.cmd.
# ------------------------------------------------------------

$npmRoot = npm root -g

if ([string]::IsNullOrWhiteSpace($npmRoot)) {
    throw "Unable to determine the global npm root."
}

$codexExe = Get-ChildItem -Path $npmRoot `
    -Recurse `
    -Filter codex.exe `
    -File `
    -ErrorAction SilentlyContinue |
    Where-Object { $_.FullName -match '@openai' } |
    Select-Object -First 1 -ExpandProperty FullName

if (-not $codexExe) {
    throw "Could not locate the OpenAI Codex executable beneath the global npm root."
}

$env:CATDESK_CODEX_CLI_EXECUTABLE = $codexExe

Write-Host "[OK] CATDESK_CODEX_CLI_EXECUTABLE located."

# ------------------------------------------------------------
# 3. Verify the executable itself runs
# ------------------------------------------------------------

Write-Host ""
Write-Host "Codex executable version:"
& $env:CATDESK_CODEX_CLI_EXECUTABLE --version

if ($LASTEXITCODE -ne 0) {
    throw "The direct Codex executable did not run successfully."
}

# ------------------------------------------------------------
# 4. Verify THIS direct executable sees the authenticated
#    Codex context when CODEX_HOME points to the same user home.
# ------------------------------------------------------------

Write-Host ""
Write-Host "Direct Codex authentication status:"

$previousCodexHome = $env:CODEX_HOME

try {
    $env:CODEX_HOME = $env:CATDESK_CODEX_HOME
    & $env:CATDESK_CODEX_CLI_EXECUTABLE login status

    if ($LASTEXITCODE -ne 0) {
        throw "The direct Codex executable could not verify login status."
    }
}
finally {
    if ($null -eq $previousCodexHome) {
        Remove-Item Env:CODEX_HOME -ErrorAction SilentlyContinue
    }
    else {
        $env:CODEX_HOME = $previousCodexHome
    }
}

# ------------------------------------------------------------
# 5. Confirm CatDesk variables are present.
#    Do not print the executable value.
# ------------------------------------------------------------

Write-Host ""
Write-Host "Environment checks:"

$codexHomePresent = -not [string]::IsNullOrWhiteSpace($env:CATDESK_CODEX_HOME)
$codexExePresent  = -not [string]::IsNullOrWhiteSpace($env:CATDESK_CODEX_CLI_EXECUTABLE)

Write-Host "  CATDESK_CODEX_HOME present:" $codexHomePresent
Write-Host "  CATDESK_CODEX_CLI_EXECUTABLE present:" $codexExePresent

if (-not $codexHomePresent) {
    throw "CATDESK_CODEX_HOME is missing."
}

if (-not $codexExePresent) {
    throw "CATDESK_CODEX_CLI_EXECUTABLE is missing."
}

# ------------------------------------------------------------
# 6. Verify we are in the CatDesk workspace.
# ------------------------------------------------------------

$workspace = (Get-Location).Path
$restartScript = Join-Path $workspace "scripts\restart_catdesk_daemon.ps1"

if (-not (Test-Path -LiteralPath $restartScript -PathType Leaf)) {
    throw "scripts\restart_catdesk_daemon.ps1 was not found. Run this script from the CatDesk-codex-loop repository root."
}

# ------------------------------------------------------------
# 7. Find the CatDesk process actually listening on local MCP
#    port 3200.
# ------------------------------------------------------------

Write-Host ""
Write-Host "Locating CatDesk MCP listener on port 3200..."

$listener = Get-NetTCPConnection `
    -State Listen `
    -LocalPort 3200 `
    -ErrorAction Stop |
    Where-Object { $_.LocalAddress -in @("127.0.0.1", "::1") } |
    Select-Object -First 1

if (-not $listener) {
    throw "No local MCP listener was found on port 3200."
}

$oldPid = [int]$listener.OwningProcess
$oldProcess = Get-Process -Id $oldPid -ErrorAction Stop

Write-Host "  Current MCP PID:" $oldPid
Write-Host "  Current MCP process:" $oldProcess.ProcessName

if ($oldProcess.ProcessName -ne "catdesk") {
    throw "Port 3200 is not owned by CatDesk. Stopping for safety."
}

# ------------------------------------------------------------
# 8. Run restart preflight.
# ------------------------------------------------------------

Write-Host ""
Write-Host "Running CatDesk restart preflight..."
Write-Host ""

& $restartScript -OldPid $oldPid

if ($LASTEXITCODE -ne 0) {
    throw "CatDesk restart preflight failed. No restart was attempted."
}

# ------------------------------------------------------------
# 9. Execute the detached restart from THIS environment.
# ------------------------------------------------------------

Write-Host ""
Write-Host "Preflight completed. Executing detached CatDesk restart..."
Write-Host ""

& $restartScript -OldPid $oldPid -Execute

if ($LASTEXITCODE -ne 0) {
    throw "CatDesk restart handoff failed."
}

Write-Host ""
Write-Host "=== Restart handoff launched successfully ==="
Write-Host ""
Write-Host "Wait several seconds for CatDesk to become ready and for the existing Secure MCP tunnel to reconnect."
Write-Host "Then return to ChatGPT and say: restarted from PowerShell"
Write-Host ""
