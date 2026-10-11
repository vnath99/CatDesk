[CmdletBinding()]
param(
    [ValidateSet("plan", "recover")][string]$Mode = "plan",
    [string]$Workspace = "",
    [string]$ConfigPath = (Join-Path ([Environment]::GetFolderPath("UserProfile")) ".catdesk\config.toml"),
    [int]$ReadyTimeoutSeconds = 180,
    [string]$ExpectedBuildSha256 = "",
    [string]$BuildFingerprintPath = "",
    [string]$TunnelClientPath = "",
    [switch]$Execute
)

$ErrorActionPreference = "Stop"
if ([string]::IsNullOrWhiteSpace($Workspace)) {
    if ([string]::IsNullOrWhiteSpace($PSScriptRoot)) { throw "script root is unavailable" }
    $Workspace = Join-Path $PSScriptRoot ".."
}
if ([string]::IsNullOrWhiteSpace($BuildFingerprintPath)) {
    $BuildFingerprintPath = Join-Path $Workspace "target\release\catdesk.exe.sha256"
}

function Resolve-TrustedBootstrapHelperPath {
    param([string]$Path, [string]$FailureMessage)
    if ([string]::IsNullOrWhiteSpace($Path)) { throw $FailureMessage }
    try { $item = Get-Item -LiteralPath $Path -Force -ErrorAction Stop } catch { throw $FailureMessage }
    if (-not ($item -is [System.IO.FileInfo]) -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) { throw $FailureMessage }
    $expectedPath = [IO.Path]::GetFullPath($Path)
    $resolvedPath = [IO.Path]::GetFullPath($item.FullName)
    if (-not $resolvedPath.Equals($expectedPath,[StringComparison]::OrdinalIgnoreCase)) { throw $FailureMessage }
    return $resolvedPath
}

$releaseRecoveryHelper = Resolve-TrustedBootstrapHelperPath -Path (Join-Path $PSScriptRoot 'catdesk-release-recovery.ps1') -FailureMessage "release recovery helper identity is invalid"
. $releaseRecoveryHelper
$script:BootstrapSeams = @{}
# Tool metadata and current tunnel-client status can legitimately include a
# large generated catalog. Keep capture bounded, but do not reject the known
# healthy runtime merely because its ordinary list/status response exceeds the
# historical 64 KiB threshold.
$script:MaxMcpReplyBytes = 1048576
$script:MaxTunnelClientOutputBytes = 1048576
$script:MaxTunnelClientCommandMilliseconds = 30000
$script:MaxCodexProbeOutputBytes = 65536
$script:MaxCodexProbeMilliseconds = 5000
# T-0333: every helper reachable from one-command recovery has a parent-side
# deadline. Wake repair gets a deliberately larger budget because its own
# native children are independently bounded and may install the pinned venv.
$script:MaxRecoveryHelperOutputBytes = 65536
$script:MaxWakeRuntimeProbeMilliseconds = 5000
$script:MaxWakeRepairMilliseconds = 1500000
$script:MaxOfficialMigrationMilliseconds = 30000
$script:MaxInterruptedPromotionRecoveryMilliseconds = 30000
# Windows networking/CIM providers are external observation dependencies. Keep
# their recovery-facing inventory probes behind the same finite child boundary
# used for other one-command recovery helpers.
$script:MaxWindowsInventoryProbeMilliseconds = 5000
$script:MaxWindowsInventoryProbeOutputBytes = 65536
$script:WindowsInventoryProbeHelperPath = Join-Path $PSScriptRoot 'query-catdesk-windows-inventory.ps1'

function Invoke-BootstrapSeam {
    param([string]$Name, [object[]]$Arguments, [scriptblock]$Default)
    if ($script:BootstrapSeams.ContainsKey($Name)) { return & $script:BootstrapSeams[$Name] @Arguments }
    return & $Default @Arguments
}

function Write-RedactedStatus {
    param([string]$State, [string]$Detail, [string]$RecoverySource = '', [string]$Gate = '')
    $result = [ordered]@{ State = $State; Detail = $Detail }
    if ($RecoverySource) { $result.RecoverySource = $RecoverySource }
    if ($Gate) { $result.Gate = $Gate }
    [pscustomobject]$result | ConvertTo-Json -Compress
}

function Test-CatDeskInteractiveDesktop {
    # Session zero and non-interactive hosts must remain daemon-only.  This is
    # intentionally a lifecycle decision, not a GUI ownership mechanism.
    $interactive = Invoke-BootstrapSeam -Name 'GuiInteractiveDesktop' -Arguments @() -Default {
        if (-not [Environment]::UserInteractive) { return $false }
        try {
            $process = Get-Process -Id $PID -ErrorAction Stop
            return [int]$process.SessionId -gt 0
        } catch {
            return $false
        }
    }
    return [bool]$interactive
}

