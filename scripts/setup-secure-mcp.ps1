param(
    [ValidateSet("managed_ephemeral_ngrok", "external_tunnel", "openai_secure_tunnel")]
    [string]$TransportMode = "openai_secure_tunnel",
    [string]$TunnelClientPath = "",
    [string]$ProfileName = "catdesk-local",
    [string]$LocalMcpUrlPlaceholder = "http://127.0.0.1:<port>/<persistent-route>/mcp",
    [switch]$Managed
)

$ErrorActionPreference = "Continue"

function Redacted-String {
    param([string]$Value)
    if ([string]::IsNullOrWhiteSpace($Value)) {
        return $null
    }
    return "<redacted>"
}

function Find-TunnelClient {
    param([string]$ExplicitPath)
    $candidates = New-Object System.Collections.Generic.List[string]
    if ($ExplicitPath) {
        $candidates.Add($ExplicitPath)
    }
    $cmd = Get-Command "tunnel-client" -ErrorAction SilentlyContinue
    if ($null -ne $cmd) {
        $candidates.Add($cmd.Source)
    }
    $homeDir = [Environment]::GetFolderPath("UserProfile")
    $candidates.Add((Join-Path $homeDir ".catdesk\tools\tunnel-client\tunnel-client.exe"))
    $candidates.Add((Join-Path $homeDir ".catdesk\tools\tunnel-client\tunnel-client"))

    foreach ($candidate in ($candidates | Select-Object -Unique)) {
        if ($candidate -and (Test-Path -LiteralPath $candidate -PathType Leaf)) {
            return $candidate
        }
    }
    return $null
}

function Client-Help-Line {
    param([string]$Path, [string[]]$Args)
    if (-not $Path) {
        return $null
    }
    try {
        return (& $Path @Args 2>$null | Select-Object -First 1) -join "`n"
    } catch {
        return "help check failed"
    }
}

$clientPath = Find-TunnelClient -ExplicitPath $TunnelClientPath
$processMode = if ($Managed) { "managed" } else { "external" }
$quickstartHelp = Client-Help-Line -Path $clientPath -Args @("help", "quickstart")
$initHelp = Client-Help-Line -Path $clientPath -Args @("init", "--help")
$runHelp = Client-Help-Line -Path $clientPath -Args @("run", "--help")
$supportsMcpServerUrl = (($quickstartHelp -match "--mcp-server-url") -or ($initHelp -match "--mcp-server-url") -or ($runHelp -match "--mcp-server-url"))

$commands = @()
if ($TransportMode -eq "openai_secure_tunnel") {
    $commands += "tunnel-client --help"
    $commands += "tunnel-client init --help"
    $commands += "tunnel-client doctor --help"
    $commands += "tunnel-client run --help"
    $commands += '$env:CONTROL_PLANE_API_KEY = "<runtime-api-key>"'
    if ($supportsMcpServerUrl) {
        $commands += "tunnel-client init --profile `"<profile-name>`" --tunnel-id `"<tunnel-id>`" --mcp-server-url `"$LocalMcpUrlPlaceholder`""
    } else {
        $commands += "Install/inspect tunnel-client help, then initialize profile with tunnel ID and HTTP MCP server URL using the syntax supported by that version."
    }
    $commands += 'tunnel-client doctor --profile "<profile-name>" --explain'
    if ($Managed) {
        $commands += "target\release\catdesk.exe"
    } else {
        $commands += 'tunnel-client run --profile "<profile-name>"'
        $commands += "target\release\catdesk.exe"
    }
}

$report = [pscustomobject][ordered]@{
    Wizard = "catdesk-secure-mcp-setup"
    TransportMode = $TransportMode
    ProcessMode = $processMode
    Client = [pscustomobject][ordered]@{
        Found = [bool]$clientPath
        Path = if ($clientPath) { "<redacted-user-path>" } else { $null }
        VersionOrHelp = Client-Help-Line -Path $clientPath -Args @("--version")
        QuickstartHelp = $quickstartHelp
        InitHelp = $initHelp
        DoctorHelp = Client-Help-Line -Path $clientPath -Args @("doctor", "--help")
        RunHelp = $runHelp
        SupportsMcpServerUrl = $supportsMcpServerUrl
    }
    Profile = [pscustomobject][ordered]@{
        Supplied = -not [string]::IsNullOrWhiteSpace($ProfileName)
        Value = Redacted-String $ProfileName
    }
    RuntimeCredential = [pscustomobject][ordered]@{
        Name = "CONTROL_PLANE_API_KEY"
        Present = [bool][Environment]::GetEnvironmentVariable("CONTROL_PLANE_API_KEY")
        Value = "<redacted>"
    }
    OperatorSteps = @(
        "Create or select an OpenAI Secure MCP tunnel in Platform.",
        "Ensure the runtime key has Tunnels Read and Use permission.",
        "Associate the tunnel with the intended ChatGPT workspace or organization.",
        "Initialize an official tunnel-client profile manually.",
        "Configure CatDesk user config for the selected transport.",
        "Start the tunnel and CatDesk, then call read-only MCP tools first."
    )
    CommandTemplates = $commands
    MutationsPerformed = @()
    Notes = @(
        "This wizard is read-only.",
        "It does not install software, create profiles, start tunnel-client, automate ChatGPT, or store credentials.",
        "Placeholders must be replaced by the operator outside review artifacts."
    )
}

$report | ConvertTo-Json -Depth 8
