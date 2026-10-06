param(
    [ValidateSet("managed_ephemeral_ngrok", "external_tunnel", "openai_secure_tunnel")]
    [string]$TransportMode = "openai_secure_tunnel",
    [ValidateSet("plan", "install", "configure", "connect", "start_all", "status", "repair", "migrate", "stop", "rollback")]
    [string]$Mode = "plan",
    [switch]$Plan,
    [switch]$InstallClient,
    [switch]$ConfigureCatDesk,
    [switch]$ConnectRuntime,
    [switch]$StartAll,
    [switch]$Status,
    [switch]$Repair,
    [switch]$MigrateOfficialRuntime,
    [switch]$PreserveExistingMigrationBackup,
    [switch]$StopRuntime,
    [switch]$RollbackClient,
    [string]$TunnelClientPath = "",
    [string]$ProfileName = "catdesk-local",
    [string]$RuntimeAlias = "catdesk-local",
    [string]$TunnelId = "",
    [string]$LocalMcpUrlPlaceholder = "http://127.0.0.1:<port>/<persistent-route>/mcp",
    [string]$ConfigPath = "",
    [ValidateRange(1, 30)][int]$RuntimeCommandTimeoutSeconds = 15,
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
if ($MigrateOfficialRuntime) { $Mode = "migrate" }
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

function ConvertTo-NativeCommandLineArgument {
    param([string]$Value)
    if ($null -eq $Value -or $Value.Length -eq 0) { return '""' }
    if ($Value -notmatch '[\s"]') { return $Value }
    $escaped = [regex]::Replace($Value, '(\\*)"', '$1$1\"')
    $escaped = [regex]::Replace($escaped, '(\\+)$', '$1$1')
    return '"' + $escaped + '"'
}

function Initialize-BoundedNativeProcessType {
    if ("CatDesk.BoundedNativeProcess" -as [type]) { return }
    Add-Type -TypeDefinition @'
using System;
using System.Diagnostics;
using System.IO;
using System.Text;
using System.Threading.Tasks;
namespace CatDesk {
    public sealed class BoundedNativeResult { public string Stdout; public string Stderr; public bool Overflow; public bool TimedOut; public bool TerminationFailed; public bool OutputDrainTimedOut; public int ExitCode; }
    internal sealed class BoundedCapture { internal string Text; internal bool Overflow; }
    public static class BoundedNativeProcess {
        private static BoundedCapture Read(StreamReader reader, int maximum) {
            var text = new StringBuilder(); var buffer = new char[4096]; var total = 0; var overflow = false; int read;
            while ((read = reader.Read(buffer, 0, buffer.Length)) > 0) {
                total += Encoding.UTF8.GetByteCount(buffer, 0, read);
                if (total > maximum) { overflow = true; continue; }
                text.Append(buffer, 0, read);
            }
            return new BoundedCapture { Text = text.ToString(), Overflow = overflow };
        }
        public static BoundedNativeResult Run(string fileName, string arguments, int timeoutMilliseconds, int maximumOutputBytes) {
            var start = new ProcessStartInfo(fileName, arguments) { UseShellExecute = false, CreateNoWindow = true, RedirectStandardInput = true, RedirectStandardOutput = true, RedirectStandardError = true };
            using (var process = Process.Start(start)) {
                if (process == null) throw new InvalidOperationException("tunnel-client command did not start");
                process.StandardInput.Close();
                var stdout = Task.Factory.StartNew(() => Read(process.StandardOutput, maximumOutputBytes));
                var stderr = Task.Factory.StartNew(() => Read(process.StandardError, maximumOutputBytes));
                var timedOut = !process.WaitForExit(timeoutMilliseconds);
                if (timedOut) {
                    try { process.Kill(); } catch (InvalidOperationException) {}
                    if (!process.WaitForExit(2000)) {
                        return new BoundedNativeResult { TimedOut = true, TerminationFailed = true, ExitCode = -1, Stdout = "", Stderr = "" };
                    }
                    // A wrapper can exit while a descendant still owns the
                    // redirected pipe handles. Never turn the bounded timeout
                    // into an unbounded stdout/stderr drain after termination.
                    return new BoundedNativeResult { TimedOut = true, TerminationFailed = false, ExitCode = process.ExitCode, Stdout = "", Stderr = "" };
                }
                // A descendant can retain inherited redirected handles after an
                // on-time wrapper exit. Keep that capture drain bounded too and
                // fail closed rather than accepting incomplete command output.
                if (!Task.WaitAll(new Task[] { stdout, stderr }, 2000)) {
                    return new BoundedNativeResult { TimedOut = false, TerminationFailed = false, OutputDrainTimedOut = true, ExitCode = process.ExitCode, Stdout = "", Stderr = "" };
                }
                return new BoundedNativeResult { Stdout = stdout.Result.Text, Stderr = stderr.Result.Text, Overflow = stdout.Result.Overflow || stderr.Result.Overflow, TimedOut = false, TerminationFailed = false, OutputDrainTimedOut = false, ExitCode = process.ExitCode };
            }
        }
    }
}
'@
}

function Invoke-BoundedTunnelClient {
    param([string]$Path, [string[]]$CommandArgs, [int]$TimeoutSeconds, [int]$MaxOutputBytes = 65536)
    if (-not $Path -or -not (Test-Path -LiteralPath $Path -PathType Leaf)) { throw "tunnel-client is not installed" }
    if ($TimeoutSeconds -lt 1 -or $TimeoutSeconds -gt 30 -or $MaxOutputBytes -lt 1 -or $MaxOutputBytes -gt 65536) {
        throw "bounded tunnel-client invocation is invalid"
    }
    Initialize-BoundedNativeProcessType
    $commandLine = [string]::Join(" ", @($CommandArgs | ForEach-Object { ConvertTo-NativeCommandLineArgument -Value $_ }))
    $result = [CatDesk.BoundedNativeProcess]::Run($Path, $commandLine, $TimeoutSeconds * 1000, $MaxOutputBytes)
    if ($result.TimedOut) {
        if ($result.TerminationFailed) { throw "tunnel-client command exceeded bounded timeout and did not terminate" }
        throw "tunnel-client command exceeded bounded timeout and was killed"
    }
    if ($result.OutputDrainTimedOut) { throw "tunnel-client command output drain exceeded bounded timeout" }
    if ($result.Overflow) { throw "tunnel-client command output exceeded bounded capture" }
    if ($result.ExitCode -ne 0) { throw "tunnel-client command failed" }
    return [pscustomobject]@{ Stdout = $result.Stdout; Stderr = $result.Stderr }
}

function Get-TunnelClientOutput {
    param([string]$Path, [string[]]$CommandArgs, [int]$MaxLines = 30)
    if (-not $Path) {
        return [pscustomobject]@{ State = "CLIENT_UNAVAILABLE"; Output = $null }
    }
    try {
        $output = ((Invoke-BoundedTunnelClient -Path $Path -CommandArgs $CommandArgs -TimeoutSeconds $RuntimeCommandTimeoutSeconds).Stdout -split "`r?`n" | Select-Object -First $MaxLines) -join "`n"
        if ([string]::IsNullOrWhiteSpace($output)) {
            return [pscustomobject]@{ State = "EMPTY_OUTPUT"; Output = $null }
        }
        return [pscustomobject]@{ State = "AVAILABLE"; Output = $output }
    } catch {
        # Keep native error output out of the operator record. The fixed state
        # makes a connector-session failure observable without disclosing the
        # command arguments, endpoint, credentials, or provider text.
        return [pscustomobject]@{ State = "COMMAND_FAILED"; Output = $null }
    }
}

function Client-Output {
    param([string]$Path, [string[]]$CommandArgs, [int]$MaxLines = 30)
    return (Get-TunnelClientOutput -Path $Path -CommandArgs $CommandArgs -MaxLines $MaxLines).Output
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
    # Route is intentionally retained in memory for the connect command.  Redaction
    # happens only when producing the final operator-facing object below.
    $result = [ordered]@{ BindHost = "127.0.0.1"; Port = 3200; Route = $null; Found = $false }
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { return [pscustomobject]$result }
    $result.Found = $true
    $section = ""
    foreach ($line in (Get-Content -LiteralPath $Path -ErrorAction SilentlyContinue)) {
        $trimmed = $line.Trim()
        if ($trimmed -match '^\[(.+)\]$') { $section = $Matches[1]; continue }
        if ($section -eq "mcp" -and $trimmed -match '^bind_host\s*=\s*"([^"]+)"') { $result.BindHost = $Matches[1] }
        if ($section -eq "mcp" -and $trimmed -match '^port\s*=\s*(\d+)') { $result.Port = [int]$Matches[1] }
        if ($section -eq "mcp" -and $trimmed -match '^route_id\s*=\s*"([^"]+)"') { $result.Route = $Matches[1] }
    }
    [pscustomobject]$result
}

function Test-SafeRuntimeRoute {
    param([string]$Route)
    if ([string]::IsNullOrWhiteSpace($Route)) { return $false }

    # Decode a bounded number of times so presentation markers cannot be smuggled
    # back through URL encoding.  A real route is restricted to the conservative
    # CatDesk route alphabet and therefore never needs percent encoding.
    $decoded = $Route.Trim()
    for ($i = 0; $i -lt 4; $i++) {
        try { $next = [Uri]::UnescapeDataString($decoded) } catch { return $false }
        if ($next -eq $decoded) { break }
        $decoded = $next
    }
    $lower = $decoded.ToLowerInvariant()
    if ($lower -match '<redacted>|<persistent-route>|placeholder|redacted') { return $false }
    return $decoded -match '^[A-Za-z0-9_-]{24,96}$'
}

function Get-LocalMcpRuntimeUrl {
    param($McpConfig)
    if (-not $McpConfig.Found -or -not (Test-SafeRuntimeRoute -Route $McpConfig.Route)) {
        throw "configured MCP route is missing or invalid; refusing runtime connect"
    }
    if ($McpConfig.BindHost -notin @("127.0.0.1", "localhost", "::1")) {
        throw "configured MCP bind host is not loopback; refusing runtime connect"
    }
    if ($McpConfig.Port -lt 1 -or $McpConfig.Port -gt 65535) {
        throw "configured MCP port is invalid; refusing runtime connect"
    }
    $authorityHost = if ($McpConfig.BindHost -eq "::1") { "[$($McpConfig.BindHost)]" } else { $McpConfig.BindHost }
    return "http://$authorityHost`:$($McpConfig.Port)/$($McpConfig.Route)/mcp"
}

function Migrate-LegacyOfficialRuntimeConfig {
    param([string]$Path, [switch]$PreserveExistingBackup)
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw "existing CatDesk config is required for explicit migration"
    }
    $text = [IO.File]::ReadAllText($Path)
    if ($text.Length -gt 1048576) { throw "CatDesk config is too large for safe migration" }
    $lines = [regex]::Split($text, "`r?`n")
    $sections = @()
    for ($index = 0; $index -lt $lines.Count; $index++) {
        $trimmed = $lines[$index].Trim()
        if ($trimmed.StartsWith("[") -and $trimmed -notmatch '^\[[A-Za-z0-9_.-]+\]\s*(#.*)?$') {
            throw "CatDesk config is malformed; refusing migration"
        }
        if ($trimmed -match '^\[([A-Za-z0-9_.-]+)\]\s*(?:#.*)?$') {
            $sections += [pscustomobject]@{ Name = $Matches[1]; Start = $index }
        }
    }
    $tunnelSections = @($sections | Where-Object Name -eq "tunnel")
    $openaiSections = @($sections | Where-Object Name -eq "openai_tunnel")
    if ($tunnelSections.Count -ne 1 -or $openaiSections.Count -ne 1) {
        throw "CatDesk config has missing or ambiguous tunnel sections; refusing migration"
    }
    $tunnel = $tunnelSections[0]
    $tunnelEnd = @($sections | Where-Object Start -gt $tunnel.Start | Select-Object -First 1).Start
    if ($null -eq $tunnelEnd) { $tunnelEnd = $lines.Count }
    $modeMatches = @()
    for ($index = $tunnel.Start + 1; $index -lt $tunnelEnd; $index++) {
        if ($lines[$index].Trim() -match '^mode\s*=\s*"([^"]+)"\s*(?:#.*)?$') { $modeMatches += $Matches[1] }
    }
    if ($modeMatches.Count -ne 1 -or $modeMatches[0] -ne "openai_secure_tunnel") {
        throw "CatDesk config is not an unambiguous openai_secure_tunnel configuration"
    }
    $openai = $openaiSections[0]
    $openaiEnd = @($sections | Where-Object Start -gt $openai.Start | Select-Object -First 1).Start
    if ($null -eq $openaiEnd) { $openaiEnd = $lines.Count }
    $processModeLines = @()
    for ($index = $openai.Start + 1; $index -lt $openaiEnd; $index++) {
        if ($lines[$index].Trim() -match '^process_mode\s*=\s*"([^"]+)"\s*(?:#.*)?$') {
            $processModeLines += [pscustomobject]@{ Index = $index; Value = $Matches[1] }
        }
    }
    if ($processModeLines.Count -ne 1) {
        throw "CatDesk config has missing or ambiguous process_mode; refusing migration"
    }
    $processMode = $processModeLines[0]
    if ($processMode.Value -eq "official_runtime") { return "already official_runtime; no config change" }
    if ($processMode.Value -ne "external_foreground") {
        throw "process_mode is not the supported legacy external_foreground value; refusing migration"
    }
    # Preserve every unrelated line, including comments. This only replaces the
    # exact legacy value in the one validated openai_tunnel section.
    $lines[$processMode.Index] = [regex]::Replace(
        $lines[$processMode.Index],
        '^(\s*process_mode\s*=\s*")external_foreground(")',
        '$1official_runtime$2'
    )
    $newline = if ($text.Contains("`r`n")) { "`r`n" } else { "`n" }
    $updated = [string]::Join($newline, $lines)
    $backup = "$Path.pre-t0051-official-runtime.bak"
    $existingBackup = Test-Path -LiteralPath $backup -PathType Leaf
    if ($existingBackup -and -not $PreserveExistingBackup) {
        throw "recoverable migration backup already exists; refusing to overwrite it"
    }
    $temporary = "$Path.t0051-migration.tmp"
    $replacementBackup = if ($existingBackup) { "$Path.t0051-stale-writer.tmp.bak" } else { $backup }
    if (Test-Path -LiteralPath $replacementBackup -PathType Leaf) {
        throw "migration recovery state is ambiguous; refusing migration"
    }
    try {
        [IO.File]::WriteAllText($temporary, $updated, [Text.UTF8Encoding]::new($false))
        [IO.File]::Replace($temporary, $Path, $replacementBackup)
    } finally {
        if (Test-Path -LiteralPath $temporary -PathType Leaf) {
            Remove-Item -LiteralPath $temporary -Force -ErrorAction SilentlyContinue
        }
        if ($existingBackup -and (Test-Path -LiteralPath $replacementBackup -PathType Leaf)) {
            Remove-Item -LiteralPath $replacementBackup -Force -ErrorAction SilentlyContinue
        }
    }
    if ($existingBackup) {
        return "migrated legacy process mode to official_runtime; existing recoverable backup preserved"
    }
    return "migrated legacy process mode to official_runtime; recoverable backup created"
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
    param([string]$ClientPath, $McpConfig)
    # Validate the target before checking any runtime inputs or invoking the
    # tunnel client.  This keeps redacted presentation values fail-closed.
    $localMcpUrl = Get-LocalMcpRuntimeUrl -McpConfig $McpConfig
    if (-not $TunnelId) { throw "TunnelId is required for runtime connect" }
    if (-not [Environment]::GetEnvironmentVariable("CONTROL_PLANE_API_KEY")) {
        throw "CONTROL_PLANE_API_KEY must be present in the process environment for runtime connect"
    }
    Invoke-BoundedTunnelClient -Path $ClientPath -CommandArgs @("runtimes", "connect", "--alias", $RuntimeAlias, "--tunnel-id", $TunnelId, "--runtime-api-key", "env:CONTROL_PLANE_API_KEY", "--mcp-server-url", $localMcpUrl) -TimeoutSeconds $RuntimeCommandTimeoutSeconds | Out-Null
    return "connect invoked"
}

