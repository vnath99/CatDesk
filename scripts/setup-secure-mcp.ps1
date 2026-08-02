param(
    [ValidateSet("managed_ephemeral_ngrok", "external_tunnel", "openai_secure_tunnel")]
    [string]$TransportMode = "openai_secure_tunnel",
    [ValidateSet("plan", "install", "configure", "connect", "start_all", "status", "repair", "stop", "rollback")]
    [string]$Mode = "plan",
    [switch]$Plan,
    [switch]$InstallClient,
    [switch]$ConfigureCatDesk,
    [switch]$ConnectRuntime,
    [switch]$StartAll,
    [switch]$Status,
    [switch]$Repair,
    [switch]$StopRuntime,
    [switch]$RollbackClient,
    [string]$TunnelClientPath = "",
    [string]$ProfileName = "catdesk-local",
    [string]$RuntimeAlias = "catdesk-local",
    [string]$TunnelId = "",
    [string]$LocalMcpUrlPlaceholder = "http://127.0.0.1:<port>/<persistent-route>/mcp",
    [string]$ConfigPath = "",
    [switch]$LegacyDirectManaged
)

$ErrorActionPreference = "Stop"

if ($Plan) { $Mode = "plan" }
if ($InstallClient) { $Mode = "install" }
if ($ConfigureCatDesk) { $Mode = "configure" }
if ($ConnectRuntime) { $Mode = "connect" }
if ($StartAll) { $Mode = "start_all" }
if ($Status) { $Mode = "status" }
if ($Repair) { $Mode = "repair" }
if ($StopRuntime) { $Mode = "stop" }
if ($RollbackClient) { $Mode = "rollback" }

$UserProfile = [Environment]::GetFolderPath("UserProfile")
if (-not $ConfigPath) {
    $ConfigPath = Join-Path $UserProfile ".catdesk\config.toml"
}

function Redact-UserPath {
    param([string]$Path)
    if (-not $Path) { return $null }
    return $Path.Replace($UserProfile, "%USERPROFILE%")
}

function Redacted-String {
    param([string]$Value)
    if ([string]::IsNullOrWhiteSpace($Value)) { return $null }
    return "<redacted>"
}

function Find-TunnelClient {
    param([string]$ExplicitPath)
    $candidates = New-Object System.Collections.Generic.List[string]
    if ($ExplicitPath) { $candidates.Add($ExplicitPath) }
    $cmd = Get-Command "tunnel-client" -ErrorAction SilentlyContinue
    if ($null -ne $cmd) { $candidates.Add($cmd.Source) }
    $candidates.Add((Join-Path $UserProfile ".catdesk\tools\tunnel-client\current\tunnel-client.exe"))
    $candidates.Add((Join-Path $UserProfile ".catdesk\tools\tunnel-client\tunnel-client.exe"))
    foreach ($candidate in ($candidates | Select-Object -Unique)) {
        if ($candidate -and (Test-Path -LiteralPath $candidate -PathType Leaf)) { return $candidate }
    }
    return $null
}

function Client-Output {
    param([string]$Path, [string[]]$CommandArgs, [int]$MaxLines = 30)
    if (-not $Path) { return $null }
    try {
        return (& $Path @CommandArgs 2>$null | Select-Object -First $MaxLines) -join "`n"
    } catch {
        return "command check failed"
    }
}

function Supports-RuntimeSurface {
    param([string]$Path)
    $runtimeHelp = Client-Output -Path $Path -CommandArgs @("runtimes", "--help")
    $connectHelp = Client-Output -Path $Path -CommandArgs @("runtimes", "connect", "--help")
    $statusHelp = Client-Output -Path $Path -CommandArgs @("runtimes", "status", "--help")
    [pscustomobject][ordered]@{
        RuntimeConnect = ($runtimeHelp -match "connect" -and $connectHelp -match "--mcp-server-url")
        RuntimeStatusJson = ($runtimeHelp -match "--json" -or $statusHelp -match "--json")
        RuntimeStop = ($runtimeHelp -match "stop")
        RuntimeRemove = ($runtimeHelp -match "rm")
    }
}

