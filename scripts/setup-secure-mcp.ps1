param(
    [ValidateSet("managed_ephemeral_ngrok", "external_tunnel", "openai_secure_tunnel")]
    [string]$TransportMode = "openai_secure_tunnel",
    [ValidateSet("plan", "install", "configure", "connect", "start_all", "status", "repair", "migrate", "stop", "rollback", "reconnect")]
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
    [switch]$ForwardLocalMcpBearer,
    [string]$TunnelClientPath = "",
    [string]$ProfileName = "catdesk-local",
    [string]$RuntimeAlias = "catdesk-local",
    [string]$TunnelId = "",
    [string]$LocalMcpUrlPlaceholder = "http://127.0.0.1:<port>/<persistent-route>/mcp",
    [string]$ConfigPath = "",
    [ValidateRange(1, 30)][int]$RuntimeCommandTimeoutSeconds = 15,
    [string]$ReconnectWorkspace,
    [string]$ExpectedBackendSha256,
    [string]$ExpectedTunnelClientSha256,
    [int]$ExpectedRuntimeOwnerPid,
    [string]$ExpectedRuntimeOwnerCreatedUtc,
    [switch]$ExecuteReconnect,
    [switch]$LegacyDirectManaged
)

$ErrorActionPreference = "Stop"

if ($Mode -eq 'reconnect' -and ($Plan -or $InstallClient -or $ConfigureCatDesk -or $ConnectRuntime -or
    $StartAll -or $Status -or $Repair -or $MigrateOfficialRuntime -or $StopRuntime -or $RollbackClient)) {
    '{"mode":"reconnect","state":"SECURE_MCP_RECONNECT_ARGUMENTS_CONFLICT","webDiscoveryVerified":false}'
    return
}
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

$lifecycleEngine = Join-Path $PSScriptRoot 'start-catdesk-stack.ps1'
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
    # Lifecycle also loads a bounded runner, with a different overload set.
    # Scope this existing runner to Secure MCP so load order cannot discard
    # child-only authentication forwarding or change lifecycle behavior.
    if ("CatDesk.SecureMcp.BoundedNativeProcess" -as [type]) { return }
    Add-Type -TypeDefinition @'