$clientPath = if ($Mode -eq "migrate") { $null } else { Find-TunnelClient -ExplicitPath $TunnelClientPath }
$mcpConfig = if ($Mode -eq "migrate") { $null } else { Read-CatDeskMcpConfig -Path $ConfigPath }
$capabilities = if ($Mode -eq "migrate") { $null } else { Supports-RuntimeSurface -Path $clientPath }
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
            $mutations.Add((Connect-Runtime -ClientPath $clientPath -McpConfig $mcpConfig))
        }
        "start_all" {
            if (-not $clientPath) {
                $installResult = Invoke-InstallScript -InstallMode "Install"
                $mutations.Add("installed tunnel-client")
                $clientPath = Find-TunnelClient -ExplicitPath $TunnelClientPath
            }
            $mutations.Add((Configure-CatDesk))
            $mutations.Add((Connect-Runtime -ClientPath $clientPath -McpConfig $mcpConfig))
        }
        "repair" {
            if (-not $clientPath) {
                $installResult = Invoke-InstallScript -InstallMode "Install"
                $mutations.Add("installed tunnel-client")
            } else {
                $mutations.Add("client present; inspect status before reconnect")
            }
        }
        "migrate" {
            $mutations.Add((Migrate-LegacyOfficialRuntimeConfig -Path $ConfigPath -PreserveExistingBackup:$PreserveExistingMigrationBackup))
        }
        "stop" {
            if (-not $clientPath) { throw "tunnel-client is not installed" }
            Invoke-BoundedTunnelClient -Path $clientPath -CommandArgs @("runtimes", "stop", $RuntimeAlias) -TimeoutSeconds $RuntimeCommandTimeoutSeconds | Out-Null
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

$clientPath = if ($Mode -eq "migrate") { $null } else { Find-TunnelClient -ExplicitPath $TunnelClientPath }
$capabilities = if ($Mode -eq "migrate") { $null } else { Supports-RuntimeSurface -Path $clientPath }
$statusOutcome = [pscustomobject]@{ State = "CLIENT_UNAVAILABLE"; Output = $null }
if ($clientPath -and $Mode -in @("status", "repair", "start_all", "connect")) {
    $statusOutcome = Get-TunnelClientOutput -Path $clientPath -CommandArgs @("runtimes", "status", $RuntimeAlias, "--json") -MaxLines 40
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
    LocalMcp = @{ ConfigFound = if ($null -eq $mcpConfig) { $null } else { $mcpConfig.Found }; Endpoint = "<redacted-local-mcp-url>" }
    RuntimeStatus = switch ($statusOutcome.State) {
        "AVAILABLE" { "<redacted-status-output-present>" }
        "COMMAND_FAILED" { "<redacted-status-command-failed>" }
        default { $null }
    }
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
        ".\scripts\setup-secure-mcp.ps1 -MigrateOfficialRuntime",
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
