[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][int]$OldPid,
    [Parameter(Mandatory = $true)][string]$ExpectedOldProcessStartedAtUtc,
    [Parameter(Mandatory = $true)][string]$ExpectedOldProcessPath,
    [Parameter(Mandatory = $true)][string]$BuildPath,
    [Parameter(Mandatory = $true)][string]$Workspace,
    [Parameter(Mandatory = $true)][int]$McpPort,
    [int]$StartupDelaySeconds = 3,
    [int]$ExitTimeoutSeconds = 15,
    [int]$ReadyTimeoutSeconds = 90
)

$ErrorActionPreference = "Stop"

function Write-HandoffState {
    param([hashtable]$State)
    $stateDir = Join-Path $resolvedWorkspace ".catdesk\restart-handoff"
    New-Item -ItemType Directory -Force -Path $stateDir | Out-Null
    $target = Join-Path $stateDir "latest.json"
    $temporary = Join-Path $stateDir ("latest-{0}.tmp" -f [guid]::NewGuid().ToString("N"))
    $State | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $temporary -Encoding UTF8
    Move-Item -LiteralPath $temporary -Destination $target -Force
}

function Test-PinnedRestartTargetIdentity {
    param(
        [Parameter(Mandatory = $true)]$Process,
        [Parameter(Mandatory = $true)][string]$ExpectedStartedAtUtc,
        [Parameter(Mandatory = $true)][string]$ExpectedPath
    )

    try {
        # Force acquisition of the OS process handle before observing identity.
        # StartTime/Path are then tied to this exact Process object rather than
        # to a later PID-only reacquisition.
        $null = $Process.Handle
        if ($Process.ProcessName -ne "catdesk" -or -not $Process.Path) { return $false }
        $expectedStarted = [DateTimeOffset]::Parse($ExpectedStartedAtUtc).ToUniversalTime()
        $actualStarted = ([DateTimeOffset]$Process.StartTime).ToUniversalTime()
        if ($actualStarted.UtcDateTime.Ticks -ne $expectedStarted.UtcDateTime.Ticks) { return $false }
        $actualPath = (Resolve-Path -LiteralPath $Process.Path -ErrorAction Stop).Path
        $expectedResolvedPath = (Resolve-Path -LiteralPath $ExpectedPath -ErrorAction Stop).Path
        return $actualPath.Equals($expectedResolvedPath, [StringComparison]::OrdinalIgnoreCase)
    } catch {
        return $false
    }
}

function Send-ControlComputerSelection {
    param([int]$ProcessId)

    if (-not ("CatDeskRestartConsoleInput" -as [type])) {
        Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class CatDeskRestartConsoleInput {
    [DllImport("kernel32.dll", SetLastError=true)] public static extern bool AttachConsole(uint processId);
    [DllImport("kernel32.dll", SetLastError=true)] public static extern bool FreeConsole();
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)] public static extern IntPtr CreateFile(string name, uint access, uint share, IntPtr securityAttributes, uint creationDisposition, uint flags, IntPtr templateFile);
    [DllImport("kernel32.dll", SetLastError=true)] public static extern bool CloseHandle(IntPtr handle);
    [DllImport("kernel32.dll", SetLastError=true)] public static extern bool WriteConsoleInput(IntPtr handle, InputRecord[] buffer, uint length, out uint written);
    [StructLayout(LayoutKind.Sequential)] public struct InputRecord { public ushort EventType; public KeyEventRecord KeyEvent; }
    [StructLayout(LayoutKind.Sequential, CharSet=CharSet.Unicode)] public struct KeyEventRecord { public bool KeyDown; public ushort RepeatCount; public ushort VirtualKeyCode; public ushort VirtualScanCode; public char UnicodeChar; public uint ControlKeyState; }
    public static InputRecord Key(bool down, ushort virtualKeyCode, char character) { return new InputRecord { EventType=1, KeyEvent=new KeyEventRecord { KeyDown=down, RepeatCount=1, VirtualKeyCode=virtualKeyCode, UnicodeChar=character } }; }
}
'@
    }

    $deadline = [DateTime]::UtcNow.AddSeconds(15)
    do {
        [void][CatDeskRestartConsoleInput]::FreeConsole()
        $inputHandle = [IntPtr]::Zero
        try {
            if (-not [CatDeskRestartConsoleInput]::AttachConsole([uint32]$ProcessId)) {
                continue
            }
            $inputHandle = [CatDeskRestartConsoleInput]::CreateFile(
                "CONIN$",
                [Convert]::ToUInt32("C0000000", 16),
                3,
                [IntPtr]::Zero,
                3,
                0,
                [IntPtr]::Zero
            )
            if ($inputHandle -eq [IntPtr](-1)) { continue }
            $events = [CatDeskRestartConsoleInput+InputRecord[]]@(
                [CatDeskRestartConsoleInput]::Key($true, 0x31, '1'),
                [CatDeskRestartConsoleInput]::Key($false, 0x31, '1'),
                [CatDeskRestartConsoleInput]::Key($true, 0x0D, [char]13),
                [CatDeskRestartConsoleInput]::Key($false, 0x0D, [char]13)
            )
            [uint32]$written = 0
            if ([CatDeskRestartConsoleInput]::WriteConsoleInput($inputHandle, $events, [uint32]$events.Length, [ref]$written) -and $written -eq $events.Length) {
                return
            }
        } finally {
            if ($inputHandle -ne [IntPtr]::Zero -and $inputHandle -ne [IntPtr](-1)) {
                [void][CatDeskRestartConsoleInput]::CloseHandle($inputHandle)
            }
            [void][CatDeskRestartConsoleInput]::FreeConsole()
        }
        Start-Sleep -Milliseconds 250
    } while ([DateTime]::UtcNow -lt $deadline)

    throw "Replacement CatDesk console did not become ready for Control Computer selection."
}