using System;
using System.Diagnostics;
using System.IO;
using System.Text;
using System.Threading.Tasks;
namespace CatDesk.SecureMcp {
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
            return Run(fileName, arguments, timeoutMilliseconds, maximumOutputBytes, null);
        }
        public static BoundedNativeResult Run(string fileName, string arguments, int timeoutMilliseconds, int maximumOutputBytes, string mcpAuthorization) {
            var start = new ProcessStartInfo(fileName, arguments) { UseShellExecute = false, CreateNoWindow = true, RedirectStandardInput = true, RedirectStandardOutput = true, RedirectStandardError = true };
            if (mcpAuthorization != null) {
                start.EnvironmentVariables["CATDESK_TUNNEL_MCP_AUTHORIZATION"] = mcpAuthorization;
                start.EnvironmentVariables["MCP_EXTRA_HEADERS"] = "Authorization: env:CATDESK_TUNNEL_MCP_AUTHORIZATION";
                start.EnvironmentVariables["MCP_DISCOVERY_EXTRA_HEADERS"] = "Authorization: env:CATDESK_TUNNEL_MCP_AUTHORIZATION";
            }
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
    param([string]$Path, [string[]]$CommandArgs, [int]$TimeoutSeconds, [int]$MaxOutputBytes = 65536, [string]$McpAuthorization = '')
    if (-not $Path -or -not (Test-Path -LiteralPath $Path -PathType Leaf)) { throw "tunnel-client is not installed" }
    if ($TimeoutSeconds -lt 1 -or $TimeoutSeconds -gt 30 -or $MaxOutputBytes -lt 1 -or $MaxOutputBytes -gt 65536) {
        throw "bounded tunnel-client invocation is invalid"
    }
    Initialize-BoundedNativeProcessType
    $commandLine = [string]::Join(" ", @($CommandArgs | ForEach-Object { ConvertTo-NativeCommandLineArgument -Value $_ }))
    $result = if ([string]::IsNullOrEmpty($McpAuthorization)) {
        [CatDesk.SecureMcp.BoundedNativeProcess]::Run($Path, $commandLine, $TimeoutSeconds * 1000, $MaxOutputBytes)
    } else {
        [CatDesk.SecureMcp.BoundedNativeProcess]::Run($Path, $commandLine, $TimeoutSeconds * 1000, $MaxOutputBytes, $McpAuthorization)
    }
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

function Get-LocalMcpRuntimeAuthorization {
    param([bool]$ForwardBearer)
    if (-not $ForwardBearer) { return '' }
    $token = [Environment]::GetEnvironmentVariable('CATDESK_MCP_AUTH_TOKEN', 'Process')
    if ($null -eq $token -or $token -cnotmatch '\A[\x21-\x7E]{24,}\z') {
        throw 'LOCAL_MCP_AUTH_TOKEN_INVALID'
    }
    $reference = 'Authorization: env:CATDESK_TUNNEL_MCP_AUTHORIZATION'
    foreach ($name in @('MCP_EXTRA_HEADERS', 'MCP_DISCOVERY_EXTRA_HEADERS')) {
        $existing = [Environment]::GetEnvironmentVariable($name, 'Process')
        if (-not [string]::IsNullOrEmpty($existing) -and $existing -cne $reference) {
            throw 'LOCAL_MCP_AUTH_HEADER_CONFLICT'
        }
    }
    # Only the owned native command receives this value. CatDesk does not write
    # it to a profile, command argument, parent environment or presentation.
    # The external managed-runtime handoff still needs its own qualification.
    return 'Bearer ' + $token
}

function Open-LocalMcpBearerFile {
    param([string]$Path, [string]$RequiredPath)
    if ([IO.Path]::GetFullPath($Path) -ine [IO.Path]::GetFullPath($RequiredPath)) { throw 'LOCAL_MCP_AUTH_SCOPE_INVALID' }
    $item = Get-Item -LiteralPath $RequiredPath -Force -ErrorAction Stop
    if ($item -isnot [IO.FileInfo] -or
        -not $item.FullName.Equals([IO.Path]::GetFullPath($RequiredPath), [StringComparison]::OrdinalIgnoreCase)) {
        throw 'LOCAL_MCP_AUTH_SCOPE_INVALID'
    }
    if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'LOCAL_MCP_AUTH_SCOPE_INVALID' }
    for ($directory = $item.Directory; $null -ne $directory; $directory = $directory.Parent) {
        if ($directory.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'LOCAL_MCP_AUTH_SCOPE_INVALID' }
    }
    return [IO.File]::Open($item.FullName, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
}

function Open-LocalMcpBearerClient {
    param([string]$Path)
    $pin = $null
    try {
        $fixedClient = Join-Path $UserProfile '.catdesk\tools\tunnel-client\current\tunnel-client.exe'
        $pin = Open-LocalMcpBearerFile $Path $fixedClient
        # The reviewed Windows v0.0.14 client used for bearer qualification.
        $digest = (Get-FileHash -Algorithm SHA256 -InputStream $pin).Hash
        if ($digest -ine 'fcc85a69ec0ad82518e4f8964f60c45e31787957782a0fc9c1b0c44e82d61b9b') {
            throw 'LOCAL_MCP_AUTH_SCOPE_INVALID'
        }
        return $pin
    } catch {
        if ($null -ne $pin) { $pin.Dispose() }
        throw 'LOCAL_MCP_AUTH_SCOPE_INVALID'
    }
}

function Open-LocalMcpBearerConfig {
    param([string]$Path, [switch]$RequireCanonicalListener)
    $pin = $null
    try {
        $fixedConfig = Join-Path $UserProfile '.catdesk\config.toml'
        $pin = Open-LocalMcpBearerFile $Path $fixedConfig
        $endpoint = & {
            . $lifecycleEngine -ConfigPath $fixedConfig
            $local = Get-ConfiguredLocalMcp -Path $fixedConfig -PinnedFile $pin
            if ($RequireCanonicalListener) {
                $canonical = Get-CanonicalCatDeskIdentity -Root $Workspace -FingerprintPath $BuildFingerprintPath
                $listener = Get-LoopbackCatDeskListener -Port $local.Port -Canonical $canonical
                if ($null -eq $listener -or -not $listener.MatchesCanonical -or
                    $listener.OwnerMode -cnotin @('BACKEND_ONLY','FULL_STACK') -or
                    $listener.ListenAddresses -cnotcontains '127.0.0.1') {
                    throw 'LOCAL_MCP_AUTH_SCOPE_INVALID'
                }
            }
            $local
        }
        return [pscustomobject]@{ File = $pin; Endpoint = $endpoint }
    } catch {
        if ($null -ne $pin) { $pin.Dispose() }
        throw 'LOCAL_MCP_AUTH_SCOPE_INVALID'
    }
}

function Connect-Runtime {
    param([string]$ClientPath, $McpConfig, [string]$ExistingProfileDirectory = '')
    $pinned = $null
    $clientPin = $null
    try {
        if ($ForwardLocalMcpBearer) {
            $clientPin = Open-LocalMcpBearerClient -Path $ClientPath
            $pinned = Open-LocalMcpBearerConfig -Path $ConfigPath -RequireCanonicalListener
            $localMcpUrl = $pinned.Endpoint.Uri
        } else {
            $localMcpUrl = Get-LocalMcpRuntimeUrl -McpConfig $McpConfig
        }
        $mcpAuthorization = Get-LocalMcpRuntimeAuthorization -ForwardBearer ([bool]$ForwardLocalMcpBearer)
        if (-not $TunnelId) { throw "TunnelId is required for runtime connect" }
        if (-not [Environment]::GetEnvironmentVariable("CONTROL_PLANE_API_KEY")) {
            throw "CONTROL_PLANE_API_KEY must be present in the process environment for runtime connect"
        }
        $arguments = @("runtimes", "connect", "--alias", $RuntimeAlias, "--tunnel-id", $TunnelId, "--runtime-api-key", "env:CONTROL_PLANE_API_KEY", "--mcp-server-url", $localMcpUrl)
        if ($ExistingProfileDirectory) { $arguments += @('--profile', $ProfileName, '--profile-dir', $ExistingProfileDirectory) }
        Invoke-BoundedTunnelClient -Path $ClientPath -CommandArgs $arguments -TimeoutSeconds $RuntimeCommandTimeoutSeconds -McpAuthorization $mcpAuthorization | Out-Null
        return "connect invoked"
    } finally {
        if ($null -ne $pinned) { $pinned.File.Dispose() }
        if ($null -ne $clientPin) { $clientPin.Dispose() }
    }
}

function Read-ReconnectRuntimeStatus {
    param([string]$ClientPath, [string]$Target)
    $result = Invoke-BoundedTunnelClient -Path $ClientPath -CommandArgs @('runtimes', 'status', $RuntimeAlias, '--json') -TimeoutSeconds $RuntimeCommandTimeoutSeconds
    $status = $result.Stdout | ConvertFrom-Json -ErrorAction Stop
    if ($status -isnot [pscustomobject] -or $status.alias -cne $RuntimeAlias -or
        $status.tunnel_id -cne $TunnelId -or $status.profile_name -cne $ProfileName -or
        $status.process_running -isnot [bool] -or -not [IO.Path]::IsPathRooted([string]$status.profile_path)) {
        throw 'SECURE_MCP_RECONNECT_IDENTITY_UNPROVEN'
    }
    if ($status.process_running -and ($status.process.alias -cne $RuntimeAlias -or
        $status.process.tunnel_id -cne $TunnelId -or $status.process.mode -cne 'process' -or
        $status.process.target_kind -cne 'server_url' -or $status.process.target_value -cne $Target -or
        $status.healthy -isnot [bool] -or $status.ready -isnot [bool])) {
        throw 'SECURE_MCP_RECONNECT_IDENTITY_UNPROVEN'
    }
    return $status
}

function Get-ReconnectRuntimeOwner {
    param($Status, [string]$ClientPath)
    $ownerPid = [int]$Status.process.pid
    if (-not $Status.process_running -or $ownerPid -le 0) { throw 'SECURE_MCP_RECONNECT_OWNER_CHANGED' }
    $owner = Get-Process -Id $ownerPid -ErrorAction Stop
    $null = $owner.Handle
    if ($owner.HasExited -or $owner.Path -ine $ClientPath) { throw 'SECURE_MCP_RECONNECT_OWNER_CHANGED' }
    return $owner
}

function Test-ReconnectBackendOwner {
    param($Local, $Canonical, $Expected, [ValidateSet('BACKEND_ONLY','FULL_STACK')][string]$OwnerMode = 'BACKEND_ONLY')
    $Current = Get-LoopbackCatDeskListener -Port $Local.Port -Canonical $Canonical
    if ($null -eq $Current -or $null -eq $Expected -or -not $Current.MatchesCanonical -or $Current.OwnerMode -cne $OwnerMode -or
        $Current.Pid -ne $Expected.Pid -or $Current.CreationTimeUtc -cne $Expected.CreationTimeUtc) { return $false }
    $daemons = @(Get-CatDeskDaemonProcessCandidates -Canonical $Canonical)
    return ($daemons.Count -eq 1 -and $daemons[0].MatchesCanonical -and
        $daemons[0].Pid -eq $Current.Pid -and $daemons[0].CreationTimeUtc -ceq $Current.CreationTimeUtc -and
        (Get-LocalMcpReadiness -LocalMcp $Local -Canonical $Canonical -RequireBearer).Ready)
}

function Get-ReconnectMcpProbeState {
    param($Status)
    # The registered alias's direct probe avoids the client's fallback to
    # another runtime. Top-level readiness also accepts auth challenges.
    $probe = $Status
    foreach ($name in @('local', 'health', 'readyz')) {
        if ($null -eq $probe) { break }
        $property = $probe.PSObject.Properties[$name]
        if ($null -eq $property) { $probe = $null; break }
        $probe = $property.Value
    }
    if ($null -eq $probe -or @($probe.PSObject.Properties.Name) -notcontains 'body' -or
        @($probe.PSObject.Properties.Name) -notcontains 'ok' -or @($probe.PSObject.Properties.Name) -notcontains 'status' -or
        $probe.ok -isnot [bool] -or -not $probe.ok -or $probe.status -ne 200 -or
        $probe.body -isnot [string] -or $probe.body.Length -gt 1024) {
        return 'SECURE_MCP_RECONNECT_MCP_PROBE_UNVERIFIED_NO_RETRY'
    }
    $body = $probe.body.Trim()
    if ($body -ceq 'ready') { return 'SECURE_MCP_RECONNECT_RUNTIME_READY' }
    if ($body.StartsWith('ready (mcp initialize requires auth:', [StringComparison]::Ordinal)) {
        return 'SECURE_MCP_RECONNECT_MCP_AUTH_REQUIRED_NO_RETRY'
    }
    if ($body.StartsWith('ready (mcp startup probe timed out:', [StringComparison]::Ordinal)) {
        return 'SECURE_MCP_RECONNECT_MCP_PROBE_TIMED_OUT_NO_RETRY'
    }
    return 'SECURE_MCP_RECONNECT_MCP_PROBE_UNVERIFIED_NO_RETRY'
}

function Get-ReconnectDisposition {
    param($Status, $Original, [string]$Attempt, [string]$Result, [bool]$Execute)
    if ((Test-Path -LiteralPath $Attempt) -or (Test-Path -LiteralPath $Result)) {
        return 'SECURE_MCP_RECONNECT_ATTEMPT_EXISTS_NO_RETRY'
    }
    if ($Original.HasExited -or -not $Status.process_running -or $Status.process.pid -ne $Original.Id) {
        return 'SECURE_MCP_RECONNECT_OWNER_CHANGED'
    }
    $probe = Get-ReconnectMcpProbeState $Status
    if ($Status.healthy -and $Status.ready -and $probe -ceq 'SECURE_MCP_RECONNECT_RUNTIME_READY') {
        return 'SECURE_MCP_RECONNECT_EXISTING_RUNTIME_READY'
    }
    # The native stop operation re-resolves the alias. Even a pinned original
    # process and a CatDesk lease cannot constrain which instance it stops.
    if ($Execute) { return 'SECURE_MCP_RECONNECT_CONDITIONAL_STOP_UNAVAILABLE' }
    if ($probe -ceq 'SECURE_MCP_RECONNECT_RUNTIME_READY') { return 'SECURE_MCP_RECONNECT_MCP_PROBE_UNVERIFIED_NO_RETRY' }
    return $probe
}

function Invoke-ExistingRuntimeReconnect {
    $original = $null
    $clientPin = $null
    $configPin = $null
    try {
        if ($LegacyDirectManaged -or $TransportMode -cne 'openai_secure_tunnel' -or -not $ForwardLocalMcpBearer -or
            $RuntimeAlias -cnotmatch '\A[A-Za-z0-9_.-]{1,64}\z' -or $ProfileName -cne $RuntimeAlias -or
            $TunnelId -cnotmatch '\Atunnel_[a-z0-9]{32}\z' -or
            $ExpectedBackendSha256 -cnotmatch '\A[a-f0-9]{64}\z' -or $ExpectedTunnelClientSha256 -cnotmatch '\A[a-f0-9]{64}\z' -or
            $ExpectedRuntimeOwnerPid -le 0 -or $ExpectedRuntimeOwnerCreatedUtc -cnotmatch '\A[0-9TZ+:.\-]{20,33}\z' -or
            [string]::IsNullOrWhiteSpace($ReconnectWorkspace) -or
            -not [IO.Path]::IsPathRooted($ReconnectWorkspace)) { return 'SECURE_MCP_RECONNECT_EXPECTED_IDENTITY_REQUIRED' }
        $fixedConfig = Join-Path $UserProfile '.catdesk\config.toml'
        if ([IO.Path]::GetFullPath($ConfigPath) -ine [IO.Path]::GetFullPath($fixedConfig)) { return 'SECURE_MCP_RECONNECT_CONFIG_UNAVAILABLE' }
        $fixedClient = Join-Path $UserProfile '.catdesk\tools\tunnel-client\current\tunnel-client.exe'
        if ([string]::IsNullOrWhiteSpace($TunnelClientPath) -or
            [IO.Path]::GetFullPath($TunnelClientPath) -ine [IO.Path]::GetFullPath($fixedClient)) {
            return 'SECURE_MCP_RECONNECT_CLIENT_CHANGED'
        }
        $root = (Resolve-Path -LiteralPath $ReconnectWorkspace).Path
        # Import only the lifecycle proof functions. The bootstrap and this
        # helper intentionally have different bounded tunnel-client runners.
        $engine = $lifecycleEngine
        $lifecycle = New-Module -Name 'CatDeskReconnectLifecycle' -ArgumentList $engine,$root,$fixedConfig -ScriptBlock {
            param($EnginePath,$ReleaseRoot,$ConfigFile)
            . $EnginePath -Workspace $ReleaseRoot -ConfigPath $ConfigFile
            Export-ModuleMember -Function @(
                'Resolve-TrustedBootstrapHelperPath', 'Get-CatDeskReleaseStartupGate', 'Assert-CatDeskRecoveryContainedPath',
                'Get-ConfiguredRuntimeAlias', 'Read-OfficialRuntimeProcessMode',
                'Get-CanonicalCatDeskIdentity', 'Get-ConfiguredLocalMcp',
                'Get-LoopbackCatDeskListener', 'Get-CatDeskDaemonProcessCandidates',
                'Get-LocalMcpReadiness'
            )
        }
        Import-Module $lifecycle -Scope Local
        $evidenceDirectory = Assert-CatDeskRecoveryContainedPath -Root $root -Path (Join-Path $root '.catdesk\recovery-evidence') -Label 'reconnect evidence'
        $identity = "$RuntimeAlias\0$TunnelId\0$ExpectedRuntimeOwnerPid\0$ExpectedRuntimeOwnerCreatedUtc"
        $sha = [Security.Cryptography.SHA256]::Create()
        try { $digest = [BitConverter]::ToString($sha.ComputeHash([Text.Encoding]::UTF8.GetBytes($identity))).Replace('-','').ToLowerInvariant() }
        finally { $sha.Dispose() }
        $attempt = Assert-CatDeskRecoveryContainedPath -Root $root -Path (Join-Path $evidenceDirectory "secure-mcp-reconnect-$digest.json") -Label 'reconnect attempt' -AllowMissingLeaf
        $resultPath = Assert-CatDeskRecoveryContainedPath -Root $root -Path (Join-Path $evidenceDirectory "secure-mcp-reconnect-$digest.result.json") -Label 'reconnect result' -AllowMissingLeaf
        # An ambiguous stop can end the very process named by its immutable journal.
        # Consult that record even if its owner or executable is no longer available.
        if ((Test-Path -LiteralPath $attempt) -or (Test-Path -LiteralPath $resultPath)) {
            return 'SECURE_MCP_RECONNECT_ATTEMPT_EXISTS_NO_RETRY'
        }
        $pinnedClient = Resolve-TrustedBootstrapHelperPath -Path $fixedClient -FailureMessage 'client identity unproven'
        $clientPin = [IO.File]::Open($pinnedClient, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
        if ((Get-FileHash -LiteralPath $pinnedClient).Hash.ToLowerInvariant() -cne $ExpectedTunnelClientSha256) { return 'SECURE_MCP_RECONNECT_CLIENT_CHANGED' }
        if ((Get-CatDeskReleaseStartupGate -Root $root) -cne 'READY') { return 'SECURE_MCP_RECONNECT_RELEASE_UNAVAILABLE' }
        $configPin = Open-LocalMcpBearerConfig -Path $fixedConfig
        if ((Get-ConfiguredRuntimeAlias -Path $fixedConfig) -cne $RuntimeAlias -or
            (Read-OfficialRuntimeProcessMode -Path $fixedConfig) -cne 'official_runtime') { return 'SECURE_MCP_RECONNECT_CONFIG_UNAVAILABLE' }
        $canonical = Get-CanonicalCatDeskIdentity -Root $root -ExpectedHash $ExpectedBackendSha256
        $local = $configPin.Endpoint
        $backend = Get-LoopbackCatDeskListener -Port $local.Port -Canonical $canonical
        if ($null -eq $backend -or $backend.OwnerMode -cnotin @('FULL_STACK','BACKEND_ONLY') -or
            -not (Test-ReconnectBackendOwner $local $canonical $backend -OwnerMode $backend.OwnerMode)) {
            return 'SECURE_MCP_RECONNECT_BACKEND_OWNER_UNPROVEN'
        }
        $target = $local.Uri
        $status = Read-ReconnectRuntimeStatus -ClientPath $pinnedClient -Target $target
        $original = Get-ReconnectRuntimeOwner -Status $status -ClientPath $pinnedClient
        if ($original.Id -ne $ExpectedRuntimeOwnerPid -or
            $original.StartTime.ToUniversalTime().ToString('o') -cne $ExpectedRuntimeOwnerCreatedUtc) { return 'SECURE_MCP_RECONNECT_OWNER_CHANGED' }
        $profile = Resolve-TrustedBootstrapHelperPath -Path $status.profile_path -FailureMessage 'runtime profile identity unproven'
        $profileDirectory = (Split-Path -Parent $profile)
        if ($profileDirectory -ine $status.profile_dir) { return 'SECURE_MCP_RECONNECT_IDENTITY_UNPROVEN' }
        return Get-ReconnectDisposition $status $original $attempt $resultPath ([bool]$ExecuteReconnect)
    } catch {
        return 'SECURE_MCP_RECONNECT_PREFLIGHT_REFUSED'
    } finally {
        if ($null -ne $configPin) { $configPin.File.Dispose() }
        if ($null -ne $clientPin) { $clientPin.Dispose() }
        if ($null -ne $original) { $original.Dispose() }
    }
}

if ($Mode -eq 'reconnect') {
    [ordered]@{ mode = 'reconnect'; state = (Invoke-ExistingRuntimeReconnect); ownerConditionalStopSupported = $false; webDiscoveryVerified = $false } |
        ConvertTo-Json -Compress
    return
}
if ($ExecuteReconnect -or $ReconnectWorkspace -or $ExpectedBackendSha256 -or $ExpectedTunnelClientSha256 -or
    $ExpectedRuntimeOwnerPid -or $ExpectedRuntimeOwnerCreatedUtc) {
    '{"state":"SECURE_MCP_RECONNECT_ARGUMENTS_CONFLICT","webDiscoveryVerified":false}'
    return
}

$forwardedConnect = $ForwardLocalMcpBearer -and $Mode -in @('connect','start_all')
$operationClientPin = $null
$bearerScopeRefused = $false
try {
if ($forwardedConnect) {
    try {
        $fixedConfig = Join-Path $UserProfile '.catdesk\config.toml'
        if ([IO.Path]::GetFullPath($ConfigPath) -ine [IO.Path]::GetFullPath($fixedConfig)) {
            throw 'LOCAL_MCP_AUTH_SCOPE_INVALID'
        }
        $fixedClient = Join-Path $UserProfile '.catdesk\tools\tunnel-client\current\tunnel-client.exe'
        $requestedClient = if ($TunnelClientPath) { $TunnelClientPath } else { $fixedClient }
        $operationClientPin = Open-LocalMcpBearerClient -Path $requestedClient
        if ($Mode -eq 'start_all') {
            # Validate the installed config and listener before the configure step.
            $verifiedConfig = Open-LocalMcpBearerConfig -Path $ConfigPath -RequireCanonicalListener
            $verifiedConfig.File.Dispose()
        }
    } catch {
        $bearerScopeRefused = $true
    }
}
$clientPath = if ($forwardedConnect) {
    if (-not $bearerScopeRefused) { $operationClientPin.Name }
} elseif ($Mode -ne "migrate") { Find-TunnelClient -ExplicitPath $TunnelClientPath }
$mcpConfig = if ($Mode -eq "migrate" -or ($Mode -in @('connect','start_all') -and $ForwardLocalMcpBearer)) { $null } else { Read-CatDeskMcpConfig -Path $ConfigPath }
$capabilities = if ($Mode -eq "migrate") { $null } else { Supports-RuntimeSurface -Path $clientPath }
$mutations = New-Object System.Collections.Generic.List[string]
$errors = New-Object System.Collections.Generic.List[string]

try {
    if ($bearerScopeRefused) { throw 'LOCAL_MCP_AUTH_SCOPE_INVALID' }
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

$clientPath = if ($forwardedConnect) {
    if (-not $bearerScopeRefused) { $operationClientPin.Name }
} elseif ($Mode -ne "migrate") { Find-TunnelClient -ExplicitPath $TunnelClientPath }
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
    LocalMcpBearerForwarding = @{ Requested = [bool]$ForwardLocalMcpBearer; Value = '<redacted>'; Scope = 'OWNED_CONNECT_CHILD_ONLY' }
    LocalMcp = @{ ConfigFound = if ($forwardedConnect -and -not $bearerScopeRefused) { Test-Path -LiteralPath $ConfigPath -PathType Leaf } elseif ($null -ne $mcpConfig) { $mcpConfig.Found } else { $null }; Endpoint = "<redacted-local-mcp-url>" }
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
} finally {
    if ($null -ne $operationClientPin) { $operationClientPin.Dispose() }
}