function Resolve-VerifiedInstalledWakeHost {
    param(
        [string]$Root,
        [string]$WakeRoot = (Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) 'CatDeskWake')
    )
    $selectorPath = Join-Path $Root '.catdesk\wake-bridge\owner.json'
    # Missing means the documented legacy default; all other lookup
    # failures are ambiguous and must not authorize a Python fallback.
    try {
        $selectorItem = Get-Item -LiteralPath $selectorPath -Force -ErrorAction Stop
    } catch [System.Management.Automation.ItemNotFoundException] {
        return $null
    } catch {
        throw 'wake owner selector is unavailable'
    }
    if (-not ($selectorItem -is [System.IO.FileInfo]) -or ($selectorItem.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0 -or $selectorItem.Length -gt 512) {
        throw 'wake owner selector is invalid'
    }
    try { $selector = [IO.File]::ReadAllText($selectorItem.FullName) | ConvertFrom-Json -ErrorAction Stop } catch { throw 'wake owner selector is invalid' }
    if ($selector.schemaVersion -ne 1) { throw 'wake owner selector is invalid' }
    if ([string]$selector.owner -eq 'legacy_python') { return $null }
    if ([string]$selector.owner -ne 'independent_v1') { throw 'wake owner selector is invalid' }

    $pointerPath = Join-Path $WakeRoot 'current.json'
    if (-not (Test-Path -LiteralPath $pointerPath -PathType Leaf)) { throw 'WakeHost installation pointer is unavailable' }
    $pointerItem = Get-Item -LiteralPath $pointerPath -Force -ErrorAction Stop
    if (-not ($pointerItem -is [System.IO.FileInfo]) -or ($pointerItem.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0 -or $pointerItem.Length -gt 4096) {
        throw 'WakeHost installation pointer is invalid'
    }
    try { $pointer = [IO.File]::ReadAllText($pointerItem.FullName) | ConvertFrom-Json -ErrorAction Stop } catch { throw 'WakeHost installation pointer is invalid' }
    if ($pointer.schemaVersion -ne 1 -or $pointer.directory -notmatch '^[A-Za-z0-9][A-Za-z0-9_.-]{0,199}$' -or $pointer.directory.Contains('..') -or [string]$pointer.hostSha256 -notmatch '^[A-Fa-f0-9]{64}$') {
        throw 'WakeHost installation pointer is invalid'
    }
    $wakeHostPath = Resolve-TrustedBootstrapHelperPath -Path (Join-Path $WakeRoot ("versions\" + $pointer.directory + '\CatDeskWakeHost.exe')) -FailureMessage 'WakeHost installed executable identity is invalid'
    $actualHash = (Get-FileHash -LiteralPath $wakeHostPath -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actualHash -ne ([string]$pointer.hostSha256).ToLowerInvariant()) { throw 'WakeHost installation hash mismatch' }
    return $wakeHostPath
}

function Start-CatDeskWakeHost {
    param(
        [string]$Root,
        [string]$WakeRoot = (Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) 'CatDeskWake')
    )
    $wakeHostPath = Resolve-VerifiedInstalledWakeHost -Root $Root -WakeRoot $WakeRoot
    if (-not $wakeHostPath) { return $false }
    Invoke-BootstrapSeam -Name 'WakeHostStart' -Arguments @($wakeHostPath) -Default {
        param($wakeHost)
        $null = Invoke-BoundedRecoveryHelper -FileName $wakeHost -Arguments @('start') -TimeoutMilliseconds 10000 -FailureMessage 'WakeHost failed to start'
    }
    return $true
}

function Start-CatDeskBinagotchyGui {
    param($Canonical, [string]$Root)
    if (-not (Test-CatDeskInteractiveDesktop)) { return $false }
    # The native mode has a fixed, zero-parameter contract and owns its own
    # local single-instance/foreground behavior.  It only reads durable
    # observability state; this lifecycle engine remains daemon/tunnel owner.
    Invoke-BootstrapSeam -Name 'GuiLaunch' -Arguments @($Canonical, $Root) -Default {
        param($identity, $workspaceRoot)
        $wakeRoot = Join-Path $env:LOCALAPPDATA 'CatDeskWake'
        $pointerPath = Join-Path $wakeRoot 'current.json'
        if (Test-Path -LiteralPath $pointerPath) {
            $pointer = Get-Content -LiteralPath $pointerPath -Raw | ConvertFrom-Json
            if ($pointer.schemaVersion -ne 1 -or $pointer.directory -notmatch '^[A-Za-z0-9][A-Za-z0-9_.-]{0,199}$' -or $pointer.directory.Contains('..')) { throw 'Binagotchy installation pointer invalid' }
            $companion = Join-Path $wakeRoot ("versions\" + $pointer.directory + '\CatDeskBinagotchy.exe')
            if ((Get-FileHash -LiteralPath $companion -Algorithm SHA256).Hash -ne $pointer.binagotchySha256) { throw 'Binagotchy installation hash mismatch' }
            Start-Process -FilePath $companion -ArgumentList '--catdesk-binagotchy-cli' -WorkingDirectory $workspaceRoot -WindowStyle Normal | Out-Null
            return
        }
        Start-Process -FilePath $identity.Path -ArgumentList '--catdesk-binagotchy-cli' -WorkingDirectory $workspaceRoot -WindowStyle Normal | Out-Null
    }
    return $true
}

function Get-RemainingReadinessMilliseconds {
    param([datetime]$Deadline = [datetime]::MaxValue, [int]$RequestedMilliseconds)
    if ($RequestedMilliseconds -lt 1 -or $RequestedMilliseconds -gt $script:MaxTunnelClientCommandMilliseconds) {
        throw "bounded runtime command timeout is invalid"
    }
    if ($Deadline -eq [datetime]::MaxValue) { return $RequestedMilliseconds }
    $remaining = [int][Math]::Floor(($Deadline - [DateTime]::UtcNow).TotalMilliseconds)
    if ($remaining -lt 1) { throw "readiness deadline elapsed before official runtime command" }
    return [Math]::Min($RequestedMilliseconds, $remaining)
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
    public sealed class BoundedNativeResult {
        public string Stdout;
        public string Stderr;
        public bool Overflow;
        public bool TimedOut;
        public bool TerminationFailed;
        public bool OutputDrainTimedOut;
        public int ExitCode;
    }
    internal sealed class BoundedCapture {
        internal string Text;
        internal bool Overflow;
    }
    public static class BoundedNativeProcess {
        private static BoundedCapture Read(StreamReader reader, int maximum) {
            var text = new StringBuilder();
            var buffer = new char[4096];
            var total = 0;
            var overflow = false;
            int read;
            while ((read = reader.Read(buffer, 0, buffer.Length)) > 0) {
                total += Encoding.UTF8.GetByteCount(buffer, 0, read);
                if (total > maximum) { overflow = true; continue; }
                text.Append(buffer, 0, read);
            }
            return new BoundedCapture { Text = text.ToString(), Overflow = overflow };
        }
        public static BoundedNativeResult Run(string fileName, string arguments, int timeoutMilliseconds, int maximumOutputBytes) {
            var start = new ProcessStartInfo(fileName, arguments);
            start.UseShellExecute = false;
            start.CreateNoWindow = true;
            start.RedirectStandardInput = true;
            start.RedirectStandardOutput = true;
            start.RedirectStandardError = true;
            using (var process = Process.Start(start)) {
                if (process == null) throw new InvalidOperationException("official runtime command did not start");
                process.StandardInput.Close();
                var stdout = Task.Factory.StartNew(() => Read(process.StandardOutput, maximumOutputBytes));
                var stderr = Task.Factory.StartNew(() => Read(process.StandardError, maximumOutputBytes));
                var timedOut = !process.WaitForExit(timeoutMilliseconds);
                if (timedOut) {
                    try { process.Kill(); } catch (InvalidOperationException) {}
                    // `Kill()` is best-effort on older Windows/.NET hosts. Never
                    // turn its post-kill wait into an unbounded lifecycle hang:
                    // this is a one-shot status child, not the external runtime.
                    if (!process.WaitForExit(2000)) {
                        return new BoundedNativeResult {
                            TimedOut = true,
                            TerminationFailed = true,
                            ExitCode = -1,
                            Stdout = "",
                            Stderr = "",
                        };
                    }
                }
                if (timedOut) {
                    // A wrapper process can exit while one of its children still
                    // inherits the redirected pipe handles. Never convert the
                    // bounded process timeout into an unbounded capture drain.
                    return new BoundedNativeResult {
                        TimedOut = true,
                        ExitCode = process.ExitCode,
                        Stdout = "",
                        Stderr = "",
                    };
                }
                // Process exit is not sufficient to prove the redirected streams
                // are closed: a descendant can inherit those handles after its
                // wrapper exits. Bound the successful-parent drain as well, and
                // fail closed rather than accept potentially incomplete output.
                if (!Task.WaitAll(new Task[] { stdout, stderr }, 2000)) {
                    return new BoundedNativeResult {
                        TimedOut = false,
                        OutputDrainTimedOut = true,
                        ExitCode = process.ExitCode,
                        Stdout = "",
                        Stderr = "",
                    };
                }
                return new BoundedNativeResult {
                    Stdout = stdout.Result.Text,
                    Stderr = stderr.Result.Text,
                    Overflow = stdout.Result.Overflow || stderr.Result.Overflow,
                    TimedOut = false,
                    OutputDrainTimedOut = false,
                    ExitCode = process.ExitCode,
                };
            }
        }
    }
}
'@
}

function Invoke-BoundedTunnelClient {
    param([string]$ClientPath, [string[]]$Arguments, [int]$TimeoutMilliseconds)
    if (-not $ClientPath -or -not (Test-Path -LiteralPath $ClientPath -PathType Leaf)) {
        throw "official runtime client unavailable"
    }
    if ($TimeoutMilliseconds -lt 1 -or $TimeoutMilliseconds -gt $script:MaxTunnelClientCommandMilliseconds) {
        throw "bounded runtime command timeout is invalid"
    }
    Initialize-BoundedNativeProcessType
    $commandLine = [string]::Join(" ", @($Arguments | ForEach-Object { ConvertTo-NativeCommandLineArgument -Value $_ }))
    $result = [CatDesk.BoundedNativeProcess]::Run($ClientPath, $commandLine, $TimeoutMilliseconds, $script:MaxTunnelClientOutputBytes)
    if ($result.TimedOut) {
        if ($result.TerminationFailed) { throw "official runtime command exceeded bounded timeout and did not terminate" }
        throw "official runtime command exceeded bounded timeout and was killed"
    }
    if ($result.OutputDrainTimedOut) { throw "official runtime command output drain exceeded bounded timeout" }
    if ($result.Overflow) { throw "official runtime command output exceeded bounded capture" }
    if ($result.ExitCode -ne 0) { throw "official runtime command failed" }
    return [pscustomobject]@{ Stdout = $result.Stdout; Stderr = $result.Stderr }
}

function Invoke-SilentLoopbackHttp {
    param([string]$Uri, [string]$Method = "Get", [string]$ContentType = "", [string]$Body = "", [int]$TimeoutSeconds = 5)
    $priorProgressPreference = $ProgressPreference
    try {
        $ProgressPreference = "SilentlyContinue"
        if ($Method -eq "Post") {
            return Invoke-WebRequest -Uri $Uri -Method Post -ContentType $ContentType -Body $Body -UseBasicParsing -MaximumRedirection 0 -TimeoutSec $TimeoutSeconds
        }
        return Invoke-WebRequest -Uri $Uri -UseBasicParsing -MaximumRedirection 0 -TimeoutSec $TimeoutSeconds
    } finally {
        $ProgressPreference = $priorProgressPreference
    }
}

function Get-VerifiedBuildFingerprint {
    param([string]$ExpectedHash, [string]$FingerprintPath)
    $candidate = $ExpectedHash.Trim()
    if (-not $candidate) {
        if (-not (Test-Path -LiteralPath $FingerprintPath -PathType Leaf)) { throw "verified release fingerprint is unavailable" }
        $item = Get-Item -LiteralPath $FingerprintPath -ErrorAction Stop
        if ($item.Length -gt 128) { throw "verified release fingerprint is malformed" }
        $candidate = [IO.File]::ReadAllText($FingerprintPath).Trim()
    }
    if ($candidate -notmatch '^[A-Fa-f0-9]{64}$') { throw "verified release fingerprint is malformed" }
    return $candidate.ToLowerInvariant()
}

function Get-CanonicalCatDeskIdentity {
    param([string]$Root, [string]$ExpectedHash, [string]$FingerprintPath)
    $path = Join-Path $Root "target\release\catdesk.exe"
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "canonical CatDesk executable is unavailable" }
    $expected = Get-VerifiedBuildFingerprint -ExpectedHash $ExpectedHash -FingerprintPath $FingerprintPath
    $hash = (Get-FileHash -Algorithm SHA256 -LiteralPath $path).Hash.ToLowerInvariant()
    if ($hash -ne $expected) { throw "canonical CatDesk build fingerprint did not match" }
    [pscustomobject]@{ Path = (Resolve-Path -LiteralPath $path).Path; Sha256 = $hash }
}

function Read-OfficialRuntimeProcessMode {
    param([string]$Path)
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { return "MISSING" }
    $text = [IO.File]::ReadAllText($Path); if ($text.Length -gt 1048576) { throw "config is too large" }
    $lines = [regex]::Split($text, "`r?`n"); $sections = @()
    for ($i=0; $i -lt $lines.Count; $i++) { $t=$lines[$i].Trim(); if ($t.StartsWith("[") -and $t -notmatch '^\[[A-Za-z0-9_.-]+\]\s*(#.*)?$') { throw "config is malformed" }; if ($t -match '^\[([A-Za-z0-9_.-]+)\]\s*(?:#.*)?$') { $sections += [pscustomobject]@{Name=$Matches[1];Start=$i} } }
    $tunnel=@($sections|Where-Object Name -eq "tunnel"); $openai=@($sections|Where-Object Name -eq "openai_tunnel")
    if ($tunnel.Count -ne 1 -or $openai.Count -ne 1) { throw "config tunnel sections are ambiguous" }
    $endFor={param($section) $next=@($sections|Where-Object Start -gt $section.Start|Select-Object -First 1).Start; if ($null -eq $next) {$lines.Count} else {$next}}
    $tunnelEnd=&$endFor $tunnel[0]; $m=@(); for($i=$tunnel[0].Start+1;$i -lt $tunnelEnd;$i++){if($lines[$i].Trim() -match '^mode\s*=\s*"([^"]+)"\s*(?:#.*)?$'){$m+=$Matches[1]}}
    if ($m.Count -ne 1 -or $m[0] -ne "openai_secure_tunnel") { throw "config is not an unambiguous secure tunnel configuration" }
    $openaiEnd=&$endFor $openai[0]; $p=@(); for($i=$openai[0].Start+1;$i -lt $openaiEnd;$i++){if($lines[$i].Trim() -match '^process_mode\s*=\s*"([^"]+)"\s*(?:#.*)?$'){$p+=$Matches[1]}}
    if ($p.Count -ne 1) { throw "config process mode is missing or ambiguous" }; return $p[0]
}

function Get-ConfiguredRuntimeAlias {
    param([string]$Path)
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { throw "runtime alias is unavailable" }
    $text=[IO.File]::ReadAllText($Path);if($text.Length -gt 1048576){throw "runtime alias is invalid"}
    $section="";$sections=0;$aliases=@()
    foreach($line in [regex]::Split($text,"`r?`n")){$trim=$line.Trim();if($trim -match '^\[([A-Za-z0-9_.-]+)\]\s*(?:#.*)?$'){$section=$Matches[1];if($section -eq "openai_tunnel"){$sections++};continue};if($section -eq "openai_tunnel" -and $trim -match '^runtime_alias\s*=\s*"([^"]+)"\s*(?:#.*)?$'){$aliases+=$Matches[1]}}
    if($sections -ne 1 -or $aliases.Count -gt 1){throw "runtime alias is invalid"}
    $alias=if($aliases.Count -eq 0){"catdesk-local"}else{$aliases[0].Trim()}
    if($alias -notmatch '^[A-Za-z0-9_.-]{1,64}$'){throw "runtime alias is invalid"}
    return $alias
}

function Find-TunnelClient {
    param([string]$ExplicitPath, [string]$UserProfilePath = ([Environment]::GetFolderPath("UserProfile")))
    # Recovery must not grant executable-selection authority to caller PATH.
    # An explicit operator-supplied path is authoritative by construction.
    if($ExplicitPath){
        if(Test-Path -LiteralPath $ExplicitPath -PathType Leaf){return (Resolve-Path -LiteralPath $ExplicitPath -ErrorAction Stop).Path}
        return $null
    }
    # Default discovery is durable CatDesk authority, not an operator override.
    # Reject leaf reparse points and path-identity drift before allowing the
    # client to attest the externally owned official runtime.
    foreach($candidate in @(
        (Join-Path $UserProfilePath ".catdesk\tools\tunnel-client\current\tunnel-client.exe"),
        (Join-Path $UserProfilePath ".catdesk\tools\tunnel-client\tunnel-client.exe")
    )){
        if(-not(Test-Path -LiteralPath $candidate -PathType Leaf)){continue}
        try{$item=Get-Item -LiteralPath $candidate -Force -ErrorAction Stop}catch{continue}
        if(-not($item -is [System.IO.FileInfo])){continue}
        if(($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0){continue}
        $expectedPath=[IO.Path]::GetFullPath($candidate)
        $resolvedPath=[IO.Path]::GetFullPath($item.FullName)
        if(-not $resolvedPath.Equals($expectedPath,[StringComparison]::OrdinalIgnoreCase)){continue}
        return $resolvedPath
    }
    return $null
}

function Get-ConfiguredLocalMcp {
    param([string]$Path)
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { throw "configured local MCP endpoint is unavailable" }
    $text=[IO.File]::ReadAllText($Path); if($text.Length -gt 1048576){throw "configured local MCP endpoint is invalid"}
    $section=""; $sections=0; $bindHost=$null; $port=$null; $route=$null
    foreach($line in [regex]::Split($text,"`r?`n")) {
        $trim=$line.Trim()
        if($trim -match '^\[([A-Za-z0-9_.-]+)\]\s*(?:#.*)?$') {$section=$Matches[1];if($section -eq "mcp"){$sections++};continue}
        if($section -ne "mcp"){continue}
        if($trim -match '^bind_host\s*=\s*"([^"]+)"\s*(?:#.*)?$'){if($null -ne $bindHost){throw "configured local MCP endpoint is invalid"};$bindHost=$Matches[1];continue}
        if($trim -match '^port\s*=\s*(\d+)\s*(?:#.*)?$'){if($null -ne $port){throw "configured local MCP endpoint is invalid"};$port=[int]$Matches[1];continue}
        if($trim -match '^route_id\s*=\s*"([^"]+)"\s*(?:#.*)?$'){if($null -ne $route){throw "configured local MCP endpoint is invalid"};$route=$Matches[1]}
    }
    if($sections -ne 1 -or $bindHost -notin @("127.0.0.1","localhost","::1") -or $port -lt 1 -or $port -gt 65535 -or $route -notmatch '^[A-Za-z0-9_-]{24,96}$'){throw "configured local MCP endpoint is invalid"}
    $authority=if($bindHost -eq "::1"){"[$bindHost]"}else{$bindHost}
    [pscustomobject]@{Port=$port; Uri="http://$authority`:$port/$route/mcp"}
}

function Get-LoopbackCatDeskListener {
    param([int]$Port, $Canonical)
    Invoke-BootstrapSeam -Name "Listener" -Arguments @($Port,$Canonical) -Default {
        param($probePort,$identity)
        # Windows can legitimately expose one IPv4 and one IPv6 loopback Listen
        # row for the same process. Bound the accepted shape to that dual-stack
        # pair, then require every row to have the same owner before trusting the
        # exact process/path/hash identity. Distinct owners or a third row remain
        # ambiguous and fail closed.
        $listeners=@(Invoke-BoundedWindowsInventoryProbe -Mode Listener -Port $probePort)
        if($listeners.Count -gt 2){throw "multiple local MCP listeners are ambiguous"}; if($listeners.Count -eq 0){return $null}
        $ownerPids=@($listeners|ForEach-Object{[int]$_.OwningProcess}|Select-Object -Unique)
        if($ownerPids.Count -ne 1){throw "multiple local MCP listeners are ambiguous"}
        $ownerPid=[int]$ownerPids[0]
        $rows=@(Invoke-BoundedWindowsInventoryProbe -Mode Process -ProcessId $ownerPid)
        if($rows.Count -ne 1){throw "local MCP port is not a verifiable CatDesk process"}
        $row=$rows[0];$creationTimeUtc=$null
        try{$creationTimeUtc=([DateTime]$row.CreationDate).ToUniversalTime().ToString('o')}catch{}
        $executablePath=[string]$row.ExecutablePath;$commandLine=[string]$row.CommandLine
        if([string]$row.Name -ne 'catdesk.exe' -or $ownerPid -lt 1 -or [string]::IsNullOrWhiteSpace($creationTimeUtc) -or [string]::IsNullOrWhiteSpace($executablePath) -or $commandLine -notmatch '(?i)(?:^|\s)--catdesk-daemon(?:\s|$)'){throw "local MCP port is not a verifiable CatDesk process"}
        $actual=(Resolve-Path -LiteralPath $executablePath -ErrorAction Stop).Path;$actualHash=(Get-FileHash -Algorithm SHA256 -LiteralPath $actual).Hash.ToLowerInvariant()
        [pscustomobject]@{Pid=$ownerPid;CreationTimeUtc=$creationTimeUtc;Path=$actual;Sha256=$actualHash;MatchesCanonical=($actual.Equals([string]$identity.Path,[StringComparison]::OrdinalIgnoreCase) -and $actualHash -eq [string]$identity.Sha256)}
    }
}

function Test-CatDeskPinnedProcessMatchesRecoveryIdentity {
    param($Process,[string]$SelectedCreation,[string]$SelectedPath)
    try {
        # Force the OS handle first; StartTime/Path are then observations of the
        # exact pinned process object rather than a second PID-only lookup.
        $null=$Process.Handle
        # Win32_Process.CreationDate is exposed at microsecond precision while
        # Process.StartTime can retain 100ns ticks. Normalize both to the CIM
        # precision boundary; this preserves instance identity without rejecting
        # the same process for sub-microsecond representation loss.
        $selectedTicks=[DateTime]::Parse($SelectedCreation).ToUniversalTime().Ticks
        $processTicks=$Process.StartTime.ToUniversalTime().Ticks
        $selectedMicros=[long]($selectedTicks - ($selectedTicks % 10))
        $processMicros=[long]($processTicks - ($processTicks % 10))
        $sameCreation=($selectedMicros -eq $processMicros)
        $processPath=(Resolve-Path -LiteralPath $Process.Path -ErrorAction Stop).Path
        return ($sameCreation -and $processPath.Equals($SelectedPath,[StringComparison]::OrdinalIgnoreCase))
    } catch {
        return $false
    }
}

function Get-CatDeskListenerProcessInstanceForRecovery {
    param($Candidate)
    return Invoke-BootstrapSeam -Name 'ListenerProcessInstance' -Arguments @($Candidate) -Default {
        param($selected)
        $processId=[int]$selected.Pid;$selectedCreation=[string]$selected.CreationTimeUtc;$selectedPath=[string]$selected.Path;$selectedHash=[string]$selected.Sha256
        if($processId -lt 1 -or [string]::IsNullOrWhiteSpace($selectedCreation) -or [string]::IsNullOrWhiteSpace($selectedPath) -or $selectedHash -notmatch '^[a-f0-9]{64}$'){throw "CatDesk listener process instance is ambiguous"}
        $rows=@(Invoke-BoundedWindowsInventoryProbe -Mode Process -ProcessId $processId)
        if($rows.Count -eq 0){return $null};if($rows.Count -ne 1){throw "CatDesk listener process instance is ambiguous"}
        $row=$rows[0];$currentCreation=$null
        try{$currentCreation=([DateTime]$row.CreationDate).ToUniversalTime().ToString('o')}catch{}
        $executablePath=[string]$row.ExecutablePath;$commandLine=[string]$row.CommandLine
        if([string]$row.Name -ne 'catdesk.exe' -or $currentCreation -ne $selectedCreation -or [string]::IsNullOrWhiteSpace($executablePath) -or $commandLine -notmatch '(?i)(?:^|\s)--catdesk-daemon(?:\s|$)'){throw "CatDesk listener process instance changed before recovery mutation"}
        try{
            $actualPath=(Resolve-Path -LiteralPath $executablePath -ErrorAction Stop).Path;$actualHash=(Get-FileHash -Algorithm SHA256 -LiteralPath $actualPath).Hash.ToLowerInvariant()
            if(-not $actualPath.Equals($selectedPath,[StringComparison]::OrdinalIgnoreCase) -or $actualHash -ne $selectedHash){throw 'identity mismatch'}
            $process=Get-Process -Id $processId -ErrorAction Stop
            if(-not(Test-CatDeskPinnedProcessMatchesRecoveryIdentity -Process $process -SelectedCreation $selectedCreation -SelectedPath $selectedPath)){throw 'identity mismatch'}
            return $process
        }catch{
            if($_.Exception.Message -eq 'identity mismatch'){throw "CatDesk listener process instance changed before recovery mutation"}
            throw "CatDesk listener process instance is ambiguous"
        }
    }
}

function Test-CatDeskListenerStillMatchesRecoveryCandidate {
    param($LocalMcp,$Canonical,$Candidate)
    $current=Get-LoopbackCatDeskListener -Port $LocalMcp.Port -Canonical $Canonical
    if($null -eq $current){return $true}
    return ([int]$current.Pid -eq [int]$Candidate.Pid -and [string]$current.CreationTimeUtc -eq [string]$Candidate.CreationTimeUtc -and [string]$current.Path -eq [string]$Candidate.Path -and [string]$current.Sha256 -eq [string]$Candidate.Sha256)
}

function Stop-CatDeskListenerProcessForRecovery {
    param($LocalMcp,$Canonical,$Candidate,[string]$FailureMessage)
    $process=Get-CatDeskListenerProcessInstanceForRecovery -Candidate $Candidate
    if($null -eq $process){return $false}
    if(-not(Test-CatDeskListenerStillMatchesRecoveryCandidate -LocalMcp $LocalMcp -Canonical $Canonical -Candidate $Candidate)){throw "CatDesk listener changed before recovery mutation"}
    try{$process.Kill()}catch [InvalidOperationException]{return $true}
    if(-not $process.WaitForExit(30000)){throw $FailureMessage}
    return $true
}

function Get-CatDeskDaemonProcessCandidates {
    param($Canonical)
    return @(Invoke-BootstrapSeam -Name "DaemonProcesses" -Arguments @($Canonical) -Default {
        param($identity)
        # A process name alone is never recovery authority.  Inspect only bounded
        # catdesk.exe rows, require the fixed daemon token, then bind each candidate
        # to the exact canonical path+hash before any stop decision is possible.
        # Scan enough rows to account for normal concurrent CatDesk processes
        # before applying the exact daemon-token/path/hash filter. Truncating
        # an arbitrary first handful can silently miss the only stale canonical
        # daemon on a busy host. The one-extra-row shape keeps enumeration
        # bounded while making an overload explicitly ambiguous and fail-closed.
        $rows = @(Invoke-BoundedWindowsInventoryProbe -Mode Daemons)
        if ($rows.Count -gt 64) { throw "too many CatDesk process rows are ambiguous" }
        $candidates = @()
        foreach ($row in $rows) {
            $commandLine = [string]$row.CommandLine
            if ($commandLine -notmatch '(?i)(?:^|\s)--catdesk-daemon(?:\s|$)') { continue }
            $processId = [int]$row.ProcessId
            $executablePath = [string]$row.ExecutablePath
            $creationTimeUtc = $null
            try { $creationTimeUtc = ([DateTime]$row.CreationDate).ToUniversalTime().ToString('o') } catch {}
            if ($processId -lt 1 -or [string]::IsNullOrWhiteSpace($executablePath) -or [string]::IsNullOrWhiteSpace($creationTimeUtc)) {
                throw "CatDesk daemon process identity is ambiguous"
            }
            try {
                $actualPath = (Resolve-Path -LiteralPath $executablePath -ErrorAction Stop).Path
                $actualHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $actualPath).Hash.ToLowerInvariant()
            } catch {
                throw "CatDesk daemon process identity is ambiguous"
            }
            $pathMatches = $actualPath.Equals([string]$identity.Path, [StringComparison]::OrdinalIgnoreCase)
            $candidates += [pscustomobject]@{
                Pid = $processId
                CreationTimeUtc = $creationTimeUtc
                MatchesCanonical = ($pathMatches -and $actualHash -eq [string]$identity.Sha256)
            }
        }
        return @($candidates)
    })
}

function Get-CatDeskDaemonProcessInstanceForRecovery {
    param($Canonical, $Candidate)
    return Invoke-BootstrapSeam -Name 'DaemonProcessInstance' -Arguments @($Canonical, $Candidate) -Default {
        param($identity, $selected)
        $processId = [int]$selected.Pid
        $selectedCreation = [string]$selected.CreationTimeUtc
        if ($processId -lt 1 -or [string]::IsNullOrWhiteSpace($selectedCreation)) {
            throw "CatDesk daemon process instance is ambiguous"
        }
        $rows = @(Invoke-BoundedWindowsInventoryProbe -Mode Process -ProcessId $processId)
        if ($rows.Count -eq 0) { return $null }
        if ($rows.Count -ne 1) { throw "CatDesk daemon process instance is ambiguous" }
        $row = $rows[0]
        $commandLine = [string]$row.CommandLine
        $executablePath = [string]$row.ExecutablePath
        $currentCreation = $null
        try { $currentCreation = ([DateTime]$row.CreationDate).ToUniversalTime().ToString('o') } catch {}
        if ($commandLine -notmatch '(?i)(?:^|\s)--catdesk-daemon(?:\s|$)' -or [string]::IsNullOrWhiteSpace($executablePath) -or $currentCreation -ne $selectedCreation) {
            throw "CatDesk daemon process instance changed before recovery mutation"
        }
        try {
            $actualPath = (Resolve-Path -LiteralPath $executablePath -ErrorAction Stop).Path
            $actualHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $actualPath).Hash.ToLowerInvariant()
            if (-not $actualPath.Equals([string]$identity.Path, [StringComparison]::OrdinalIgnoreCase) -or $actualHash -ne [string]$identity.Sha256) {
                throw "identity mismatch"
            }
            $process = Get-Process -Id $processId -ErrorAction Stop
            if (-not (Test-CatDeskPinnedProcessMatchesRecoveryIdentity -Process $process -SelectedCreation $selectedCreation -SelectedPath ([string]$identity.Path))) { throw "identity mismatch" }
            return $process
        } catch {
            if ($_.Exception.Message -eq 'identity mismatch') { throw "CatDesk daemon process instance changed before recovery mutation" }
            throw "CatDesk daemon process instance is ambiguous"
        }
    }
}

