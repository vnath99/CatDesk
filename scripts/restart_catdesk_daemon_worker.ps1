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
    [int]$ReadyTimeoutSeconds = 90,
    [switch]$BackendOnly,
    [string]$ExpectedBuildSha256 = ''
)

$ErrorActionPreference = "Stop"

function Get-RestartSha256([string]$Path) {
    $sha=[Security.Cryptography.SHA256]::Create();$stream=[IO.File]::OpenRead($Path)
    try { return ([BitConverter]::ToString($sha.ComputeHash($stream)).Replace('-', '')).ToLowerInvariant() }
    finally { $stream.Dispose();$sha.Dispose() }
}

function Test-RestartBackendOnlySupport([string]$Path, [string]$ExpectedHash) {
    $process = $null
    try {
        if ((Get-RestartSha256 $Path) -cne $ExpectedHash) { return $false }
        $info = [Diagnostics.ProcessStartInfo]::new()
        $info.FileName = $Path; $info.Arguments = '--catdesk-daemon --backend-only --check-support'
        $info.UseShellExecute = $false; $info.CreateNoWindow = $true
        $info.RedirectStandardOutput = $true; $info.RedirectStandardError = $true
        $process = [Diagnostics.Process]::Start($info)
        $output = $process.StandardOutput.ReadToEndAsync(); $errorOutput = $process.StandardError.ReadToEndAsync()
        if (-not $process.WaitForExit(5000) -or -not $output.Wait(1000) -or -not $errorOutput.Wait(1000) -or $process.ExitCode -ne 0 -or $output.Result.Length -gt 1024 -or $errorOutput.Result.Length -ne 0) { return $false }
        $value = $output.Result | ConvertFrom-Json -ErrorAction Stop
        return ($value -isnot [array] -and @($value.PSObject.Properties).Count -eq 5 -and $value.schemaVersion -eq 1 -and $value.mode -ceq 'LOCAL_BACKEND_ONLY' -and $value.supported -is [bool] -and $value.supported -and $value.stateCreated -is [bool] -and -not $value.stateCreated -and $value.servicesStarted -is [bool] -and -not $value.servicesStarted -and (Get-RestartSha256 $Path) -ceq $ExpectedHash)
    } catch { return $false } finally {
        if ($null -ne $process) { if (-not $process.HasExited) { $process.Kill(); [void]$process.WaitForExit(5000) }; $process.Dispose() }
    }
}

function Get-RestartBackendOnlyContext([int]$Port) {
    if ($Port -ne 3200) { throw 'Backend-only handoff requires the fixed local port 3200.' }
    $token = [string]$env:CATDESK_MCP_AUTH_TOKEN
    if ($token -cnotmatch '\A[\x21-\x7E]{24,}\z') { throw 'Backend-only MCP authentication is unavailable.' }
    $path = if ($env:CATDESK_BACKEND_CONFIG_PATH) { $env:CATDESK_BACKEND_CONFIG_PATH } else { Join-Path ([Environment]::GetFolderPath('UserProfile')) '.catdesk\config.toml' }
    $path = (Resolve-Path -LiteralPath $path -ErrorAction Stop).Path
    $stream = [IO.File]::Open($path, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
    try {
        if ($stream.Length -lt 1 -or $stream.Length -gt 1048576) { throw 'Backend-only configuration is invalid.' }
        $text = [IO.File]::ReadAllText($path); $section=''; $sections=0; $bindHost=$null; $configuredPort=$null; $route=$null
        foreach ($line in [regex]::Split($text, "`r?`n")) {
            $trim=$line.Trim()
            if ($trim -match '^\[([A-Za-z0-9_.-]+)\]\s*(?:#.*)?$') { $section=$Matches[1]; if($section -eq 'mcp'){$sections++}; continue }
            if ($section -ne 'mcp') { continue }
            if ($trim -match '^bind_host\s*=\s*"([^"]+)"\s*(?:#.*)?$') { if($null -ne $bindHost){throw 'Duplicate backend bind host.'};$bindHost=$Matches[1];continue }
            if ($trim -match '^port\s*=\s*(\d+)\s*(?:#.*)?$') { if($null -ne $configuredPort){throw 'Duplicate backend port.'};$configuredPort=[int]$Matches[1];continue }
            if ($trim -match '^route_id\s*=\s*"([^"]+)"\s*(?:#.*)?$') { if($null -ne $route){throw 'Duplicate backend route.'};$route=$Matches[1] }
        }
        if ($sections -ne 1 -or $bindHost -cne '127.0.0.1' -or $configuredPort -ne $Port -or $route -cnotmatch '^[A-Za-z0-9_-]{24,96}$') { throw 'Backend-only local endpoint is invalid.' }
        return @{ Stream=$stream; Path=$path; Uri="http://127.0.0.1:$Port/$route/mcp"; Token=$token }
    } catch { $stream.Dispose(); throw }
}

function Test-RestartBackendOnlyMcp($Context) {
    try {
        $uri=[Uri]$Context.Uri
        if ($uri.Scheme -cne 'http' -or $uri.Host -cne '127.0.0.1' -or $uri.Port -ne $McpPort -or $uri.UserInfo -or $uri.Query -or $uri.Fragment -or $uri.AbsolutePath -cnotmatch '\A/[A-Za-z0-9_-]{24,96}/mcp\z' -or $Context.Token -cnotmatch '\A[\x21-\x7E]{24,}\z') { return $false }
        $request='{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}'
        $unauthorized=$false
        try { $probe=Invoke-WebRequest -UseBasicParsing -Uri $Context.Uri -Method Post -ContentType 'application/json' -Body $request -MaximumRedirection 0 -TimeoutSec 3; $unauthorized=($probe.StatusCode -eq 401) } catch { $unauthorized=($null -ne $_.Exception.Response -and [int]$_.Exception.Response.StatusCode -eq 401) }
        if (-not $unauthorized) { return $false }
        foreach ($method in @('initialize','tools/list')) {
            $params=if($method -eq 'initialize'){@{protocolVersion='2025-03-26';capabilities=@{};clientInfo=@{name='catdesk-backend-handoff';version='1'}}}else{@{}}
            $body=@{jsonrpc='2.0';id=2;method=$method;params=$params}|ConvertTo-Json -Depth 8 -Compress
            $response=Invoke-WebRequest -UseBasicParsing -Uri $Context.Uri -Method Post -Headers @{Authorization='Bearer '+$Context.Token} -ContentType 'application/json' -Body $body -MaximumRedirection 0 -TimeoutSec 3
            if ($response.StatusCode -ne 200 -or $response.Content.Length -gt 1048576) { return $false }
            $value=$response.Content|ConvertFrom-Json -ErrorAction Stop
            if ($value.error -or $null -eq $value.result -or ($method -eq 'initialize' -and $null -eq $value.result.serverInfo) -or ($method -eq 'tools/list' -and @($value.result.tools).Count -eq 0)) { return $false }
        }
        return $true
    } catch { return $false }
}

function Get-RestartLaunchArguments([bool]$LocalOnly) {
    if ($LocalOnly) { return @('--catdesk-daemon', '--backend-only') }
    return @('--auto-start-computer')
}

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
        if ($BackendOnly) { return Test-RestartBackendOnlyMcp $backendContext }
        $response = Invoke-WebRequest -UseBasicParsing -Uri "http://127.0.0.1:$McpPort/" -TimeoutSec 3
        return $response.StatusCode -eq 200
    } catch {
        return $false
    }
}