function Test-ReplacementMcpReady {
    param([int]$ProcessId)
    try {
        # During startup Get-NetTCPConnection can surface an empty-query
        # failure under the worker's Stop preference. That is a retryable
        # readiness sample, not a failed handoff.
        $listener = @(Get-NetTCPConnection -State Listen -LocalPort $McpPort -ErrorAction Stop |
            Where-Object { $_.OwningProcess -eq $ProcessId -and $_.LocalAddress -in @("127.0.0.1", "::1") })
        if ($listener.Count -eq 0) { return $false }
        $response = Invoke-WebRequest -UseBasicParsing -Uri "http://127.0.0.1:$McpPort/" -TimeoutSec 3
        return $response.StatusCode -eq 200
    } catch {
        return $false
    }
}

$resolvedBuild = (Resolve-Path -LiteralPath $BuildPath -ErrorAction Stop).Path
$resolvedWorkspace = (Resolve-Path -LiteralPath $Workspace -ErrorAction Stop).Path
$buildHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $resolvedBuild).Hash.ToLowerInvariant()
$oldPorts = @()
$stage = "startup-delay"

try {
    if ($env:CATDESK_CODEX_CLI_EXECUTABLE -and -not (Test-Path -LiteralPath $env:CATDESK_CODEX_CLI_EXECUTABLE -PathType Leaf)) {
        throw "Replacement environment has an invalid CATDESK_CODEX_CLI_EXECUTABLE path."
    }
    if ($env:CATDESK_CODEX_HOME -and -not (Test-Path -LiteralPath $env:CATDESK_CODEX_HOME -PathType Container)) {
        throw "Replacement environment has an invalid operator-local CATDESK_CODEX_HOME directory."
    }
    Start-Sleep -Seconds $StartupDelaySeconds
    $stage = "validate-target"
    $oldProcess = Get-Process -Id $OldPid -ErrorAction Stop
    if (-not (Test-PinnedRestartTargetIdentity -Process $oldProcess -ExpectedStartedAtUtc $ExpectedOldProcessStartedAtUtc -ExpectedPath $ExpectedOldProcessPath)) {
        throw "Target process instance changed before restart mutation; refusing replacement."
    }
    $oldPorts = @(Get-NetTCPConnection -State Listen -OwningProcess $OldPid -ErrorAction SilentlyContinue |
        Where-Object { $_.LocalAddress -in @("127.0.0.1", "::1") } |
        Select-Object -ExpandProperty LocalPort -Unique)

    # Only the exact pinned CatDesk Process object selected during launcher
    # preflight is stopped. PID reuse after validation cannot redirect Kill or
    # WaitForExit to another process. The external tunnel-client is untouched.
    $stage = "stop-old-catdesk"
    try {
        $oldProcess.Kill()
    } catch [InvalidOperationException] {
        # The exact pinned process already exited; never reacquire by PID.
    }
    if (-not $oldProcess.HasExited -and -not $oldProcess.WaitForExit($ExitTimeoutSeconds * 1000)) {
        throw "Exact CatDesk restart target did not exit before timeout."
    }

    $stage = "launch-updated-catdesk"
    $newProcess = Start-Process -FilePath $resolvedBuild -ArgumentList @("--auto-start-computer") -WorkingDirectory $resolvedWorkspace -WindowStyle Hidden -PassThru
    $null = $newProcess.Handle

    $stage = "poll-local-mcp-readiness"
    $deadline = [DateTime]::UtcNow.AddSeconds($ReadyTimeoutSeconds)
    do {
        if ($newProcess.HasExited) {
            throw "Updated CatDesk process exited before readiness."
        }
        if (Test-ReplacementMcpReady -ProcessId $newProcess.Id) {
            if ($newProcess.HasExited) { throw "Updated CatDesk process exited during readiness validation." }
            $newProcessStartedAtUtc = $newProcess.StartTime.ToUniversalTime().ToString("o")
            Write-HandoffState -State ([ordered]@{
                schemaVersion = 3; status = "RECOVERED_PENDING_TRANSPORT_CHECK"; completedAtUtc = [DateTime]::UtcNow.ToString("o")
                oldPid = $OldPid; newPid = $newProcess.Id; newProcessStartedAtUtc = $newProcessStartedAtUtc; mcpPort = $McpPort; oldListenerPorts = @($oldPorts); workspacePath = $resolvedWorkspace; buildPath = $resolvedBuild; buildSha256 = $buildHash; stage = "complete"
                recovery = "Updated CatDesk local MCP is ready. Verify external Secure MCP reconnection and new control surfaces after ChatGPT reconnects."
            })
            exit 0
        }
        Start-Sleep -Milliseconds 500
    } while ([DateTime]::UtcNow -lt $deadline)

    throw "Updated CatDesk did not restore local MCP readiness before timeout."
} catch {
    Write-HandoffState -State ([ordered]@{
        schemaVersion = 3; status = "FAILED_OPERATOR_ATTENTION"; completedAtUtc = [DateTime]::UtcNow.ToString("o")
        oldPid = $OldPid; mcpPort = $McpPort; oldListenerPorts = @($oldPorts); workspacePath = $resolvedWorkspace; buildPath = $resolvedBuild; buildSha256 = $buildHash; stage = $stage
        recovery = "Restart handoff failed. Start the verified CatDesk build manually from the approved workspace and verify the existing Secure MCP tunnel reconnects; do not create or restart a replacement tunnel."
    })
    exit 1
}