function Stop-StaleCanonicalCatDeskDaemonForRecovery {
    param($Canonical, $LocalMcp)
    # Listener ownership remains the strongest live signal. Re-check before
    # discovery and once more after pinning the exact process instance so a
    # daemon that becomes ready during recovery is never mistaken for stale.
    if ($null -ne (Get-LoopbackCatDeskListener -Port $LocalMcp.Port -Canonical $Canonical)) { return $false }
    $candidates = @(Get-CatDeskDaemonProcessCandidates -Canonical $Canonical)
    if ($candidates.Count -eq 0) { return $false }
    if (@($candidates | Where-Object { -not $_.MatchesCanonical }).Count -gt 0) {
        throw "noncanonical CatDesk daemon process is ambiguous"
    }
    if ($candidates.Count -ne 1) { throw "multiple canonical CatDesk daemon processes are ambiguous" }
    $candidate = $candidates[0]
    $process = Get-CatDeskDaemonProcessInstanceForRecovery -Canonical $Canonical -Candidate $candidate
    if ($null -eq $process) { return $false }
    if ($null -ne (Get-LoopbackCatDeskListener -Port $LocalMcp.Port -Canonical $Canonical)) { return $false }
    try {
        $process.Kill()
    } catch [InvalidOperationException] {
        # The pinned process exited after identity proof. The stale instance is
        # already gone; importantly, no replacement PID can be targeted here.
        return $true
    }
    if (-not $process.WaitForExit(30000)) { throw "stale canonical CatDesk daemon did not stop" }
    return $true
}

