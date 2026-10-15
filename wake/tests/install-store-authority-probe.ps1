param([Parameter(Mandatory=$true)][string]$NativeHost, [switch]$EnableOwnerProofHost)
$ErrorActionPreference = 'Stop'
$probe = Join-Path (Split-Path -Parent $PSScriptRoot) 'check-install-store-authority.ps1'
$installer = Join-Path (Split-Path -Parent $PSScriptRoot) 'install.ps1'
$installerSource = [IO.File]::ReadAllText($installer)
$brokerHoldOffset = $installerSource.IndexOf(
    "if (-not `$BuildOnly) { throw 'WAKE_INSTALL_BROKER_UNPROVISIONED' }",
    [StringComparison]::Ordinal
)
$firstBuildOffset = $installerSource.IndexOf('& cargo build', [StringComparison]::Ordinal)
$buildOnlyReturnOffset = $installerSource.IndexOf('if ($BuildOnly) { return }', [StringComparison]::Ordinal)
$holdOffset = $installerSource.IndexOf('-RequireLiveRootProof', [StringComparison]::Ordinal)
$nativeHoldOffset = $installerSource.IndexOf('& $hostBuild install-authority-probe', [StringComparison]::Ordinal)
$firstStoreWriteOffset = $installerSource.IndexOf('$pinnedStore.PinVersions()', [StringComparison]::Ordinal)
if ($brokerHoldOffset -lt 0 -or $firstBuildOffset -le $brokerHoldOffset -or
    $buildOnlyReturnOffset -lt 0 -or $firstStoreWriteOffset -le $buildOnlyReturnOffset -or
    $holdOffset -lt 0 -or $holdOffset -ge $firstStoreWriteOffset -or
    $nativeHoldOffset -le $buildOnlyReturnOffset -or
    $nativeHoldOffset -ge $firstStoreWriteOffset) {
    throw 'TEST_INSTALLER_NATIVE_ROOT_HOLD_NOT_BEFORE_STORE_WRITE'
}
$temporaryParent = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
$fixture = Join-Path $temporaryParent ("catdesk-wake-install-authority-test-$([Guid]::NewGuid().ToString('N'))")
$fixture = [IO.Path]::GetFullPath($fixture)
if (-not $fixture.StartsWith($temporaryParent, [StringComparison]::OrdinalIgnoreCase) -or
    -not (Split-Path -Leaf $fixture).StartsWith('catdesk-wake-install-authority-test-')) {
    throw 'TEST_FIXTURE_PATH_INVALID'
}
$root = Join-Path $fixture 'CatDeskWake'
$otherRoot = Join-Path $fixture 'other\CatDeskWake'
$ready = Join-Path $fixture 'ready.txt'
$holderScript = Join-Path $fixture 'hold-lock.ps1'
$holder = $null
$otherHolder = $null
$proofHost = $null
$spoofPipeHost = $null
try {
    New-Item -ItemType Directory -Path $root -Force | Out-Null
    New-Item -ItemType Directory -Path $otherRoot -Force | Out-Null
    $lockPath = Join-Path $root 'host.lock'
    $otherLockPath = Join-Path $otherRoot 'host.lock'
    [IO.File]::WriteAllBytes($lockPath, [byte[]]@())
    [IO.File]::WriteAllBytes($otherLockPath, [byte[]]@())
    [IO.File]::WriteAllText($holderScript, @'
param([string]$LockPath, [string]$RootPath, [string]$ReadyPath,
    [switch]$OpenTwice, [switch]$HoldRoot, [switch]$DoNotLock)
$ErrorActionPreference = 'Stop'
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public static class TestWakeRootPin {
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
    public static extern IntPtr CreateFile(string path, uint access, uint share,
        IntPtr security, uint disposition, uint flags, IntPtr template);
    [DllImport("kernel32.dll")]
    public static extern bool CloseHandle(IntPtr handle);
}
"@
$share = [IO.FileShare]::ReadWrite -bor [IO.FileShare]::Delete
$stream = [IO.File]::Open($LockPath, [IO.FileMode]::Open, [IO.FileAccess]::ReadWrite, $share)
$second = $null
$rootHandle = [IntPtr]::Zero
try {
    if (-not $DoNotLock) { $stream.Lock(0, 1) }
    if ($HoldRoot) {
        $rootHandle = [TestWakeRootPin]::CreateFile($RootPath, 0, 3,
            [IntPtr]::Zero, 3, 0x02200000, [IntPtr]::Zero)
        if ($rootHandle -eq [IntPtr]::Zero -or $rootHandle -eq [IntPtr](-1)) {
            throw 'TEST_ROOT_PIN_UNAVAILABLE'
        }
    }
    if ($OpenTwice) {
        $second = [IO.File]::Open($LockPath, [IO.FileMode]::Open, [IO.FileAccess]::ReadWrite, $share)
    }
    [IO.File]::WriteAllText($ReadyPath, [string]$PID)
    Start-Sleep -Seconds 30
} finally {
    if ($rootHandle -ne [IntPtr]::Zero -and $rootHandle -ne [IntPtr](-1)) {
        [void][TestWakeRootPin]::CloseHandle($rootHandle)
    }
    if ($null -ne $second) { $second.Dispose() }
    if (-not $DoNotLock) { $stream.Unlock(0, 1) }
    $stream.Dispose()
}
'@)
    $holder = Start-Process -FilePath (Get-Command powershell.exe).Source `
        -ArgumentList @('-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass',
            '-File',"`"$holderScript`"",'-LockPath',"`"$lockPath`"",
            '-RootPath',"`"$root`"",'-ReadyPath',"`"$ready`"") `
        -WindowStyle Hidden -PassThru
    $deadline = [DateTime]::UtcNow.AddSeconds(10)
    while (-not (Test-Path -LiteralPath $ready -PathType Leaf)) {
        if ($holder.HasExited -or [DateTime]::UtcNow -ge $deadline) {
            throw 'TEST_HOLDER_UNAVAILABLE'
        }
        Start-Sleep -Milliseconds 50
    }
    $observed = & $probe -WakeRoot $root -HostProcessId $holder.Id
    if ($observed -notmatch '^[0-9a-f]{64}$') {
        throw 'TEST_MATCHING_LOCK_NOT_PROVEN'
    }
    try {
        & $probe -WakeRoot $root -HostProcessId $holder.Id -RequireLiveRootProof | Out-Null
        throw 'TEST_MATCHING_LOCK_AUTHORIZED_UNPROVEN_ROOT'
    } catch {
        if ($_.Exception.Message -notmatch 'WAKE_STORE_AUTHORITY_LIVE_ROOT_UNPROVEN') {
            throw
        }
    }
    $priorLocalAppDataForNative = $env:LOCALAPPDATA
    $priorTestParent = $env:CATDESK_WAKE_TEST_ONLY_ROOT_PARENT
    try {
        $env:LOCALAPPDATA = $fixture
        $env:CATDESK_WAKE_TEST_ONLY_ROOT_PARENT = $fixture
        $nativeWithoutRoot = & $NativeHost install-authority-probe $holder.Id $observed
        if ($LASTEXITCODE -eq 0 -or $nativeWithoutRoot -notmatch
            'WAKE_STORE_AUTHORITY_LIVE_ROOT_UNPROVEN') {
            throw 'TEST_NATIVE_ACCEPTED_LOCK_ONLY_HOLDER'
        }
        if (Test-Path -LiteralPath (Join-Path $root 'config.json')) {
            throw 'TEST_NATIVE_PROBE_MUTATED_STORE'
        }
        foreach ($unguarded in @(
            @('publish-reviewed-install','.staging-test','candidate'),
            @('activate-reviewed-install'),
            @('register-install'),
            @('init'),
            @('set-source',$fixture)
        )) {
            $result = & $NativeHost @unguarded
            if ($LASTEXITCODE -eq 0 -or $result -notmatch 'CLI_ARGUMENTS_INVALID') {
                throw "TEST_UNGUARDED_INSTALL_COMMAND_ACCEPTED: $unguarded $result"
            }
        }
    } finally {
        $env:LOCALAPPDATA = $priorLocalAppDataForNative
        $env:CATDESK_WAKE_TEST_ONLY_ROOT_PARENT = $priorTestParent
    }
    try {
        & $probe -WakeRoot $otherRoot -HostProcessId $holder.Id | Out-Null
        throw 'TEST_MISMATCH_WAS_ACCEPTED'
    } catch {
        if ($_.Exception.Message -notmatch 'WAKE_STORE_AUTHORITY_HELD_LOCK_NOT_FOUND') {
            throw
        }
    }
    Stop-Process -Id $holder.Id -Force
    [void]$holder.WaitForExit(5000)
    $holder.Dispose()
    $holder = $null
    Remove-Item -LiteralPath $ready -Force
    $holder = Start-Process -FilePath (Get-Command powershell.exe).Source `
        -ArgumentList @('-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass',
            '-File',"`"$holderScript`"",'-LockPath',"`"$lockPath`"",
            '-RootPath',"`"$root`"",'-ReadyPath',"`"$ready`"",'-HoldRoot') `
        -WindowStyle Hidden -PassThru
    $deadline = [DateTime]::UtcNow.AddSeconds(10)
    while (-not (Test-Path -LiteralPath $ready -PathType Leaf)) {
        if ($holder.HasExited -or [DateTime]::UtcNow -ge $deadline) {
            throw 'TEST_ROOT_HOLDER_UNAVAILABLE'
        }
        Start-Sleep -Milliseconds 50
    }
    $authoritative = & $probe -WakeRoot $root -HostProcessId $holder.Id -RequireLiveRootProof
    if ($authoritative -notmatch '^[0-9a-f]{64}$') {
        throw 'TEST_LIVE_ROOT_NOT_PROVEN'
    }
    try {
        [CatDeskWakeInstallStoreAuthority]::Pin($root, ('0' * 64)) | Out-Null
        throw 'TEST_PIN_ACCEPTED_WRONG_FINGERPRINT'
    } catch {
        if ($_.Exception.Message -notmatch 'WAKE_STORE_AUTHORITY_PIN_MISMATCH') { throw }
    }
    $pinned = [CatDeskWakeInstallStoreAuthority]::Pin($root, $authoritative)
    try {
        $pinned.PinVersions()
        $pinned.Verify()
        try {
            Rename-Item -LiteralPath $root -NewName 'CatDeskWake-retired' -ErrorAction Stop
            Rename-Item -LiteralPath (Join-Path $fixture 'CatDeskWake-retired') `
                -NewName 'CatDeskWake' -ErrorAction Stop
            throw 'TEST_PIN_PERMITTED_ROOT_RENAME'
        } catch {
            if ($_.Exception.Message -eq 'TEST_PIN_PERMITTED_ROOT_RENAME') { throw }
        }
        $pinned.Verify()
    } finally { $pinned.Dispose() }
    $priorLocalAppDataForNative = $env:LOCALAPPDATA
    $priorTestParent = $env:CATDESK_WAKE_TEST_ONLY_ROOT_PARENT
    try {
        $env:LOCALAPPDATA = $fixture
        $env:CATDESK_WAKE_TEST_ONLY_ROOT_PARENT = $fixture
        $objectProof = & $NativeHost install-object-probe $holder.Id $authoritative
        if ($LASTEXITCODE -ne 0 -or $objectProof -notmatch '"objectMatch":true' -or
            $objectProof -notmatch '"lockOwnerProven":false') {
            throw "TEST_NATIVE_OBJECT_PROOF_FAILED: $objectProof"
        }
        $ownerProof = & $NativeHost install-authority-probe $holder.Id $authoritative
        if ($LASTEXITCODE -eq 0 -or $ownerProof -notmatch
            'WAKE_STORE_AUTHORITY_LOCK_OWNER_UNPROVEN') {
            throw "TEST_NATIVE_OWNER_HOLD_MISSING: $ownerProof"
        }
        $wrongFingerprint = & $NativeHost install-init $holder.Id ('0' * 64)
        if ($LASTEXITCODE -eq 0 -or $wrongFingerprint -notmatch
            'WAKE_STORE_AUTHORITY_FINGERPRINT_MISMATCH' -or
            (Test-Path -LiteralPath (Join-Path $root 'config.json'))) {
            throw "TEST_NATIVE_MUTATED_ON_WRONG_FINGERPRINT: $wrongFingerprint"
        }
        $nativeInit = & $NativeHost install-init $holder.Id $authoritative
        if ($LASTEXITCODE -eq 0 -or $nativeInit -notmatch
            'WAKE_STORE_AUTHORITY_LOCK_OWNER_UNPROVEN' -or
            (Test-Path -LiteralPath (Join-Path $root 'config.json'))) {
            throw "TEST_GUARDED_INIT_BYPASSED_OWNER_HOLD: $nativeInit"
        }
    } finally {
        $env:LOCALAPPDATA = $priorLocalAppDataForNative
        $env:CATDESK_WAKE_TEST_ONLY_ROOT_PARENT = $priorTestParent
    }
    Stop-Process -Id $holder.Id -Force
    [void]$holder.WaitForExit(5000)
    $holder.Dispose()
    $holder = $null
    Remove-Item -LiteralPath $ready -Force
    $otherReady = Join-Path $fixture 'other-ready.txt'
    $holder = Start-Process -FilePath (Get-Command powershell.exe).Source `
        -ArgumentList @('-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass',
            '-File',"`"$holderScript`"",'-LockPath',"`"$lockPath`"",
            '-RootPath',"`"$root`"",'-ReadyPath',"`"$ready`"",'-HoldRoot','-DoNotLock') `
        -WindowStyle Hidden -PassThru
    $deadline = [DateTime]::UtcNow.AddSeconds(10)
    while (-not (Test-Path -LiteralPath $ready -PathType Leaf)) {
        if ($holder.HasExited -or [DateTime]::UtcNow -ge $deadline) {
            throw 'TEST_SPLIT_HANDLE_HOLDER_UNAVAILABLE'
        }
        Start-Sleep -Milliseconds 50
    }
    $otherHolder = Start-Process -FilePath (Get-Command powershell.exe).Source `
        -ArgumentList @('-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass',
            '-File',"`"$holderScript`"",'-LockPath',"`"$lockPath`"",
            '-RootPath',"`"$root`"",'-ReadyPath',"`"$otherReady`"") `
        -WindowStyle Hidden -PassThru
    $deadline = [DateTime]::UtcNow.AddSeconds(10)
    while (-not (Test-Path -LiteralPath $otherReady -PathType Leaf)) {
        if ($otherHolder.HasExited -or [DateTime]::UtcNow -ge $deadline) {
            throw 'TEST_SPLIT_LEASE_HOLDER_UNAVAILABLE'
        }
        Start-Sleep -Milliseconds 50
    }
    $splitFingerprint = & $probe -WakeRoot $root -HostProcessId $holder.Id -RequireLiveRootProof
    $spoofPipeScript = Join-Path $fixture 'spoof-owner-pipe.ps1'
    $spoofPipeReady = Join-Path $fixture 'spoof-owner-pipe-ready.txt'
    [IO.File]::WriteAllText($spoofPipeScript, @'
param([string]$PipeName, [string]$ReadyPath)
$ErrorActionPreference = 'Stop'
$pipe = [IO.Pipes.NamedPipeServerStream]::new(
    $PipeName, [IO.Pipes.PipeDirection]::InOut, 1,
    [IO.Pipes.PipeTransmissionMode]::Byte, [IO.Pipes.PipeOptions]::None
)
try {
    [IO.File]::WriteAllText($ReadyPath, [string]$PID)
    $pipe.WaitForConnection()
    Start-Sleep -Seconds 10
} finally { $pipe.Dispose() }
'@)
    $spoofPipeName = "CatDeskWakeAuthorityV1-$($holder.Id)-$($splitFingerprint.Substring(0,24))"
    $spoofPipeHost = Start-Process -FilePath (Get-Command powershell.exe).Source `
        -ArgumentList @('-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass',
            '-File',"`"$spoofPipeScript`"",'-PipeName',$spoofPipeName,
            '-ReadyPath',"`"$spoofPipeReady`"") `
        -WindowStyle Hidden -PassThru
    $deadline = [DateTime]::UtcNow.AddSeconds(10)
    while (-not (Test-Path -LiteralPath $spoofPipeReady -PathType Leaf)) {
        if ($spoofPipeHost.HasExited -or [DateTime]::UtcNow -ge $deadline) {
            throw 'TEST_SPOOF_PIPE_UNAVAILABLE'
        }
        Start-Sleep -Milliseconds 50
    }
    $priorLocalAppDataForNative = $env:LOCALAPPDATA
    $priorTestParent = $env:CATDESK_WAKE_TEST_ONLY_ROOT_PARENT
    try {
        $env:LOCALAPPDATA = $fixture
        $env:CATDESK_WAKE_TEST_ONLY_ROOT_PARENT = $fixture
        $splitObject = & $NativeHost install-object-probe $holder.Id $splitFingerprint
        if ($LASTEXITCODE -ne 0 -or $splitObject -notmatch '"objectMatch":true') {
            throw "TEST_SPLIT_OBJECT_SETUP_FAILED: $splitObject"
        }
        $spoofed = & $NativeHost install-authority-probe $holder.Id $splitFingerprint
        if ($LASTEXITCODE -eq 0 -or $spoofed -notmatch
            'WAKE_STORE_AUTHORITY_LOCK_OWNER_UNPROVEN' -or
            ($EnableOwnerProofHost -and $spoofed -notmatch
                'WAKE_STORE_AUTHORITY_LOCK_OWNER_UNPROVEN_PIPE_PID')) {
            throw "TEST_SPOOF_PIPE_PID_ACCEPTED: $spoofed"
        }
        $splitDenied = & $NativeHost install-init $holder.Id $splitFingerprint
        if ($LASTEXITCODE -eq 0 -or $splitDenied -notmatch
            'WAKE_STORE_AUTHORITY_LOCK_OWNER_UNPROVEN' -or
            (Test-Path -LiteralPath (Join-Path $root 'config.json'))) {
            throw "TEST_SPLIT_OWNER_ACCEPTED: $splitDenied"
        }
    } finally {
        $env:LOCALAPPDATA = $priorLocalAppDataForNative
        $env:CATDESK_WAKE_TEST_ONLY_ROOT_PARENT = $priorTestParent
    }
    Stop-Process -Id $spoofPipeHost.Id -Force
    [void]$spoofPipeHost.WaitForExit(5000)
    $spoofPipeHost.Dispose()
    $spoofPipeHost = $null
    Stop-Process -Id $holder.Id -Force
    [void]$holder.WaitForExit(5000)
    $holder.Dispose()
    $holder = $null
    Stop-Process -Id $otherHolder.Id -Force
    [void]$otherHolder.WaitForExit(5000)
    $otherHolder.Dispose()
    $otherHolder = $null
    Remove-Item -LiteralPath $ready -Force
    Remove-Item -LiteralPath $otherReady -Force
    $versions = Join-Path $root 'versions'
    $outsideVersions = Join-Path $fixture 'outside-versions'
    New-Item -ItemType Directory -Path $outsideVersions | Out-Null
    Remove-Item -LiteralPath $versions -Force
    New-Item -ItemType Junction -Path $versions -Target $outsideVersions | Out-Null
    $holder = Start-Process -FilePath (Get-Command powershell.exe).Source `
        -ArgumentList @('-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass',
            '-File',"`"$holderScript`"",'-LockPath',"`"$lockPath`"",
            '-RootPath',"`"$root`"",'-ReadyPath',"`"$ready`"",'-HoldRoot') `
        -WindowStyle Hidden -PassThru
    $deadline = [DateTime]::UtcNow.AddSeconds(10)
    while (-not (Test-Path -LiteralPath $ready -PathType Leaf)) {
        if ($holder.HasExited -or [DateTime]::UtcNow -ge $deadline) {
            throw 'TEST_JUNCTION_HOLDER_UNAVAILABLE'
        }
        Start-Sleep -Milliseconds 50
    }
    $junctionFingerprint = & $probe -WakeRoot $root -HostProcessId $holder.Id -RequireLiveRootProof
    $junctionPin = [CatDeskWakeInstallStoreAuthority]::Pin($root, $junctionFingerprint)
    try {
        try {
            $junctionPin.PinVersions()
            throw 'TEST_JUNCTION_ACCEPTED_BY_POWERSHELL_PIN'
        } catch {
            if ($_.Exception.Message -notmatch 'WAKE_STORE_AUTHORITY_VERSIONS_INVALID') { throw }
        }
    } finally { $junctionPin.Dispose() }
    $priorLocalAppDataForNative = $env:LOCALAPPDATA
    $priorTestParent = $env:CATDESK_WAKE_TEST_ONLY_ROOT_PARENT
    try {
        $env:LOCALAPPDATA = $fixture
        $env:CATDESK_WAKE_TEST_ONLY_ROOT_PARENT = $fixture
        $junctionDenied = & $NativeHost install-init $holder.Id $junctionFingerprint
        if ($LASTEXITCODE -eq 0 -or $junctionDenied -notmatch
            'WAKE_STORE_AUTHORITY_VERSIONS_(INVALID|UNAVAILABLE)' -or
            (Test-Path -LiteralPath (Join-Path $outsideVersions 'config.json'))) {
            throw "TEST_JUNCTION_ACCEPTED_BY_NATIVE: $junctionDenied"
        }
        $reviewedStatus = & $NativeHost reviewed-install-status
        if ($LASTEXITCODE -eq 0 -or $reviewedStatus -notmatch
            'HOST_INSTALL_DIRECTORY_REPARSE') {
            throw "TEST_JUNCTION_ACCEPTED_BY_RUNTIME: $reviewedStatus"
        }
    } finally {
        $env:LOCALAPPDATA = $priorLocalAppDataForNative
        $env:CATDESK_WAKE_TEST_ONLY_ROOT_PARENT = $priorTestParent
    }
    Stop-Process -Id $holder.Id -Force
    [void]$holder.WaitForExit(5000)
    $holder.Dispose()
    $holder = $null
    Remove-Item -LiteralPath $ready -Force
    Remove-Item -LiteralPath $versions -Force
    if (-not (Test-Path -LiteralPath $outsideVersions -PathType Container)) {
        throw 'TEST_JUNCTION_REMOVAL_DELETED_TARGET'
    }
    $holder = Start-Process -FilePath (Get-Command powershell.exe).Source `
        -ArgumentList @('-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass',
            '-File',"`"$holderScript`"",'-LockPath',"`"$lockPath`"",
            '-RootPath',"`"$root`"",'-ReadyPath',"`"$ready`"",'-OpenTwice') `
        -WindowStyle Hidden -PassThru
    $deadline = [DateTime]::UtcNow.AddSeconds(10)
    while (-not (Test-Path -LiteralPath $ready -PathType Leaf)) {
        if ($holder.HasExited -or [DateTime]::UtcNow -ge $deadline) {
            throw 'TEST_DOUBLE_HOLDER_UNAVAILABLE'
        }
        Start-Sleep -Milliseconds 50
    }
    try {
        & $probe -WakeRoot $root -HostProcessId $holder.Id | Out-Null
        throw 'TEST_AMBIGUOUS_LOCK_WAS_ACCEPTED'
    } catch {
        if ($_.Exception.Message -notmatch 'WAKE_STORE_AUTHORITY_LOCK_AMBIGUOUS') {
            throw
        }
    }
    $isolatedLocalAppData = Join-Path $fixture 'isolated-local-app-data'
    $priorLocalAppData = $env:LOCALAPPDATA
    try {
        $env:LOCALAPPDATA = $isolatedLocalAppData
        try {
            & $installer
            throw 'TEST_INSTALLER_REJECTION_MISSING'
        } catch {
            if ($_.Exception.Message -ne 'WAKE_INSTALL_BROKER_UNPROVISIONED') {
                throw
            }
        }
        if (Test-Path -LiteralPath (Join-Path $isolatedLocalAppData 'CatDeskWake')) {
            throw 'TEST_INSTALLER_CREATED_STORE_BEFORE_AUTHORITY'
        }
    } finally { $env:LOCALAPPDATA = $priorLocalAppData }
    if ($EnableOwnerProofHost) {
        Stop-Process -Id $holder.Id -Force
        [void]$holder.WaitForExit(5000)
        $holder.Dispose()
        $holder = $null
        Remove-Item -LiteralPath $ready -Force
        $versions = Join-Path $root 'versions'
        New-Item -ItemType Directory -Path $versions | Out-Null
        $proofReady = Join-Path $fixture 'proof-ready.txt'
        $proofErrors = Join-Path $fixture 'proof-errors.txt'
        $priorLocalAppData = $env:LOCALAPPDATA
        $priorTestParent = $env:CATDESK_WAKE_TEST_ONLY_ROOT_PARENT
        try {
            $env:LOCALAPPDATA = $fixture
            $env:CATDESK_WAKE_TEST_ONLY_ROOT_PARENT = $fixture
            $proofHost = Start-Process -FilePath $NativeHost `
                -ArgumentList @('test-owner-proof-host') -WindowStyle Hidden `
                -RedirectStandardOutput $proofReady -RedirectStandardError $proofErrors -PassThru
            $proofStarted = [DateTime]::UtcNow
            $deadline = [DateTime]::UtcNow.AddSeconds(10)
            while (-not (Test-Path -LiteralPath $proofReady -PathType Leaf) -or
                (Get-Content -LiteralPath $proofReady -Raw -ErrorAction SilentlyContinue) -notmatch 'OWNER_PROOF_READY') {
                if ($proofHost.HasExited -or [DateTime]::UtcNow -ge $deadline) {
                    throw 'TEST_OWNER_PROOF_HOST_UNAVAILABLE'
                }
                Start-Sleep -Milliseconds 50
            }
            if ($proofHost.HasExited) {
                throw "TEST_OWNER_PROOF_HOST_EXITED: $(Get-Content -LiteralPath $proofErrors -Raw)"
            }
            try {
                $nativeFingerprint = & $probe -WakeRoot $root `
                    -HostProcessId $proofHost.Id -RequireLiveRootProof
            } catch {
                $nativeObject = & $NativeHost install-object-probe $proofHost.Id $authoritative
                throw "TEST_PROOF_HOST_OBJECT_PROBE: $($_.Exception.Message); native=$nativeObject; exited=$($proofHost.HasExited); elapsed=$(([DateTime]::UtcNow - $proofStarted).TotalSeconds); exitcode=$($proofHost.ExitCode); stderr=$(Get-Content -LiteralPath $proofErrors -Raw)"
            }
            $nativeProof = & $NativeHost install-authority-probe `
                $proofHost.Id $nativeFingerprint
            if ($LASTEXITCODE -ne 0 -or $nativeProof -notmatch '"lockOwnerProven":true') {
                throw "TEST_NATIVE_OWNER_PROOF_FAILED: $nativeProof $(Get-Content -LiteralPath $proofErrors -Raw)"
            }
            $nativeInit = & $NativeHost install-init $proofHost.Id $nativeFingerprint
            if ($LASTEXITCODE -ne 0 -or $nativeInit -notmatch '"ok":true' -or
                -not (Test-Path -LiteralPath (Join-Path $root 'config.json') -PathType Leaf)) {
                throw "TEST_NATIVE_OWNER_PROOF_DID_NOT_AUTHORIZE_ISOLATED_INIT: $nativeInit"
            }
        } finally {
            $env:LOCALAPPDATA = $priorLocalAppData
            $env:CATDESK_WAKE_TEST_ONLY_ROOT_PARENT = $priorTestParent
        }
        Stop-Process -Id $proofHost.Id -Force
        [void]$proofHost.WaitForExit(5000)
        $proofHost.Dispose()
        $proofHost = $null
    }
    'PASS: root and lock object match remains read-only, split lease owner and versions junction denied, legacy install aliases denied, future host owner challenge tested when enabled'
} finally {
    if ($null -ne $proofHost) {
        if (-not $proofHost.HasExited) {
            Stop-Process -Id $proofHost.Id -Force
            [void]$proofHost.WaitForExit(5000)
        }
        $proofHost.Dispose()
    }
    if ($null -ne $spoofPipeHost) {
        if (-not $spoofPipeHost.HasExited) {
            Stop-Process -Id $spoofPipeHost.Id -Force
            [void]$spoofPipeHost.WaitForExit(5000)
        }
        $spoofPipeHost.Dispose()
    }
    if ($null -ne $otherHolder) {
        if (-not $otherHolder.HasExited) {
            Stop-Process -Id $otherHolder.Id -Force
            [void]$otherHolder.WaitForExit(5000)
        }
        $otherHolder.Dispose()
    }
    if ($null -ne $holder) {
        if (-not $holder.HasExited) {
            Stop-Process -Id $holder.Id -Force
            [void]$holder.WaitForExit(5000)
        }
        $holder.Dispose()
    }
    if (Test-Path -LiteralPath $fixture) {
        Remove-Item -LiteralPath $fixture -Recurse -Force
    }
}
