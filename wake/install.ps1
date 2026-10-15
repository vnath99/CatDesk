param([switch]$BuildOnly, [switch]$RebuildGui)
$ErrorActionPreference = 'Stop'

$wakeSource = $PSScriptRoot
$repoSource = Split-Path -Parent $wakeSource
$wakeRoot = Join-Path $env:LOCALAPPDATA 'CatDeskWake'
$versionsRoot = Join-Path $wakeRoot 'versions'

function Assert-WakeInstallStoreAuthority {
    # The connected host may resolve the same displayed path to a different
    # physical store than this installer process. Check the exact live lock
    # object before creating even a staging directory in the caller's view.
    # Native Wake commands must also verify this identity in their own process
    # before mutation; a PowerShell check cannot pin their later pathname opens.
    $hosts = @(Get-CimInstance Win32_Process -Filter "Name = 'CatDeskWakeHost.exe'")
    if ($hosts.Count -ne 1) { throw 'WAKE_STORE_AUTHORITY_HOST_NOT_UNIQUE' }
    $hostProcess = $hosts[0]
    $imagePath = [string]$hostProcess.ExecutablePath
    $pointerPath = Join-Path $wakeRoot 'current.json'
    if (-not (Test-Path -LiteralPath $pointerPath -PathType Leaf)) {
        throw 'WAKE_STORE_AUTHORITY_POINTER_UNAVAILABLE'
    }
    $pointerItem = Get-Item -LiteralPath $pointerPath
    if ($pointerItem.Attributes -band [IO.FileAttributes]::ReparsePoint) {
        throw 'WAKE_STORE_AUTHORITY_POINTER_INVALID'
    }
    $pointer = Get-Content -LiteralPath $pointerPath -Raw | ConvertFrom-Json
    $directory = [string]$pointer.directory
    $hostHash = [string]$pointer.hostSha256
    if ($pointer.schemaVersion -ne 1 -or
        $directory -notmatch '^1\.0\.0-dev\.\d+-[0-9a-f]{12}-[0-9a-f]{12}$' -or
        $hostHash -notmatch '^[0-9a-fA-F]{64}$') {
        throw 'WAKE_STORE_AUTHORITY_POINTER_INVALID'
    }
    $expectedImage = Join-Path (Join-Path $versionsRoot $directory) 'CatDeskWakeHost.exe'
    if (-not [string]::Equals($imagePath, $expectedImage, [StringComparison]::OrdinalIgnoreCase)) {
        throw 'WAKE_STORE_AUTHORITY_PROCESS_IMAGE_MISMATCH'
    }
    $imageItem = Get-Item -LiteralPath $expectedImage
    if ($imageItem.Attributes -band [IO.FileAttributes]::ReparsePoint -or
        (Get-FileHash -LiteralPath $expectedImage -Algorithm SHA256).Hash -ine $hostHash) {
        throw 'WAKE_STORE_AUTHORITY_PROCESS_IMAGE_HASH_MISMATCH'
    }
    $pidBefore = [int]$hostProcess.ProcessId
    $startedBefore = $hostProcess.CreationDate.ToUniversalTime().Ticks
    $objectIdentity = @(
        & (Join-Path $wakeSource 'check-install-store-authority.ps1') `
            -WakeRoot $wakeRoot -HostProcessId $pidBefore -RequireLiveRootProof
    )
    if ($objectIdentity.Count -ne 1 -or
        $objectIdentity[0] -notmatch '^[0-9a-f]{64}$') {
        throw 'WAKE_STORE_AUTHORITY_PROBE_INVALID'
    }
    $hostAfter = Get-CimInstance Win32_Process -Filter "ProcessId = $pidBefore"
    if ($null -eq $hostAfter -or
        $hostAfter.CreationDate.ToUniversalTime().Ticks -ne $startedBefore -or
        -not [string]::Equals([string]$hostAfter.ExecutablePath, $imagePath,
            [StringComparison]::OrdinalIgnoreCase)) {
        throw 'WAKE_STORE_AUTHORITY_PROCESS_CHANGED'
    }
    return [pscustomobject]@{
        Fingerprint = $objectIdentity[0]
        HostProcessId = $pidBefore
        HostStartedTicks = $startedBefore
    }
}

$prebuildStoreIdentity = $null
if (-not $BuildOnly) {
    $prebuildStoreIdentity = Assert-WakeInstallStoreAuthority
}

& cargo build --release --locked --offline --manifest-path (Join-Path $wakeSource 'Cargo.toml')
if ($LASTEXITCODE -ne 0) { throw 'Wake Rust build failed' }

$guiTarget = Join-Path $wakeSource 'target\catdesk-gui'
$guiBuild = Join-Path $guiTarget 'release\catdesk.exe'
if ($RebuildGui -or -not (Test-Path -LiteralPath $guiBuild -PathType Leaf)) {
    & cargo build --release --locked --offline --manifest-path (Join-Path $repoSource 'Cargo.toml') --bin catdesk --target-dir $guiTarget
    if ($LASTEXITCODE -ne 0) { throw 'CatDesk Binagotchy build failed' }
}
if ($BuildOnly) { return }

$postbuildStoreIdentity = Assert-WakeInstallStoreAuthority
if ($postbuildStoreIdentity.Fingerprint -ne $prebuildStoreIdentity.Fingerprint -or
    $postbuildStoreIdentity.HostProcessId -ne $prebuildStoreIdentity.HostProcessId -or
    $postbuildStoreIdentity.HostStartedTicks -ne $prebuildStoreIdentity.HostStartedTicks) {
    throw 'WAKE_STORE_AUTHORITY_CHANGED_DURING_BUILD'
}
function Assert-WakeInstallStoreStillSame {
    if ($null -ne $pinnedStore) { $pinnedStore.Verify() }
    $observed = Assert-WakeInstallStoreAuthority
    if ($observed.Fingerprint -ne $postbuildStoreIdentity.Fingerprint -or
        $observed.HostProcessId -ne $postbuildStoreIdentity.HostProcessId -or
        $observed.HostStartedTicks -ne $postbuildStoreIdentity.HostStartedTicks) {
        throw 'WAKE_STORE_AUTHORITY_CHANGED_DURING_HANDOFF'
    }
}
$hostBuild = Join-Path $wakeSource 'target\release\CatDeskWakeHost.exe'
& $hostBuild install-authority-probe `
    $postbuildStoreIdentity.HostProcessId $postbuildStoreIdentity.Fingerprint | Out-Null
if ($LASTEXITCODE -ne 0) { throw 'WAKE_STORE_AUTHORITY_NATIVE_PROBE_FAILED' }
$pinnedStore = [CatDeskWakeInstallStoreAuthority]::Pin(
    $wakeRoot, $postbuildStoreIdentity.Fingerprint
)
try {
$pinnedStore.PinVersions()

$wakeManifestPath = Join-Path $wakeSource 'Cargo.toml'
$wakeManifest = [IO.File]::ReadAllText($wakeManifestPath)
$versionMatch = [regex]::Match(
    $wakeManifest,
    '(?ms)^\[package\]\s*.*?^version\s*=\s*"([^"]+)"\s*$'
)
if (-not $versionMatch.Success) { throw 'Wake package version unavailable' }
$version = $versionMatch.Groups[1].Value
if ($version -notmatch '^1\.0\.0-dev\.\d+$') { throw 'Wake package version invalid' }

$inputs = @{
    'CatDeskWakeHost.exe' = $hostBuild
    'CatDeskBinagotchy.exe' = $guiBuild
    'adapter.py' = (Join-Path $wakeSource 'adapter.py')
    'wake_bridge.py' = (Join-Path $repoSource 'scripts\wake_bridge.py')
}
$hashes = @{}
foreach ($name in $inputs.Keys) {
    $hashes[$name] = (Get-FileHash -LiteralPath $inputs[$name] -Algorithm SHA256).Hash.ToLowerInvariant()
}

$installId = "$version-$($hashes['CatDeskWakeHost.exe'].Substring(0,12))-$($hashes['CatDeskBinagotchy.exe'].Substring(0,12))"
$installDirectory = Join-Path $versionsRoot $installId
$stagingName = ".staging-$installId-$PID-$([Guid]::NewGuid().ToString('N'))"
$stagingDirectory = Join-Path $versionsRoot $stagingName

# PowerShell never owns package installation authority. It only prepares a
# unique inert staging directory. Rust serializes publication and activation.
New-Item -ItemType Directory -Path $stagingDirectory | Out-Null
try {
    foreach ($name in $inputs.Keys) {
        $destination = Join-Path $stagingDirectory $name
        Copy-Item -LiteralPath $inputs[$name] -Destination $destination
        $actual = (Get-FileHash -LiteralPath $destination -Algorithm SHA256).Hash.ToLowerInvariant()
        if ($actual -ne $hashes[$name]) { throw 'Installed artifact readback mismatch' }
    }
    $manifest = @{
        schemaVersion = 1
        version = $version
        protocolVersion = 1
        installedUtc = [DateTime]::UtcNow.ToString('o')
        artifacts = $hashes
        acceptance = 'DEVELOPMENT_NOT_ACTIVATED'
    }
    $manifestJson = $manifest | ConvertTo-Json -Depth 5
    [IO.File]::WriteAllText(
        (Join-Path $stagingDirectory 'manifest.json'),
        $manifestJson,
        (New-Object Text.UTF8Encoding($false))
    )

    Assert-WakeInstallStoreStillSame
    $publicationJson = & $hostBuild publish-reviewed-install $stagingName $installId `
        $hashes['CatDeskWakeHost.exe'] $hashes['CatDeskBinagotchy.exe'] `
        $hashes['adapter.py'] $hashes['wake_bridge.py'] `
        $postbuildStoreIdentity.HostProcessId $postbuildStoreIdentity.Fingerprint
    if ($LASTEXITCODE -ne 0) { throw 'Wake reviewed package publication failed' }
    $publication = $publicationJson | ConvertFrom-Json
    if ($publication.version -ne $version -or $publication.directory -ne $installId) {
        throw 'Wake reviewed package publication readback mismatch'
    }
} catch {
    # A failed stage may contain a redirected child. Preserve it for reviewed
    # cleanup instead of recursively deleting through a mutable pathname.
    throw
}

$candidateHost = Join-Path $installDirectory 'CatDeskWakeHost.exe'
Assert-WakeInstallStoreStillSame
& $candidateHost install-init `
    $postbuildStoreIdentity.HostProcessId $postbuildStoreIdentity.Fingerprint
if ($LASTEXITCODE -ne 0) { throw 'Wake config initialization failed' }
Assert-WakeInstallStoreStillSame
& $candidateHost install-set-source $repoSource `
    $postbuildStoreIdentity.HostProcessId $postbuildStoreIdentity.Fingerprint
if ($LASTEXITCODE -ne 0) { throw 'Wake review-source binding failed' }

# Rust owns the stop -> exact-pointer switch -> desired-state restore
# transaction under the same exclusive install lease used by publication.
Assert-WakeInstallStoreStillSame
$handoffOutput = @(& $candidateHost activate-reviewed-install `
    $hashes['CatDeskWakeHost.exe'] $hashes['CatDeskBinagotchy.exe'] `
    $hashes['adapter.py'] $hashes['wake_bridge.py'] `
    $postbuildStoreIdentity.HostProcessId $postbuildStoreIdentity.Fingerprint)
$activationExitCode = $LASTEXITCODE
$handoffJson = [string]($handoffOutput | Out-String)
if ($activationExitCode -ne 0) {
    $reason = 'UNKNOWN'
    try {
        $activationError = $handoffJson | ConvertFrom-Json
        if (
            $null -ne $activationError.error -and
            ([string]$activationError.error) -match '^[A-Z0-9_]{1,128}$'
        ) {
            $reason = [string]$activationError.error
        }
    } catch {}
    throw "Wake reviewed package activation failed ($reason)"
}
$handoff = $handoffJson | ConvertFrom-Json
if ($handoff.version -ne $version -or $handoff.directory -ne $installId) {
    throw 'Wake reviewed package activation readback mismatch'
}

$installResult = [pscustomobject]@{
    InstallDirectory = $installDirectory
    Version = $version
    Artifacts = $hashes
    AlreadyMaterialized = [bool]$publication.alreadyMaterialized
    Activation = $handoff
}

# Shortcut publication is presentation-only. Run COM in a bounded child
# process so a shell/COM stall can never keep the installer process alive.
$shortcutState = 'NOT_ATTEMPTED'
$shortcutHelper = Join-Path $wakeRoot ("shortcut-publish-$PID-$([Guid]::NewGuid().ToString('N')).ps1")
$shortcutScript = @'
$ErrorActionPreference = 'Stop'
$installDirectory = $env:CATDESK_WAKE_SHORTCUT_INSTALL
$repoSource = $env:CATDESK_WAKE_SHORTCUT_REPO
$shortcuts = Join-Path ([Environment]::GetFolderPath('StartMenu')) 'Programs'
$shell = $null
$binagotchyShortcut = $null
$shortcut = $null
$exitCode = 0
try {
    $shell = New-Object -ComObject WScript.Shell
    $legacy = Join-Path $shortcuts 'CatDesk Wake.lnk'
    if (Test-Path -LiteralPath $legacy) {
        Remove-Item -LiteralPath $legacy -Force
    }
    $binagotchyShortcut = $shell.CreateShortcut((Join-Path $shortcuts 'CatDesk Binagotchy.lnk'))
    $binagotchyShortcut.TargetPath = Join-Path $installDirectory 'CatDeskBinagotchy.exe'
    $binagotchyShortcut.Arguments = '--catdesk-binagotchy-cli'
    $binagotchyShortcut.WorkingDirectory = $repoSource
    $binagotchyShortcut.Description = 'CatDesk Binagotchy and wake controls'
    $binagotchyShortcut.Save()
    $shortcut = $binagotchyShortcut
} catch {
    $exitCode = 1
} finally {
    if ($null -ne $shortcut) {
        [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($shortcut)
    }
    if ($null -ne $shell) {
        [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($shell)
    }
}
exit $exitCode
'@

$priorShortcutInstall = $env:CATDESK_WAKE_SHORTCUT_INSTALL
$priorShortcutRepo = $env:CATDESK_WAKE_SHORTCUT_REPO
$shortcutProcess = $null
try {
    [IO.File]::WriteAllText(
        $shortcutHelper,
        $shortcutScript,
        (New-Object Text.UTF8Encoding($false))
    )
    $env:CATDESK_WAKE_SHORTCUT_INSTALL = $installDirectory
    $env:CATDESK_WAKE_SHORTCUT_REPO = $repoSource

    $psi = New-Object Diagnostics.ProcessStartInfo
    $psi.FileName = 'powershell.exe'
    $psi.Arguments = "-NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File `"$shortcutHelper`""
    $psi.UseShellExecute = $false
    $psi.CreateNoWindow = $true
    $shortcutProcess = [Diagnostics.Process]::Start($psi)
    if ($null -eq $shortcutProcess) {
        $shortcutState = 'START_FAILED'
    } elseif (-not $shortcutProcess.WaitForExit(10000)) {
        $shortcutProcess.Kill()
        $shortcutProcess.WaitForExit()
        $shortcutState = 'TIMEOUT'
    } elseif ($shortcutProcess.ExitCode -eq 0) {
        $shortcutState = 'READY'
    } else {
        $shortcutState = 'FAILED'
    }
} catch {
    $shortcutState = 'FAILED'
} finally {
    $env:CATDESK_WAKE_SHORTCUT_INSTALL = $priorShortcutInstall
    $env:CATDESK_WAKE_SHORTCUT_REPO = $priorShortcutRepo
    if ($null -ne $shortcutProcess) {
        $shortcutProcess.Dispose()
    }
    if (Test-Path -LiteralPath $shortcutHelper) {
        Remove-Item -LiteralPath $shortcutHelper -Force
    }
}

$installResult | Add-Member -NotePropertyName ShortcutState -NotePropertyValue $shortcutState
$installResult | ConvertTo-Json -Depth 6
} finally {
    $pinnedStore.Dispose()
}