function Invoke-BoundedCodexProbe {
    param($Command,[string[]]$Arguments,[string]$FailureMessage)
    $source=[string]$Command.Source
    if([string]::IsNullOrWhiteSpace($source) -or -not(Test-Path -LiteralPath $source -PathType Leaf)){throw "Codex CLI is unavailable"}
    $extension=[IO.Path]::GetExtension($source).ToLowerInvariant()
    $fileName=$source
    $commandLine=[string]::Join(" ", @($Arguments | ForEach-Object { ConvertTo-NativeCommandLineArgument -Value $_ }))
    if($extension -in @('.cmd','.bat')){
        $fileName=Resolve-TrustedWindowsCommandPromptPath
        # The Codex arguments are fixed internal tokens. The source comes from
        # Get-Command and Windows paths cannot contain a literal quote.
        $commandLine='/d /s /c ""'+$source+'" '+([string]::Join(' ',[string[]]$Arguments))+'"'
    } elseif($extension -eq '.ps1') {
        $fileName=Resolve-TrustedWindowsPowerShellPath
        $wrapped=@('-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File',$source)+$Arguments
        $commandLine=[string]::Join(" ", @($wrapped | ForEach-Object { ConvertTo-NativeCommandLineArgument -Value $_ }))
    }
    Initialize-BoundedNativeProcessType
    $result=[CatDesk.BoundedNativeProcess]::Run($fileName,$commandLine,$script:MaxCodexProbeMilliseconds,$script:MaxCodexProbeOutputBytes)
    if($result.TimedOut){
        if($result.TerminationFailed){throw "Codex CLI probe exceeded bounded timeout and did not terminate"}
        throw "Codex CLI probe exceeded bounded timeout and was killed"
    }
    if($result.OutputDrainTimedOut){throw "Codex CLI probe output drain exceeded bounded timeout"}
    if($result.Overflow){throw "Codex CLI probe output exceeded bounded capture"}
    if($result.ExitCode -ne 0){throw $FailureMessage}
}

