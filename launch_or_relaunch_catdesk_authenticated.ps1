# CatDesk authenticated Codex launch/relaunch helper
#
# Run from:
# <USER_PROFILE>\OneDrive\Desktop\Projects\CatDesk-codex-loop
#
# This script:
#   1. Uses the normal authenticated per-user Codex home.
#   2. Finds the real OpenAI Codex executable.
#   3. Proves that exact executable sees the ChatGPT login.
#   4. Sets the CatDesk operator-local Codex environment.
#   5. If CatDesk is already listening on MCP port 3200, performs the
#      verified PID-scoped restart handoff.
#   6. If CatDesk is NOT running, launches the verified release binary
#      directly from this PowerShell environment.
#   7. Waits for local MCP readiness.
#
# It does NOT read or print Codex credential contents.

$ErrorActionPreference = "Stop"

Write-Host ""
Write-Host "=== CatDesk authenticated Codex launch / relaunch ==="
Write-Host ""

# ------------------------------------------------------------
# 1. Resolve workspace and verified CatDesk release binary
# ------------------------------------------------------------

$workspace = (Get-Location).Path
$catdeskExe = Join-Path $workspace "target\release\catdesk.exe"
$restartScript = Join-Path $workspace "scripts\restart_catdesk_daemon.ps1"

if (-not (Test-Path -LiteralPath $catdeskExe -PathType Leaf)) {
    throw "CatDesk release binary not found at: $catdeskExe"
}

if (-not (Test-Path -LiteralPath $restartScript -PathType Leaf)) {
    throw "Restart helper not found at: $restartScript"
}

Write-Host "[OK] CatDesk release binary found."
Write-Host "[OK] CatDesk restart helper found."

# ------------------------------------------------------------
# 2. Use normal authenticated global/per-user Codex home
# ------------------------------------------------------------

$env:CATDESK_CODEX_HOME = Join-Path $HOME ".codex"

if (-not (Test-Path -LiteralPath $env:CATDESK_CODEX_HOME -PathType Container)) {
    throw "Expected Codex home was not found."
}

Write-Host "[OK] CATDESK_CODEX_HOME exists."

# ------------------------------------------------------------
# 3. Locate the real OpenAI Codex executable
# ------------------------------------------------------------

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
    throw "Could not locate the OpenAI Codex executable."
}

$env:CATDESK_CODEX_CLI_EXECUTABLE = $codexExe

Write-Host "[OK] CATDESK_CODEX_CLI_EXECUTABLE located."

# ------------------------------------------------------------
# 4. Verify the exact Codex executable and authentication context
# ------------------------------------------------------------

Write-Host ""
Write-Host "Codex executable version:"
& $env:CATDESK_CODEX_CLI_EXECUTABLE --version

if ($LASTEXITCODE -ne 0) {
    throw "Direct Codex executable did not run successfully."
}

Write-Host ""
Write-Host "Direct Codex authentication status:"

$previousCodexHome = $env:CODEX_HOME