$resolvedBuild = (Resolve-Path -LiteralPath $BuildPath -ErrorAction Stop).Path
$resolvedWorkspace = (Resolve-Path -LiteralPath $Workspace -ErrorAction Stop).Path
$buildHash = Get-RestartSha256 $resolvedBuild
$oldPorts = @()
$stage = "startup-delay"
$backendContext=$null; $candidatePin=$null; $newProcess=$null; $replacementReady=$false

try {
    if ($BackendOnly) {
        $stage='backend-only-preflight'
        if ($McpPort -ne 3200) { throw 'Backend-only handoff requires the fixed local port 3200.' }
        if ($ExpectedBuildSha256 -cnotmatch '^[a-f0-9]{64}$' -or $buildHash -cne $ExpectedBuildSha256) { throw 'Backend-only target identity changed.' }
        $candidatePin=[IO.File]::Open($resolvedBuild,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::Read)
        if (-not (Test-RestartBackendOnlySupport -Path $resolvedBuild -ExpectedHash $ExpectedBuildSha256)) { throw 'Replacement does not support backend-only operation; old process was not stopped.' }
        $backendContext=Get-RestartBackendOnlyContext -Port $McpPort
    }
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
    if ($BackendOnly) {
        $info=[Diagnostics.ProcessStartInfo]::new();$info.FileName=$resolvedBuild;$info.Arguments=(Get-RestartLaunchArguments $true) -join ' ';$info.WorkingDirectory=$resolvedWorkspace;$info.UseShellExecute=$false;$info.CreateNoWindow=$true
        $info.EnvironmentVariables['WORKSPACE_ROOT']=$resolvedWorkspace;$info.EnvironmentVariables['CATDESK_BACKEND_CONFIG_PATH']=$backendContext.Path
        $newProcess=[Diagnostics.Process]::Start($info)
    } else {
        $newProcess = Start-Process -FilePath $resolvedBuild -ArgumentList @(Get-RestartLaunchArguments $false) -WorkingDirectory $resolvedWorkspace -WindowStyle Hidden -PassThru
    }
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
            $completeState=[ordered]@{
                schemaVersion = 3; status = "RECOVERED_PENDING_TRANSPORT_CHECK"; completedAtUtc = [DateTime]::UtcNow.ToString("o")
                oldPid = $OldPid; newPid = $newProcess.Id; newProcessStartedAtUtc = $newProcessStartedAtUtc; mcpPort = $McpPort; oldListenerPorts = @($oldPorts); workspacePath = $resolvedWorkspace; buildPath = $resolvedBuild; buildSha256 = $buildHash; stage = "complete"
                recovery = "Updated CatDesk local MCP is ready. Verify external Secure MCP reconnection and new control surfaces after ChatGPT reconnects."
            }
            if ($BackendOnly) { $completeState.schemaVersion=4; $completeState['profile']='LOCAL_BACKEND_ONLY'; $completeState['mcpAuthenticated']=$true }
            Write-HandoffState -State $completeState
            $replacementReady=$true
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
} finally {
    if ($BackendOnly -and -not $replacementReady -and $null -ne $newProcess -and -not $newProcess.HasExited) { $newProcess.Kill(); [void]$newProcess.WaitForExit(5000) }
    if ($null -ne $candidatePin) { $candidatePin.Dispose() }
    if ($null -ne $backendContext) { $backendContext.Stream.Dispose() }
}