function Resolve-TrustedCurrentUserCodexCommand {
    param([string]$CurrentUserNpmRoot = "")
    # T-0343: recovery prerequisite checks must not let caller PATH choose the
    # Codex executable. Preserve the established explicit recovery override,
    # otherwise use only the deterministic current-user npm command directory.
    $candidates=[Collections.Generic.List[string]]::new()
    $explicit=[string]$env:CATDESK_CODEX_CLI_EXECUTABLE
    if(-not [string]::IsNullOrWhiteSpace($explicit)){
        $candidates.Add($explicit)
    } else {
        $npmRoot=$CurrentUserNpmRoot
        if([string]::IsNullOrWhiteSpace($npmRoot)){
            $appData=[Environment]::GetFolderPath('ApplicationData')
            if([string]::IsNullOrWhiteSpace($appData)){throw "Codex current-user installation directory is unavailable"}
            $npmRoot=Join-Path $appData 'npm'
        }
        foreach($leaf in @('codex.exe','codex.cmd','codex.ps1')){$candidates.Add((Join-Path $npmRoot $leaf))}
    }
    foreach($candidate in $candidates){
        if([string]::IsNullOrWhiteSpace($candidate) -or -not(Test-Path -LiteralPath $candidate -PathType Leaf)){continue}
        try {$item=Get-Item -LiteralPath $candidate -Force -ErrorAction Stop} catch {continue}
        if(-not($item -is [System.IO.FileInfo])){continue}
        if(($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0){continue}
        $expectedPath=[IO.Path]::GetFullPath($candidate)
        $resolvedPath=[IO.Path]::GetFullPath($item.FullName)
        if(-not $resolvedPath.Equals($expectedPath,[StringComparison]::OrdinalIgnoreCase)){continue}
        return [pscustomobject]@{Source=$resolvedPath}
    }
    throw "Codex CLI is unavailable"
}

function Test-CodexOnDemandPrerequisites {
    $codex=Resolve-TrustedCurrentUserCodexCommand
    Invoke-BoundedCodexProbe -Command $codex -Arguments @('--version') -FailureMessage "Codex CLI is not runnable"
    Invoke-BoundedCodexProbe -Command $codex -Arguments @('login','status') -FailureMessage "Codex current-user authentication is unavailable"
}

function Resolve-TrustedWindowsCommandPromptPath {
    $systemDirectory=[Environment]::SystemDirectory
    if([string]::IsNullOrWhiteSpace($systemDirectory)){throw "trusted Windows command processor system directory is unavailable"}
    $candidate=Join-Path $systemDirectory "cmd.exe"
    try {$item=Get-Item -LiteralPath $candidate -Force -ErrorAction Stop} catch {throw "trusted Windows command processor executable is unavailable"}
    if(-not($item -is [System.IO.FileInfo])){throw "trusted Windows command processor identity is invalid"}
    if(($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0){throw "trusted Windows command processor executable is a reparse point"}
    $expectedPath=[IO.Path]::GetFullPath($candidate)
    $resolvedPath=[IO.Path]::GetFullPath($item.FullName)
    if(-not $resolvedPath.Equals($expectedPath,[StringComparison]::OrdinalIgnoreCase)){throw "trusted Windows command processor identity drifted"}
    return $resolvedPath
}

function Resolve-TrustedWindowsPowerShellPath {
    $systemDirectory=[Environment]::SystemDirectory
    if([string]::IsNullOrWhiteSpace($systemDirectory)){throw "trusted Windows PowerShell system directory is unavailable"}
    $candidate=Join-Path $systemDirectory "WindowsPowerShell\v1.0\powershell.exe"
    try {$item=Get-Item -LiteralPath $candidate -Force -ErrorAction Stop} catch {throw "trusted Windows PowerShell executable is unavailable"}
    if(-not($item -is [System.IO.FileInfo])){throw "trusted Windows PowerShell identity is invalid"}
    if(($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0){throw "trusted Windows PowerShell executable is a reparse point"}
    $expectedPath=[IO.Path]::GetFullPath($candidate)
    $resolvedPath=[IO.Path]::GetFullPath($item.FullName)
    if(-not $resolvedPath.Equals($expectedPath,[StringComparison]::OrdinalIgnoreCase)){throw "trusted Windows PowerShell identity drifted"}
    return $resolvedPath
}

function Invoke-BoundedRecoveryHelper {
    param([string]$FileName,[string[]]$Arguments,[int]$TimeoutMilliseconds,[string]$FailureMessage)
    if([string]::IsNullOrWhiteSpace($FileName) -or -not(Test-Path -LiteralPath $FileName -PathType Leaf)){throw $FailureMessage}
    Initialize-BoundedNativeProcessType
    $commandLine=[string]::Join(" ", @($Arguments | ForEach-Object { ConvertTo-NativeCommandLineArgument -Value $_ }))
    $result=[CatDesk.BoundedNativeProcess]::Run($FileName,$commandLine,$TimeoutMilliseconds,$script:MaxRecoveryHelperOutputBytes)
    if($result.TimedOut){
        if($result.TerminationFailed){throw "$FailureMessage; bounded helper did not terminate"}
        throw "$FailureMessage; bounded helper timed out"
    }
    if($result.OutputDrainTimedOut){throw "$FailureMessage; bounded helper output drain timed out"}
    if($result.Overflow){throw "$FailureMessage; bounded helper output exceeded capture"}
    if($result.ExitCode -ne 0){throw $FailureMessage}
    return $result
}

function Invoke-BoundedWindowsInventoryProbe {
    param(
        [ValidateSet('Listener','Process','Daemons')][string]$Mode,
        [int]$Port = 0,
        [int]$ProcessId = 0,
        [string]$HelperPath = ''
    )
    $helper = if ([string]::IsNullOrWhiteSpace($HelperPath)) { [string]$script:WindowsInventoryProbeHelperPath } else { $HelperPath }
    # Project-owned PowerShell helpers are executable recovery authority. Keep
    # inventory observation on the same exact, non-reparse leaf identity policy
    # as release recovery, wake repair, migration, and interrupted promotion.
    $resolvedPath = Resolve-TrustedBootstrapHelperPath -Path $helper -FailureMessage 'Windows inventory probe helper identity is invalid'
    $powershell = Resolve-TrustedWindowsPowerShellPath
    $arguments = @('-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File',$resolvedPath,'-Mode',$Mode)
    if ($Mode -eq 'Listener') {
        if ($Port -lt 1 -or $Port -gt 65535) { throw 'Windows inventory listener port is invalid' }
        $arguments += @('-Port',[string]$Port)
    } elseif ($Mode -eq 'Process') {
        if ($ProcessId -lt 1) { throw 'Windows inventory process id is invalid' }
        $arguments += @('-ProcessId',[string]$ProcessId)
    }
    Initialize-BoundedNativeProcessType
    $commandLine = [string]::Join(' ', @($arguments | ForEach-Object { ConvertTo-NativeCommandLineArgument -Value $_ }))
    $result = [CatDesk.BoundedNativeProcess]::Run($powershell,$commandLine,$script:MaxWindowsInventoryProbeMilliseconds,$script:MaxWindowsInventoryProbeOutputBytes)
    if ($result.TimedOut) {
        if ($result.TerminationFailed) { throw 'Windows inventory probe exceeded bounded timeout and did not terminate' }
        throw 'Windows inventory probe exceeded bounded timeout and was killed'
    }
    if ($result.OutputDrainTimedOut) { throw 'Windows inventory probe output drain exceeded bounded timeout' }
    if ($result.Overflow) { throw 'Windows inventory probe output exceeded bounded capture' }
    if ($result.ExitCode -ne 0) { throw 'Windows inventory probe failed' }
    $text = [string]$result.Stdout
    if ([string]::IsNullOrWhiteSpace($text)) { throw 'Windows inventory probe returned no result' }
    try { $parsed = $text | ConvertFrom-Json -ErrorAction Stop } catch { throw 'Windows inventory probe result is invalid' }
    if ($null -eq $parsed -or $null -eq $parsed.PSObject.Properties['Rows']) { throw 'Windows inventory probe result is invalid' }
    return @($parsed.Rows)
}

function Test-WakeBridgeRuntime {
    param([string]$Root)
    Invoke-BootstrapSeam -Name "WakeRuntime" -Arguments @($Root) -Default {
        param($workspace)
        try {
            $independentHost = Resolve-VerifiedInstalledWakeHost -Root $workspace
            if ($independentHost) {
                # The independent WakeHost supersedes the retired Python venv.
                # Status is read-only and checks the version-pinned executable;
                # it does not start a stopped owner or touch any browser.
                $observed = Invoke-BoundedRecoveryHelper -FileName $independentHost -Arguments @('status') -TimeoutMilliseconds $script:MaxWakeRuntimeProbeMilliseconds -FailureMessage 'independent WakeHost status unavailable'
                if ($observed.ExitCode -ne 0 -or $observed.Overflow -or $observed.OutputDrainTimedOut) { return $false }
                $status = [string]$observed.Stdout | ConvertFrom-Json -ErrorAction Stop
                if ($status.protocolVersion -ne 1 -or $status.host -notin @('RUNNING','STOPPED') -or
                    $null -eq $status.targets -or $null -eq $status.targets.catdesk -or
                    [string]$status.targets.catdesk.digest -notmatch '^[a-fA-F0-9]{64}$' -or
                    [int]$status.targets.catdesk.generation -lt 1) { return $false }
                return $true
            }
            # The provisioned wake venv interpreter is executable recovery
            # authority just like project-owned PowerShell helpers. A leaf
            # existence check alone would allow a reparse-point replacement to
            # redirect plan/recover before the bounded probe even starts.
            $python=Resolve-TrustedBootstrapHelperPath -Path (Join-Path $workspace ".catdesk\wake-bridge\venv\Scripts\python.exe") -FailureMessage "wake runtime interpreter identity is invalid"
            $result=Invoke-BoundedRecoveryHelper -FileName $python -Arguments @('-c','import sys, seleniumbase; raise SystemExit(0 if sys.version_info >= (3,10) else 1)') -TimeoutMilliseconds $script:MaxWakeRuntimeProbeMilliseconds -FailureMessage "wake runtime probe did not complete"
            return ($result.ExitCode -eq 0)
        } catch {return $false}
    }
}

function Invoke-WakeBridgeRuntimeRepair {
    param([string]$Root)
    Invoke-BootstrapSeam -Name "WakeRepair" -Arguments @($Root) -Default {
        param($workspace)
        # If independent_v1 is selected, repairing the retired Python venv
        # cannot repair the active WakeHost. Refuse instead of mutating it.
        $independentHost = Resolve-VerifiedInstalledWakeHost -Root $workspace
        if ($independentHost) { throw 'independent wake repair requires installed host authority' }
        $repair=Resolve-TrustedBootstrapHelperPath -Path (Join-Path $workspace "scripts\repair_wake_bridge_environment.ps1") -FailureMessage "wake runtime repair helper identity is invalid"
        $powershell=Resolve-TrustedWindowsPowerShellPath
        $null=Invoke-BoundedRecoveryHelper -FileName $powershell -Arguments @('-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File',$repair) -TimeoutMilliseconds $script:MaxWakeRepairMilliseconds -FailureMessage "wake runtime repair did not complete"
    }
}

function Invoke-OrderedOfficialRuntimeMigration {
    param([string]$SetupScript,[string]$Path)
    $trustedSetupScript=Resolve-TrustedBootstrapHelperPath -Path $SetupScript -FailureMessage "secure MCP setup helper identity is invalid"
    $powershell=Resolve-TrustedWindowsPowerShellPath
    $null=Invoke-BoundedRecoveryHelper -FileName $powershell -Arguments @('-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File',$trustedSetupScript,'-Mode','migrate','-ConfigPath',$Path,'-PreserveExistingMigrationBackup') -TimeoutMilliseconds $script:MaxOfficialMigrationMilliseconds -FailureMessage "official runtime migration did not complete"
}

function New-LocalMcpReadinessResult {
    param([bool]$Ready,[string]$Gate)
    if($Gate -notin @(
        'READY',
        'LOCAL_MCP_LISTENER_MISSING',
        'LOCAL_MCP_LISTENER_IDENTITY_MISMATCH',
        'LOCAL_MCP_LAUNCHED_PROCESS_MISMATCH',
        'LOCAL_MCP_RESPONSE_TIMEOUT',
        'LOCAL_MCP_RESPONSE_UNAVAILABLE',
        'LOCAL_MCP_RESPONSE_INVALID',
        'LOCAL_MCP_PROTOCOL_UNREADY'
    )){throw 'local MCP readiness gate is invalid'}
    return [pscustomobject]@{Ready=$Ready;Gate=$Gate}
}

function Get-LocalMcpReadiness {
    param($LocalMcp,$Canonical,$ExpectedDaemonProcess=$null)
    $observed=Invoke-BootstrapSeam -Name "LocalMcp" -Arguments @($LocalMcp,$Canonical,$ExpectedDaemonProcess) -Default {
        param($endpoint,$identity,$expectedProcess)
        $listener=Get-LoopbackCatDeskListener -Port $endpoint.Port -Canonical $identity
        if($null -eq $listener){return New-LocalMcpReadinessResult -Ready $false -Gate 'LOCAL_MCP_LISTENER_MISSING'}
        if(-not $listener.MatchesCanonical){return New-LocalMcpReadinessResult -Ready $false -Gate 'LOCAL_MCP_LISTENER_IDENTITY_MISMATCH'}
        if($null -ne $expectedProcess){
            # A recovery invocation that launched the daemon must prove that the
            # exact process it launched owns the listener. Canonical path/hash
            # equivalence alone is insufficient because another canonical daemon
            # can race the launch and otherwise be mistaken for our child.
            if([int]$listener.Pid -ne [int]$expectedProcess.Id){return New-LocalMcpReadinessResult -Ready $false -Gate 'LOCAL_MCP_LAUNCHED_PROCESS_MISMATCH'}
            if(-not(Test-CatDeskPinnedProcessMatchesRecoveryIdentity -Process $expectedProcess -SelectedCreation ([string]$listener.CreationTimeUtc) -SelectedPath ([string]$identity.Path))){return New-LocalMcpReadinessResult -Ready $false -Gate 'LOCAL_MCP_LAUNCHED_PROCESS_MISMATCH'}
        }
        foreach($request in @(
            @{jsonrpc="2.0";id="catdesk-bootstrap-initialize";method="initialize";params=@{protocolVersion="2025-03-26";capabilities=@{};clientInfo=@{name="catdesk-bootstrap";version="1"}}},
            @{jsonrpc="2.0";id="catdesk-bootstrap-tools";method="tools/list";params=@{}}
        )) {
            try {
                $response=Invoke-BootstrapSeam -Name "LocalMcpHttp" -Arguments @($endpoint.Uri,($request|ConvertTo-Json -Compress -Depth 5)) -Default { param($uri,$body) Invoke-SilentLoopbackHttp -Uri $uri -Method Post -ContentType "application/json" -Body $body -TimeoutSeconds 5 }
            } catch {
                $exception=$_.Exception
                $timedOut=($exception -is [System.Net.WebException] -and $exception.Status -eq [System.Net.WebExceptionStatus]::Timeout) -or ([string]$exception.Message -match '(?i)timed out|timeout')
                return New-LocalMcpReadinessResult -Ready $false -Gate $(if($timedOut){'LOCAL_MCP_RESPONSE_TIMEOUT'}else{'LOCAL_MCP_RESPONSE_UNAVAILABLE'})
            }
            if($null -eq $response -or $response.StatusCode -lt 200 -or $response.StatusCode -ge 300){return New-LocalMcpReadinessResult -Ready $false -Gate 'LOCAL_MCP_RESPONSE_UNAVAILABLE'}
            $content=[string]$response.Content
            if($content.Length -gt $script:MaxMcpReplyBytes){return New-LocalMcpReadinessResult -Ready $false -Gate 'LOCAL_MCP_RESPONSE_INVALID'}
            try{$parsed=$content|ConvertFrom-Json -ErrorAction Stop}catch{return New-LocalMcpReadinessResult -Ready $false -Gate 'LOCAL_MCP_RESPONSE_INVALID'}
            if($parsed.error){return New-LocalMcpReadinessResult -Ready $false -Gate 'LOCAL_MCP_PROTOCOL_UNREADY'}
            if($request.method -eq "initialize" -and $null -eq $parsed.result.serverInfo){return New-LocalMcpReadinessResult -Ready $false -Gate 'LOCAL_MCP_PROTOCOL_UNREADY'}
            if($request.method -eq "tools/list"){$names=@($parsed.result.tools|ForEach-Object{$_.name});if($names -notcontains "catdesk_instruction" -or $names -notcontains "delegated_run_list"){return New-LocalMcpReadinessResult -Ready $false -Gate 'LOCAL_MCP_PROTOCOL_UNREADY'}}
        }
        return New-LocalMcpReadinessResult -Ready $true -Gate 'READY'
    }
    if($observed -is [bool]){return New-LocalMcpReadinessResult -Ready ([bool]$observed) -Gate $(if($observed){'READY'}else{'LOCAL_MCP_RESPONSE_UNAVAILABLE'})}
    if($null -eq $observed -or $null -eq $observed.PSObject.Properties['Ready'] -or $null -eq $observed.PSObject.Properties['Gate']){throw 'local MCP readiness result is invalid'}
    return New-LocalMcpReadinessResult -Ready ([bool]$observed.Ready) -Gate ([string]$observed.Gate)
}

function Test-LocalMcpReadiness {
    param($LocalMcp,$Canonical,$ExpectedDaemonProcess=$null)
    return [bool](Get-LocalMcpReadiness -LocalMcp $LocalMcp -Canonical $Canonical -ExpectedDaemonProcess $ExpectedDaemonProcess).Ready
}

function Get-AuthoritativeRuntimeBase {
    param($Status,[string]$Alias)
    $url=$null; foreach($name in @("health_url","healthUrl","health_base_url","healthBaseUrl","runtime_health_url","runtimeHealthUrl")){if($Status.PSObject.Properties[$name] -and $Status.$name){$url=[string]$Status.$name;break}}
    if(-not $url){foreach($name in @("health_url_file","healthUrlFile","runtime_health_url_file","runtimeHealthUrlFile")){if($Status.PSObject.Properties[$name] -and $Status.$name){$file=[string]$Status.$name;$leaf=[IO.Path]::GetFileName($file);$parent=[IO.Path]::GetFileName([IO.Path]::GetDirectoryName($file));$approved=($leaf -eq "$Alias.health-url" -or $leaf -eq "$Alias.health_url" -or ($leaf -eq "$Alias.url" -and $parent -eq "health"));if(-not $approved){throw "runtime health state is invalid"};$info=Get-Item -LiteralPath $file -ErrorAction Stop;if($info.PSIsContainer -eq $true -or $info.Length -gt 4096){throw "runtime health state is invalid"};$url=(Get-Content -LiteralPath $file -TotalCount 1).Trim();break}}}
    if(-not $url){throw "runtime health state is unavailable"};$parsed=[Uri]$url
    if($parsed.Scheme -ne "http" -or -not $parsed.IsLoopback -or $parsed.UserInfo -or $parsed.Query -or $parsed.Fragment -or $parsed.Port -eq 3220){throw "runtime health state is invalid"}
    return $parsed.GetLeftPart([UriPartial]::Authority)
}

function New-OfficialRuntimeVerificationResult {
    param([bool]$Verified, [string]$Gate)
    if ($Gate -notin @(
        'READY', 'RUNTIME_CLIENT_UNAVAILABLE', 'RUNTIME_STATUS_TIMEOUT',
        'RUNTIME_STATUS_OVERSIZED', 'RUNTIME_STATUS_COMMAND_FAILED',
        'RUNTIME_STATUS_INVALID', 'RUNTIME_STATUS_UNAVAILABLE',
        'RUNTIME_STATUS_NOT_READY', 'RUNTIME_STATUS_TRANSIENT',
        'RUNTIME_HEALTH_REFERENCE_INVALID', 'RUNTIME_HEALTHZ_UNAVAILABLE',
        'RUNTIME_HEALTHZ_FAILED', 'RUNTIME_READYZ_UNAVAILABLE',
        'RUNTIME_READYZ_FAILED', 'LOCAL_MCP_PENDING'
    )) { throw 'runtime verification gate is invalid' }
    return [pscustomobject]@{ Verified = $Verified; Gate = $Gate }
}

function Get-OfficialRuntimeVerification {
    param([string]$Root,[int]$TimeoutSeconds,[string]$Alias,[string]$ClientPath,[datetime]$Deadline = [datetime]::MaxValue)
    $gate = 'RUNTIME_STATUS_TRANSIENT'
    try {
        if($script:BootstrapSeams.ContainsKey("Runtime")){
            $verified = [bool](& $script:BootstrapSeams["Runtime"] $Root $TimeoutSeconds)
            return New-OfficialRuntimeVerificationResult -Verified $verified -Gate $(if($verified){'READY'}else{'RUNTIME_STATUS_NOT_READY'})
        }
        $gate = 'RUNTIME_STATUS_TRANSIENT'
        $status=Invoke-BootstrapSeam -Name "RuntimeStatus" -Arguments @($Root,$TimeoutSeconds,$Alias,$ClientPath,$Deadline) -Default {param($workspace,$timeout,$runtimeAlias,$clientPath,$commandDeadline)if(-not $clientPath){throw "official runtime client unavailable"};$timeoutMilliseconds=Get-RemainingReadinessMilliseconds -Deadline $commandDeadline -RequestedMilliseconds ([Math]::Min($timeout * 1000,$script:MaxTunnelClientCommandMilliseconds));$result=Invoke-BoundedTunnelClient -ClientPath $clientPath -Arguments @("runtimes","status",$runtimeAlias,"--json") -TimeoutMilliseconds $timeoutMilliseconds;$json=$result.Stdout;if($json.Length -gt $script:MaxTunnelClientOutputBytes){throw "runtime status unavailable"};$json|ConvertFrom-Json -ErrorAction Stop}
        if($null -eq $status){return New-OfficialRuntimeVerificationResult -Verified $false -Gate 'RUNTIME_STATUS_UNAVAILABLE'}
        $state=if($status.status){[string]$status.status}elseif($status.state){[string]$status.state}else{""}
        $running=[bool]($status.running -or $status.process_running -or $status.processRunning -or $state -in @("running","ready","connected"));$healthy=[bool]($status.healthy -or $status.health_ok -or $status.healthOk -or $state -in @("healthy","ready","connected"));$ready=[bool]($status.ready -or $status.isReady -or $state -in @("ready","connected"))
        if(-not($running -and $healthy -and $ready)){return New-OfficialRuntimeVerificationResult -Verified $false -Gate 'RUNTIME_STATUS_NOT_READY'}
        $gate = 'RUNTIME_HEALTH_REFERENCE_INVALID'
        $base=Get-AuthoritativeRuntimeBase -Status $status -Alias $Alias
        foreach($path in @("healthz","readyz")){
            $gate = if($path -eq 'healthz'){'RUNTIME_HEALTHZ_UNAVAILABLE'}else{'RUNTIME_READYZ_UNAVAILABLE'}
            $response=Invoke-BootstrapSeam -Name "RuntimeHttp" -Arguments @("$base/$path") -Default {param($uri) Invoke-SilentLoopbackHttp -Uri $uri -TimeoutSeconds 5}
            if($null -eq $response){return New-OfficialRuntimeVerificationResult -Verified $false -Gate $gate}
            if($response.StatusCode -lt 200 -or $response.StatusCode -ge 300){
                $failedGate = if($path -eq 'healthz'){'RUNTIME_HEALTHZ_FAILED'}else{'RUNTIME_READYZ_FAILED'}
                return New-OfficialRuntimeVerificationResult -Verified $false -Gate $failedGate
            }
        }
        return New-OfficialRuntimeVerificationResult -Verified $true -Gate 'READY'
    } catch {
        $message = [string]$_.Exception.Message
        $mapped = if($message -eq 'official runtime client unavailable'){'RUNTIME_CLIENT_UNAVAILABLE'}
            elseif($message -match 'bounded timeout|deadline elapsed'){'RUNTIME_STATUS_TIMEOUT'}
            elseif($message -match 'bounded capture|output exceeded'){'RUNTIME_STATUS_OVERSIZED'}
            elseif($message -match 'command failed|did not start'){'RUNTIME_STATUS_COMMAND_FAILED'}
            elseif($message -match 'ConvertFrom-Json|JSON|runtime status unavailable'){'RUNTIME_STATUS_INVALID'}
            else{$gate}
        return New-OfficialRuntimeVerificationResult -Verified $false -Gate $mapped
    }
}

function Test-OfficialRuntimeVerified {
    param([string]$Root,[int]$TimeoutSeconds,[string]$Alias,[string]$ClientPath,[datetime]$Deadline = [datetime]::MaxValue)
    return [bool](Get-OfficialRuntimeVerification -Root $Root -TimeoutSeconds $TimeoutSeconds -Alias $Alias -ClientPath $ClientPath -Deadline $Deadline).Verified
}

function Wait-FullStackReadiness {
    param([string]$Root,$LocalMcp,[int]$TimeoutSeconds,$Canonical,[string]$Alias,[string]$ClientPath,[int]$PollMilliseconds = 500,[datetime]$Deadline = [datetime]::MaxValue,$ExpectedDaemonProcess=$null)
    if($PollMilliseconds -lt 1 -or $PollMilliseconds -gt 5000){throw "readiness poll interval is invalid"}
    if($Deadline -eq [datetime]::MaxValue){$deadline=[DateTime]::UtcNow.AddSeconds($TimeoutSeconds)}else{$deadline=$Deadline}
    $lastLocal="LOCAL_MCP_RESPONSE_UNAVAILABLE";$lastRuntime="RUNTIME_STATUS_UNAVAILABLE"
    while([DateTime]::UtcNow -lt $deadline){
        $local=Get-LocalMcpReadiness -LocalMcp $LocalMcp -Canonical $Canonical -ExpectedDaemonProcess $ExpectedDaemonProcess
        $runtime=Get-OfficialRuntimeVerification -Root $Root -TimeoutSeconds 5 -Alias $Alias -ClientPath $ClientPath -Deadline $deadline
        $lastLocal=[string]$local.Gate;$lastRuntime=[string]$runtime.Gate
        if($local.Ready -and $runtime.Verified){return $true}
        Start-Sleep -Milliseconds $PollMilliseconds
    }
    throw "full stack did not reach verified readiness; local_mcp=$lastLocal; official_runtime=$lastRuntime"
}

function Get-RedactedReadinessFailureGate {
    param([string]$Message)
    # Local readiness is independently meaningful. Preserve the bounded local
    # failure class even when the externally owned runtime is healthy.
    if($Message -match 'local_mcp=(LOCAL_MCP_[A-Z0-9_]+)'){
        $gate=[string]$Matches[1]
        [void](New-LocalMcpReadinessResult -Ready $false -Gate $gate)
        return $gate
    }
    if($Message -match 'local_mcp=PENDING'){return 'LOCAL_MCP_RESPONSE_UNAVAILABLE'}
    if($Message -match 'official_runtime=([A-Z0-9_]+)'){
        $gate = [string]$Matches[1]
        [void](New-OfficialRuntimeVerificationResult -Verified $false -Gate $gate)
        return $gate
    }
    return 'RUNTIME_STATUS_TRANSIENT'
}

function Save-RedactedRecoveryEvidence {
    param([string]$Root,[string]$State,[string]$Gate)
    if($State -notin @('CONNECTED_VERIFIED','TRANSPORT_VERIFICATION_FAILED')){throw 'recovery evidence state is invalid'}
    [void](New-OfficialRuntimeVerificationResult -Verified ($State -eq 'CONNECTED_VERIFIED') -Gate $Gate)
    $rootPath = Resolve-CatDeskRecoveryRoot $Root
    $control = Join-Path $rootPath '.catdesk'
    if(-not(Test-Path -LiteralPath $control)){New-Item -ItemType Directory -Path $control -ErrorAction Stop|Out-Null}
    $controlItem=Get-Item -LiteralPath $control -Force -ErrorAction Stop
    if(-not $controlItem.PSIsContainer -or ($controlItem.Attributes -band [IO.FileAttributes]::ReparsePoint)){throw 'recovery evidence control directory is invalid'}
    $directory=Join-Path $control 'recovery-evidence'
    if(-not(Test-Path -LiteralPath $directory)){New-Item -ItemType Directory -Path $directory -ErrorAction Stop|Out-Null}
    $directoryItem=Get-Item -LiteralPath $directory -Force -ErrorAction Stop
    if(-not $directoryItem.PSIsContainer -or ($directoryItem.Attributes -band [IO.FileAttributes]::ReparsePoint)){throw 'recovery evidence directory is invalid'}
    $path=Join-Path $directory 'latest.json'
    if(Test-Path -LiteralPath $path){$item=Get-Item -LiteralPath $path -Force -ErrorAction Stop;if($item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)){throw 'recovery evidence file is invalid'}}
    $record=[ordered]@{schemaVersion=1;observedAtUtc=[DateTime]::UtcNow.ToString('o');state=$State;gate=$Gate}
    Write-CatDeskAtomicUtf8 -Path $path -Text ($record|ConvertTo-Json -Compress)
    $item=Get-Item -LiteralPath $path -Force -ErrorAction Stop
    if($item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -or $item.Length -gt 1024){throw 'recovery evidence write validation failed'}
}

function Invoke-InterruptedPromotionPairRecovery([string]$Root) {
    $transaction = Join-Path $Root '.catdesk\promotion-recovery\promotion-transaction.json'
    if (-not (Test-Path -LiteralPath $transaction -PathType Leaf)) { return $false }
    $promotion = Resolve-TrustedBootstrapHelperPath -Path (Join-Path $Root 'scripts\promote-reviewed-catdesk-build.ps1') -FailureMessage "promotion recovery helper identity is invalid"
    # BuildPath is syntactically mandatory for the promotion helper, but an
    # interrupted transaction is resolved before that path is consulted.
    # T-0342: this mutation-capable recovery helper is a native child boundary,
    # not an in-process extension of recover. Give it the same trusted interpreter,
    # bounded output, bounded drain, and parent-side deadline as the other
    # one-command recovery helpers.
    $placeholder = Join-Path $Root 'target\release\catdesk.exe'
    $powerShell = Resolve-TrustedWindowsPowerShellPath
    $result = Invoke-BoundedRecoveryHelper -FileName $powerShell -Arguments @(
        '-NoProfile',
        '-NonInteractive',
        '-ExecutionPolicy', 'Bypass',
        '-File', $promotion,
        '-BuildPath', $placeholder,
        '-Workspace', $Root,
        '-Execute',
        '-RecoveryRollbackOnly'
    ) -TimeoutMilliseconds $script:MaxInterruptedPromotionRecoveryMilliseconds -FailureMessage "interrupted promotion recovery helper failed"
    $text = [string]$result.Stdout
    if ($text.Length -gt 4096) { throw "promotion recovery result is oversized" }
    if ($text -notmatch '"(?:State|state)"\s*:\s*"RECOVERED_PRIOR_CANONICAL"') {
        throw "interrupted promotion recovery was not proven"
    }
    return $true
}

function Get-CanonicalReleaseRecoveryAssessment([string]$Root, [string]$ExpectedHash) {
    # This is deliberately read-only.  It proves only that the existing
    # persistent LKG authority can repair a broken canonical pair; it never
    # invokes the promotion helper, starts a daemon, or touches the external
    # runtime.  An interrupted promotion is executed only by the existing
    # bounded recovery route, which can fall back to this exact LKG authority.
    try {
        $trusted = Get-CatDeskLastKnownGoodRelease -Root $Root
    } catch {
        return [pscustomobject]@{ State = (Get-CatDeskLkgAuthorityFailure -Root $Root); RecoverySource = '' }
    }
    if ($ExpectedHash -and $ExpectedHash.ToLowerInvariant() -ne $trusted.Hash) {
        return [pscustomobject]@{ State = 'RECOVERY_RELEASE_AUTHORITY_REQUIRED'; RecoverySource = '' }
    }
    return [pscustomobject]@{ State = 'RECOVERY_READY'; RecoverySource = 'LAST_KNOWN_GOOD' }
}

function Stop-CanonicalPathCatDeskForReleaseRepair([string]$Root, [string]$Config) {
    $local=Get-ConfiguredLocalMcp -Path $Config
    $canonicalPath=[IO.Path]::GetFullPath((Join-Path $Root 'target\release\catdesk.exe'))
    if(-not(Test-Path -LiteralPath $canonicalPath -PathType Leaf)){return $false}
    $currentHash=Get-CatDeskRecoverySha256 $canonicalPath
    $observedCanonical=[pscustomobject]@{Path=$canonicalPath;Sha256=$currentHash}
    $listener=Get-LoopbackCatDeskListener -Port $local.Port -Canonical $observedCanonical
    if($null -eq $listener){return $false}
    if(-not $listener.MatchesCanonical){throw "release repair listener does not own the canonical path and bytes"}
    return (Stop-CatDeskListenerProcessForRecovery -LocalMcp $local -Canonical $observedCanonical -Candidate $listener -FailureMessage "canonical CatDesk did not stop for release repair")
}

function Repair-CanonicalReleaseForRecovery([string]$Root, [string]$Config, [string]$ExpectedHash) {
    try {
        if (Invoke-InterruptedPromotionPairRecovery -Root $Root) {
            return [pscustomobject]@{ Source = 'INTERRUPTED_PROMOTION'; RestoredBinary = $true; RestoredManifest = $true }
        }
    } catch {
        # The interrupted-promotion helper is not recovery authority by itself.
        # If it cannot prove completion, continue only with the independently
        # validated persistent LKG pair below.  Missing or ambiguous LKG still
        # fails closed through the existing bounded categories.
    }
    try { $trusted = Get-CatDeskLastKnownGoodRelease -Root $Root } catch { throw (Get-CatDeskLkgAuthorityFailure -Root $Root) }
    if ($ExpectedHash -and $ExpectedHash.ToLowerInvariant() -ne $trusted.Hash) {
        throw "trusted release recovery snapshot does not match requested hash"
    }
    $canonicalBinary = Join-Path $Root 'target\release\catdesk.exe'
    $actualHash = $null
    if (Test-Path -LiteralPath $canonicalBinary -PathType Leaf) {
        try { $actualHash = Get-CatDeskRecoverySha256 $canonicalBinary } catch { $actualHash = $null }
    }
    if ($actualHash -ne $trusted.Hash) {
        [void](Stop-CanonicalPathCatDeskForReleaseRepair -Root $Root -Config $Config)
    }
    $restored = Restore-CatDeskCanonicalFromLastKnownGood -Root $Root -ExpectedHash $ExpectedHash
    return [pscustomobject]@{ Source = 'LAST_KNOWN_GOOD'; RestoredBinary = $restored.RestoredBinary; RestoredManifest = $restored.RestoredManifest; Hash = $restored.Hash }
}

function Invoke-CanonicalStackBootstrap {
    $root=(Resolve-Path -LiteralPath $Workspace -ErrorAction Stop).Path;if($ReadyTimeoutSeconds -lt 5 -or $ReadyTimeoutSeconds -gt 180){throw "readiness timeout is invalid"}
    $releaseRestored=$false;$releaseRecoverySource=''
    try {
        $canonical=Get-CanonicalCatDeskIdentity -Root $root -ExpectedHash $ExpectedBuildSha256 -FingerprintPath $BuildFingerprintPath
    } catch {
        if($Mode -eq "recover" -and -not $Execute){
            $assessment = Get-CanonicalReleaseRecoveryAssessment -Root $root -ExpectedHash $ExpectedBuildSha256
            Write-RedactedStatus $assessment.State "trusted reviewed release recovery assessment" $assessment.RecoverySource
            return
        }
        if($Mode -ne "recover"){throw}
        try {
            $recovery=Repair-CanonicalReleaseForRecovery -Root $root -Config $ConfigPath -ExpectedHash $ExpectedBuildSha256
            $canonical=Get-CanonicalCatDeskIdentity -Root $root -ExpectedHash $ExpectedBuildSha256 -FingerprintPath $BuildFingerprintPath
            $releaseRestored=$true;$releaseRecoverySource=[string]$recovery.Source
        } catch {
            $authorityState = if ($_.Exception.Message -eq 'LKG_AUTHORITY_MISSING') { 'LKG_AUTHORITY_MISSING' } elseif ($_.Exception.Message -eq 'LKG_AUTHORITY_AMBIGUOUS_OR_DAMAGED') { 'LKG_AUTHORITY_AMBIGUOUS_OR_DAMAGED' } else { 'RECOVERY_RELEASE_AUTHORITY_REQUIRED' }
            Write-RedactedStatus $authorityState "trusted reviewed release recovery was unavailable or unproven"
            return
        }
    }
    $localMcp=Get-ConfiguredLocalMcp -Path $ConfigPath;$runtimeAlias=Get-ConfiguredRuntimeAlias -Path $ConfigPath;$client=Find-TunnelClient -ExplicitPath $TunnelClientPath;if(-not $client){throw "official runtime client unavailable"};$setup=Join-Path $root "scripts\setup-secure-mcp.ps1";$diskMode=Read-OfficialRuntimeProcessMode -Path $ConfigPath;$existing=Get-LoopbackCatDeskListener -Port $localMcp.Port -Canonical $canonical;$launched=$false
    Test-CodexOnDemandPrerequisites
    $wakeReady=Test-WakeBridgeRuntime -Root $root
    if($Mode -eq "plan"){
        if(-not $wakeReady){Write-RedactedStatus "WAKE_RUNTIME_NOT_READY" "redacted preflight";return}
        if($diskMode -eq "external_foreground"){Write-RedactedStatus "ORDERED_MIGRATION_REQUIRED" "redacted preflight";return}
        if($diskMode -ne "official_runtime"){Write-RedactedStatus "OFFICIAL_RUNTIME_REQUIRED" "redacted preflight";return}
        if($existing -and -not $existing.MatchesCanonical){Write-RedactedStatus "CANONICAL_RELOAD_REQUIRED" "redacted preflight";return}
        $localPlan=Get-LocalMcpReadiness -LocalMcp $localMcp -Canonical $canonical
        if(-not $localPlan.Ready){Write-RedactedStatus "LOCAL_MCP_NOT_READY" "redacted preflight" "" ([string]$localPlan.Gate);return}
        $runtimePlan=Get-OfficialRuntimeVerification -Root $root -TimeoutSeconds 5 -Alias $runtimeAlias -ClientPath $client
        if(-not $runtimePlan.Verified){Write-RedactedStatus "OFFICIAL_RUNTIME_NOT_READY" "redacted preflight" "" ([string]$runtimePlan.Gate);return}
        Write-RedactedStatus "PREFLIGHT_READY" "local MCP and official runtime verified; Codex app-server remains on-demand"
        return
    }
    if(-not $Execute){throw "recovery requires -Execute after a successful plan"};if($diskMode -ne "official_runtime" -and $diskMode -ne "external_foreground"){throw "official runtime configuration is unavailable or unverified"}
    if(-not $wakeReady){Invoke-WakeBridgeRuntimeRepair -Root $root;if(-not(Test-WakeBridgeRuntime -Root $root)){throw "wake runtime remains unavailable after bounded repair"}}
    # A daemon can remain alive after losing its listener.  Previously that shape
    # looked identical to "no daemon", so recovery launched a second copy that
    # could not converge while the stale canonical process still held runtime
    # ownership.  Recover only the exact unambiguous canonical daemon case; a
    # foreign or multiple daemon process remains fail-closed.
    if($null -eq $existing){[void](Stop-StaleCanonicalCatDeskDaemonForRecovery -Canonical $canonical -LocalMcp $localMcp);$existing=Get-LoopbackCatDeskListener -Port $localMcp.Port -Canonical $canonical}
    $mustRestart=($null -ne $existing -and (-not $existing.MatchesCanonical -or $diskMode -eq "external_foreground"));if($mustRestart){[void](Stop-CatDeskListenerProcessForRecovery -LocalMcp $localMcp -Canonical $canonical -Candidate $existing -FailureMessage "verified CatDesk did not stop");$afterStop=Get-LoopbackCatDeskListener -Port $localMcp.Port -Canonical $canonical;if($null -ne $afterStop){throw "CatDesk listener replacement appeared during recovery mutation"};$existing=$null}
    if($diskMode -eq "external_foreground"){Invoke-OrderedOfficialRuntimeMigration -SetupScript $setup -Path $ConfigPath;if((Read-OfficialRuntimeProcessMode -Path $ConfigPath) -ne "official_runtime"){throw "official runtime migration was not durable"}}
    # A canonical listener can remain alive while its MCP request path stops
    # answering. Distinguish that downstream failure from a tunnel failure.
    # Only the exact timeout + independently healthy official-runtime shape may
    # recycle the pinned local daemon; no tunnel connect/stop/create operation is
    # performed here. Every other local failure remains fail-closed/readiness-only.
    if($null -ne $existing -and $existing.MatchesCanonical -and $diskMode -eq "official_runtime"){
        $localBeforeRecovery=Get-LocalMcpReadiness -LocalMcp $localMcp -Canonical $canonical
        if(-not $localBeforeRecovery.Ready -and [string]$localBeforeRecovery.Gate -eq 'LOCAL_MCP_RESPONSE_TIMEOUT'){
            $runtimeBeforeLocalRecovery=Get-OfficialRuntimeVerification -Root $root -TimeoutSeconds 5 -Alias $runtimeAlias -ClientPath $client
            if($runtimeBeforeLocalRecovery.Verified -and [string]$runtimeBeforeLocalRecovery.Gate -eq 'READY'){
                [void](Stop-CatDeskListenerProcessForRecovery -LocalMcp $localMcp -Canonical $canonical -Candidate $existing -FailureMessage "timed-out canonical CatDesk MCP did not stop")
                $afterLocalRecoveryStop=Get-LoopbackCatDeskListener -Port $localMcp.Port -Canonical $canonical
                if($null -ne $afterLocalRecoveryStop){throw "CatDesk listener replacement appeared during local MCP timeout recovery"}
                $existing=$null
            }
        }
    }
    # Prerequisite repair/migration phases are independently bounded above. Start
    # the final convergence budget only after they complete so one recover command
    # retains the full readiness window even after a long but successful repair.
    $readinessDeadline=[DateTime]::UtcNow.AddSeconds($ReadyTimeoutSeconds)
    $launchedDaemonProcess=$null
    if(-not $existing){
        $launchedDaemonProcess=Start-Process -FilePath $canonical.Path -ArgumentList "--catdesk-daemon" -WorkingDirectory $root -WindowStyle Hidden -PassThru
        # Pin the exact child immediately. Readiness below must bind to this
        # process instance rather than accepting a canonical same-path racer.
        $null=$launchedDaemonProcess.Handle
        $launched=$true
    }
    # The official runtime is externally owned. A transient status/health
    # observation does not prove the canonical daemon is bad, so never recycle a
    # verified existing daemon merely to retry the external-runtime probe. The
    # bounded convergence loop below retries both gates and reports its final
    # fixed reason without taking tunnel ownership.
    try {
        Wait-FullStackReadiness -Root $root -LocalMcp $localMcp -TimeoutSeconds $ReadyTimeoutSeconds -Canonical $canonical -Alias $runtimeAlias -ClientPath $client -Deadline $readinessDeadline -ExpectedDaemonProcess $launchedDaemonProcess|Out-Null
    } catch {
        $gate=Get-RedactedReadinessFailureGate -Message ([string]$_.Exception.Message)
        Save-RedactedRecoveryEvidence -Root $root -State 'TRANSPORT_VERIFICATION_FAILED' -Gate $gate
        Write-RedactedStatus 'TRANSPORT_VERIFICATION_FAILED' 'bounded local/runtime readiness did not converge' '' $gate
        return
    }
    # Core recovery checkpoint: local/runtime health may re-use an already-matching
    # reviewed LKG, but operational health alone must never mint or advance rollback
    # authority. A healthy valid canonical pair remains restartable even when LKG
    # authority is missing; broken-pair recovery still requires reviewed authority.
    $operationalPair=Get-CatDeskCanonicalPairEvidence $root
    try { [void](Save-CatDeskLastKnownGoodRelease -Root $root -Canonical $operationalPair -Source 'operational_verified') } catch {}
    # Once the independent wake owner is selected, public recovery also heals
    # the separately installed WakeHost. This does not make WakeHost a CatDesk
    # daemon/tunnel child: the installed host owns its own lifetime and durable
    # per-user state, while recovery only ensures it is running.
    [void](Start-CatDeskWakeHost -Root $root)
    Save-RedactedRecoveryEvidence -Root $root -State 'CONNECTED_VERIFIED' -Gate 'READY'
    # A GUI failure must not alter the verified daemon/tunnel result.  The UI
    # is a separate observer/control surface that can be relaunched later.
    try { [void](Start-CatDeskBinagotchyGui -Canonical $canonical -Root $root) } catch {}
    Write-RedactedStatus "CONNECTED_VERIFIED" "local MCP and official runtime verified" $releaseRecoverySource
}

if($MyInvocation.InvocationName -ne '.'){Invoke-CanonicalStackBootstrap}