function Read-CatDeskMcpConfig {
    param([string]$Path)
    $result = [ordered]@{ BindHost = "127.0.0.1"; Port = 3200; Route = "<persistent-route>"; Found = $false }
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { return [pscustomobject]$result }
    $result.Found = $true
    $section = ""
    foreach ($line in (Get-Content -LiteralPath $Path -ErrorAction SilentlyContinue)) {
        $trimmed = $line.Trim()
        if ($trimmed -match '^\[(.+)\]$') { $section = $Matches[1]; continue }
        if ($section -eq "mcp" -and $trimmed -match '^bind_host\s*=\s*"([^"]+)"') { $result.BindHost = $Matches[1] }
        if ($section -eq "mcp" -and $trimmed -match '^port\s*=\s*(\d+)') { $result.Port = [int]$Matches[1] }
        if ($section -eq "mcp" -and $trimmed -match '^route_id\s*=\s*"([^"]+)"') { $result.Route = "<redacted>" }
    }
    [pscustomobject]$result
}

function Configure-CatDesk {
    $dir = Split-Path -Parent $ConfigPath
    New-Item -ItemType Directory -Force -Path $dir | Out-Null
    $processMode = if ($LegacyDirectManaged) { "legacy_direct_managed" } else { "official_runtime" }
    $snippet = @"

[tunnel]
mode = "openai_secure_tunnel"
manage_process = false

[openai_tunnel]
profile_name = "$ProfileName"
runtime_alias = "$RuntimeAlias"
process_mode = "$processMode"
auto_connect = true
auto_recover = true
health_poll_seconds = 5
failure_threshold = 3
recovery_cooldown_seconds = 60
keep_runtime_on_catdesk_exit = true
"@
    if (Test-Path -LiteralPath $ConfigPath -PathType Leaf) {
        $existing = Get-Content -LiteralPath $ConfigPath -Raw
        if ($existing -match '\[openai_tunnel\]') {
            return "config already contains [openai_tunnel]; no automatic rewrite performed"
        }
        Add-Content -LiteralPath $ConfigPath -Value $snippet
    } else {
        Set-Content -LiteralPath $ConfigPath -Value $snippet -Encoding UTF8
    }
    return "configured"
}

function Invoke-InstallScript {
    param([string]$InstallMode)
    $script = Join-Path (Get-Location).Path "scripts\install-openai-tunnel-client.ps1"
    & powershell -NoProfile -ExecutionPolicy Bypass -File $script "-$InstallMode" | ConvertFrom-Json
}

function Connect-Runtime {
    param([string]$ClientPath, [string]$LocalMcpUrl)
    if (-not $TunnelId) { throw "TunnelId is required for runtime connect" }
    if (-not [Environment]::GetEnvironmentVariable("CONTROL_PLANE_API_KEY")) {
        throw "CONTROL_PLANE_API_KEY must be present in the process environment for runtime connect"
    }
    & $ClientPath runtimes connect --alias $RuntimeAlias --tunnel-id $TunnelId --runtime-api-key "env:CONTROL_PLANE_API_KEY" --mcp-server-url $LocalMcpUrl | Out-Null
    return "connect invoked"
}

$clientPath = Find-TunnelClient -ExplicitPath $TunnelClientPath
$mcpConfig = Read-CatDeskMcpConfig -Path $ConfigPath
$localMcpUrl = $LocalMcpUrlPlaceholder
if ($mcpConfig.Found -and $mcpConfig.Route -eq "<redacted>") {
    $localMcpUrl = "http://$($mcpConfig.BindHost):$($mcpConfig.Port)/<redacted>/mcp"
}
$capabilities = Supports-RuntimeSurface -Path $clientPath
$mutations = New-Object System.Collections.Generic.List[string]
$errors = New-Object System.Collections.Generic.List[string]

