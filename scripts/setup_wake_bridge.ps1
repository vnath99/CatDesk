[CmdletBinding()]
param(
    [string]$ConversationUrl
)

$ErrorActionPreference = "Stop"
$workspace = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$wakeRoot = Join-Path $workspace ".catdesk\wake-bridge"
$venvPython = Join-Path $wakeRoot "venv\Scripts\python.exe"
$profile = Join-Path $wakeRoot "browser-profile"
$config = Join-Path $wakeRoot "config.json"
$mcpReadyReceipt = Join-Path $wakeRoot "mcp-ready.json"
$bootstrap = Join-Path $PSScriptRoot "bootstrap_wake_bridge.ps1"
$bridge = Join-Path $PSScriptRoot "wake_bridge.py"
$loginHelper = Join-Path $PSScriptRoot "wake_profile_login.py"
Set-Location $workspace

function Require-Success([string]$Operation) {
    if ($LASTEXITCODE -ne 0) { throw "$Operation failed with exit code $LASTEXITCODE." }
}

function Resolve-Python3 {
    $py = Get-Command py.exe -ErrorAction SilentlyContinue
    if ($py) {
        foreach ($selector in @("-3.12", "-3")) {
            $candidate = & $py.Source $selector -c "import sys; print(sys.executable)" 2>$null
            if ($LASTEXITCODE -eq 0 -and $candidate) {
                $path = ([string]($candidate | Select-Object -Last 1)).Trim()
                if ($path -and (Test-Path -LiteralPath $path -PathType Leaf)) { return $path }
            }
        }
    }
    $python = Get-Command python.exe -ErrorAction SilentlyContinue
    if ($python) {
        $candidate = & $python.Source -c "import sys; assert sys.version_info.major == 3; print(sys.executable)" 2>$null
        if ($LASTEXITCODE -eq 0 -and $candidate) {
            $path = ([string]($candidate | Select-Object -Last 1)).Trim()
            if ($path -and (Test-Path -LiteralPath $path -PathType Leaf)) { return $path }
        }
    }
    throw "No usable Python 3 interpreter was found."
}

if (-not (Test-Path -LiteralPath $bootstrap -PathType Leaf) -or
    -not (Test-Path -LiteralPath $bridge -PathType Leaf) -or
    -not (Test-Path -LiteralPath $loginHelper -PathType Leaf)) {
    throw "Wake-bridge setup files are missing from this workspace."
}

if ([string]::IsNullOrWhiteSpace($ConversationUrl)) {
    $ConversationUrl = Read-Host "Paste the exact URL of THIS ChatGPT conversation"
}
if ([string]::IsNullOrWhiteSpace($ConversationUrl)) { throw "Conversation URL is required." }

$python3 = Resolve-Python3
New-Item -ItemType Directory -Force -Path $wakeRoot | Out-Null

if (-not (Test-Path -LiteralPath $venvPython -PathType Leaf)) {
    & $bootstrap -PythonExecutable $python3 -InitializeEnvironment
    Require-Success "Wake-bridge virtual environment creation"
}

& $venvPython -m pip install --disable-pip-version-check --upgrade pip
Require-Success "Wake-bridge pip upgrade"

& $bootstrap -PythonExecutable $python3 -InstallSeleniumBase
Require-Success "SeleniumBase installation"

& $bootstrap -PythonExecutable $python3 -WriteConfig -OverwriteConfig -ConversationUrl $ConversationUrl
Require-Success "Wake-bridge local configuration"

& $venvPython $loginHelper --conversation-url $ConversationUrl --profile-dir $profile --require-mcp-ready --mcp-ready-receipt $mcpReadyReceipt
Require-Success "Dedicated ChatGPT profile authentication and MCP readiness"

& $venvPython $bridge --workspace $workspace --config $config --clear-operator-attention --retire-backlog
Require-Success "Historical wake-inbox retirement"

& $venvPython -m unittest tests/test_wake_bridge.py -v
Require-Success "Wake-bridge deterministic Python tests"

Write-Host "WAKE_BRIDGE_SETUP_READY"
Write-Host "The dedicated profile has operator-confirmed CatDesk MCP readiness, historical inbox records are retired, and deterministic wake tests passed."
