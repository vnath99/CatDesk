[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateNotNullOrEmpty()]
    [string]$PythonExecutable,

    [switch]$InitializeEnvironment,
    [switch]$InstallSeleniumBase,
    [switch]$WriteConfig,
    [switch]$OverwriteConfig,

    [string]$ConversationUrl,
    [string]$ProfileRelativePath = ".catdesk/wake-bridge/browser-profile"
)

$ErrorActionPreference = "Stop"
$workspace = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$wakeRoot = Join-Path $workspace ".catdesk/wake-bridge"
$venvRoot = Join-Path $wakeRoot "venv"
$venvPython = Join-Path $venvRoot "Scripts/python.exe"
$requirements = Join-Path $PSScriptRoot "requirements-wake-bridge.txt"
$bridge = Join-Path $PSScriptRoot "wake_bridge.py"

function Require-Success([string]$Operation) {
    if ($LASTEXITCODE -ne 0) {
        throw "$Operation failed with exit code $LASTEXITCODE."
    }
}

function Resolve-ProjectPath([string]$Candidate) {
    $full = if ([IO.Path]::IsPathRooted($Candidate)) {
        [IO.Path]::GetFullPath($Candidate)
    } else {
        [IO.Path]::GetFullPath((Join-Path $workspace $Candidate))
    }
    $rootWithSeparator = $workspace.TrimEnd([IO.Path]::DirectorySeparatorChar, [IO.Path]::AltDirectorySeparatorChar) + [IO.Path]::DirectorySeparatorChar
    if (-not $full.StartsWith($rootWithSeparator, [StringComparison]::OrdinalIgnoreCase)) {
        throw "Wake profile path must stay inside the approved workspace."
    }
    return $full
}

if (-not (Test-Path -LiteralPath $requirements -PathType Leaf) -or -not (Test-Path -LiteralPath $bridge -PathType Leaf)) {
    throw "Wake-bridge bootstrap files are missing from this workspace."
}

$profilePath = Resolve-ProjectPath $ProfileRelativePath

if ($InitializeEnvironment) {
    if (-not (Test-Path -LiteralPath $PythonExecutable -PathType Leaf)) {
        throw "The explicitly supplied Python executable was not found."
    }
    if (Test-Path -LiteralPath $venvPython -PathType Leaf) {
        throw "The project-local wake-bridge environment already exists; refusing to replace it."
    }
    New-Item -ItemType Directory -Force -Path $wakeRoot | Out-Null
    & $PythonExecutable -m venv $venvRoot
    Require-Success "Project-local virtual-environment creation"
}

if (($InstallSeleniumBase -or $WriteConfig) -and -not (Test-Path -LiteralPath $venvPython -PathType Leaf)) {
    throw "Create the project-local environment first with -InitializeEnvironment."
}

if ($InstallSeleniumBase) {
    # This is an explicit operator action. It uses only the pinned local
    # manifest and does not launch a browser, open a profile, or authenticate.
    & $venvPython -m pip install --disable-pip-version-check --no-input -r $requirements
    Require-Success "Pinned SeleniumBase installation"
}

if ($WriteConfig) {
    if ([string]::IsNullOrWhiteSpace($ConversationUrl)) {
        throw "-ConversationUrl is required with -WriteConfig."
    }

    try {
        $conversationUri = [Uri]$ConversationUrl
    } catch {
        throw "Conversation URL is invalid."
    }
    if (-not $conversationUri.IsAbsoluteUri -or
        $conversationUri.Scheme -ne "https" -or
        $conversationUri.Host -notin @("chatgpt.com", "chat.openai.com") -or
        -not [string]::IsNullOrEmpty($conversationUri.Query) -or
        -not [string]::IsNullOrEmpty($conversationUri.Fragment) -or
        -not [string]::IsNullOrEmpty($conversationUri.UserInfo) -or
        -not $conversationUri.IsDefaultPort -or
        $conversationUri.AbsolutePath -notmatch '^/(?:c/[A-Za-z0-9_-]{1,200}|g/[A-Za-z0-9_-]{1,200}/c/[A-Za-z0-9_-]{1,200})$') {
        throw "Conversation URL must be an exact supported ChatGPT conversation URL."
    }

    $configPath = Join-Path $wakeRoot "config.json"
    if ((Test-Path -LiteralPath $configPath -PathType Leaf) -and -not $OverwriteConfig) {
        throw "Wake-bridge config already exists; pass -OverwriteConfig to replace it."
    }

    New-Item -ItemType Directory -Force -Path $wakeRoot | Out-Null
    New-Item -ItemType Directory -Force -Path $profilePath | Out-Null

    $config = [ordered]@{
        conversation_url = $ConversationUrl
        debounce_seconds = 60
        profile_dir = $profilePath
        schema_version = 1
        send_confirmation_timeout_seconds = 15.0
        ui_poll_interval_seconds = 0.25
        ui_ready_timeout_seconds = 20.0
    }
    $json = $config | ConvertTo-Json -Depth 4
    $temporaryConfig = "$configPath.tmp"
    [IO.File]::WriteAllText(
        $temporaryConfig,
        $json + [Environment]::NewLine,
        [Text.UTF8Encoding]::new($false)
    )
    if (Test-Path -LiteralPath $configPath -PathType Leaf) {
        # File.Replace() is not reliable across the Windows PowerShell/.NET
        # combinations used by operators (a null backup path can be rejected
        # as an illegal path). Copy with overwrite is supported consistently
        # on Windows PowerShell 5.1 and preserves the deterministic temp-file
        # write before replacing the configured contents.
        [IO.File]::Copy($temporaryConfig, $configPath, $true)
        [IO.File]::Delete($temporaryConfig)
    } else {
        [IO.File]::Move($temporaryConfig, $configPath)
    }
}

if (-not $InitializeEnvironment -and -not $InstallSeleniumBase -and -not $WriteConfig) {
    throw "Select at least one explicit bootstrap action."
}

Write-Host "Wake-bridge bootstrap step completed. No browser was launched and no credentials were read."