try {
    switch ($Mode) {
        "install" {
            $installResult = Invoke-InstallScript -InstallMode "Install"
            $mutations.Add("installed tunnel-client")
            $clientPath = Find-TunnelClient -ExplicitPath $TunnelClientPath
        }
        "configure" {
            $mutations.Add((Configure-CatDesk))
        }
        "connect" {
            $mutations.Add((Connect-Runtime -ClientPath $clientPath -LocalMcpUrl $localMcpUrl))
        }
        "start_all" {
            if (-not $clientPath) {
                $installResult = Invoke-InstallScript -InstallMode "Install"
                $mutations.Add("installed tunnel-client")
                $clientPath = Find-TunnelClient -ExplicitPath $TunnelClientPath
            }
            $mutations.Add((Configure-CatDesk))
            $mutations.Add((Connect-Runtime -ClientPath $clientPath -LocalMcpUrl $localMcpUrl))
        }
        "repair" {
            if (-not $clientPath) {
                $installResult = Invoke-InstallScript -InstallMode "Install"
                $mutations.Add("installed tunnel-client")
            } else {
                $mutations.Add("client present; inspect status before reconnect")
            }
        }
        "stop" {
            if (-not $clientPath) { throw "tunnel-client is not installed" }
            & $clientPath runtimes stop $RuntimeAlias | Out-Null
            $mutations.Add("stopped runtime alias")
        }
        "rollback" {
            $rollbackResult = Invoke-InstallScript -InstallMode "Rollback"
            $mutations.Add("rolled back tunnel-client")
        }
    }
} catch {
    $errors.Add($_.Exception.Message)
}

$clientPath = Find-TunnelClient -ExplicitPath $TunnelClientPath
$capabilities = Supports-RuntimeSurface -Path $clientPath
$statusOutput = $null
if ($clientPath -and $Mode -in @("status", "repair", "start_all", "connect")) {
    $statusOutput = Client-Output -Path $clientPath -CommandArgs @("runtimes", "status", $RuntimeAlias, "--json") -MaxLines 40
}

[pscustomobject][ordered]@{
    Wizard = "catdesk-secure-mcp-setup"
    Mode = $Mode
    TransportMode = $TransportMode
    ProcessMode = if ($LegacyDirectManaged) { "legacy_direct_managed" } else { "official_runtime" }
    Client = [pscustomobject][ordered]@{
        Found = [bool]$clientPath
        Path = if ($clientPath) { Redact-UserPath $clientPath } else { $null }
        Version = Client-Output -Path $clientPath -CommandArgs @("--version") -MaxLines 1
        Capabilities = $capabilities
    }
    Profile = @{ Supplied = -not [string]::IsNullOrWhiteSpace($ProfileName); Value = Redacted-String $ProfileName }
    RuntimeAlias = @{ Supplied = -not [string]::IsNullOrWhiteSpace($RuntimeAlias); Value = Redacted-String $RuntimeAlias }
    TunnelId = @{ Supplied = -not [string]::IsNullOrWhiteSpace($TunnelId); Value = Redacted-String $TunnelId }
    RuntimeCredential = @{ Name = "CONTROL_PLANE_API_KEY"; Present = [bool][Environment]::GetEnvironmentVariable("CONTROL_PLANE_API_KEY"); Value = "<redacted>" }
    LocalMcp = @{ ConfigFound = $mcpConfig.Found; Endpoint = "<redacted-local-mcp-url>" }
    RuntimeStatus = if ($statusOutput) { "<redacted-status-output-present>" } else { $null }
    OperatorSteps = @(
        "Create or select an OpenAI Secure MCP tunnel in Platform.",
        "Provide CATDESK_OPENAI_TUNNEL_ID/TunnelId and CONTROL_PLANE_API_KEY only at runtime.",
        "Use this wizard in explicit mutating modes only when ready.",
        "Create/select the ChatGPT connector through the OpenAI tunnel after runtime readiness."
    )
    CommandTemplates = @(
        ".\scripts\install-openai-tunnel-client.ps1 -Plan",
        ".\scripts\install-openai-tunnel-client.ps1 -Install",
        '$env:CONTROL_PLANE_API_KEY = "<runtime-api-key>"',
        ".\scripts\setup-secure-mcp.ps1 -ConnectRuntime -TunnelId <tunnel-id>",
        ".\scripts\setup-secure-mcp.ps1 -Status",
        ".\scripts\setup-secure-mcp.ps1 -StopRuntime",
        ".\scripts\install-openai-tunnel-client.ps1 -Rollback"
    )
    MutationsPerformed = @($mutations)
    Errors = @($errors)
    Notes = @(
        "Default mode is plan and performs no mutation.",
        "The runtime API key is never stored or printed.",
        "No Platform tunnel, ChatGPT connector, service, firewall, or PATH mutation is created."
    )
} | ConvertTo-Json -Depth 10