try {
    $env:CODEX_HOME = $env:CATDESK_CODEX_HOME
    & $env:CATDESK_CODEX_CLI_EXECUTABLE login status

    if ($LASTEXITCODE -ne 0) {
        throw "Direct Codex executable could not verify login status."
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

Write-Host ""
Write-Host "Environment checks:"
Write-Host "  CATDESK_CODEX_HOME present:" `
    (-not [string]::IsNullOrWhiteSpace($env:CATDESK_CODEX_HOME))
Write-Host "  CATDESK_CODEX_CLI_EXECUTABLE present:" `
    (-not [string]::IsNullOrWhiteSpace($env:CATDESK_CODEX_CLI_EXECUTABLE))

# ------------------------------------------------------------
# 5. Detect whether CatDesk is already listening on MCP port 3200
# ------------------------------------------------------------

Write-Host ""
Write-Host "Checking local MCP port 3200..."

$listener = Get-NetTCPConnection `
    -State Listen `
    -LocalPort 3200 `
    -ErrorAction SilentlyContinue |
    Where-Object { $_.LocalAddress -in @("127.0.0.1", "::1") } |
    Select-Object -First 1

if ($listener) {
    # --------------------------------------------------------
    # Existing CatDesk: use verified restart helper
    # --------------------------------------------------------

    $oldPid = [int]$listener.OwningProcess
    $oldProcess = Get-Process -Id $oldPid -ErrorAction Stop

    Write-Host "[INFO] Existing listener PID:" $oldPid
    Write-Host "[INFO] Existing listener process:" $oldProcess.ProcessName

    if ($oldProcess.ProcessName -ne "catdesk") {
        throw "Port 3200 is owned by a non-CatDesk process. Stopping for safety."
    }

    Write-Host ""
    Write-Host "Running restart preflight..."

    & $restartScript -OldPid $oldPid

    if ($LASTEXITCODE -ne 0) {
        throw "CatDesk restart preflight failed."
    }

    Write-Host ""
    Write-Host "Executing detached CatDesk restart..."

    & $restartScript -OldPid $oldPid -Execute

    if ($LASTEXITCODE -ne 0) {
        throw "CatDesk restart handoff failed."
    }
}
else {
    # --------------------------------------------------------
    # No CatDesk listener: launch directly from THIS shell
    # --------------------------------------------------------

    Write-Host "[INFO] No CatDesk listener exists on port 3200."
    Write-Host "[INFO] Launching CatDesk directly from this authenticated environment..."

    $catdeskProcess = Start-Process `
        -FilePath $catdeskExe `
        -ArgumentList "--auto-start-computer" `
        -WorkingDirectory $workspace `
        -PassThru

    Write-Host "[OK] CatDesk process launched. PID:" $catdeskProcess.Id
}

# ------------------------------------------------------------
# 6. Wait for MCP port 3200 to become ready
# ------------------------------------------------------------

Write-Host ""
Write-Host "Waiting for local MCP listener on port 3200..."

$deadline = (Get-Date).AddSeconds(90)
$readyListener = $null

while ((Get-Date) -lt $deadline) {
    Start-Sleep -Seconds 1

    $readyListener = Get-NetTCPConnection `
        -State Listen `
        -LocalPort 3200 `
        -ErrorAction SilentlyContinue |
        Where-Object { $_.LocalAddress -in @("127.0.0.1", "::1") } |
        Select-Object -First 1

    if ($readyListener) {
        break
    }
}

if (-not $readyListener) {
    throw "CatDesk did not open local MCP port 3200 within 90 seconds."
}

$newPid = [int]$readyListener.OwningProcess
$newProcess = Get-Process -Id $newPid -ErrorAction Stop

if ($newProcess.ProcessName -ne "catdesk") {
    throw "Port 3200 became ready, but it is not owned by CatDesk."
}

Write-Host "[OK] Local MCP port 3200 is listening."
Write-Host "[OK] CatDesk PID:" $newPid

# ------------------------------------------------------------
# 7. Bounded HTTP readiness check
# ------------------------------------------------------------

Write-Host ""
Write-Host "Checking CatDesk loopback HTTP readiness..."

try {
    $response = Invoke-WebRequest `
        -Uri "http://127.0.0.1:3200/" `
        -UseBasicParsing `
        -TimeoutSec 10

    Write-Host "[OK] CatDesk loopback HTTP status:" $response.StatusCode
}
catch {
    Write-Warning "Port 3200 is listening, but the HTTP readiness check did not complete successfully."
    Write-Warning $_.Exception.Message
}

Write-Host ""
Write-Host "=== CatDesk authenticated launch/relaunch completed ==="
Write-Host ""
Write-Host "The existing external Secure MCP tunnel was not intentionally stopped."
Write-Host "Wait a few seconds for it to reconnect to the new local CatDesk process."
Write-Host ""
Write-Host "Then return to ChatGPT and say:"
Write-Host "  CatDesk authenticated launch complete"
Write-Host ""
