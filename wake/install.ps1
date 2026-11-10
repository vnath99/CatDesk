param(
    [switch]$BuildOnly,
    [switch]$RebuildGui,
    [switch]$Bootstrap,
    [switch]$Execute,
    [string]$QualifiedHost,
    [string]$ExpectedHostSha256,
    [string]$ApprovalFile,
    [string]$ExpectedApprovalSha256,
    [string]$ExpectedPublicKeySha256
)
$ErrorActionPreference = 'Stop'

# First enrollment remains an explicit operator operation. The frozen native
# binary performs signature, elevated-owner, existing-state, create-only and
# security checks. This wrapper neither creates trust nor edits protected data.
# Without -Execute it invokes only the bounded, read-only native preflight.
if ($Bootstrap) {
    if ($BuildOnly -or $RebuildGui -or $PSVersionTable.PSVersion.Major -lt 7) {
        throw 'WAKE_BOOTSTRAP_POWERSHELL_7_AND_EXCLUSIVE_MODE_REQUIRED'
    }
    foreach ($value in @($ExpectedHostSha256, $ExpectedApprovalSha256, $ExpectedPublicKeySha256)) {
        if ($value -cnotmatch '^[a-f0-9]{64}$') { throw 'WAKE_BOOTSTRAP_FROZEN_IDENTITY_REQUIRED' }
    }
    foreach ($path in @($QualifiedHost, $ApprovalFile)) {
        if (-not [IO.Path]::IsPathFullyQualified($path) -or
            $path -cnotmatch '^[A-Za-z]:\\' -or
            $path -match '[\x00-\x1f]') {
            throw 'WAKE_BOOTSTRAP_FROZEN_FILE_REQUIRED'
        }
    }
    if (-not ('CatDeskWakeBootstrapPathPinV1' -as [type])) {
        Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;
using Microsoft.Win32.SafeHandles;
public sealed class CatDeskWakeBootstrapPathPinV1 : IDisposable {
    [StructLayout(LayoutKind.Sequential)]
    private struct Info {
        public uint Attributes, CreationLow, CreationHigh, AccessLow, AccessHigh;
        public uint WriteLow, WriteHigh, Volume, SizeHigh, SizeLow, Links, IndexHigh, IndexLow;
    }
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true, ExactSpelling=true)]
    private static extern SafeFileHandle CreateFileW(string path, uint access, uint share,
        IntPtr security, uint disposition, uint flags, IntPtr template);
    [DllImport("kernel32.dll", SetLastError=true, ExactSpelling=true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool GetFileInformationByHandle(SafeFileHandle file, out Info info);
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true, ExactSpelling=true)]
    private static extern uint GetFinalPathNameByHandleW(SafeFileHandle file, StringBuilder path, uint size, uint flags);
    private readonly List<SafeFileHandle> parents = new List<SafeFileHandle>();
    private readonly List<Info> parentInfo = new List<Info>();
    private FileStream stream;
    private Info leafInfo;
    private bool disposed;
    public string CanonicalFilePath { get; private set; }
    public FileStream Stream { get { if (disposed) throw new ObjectDisposedException("PathPin"); return stream; } }
    public long Length { get { return Stream.Length; } }
    private CatDeskWakeBootstrapPathPinV1() { }
    private static IOException Failure(string reason) { return new IOException("WAKE_BOOTSTRAP_FROZEN_"+reason); }
    private static SafeFileHandle OpenHandle(string path, bool directory) {
        // LIST_DIRECTORY makes directory sharing effective; attribute-only access is separate.
        // A retained child also keeps the directory nonempty, which refuses mount-point replacement.
        var handle = CreateFileW(path, directory ? 0x00120081u : 0x80000000u, 1,
            IntPtr.Zero, 3, 0x00200000u | (directory ? 0x02000000u : 0u), IntPtr.Zero);
        if (handle.IsInvalid) {
            int code = Marshal.GetLastWin32Error(); handle.Dispose();
            throw Failure(code == 32 || code == 33 ? "PATH_BUSY" : "PATH_UNAVAILABLE");
        }
        return handle;
    }
    private static Info Inspect(SafeFileHandle handle, bool directory) {
        Info info;
        if (!GetFileInformationByHandle(handle, out info)) throw Failure("PATH_UNAVAILABLE");
        if ((info.Attributes & 0x400) != 0) throw Failure("REPARSE_POINT_REFUSED");
        if (((info.Attributes & 0x10) != 0) != directory || info.Volume == 0 ||
            (info.IndexHigh == 0 && info.IndexLow == 0) || (!directory && info.Links != 1))
            throw Failure("FILE_REQUIRED");
        return info;
    }
    private static bool Same(Info a, Info b, bool directory) {
        return a.Volume == b.Volume && a.IndexHigh == b.IndexHigh && a.IndexLow == b.IndexLow &&
            (directory || (a.Links == b.Links && a.SizeHigh == b.SizeHigh && a.SizeLow == b.SizeLow &&
                          a.WriteLow == b.WriteLow && a.WriteHigh == b.WriteHigh));
    }
    private static string FinalPath(SafeFileHandle handle) {
        var buffer = new StringBuilder(32768);
        uint length = GetFinalPathNameByHandleW(handle, buffer, (uint)buffer.Capacity, 0);
        if (length == 0 || length >= buffer.Capacity) throw Failure("PATH_UNAVAILABLE");
        string value = buffer.ToString();
        if (value.StartsWith(@"\\?\", StringComparison.Ordinal)) value = value.Substring(4);
        if (value.Length < 3 || value[1] != ':' || value[2] != '\\') throw Failure("PATH_INVALID");
        return Path.GetFullPath(value);
    }
    public static CatDeskWakeBootstrapPathPinV1 Open(string path) {
        if (!RuntimeInformation.IsOSPlatform(OSPlatform.Windows)) throw Failure("PATH_PLATFORM_REQUIRED");
        if (String.IsNullOrEmpty(path) || path.Length > 2048 || path.Length < 4 ||
            !Char.IsLetter(path[0]) || path[1] != ':' || path[2] != '\\') throw Failure("PATH_INVALID");
        foreach (char c in path) if (Char.IsControl(c)) throw Failure("PATH_INVALID");
        string canonical;
        try { canonical = Path.GetFullPath(path); } catch { throw Failure("PATH_INVALID"); }
        if (!String.Equals(path, canonical, StringComparison.OrdinalIgnoreCase)) throw Failure("PATH_INVALID");
        string root = Path.GetPathRoot(canonical);
        string[] components = canonical.Substring(root.Length).Split('\\');
        foreach (string part in components) if (part.Length == 0 || part == "." || part == ".." ||
            part.EndsWith(".", StringComparison.Ordinal) || part.EndsWith(" ", StringComparison.Ordinal) ||
            part.IndexOf(':') >= 0) throw Failure("PATH_INVALID");
        var pin = new CatDeskWakeBootstrapPathPinV1();
        SafeFileHandle leaf = null;
        try {
            string current = root;
            var parent = OpenHandle(current, true); pin.parents.Add(parent); pin.parentInfo.Add(Inspect(parent, true));
            for (int i=0; i<components.Length-1; i++) {
                current = Path.Combine(current, components[i]);
                parent = OpenHandle(current, true); pin.parents.Add(parent); pin.parentInfo.Add(Inspect(parent, true));
            }
            leaf = OpenHandle(canonical, false); pin.leafInfo = Inspect(leaf, false);
            if (!String.Equals(FinalPath(leaf), canonical, StringComparison.OrdinalIgnoreCase)) throw Failure("PATH_CHANGED");
            pin.CanonicalFilePath = canonical;
            pin.stream = new FileStream(leaf, FileAccess.Read, 4096, false); leaf = null;
            pin.Revalidate(); return pin;
        } catch { if (leaf != null) leaf.Dispose(); pin.Dispose(); throw; }
    }
    public void Revalidate() {
        if (disposed) throw new ObjectDisposedException("PathPin");
        for (int i=0; i<parents.Count; i++) if (!Same(parentInfo[i], Inspect(parents[i], true), true)) throw Failure("PATH_CHANGED");
        if (!Same(leafInfo, Inspect(stream.SafeFileHandle, false), false) ||
            !String.Equals(FinalPath(stream.SafeFileHandle), CanonicalFilePath, StringComparison.OrdinalIgnoreCase))
            throw Failure("PATH_CHANGED");
    }
    public void Dispose() {
        if (disposed) return; disposed=true;
        try { if (stream != null) stream.Dispose(); }
        finally { for (int i=parents.Count-1; i>=0; i--) parents[i].Dispose(); }
    }
}
'@
    }
    $imagePin = $null
    $approvalPin = $null
    $native = $null
    try {
        try {
            $imagePin = [CatDeskWakeBootstrapPathPinV1]::Open($QualifiedHost)
            $approvalPin = [CatDeskWakeBootstrapPathPinV1]::Open($ApprovalFile)
        } catch {
            $code = $_.Exception.GetBaseException().Message
            if ($code -cmatch '^WAKE_BOOTSTRAP_FROZEN_[A-Z_]+$') { throw $code }
            throw 'WAKE_BOOTSTRAP_FROZEN_PATH_UNAVAILABLE'
        }
        if ($imagePin.Length -le 0 -or $imagePin.Length -gt 80MB -or
            $approvalPin.Length -le 0 -or $approvalPin.Length -gt 64KB) {
            throw 'WAKE_BOOTSTRAP_FROZEN_FILE_SIZE_INVALID'
        }
        $imageHash = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($imagePin.Stream)).ToLowerInvariant()
        $approvalHash = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($approvalPin.Stream)).ToLowerInvariant()
        if ($imageHash -cne $ExpectedHostSha256 -or $approvalHash -cne $ExpectedApprovalSha256) {
            throw 'WAKE_BOOTSTRAP_FROZEN_FILE_CHANGED'
        }
        if (-not ('CatDeskWakeBootstrapBoundedOutputV1' -as [type])) {
            Add-Type -TypeDefinition @'
using System.IO;
using System.Text;
using System.Threading.Tasks;
public sealed class CatDeskWakeBootstrapOutputV1 {
    public string Text;
    public bool Overflow;
}
public static class CatDeskWakeBootstrapBoundedOutputV1 {
    public static async Task<CatDeskWakeBootstrapOutputV1> Drain(StreamReader reader, int limit) {
        var buffer = new char[1024];
        var text = new StringBuilder();
        bool overflow = false;
        int count;
        while ((count = await reader.ReadAsync(buffer, 0, buffer.Length).ConfigureAwait(false)) != 0) {
            int keep = System.Math.Min(count, limit - text.Length);
            if (keep > 0) text.Append(buffer, 0, keep);
            if (count > keep) overflow = true;
        }
        return new CatDeskWakeBootstrapOutputV1 { Text = text.ToString(), Overflow = overflow };
    }
}
'@
        }
        $info = [Diagnostics.ProcessStartInfo]::new()
        $info.FileName = $imagePin.CanonicalFilePath
        $info.UseShellExecute = $false
        $info.CreateNoWindow = $true
        $info.RedirectStandardOutput = $true
        $info.RedirectStandardError = $true
        $action = if ($Execute) { 'bootstrap' } else { 'bootstrap-preflight' }
        foreach ($argument in @('install-broker', $action, '--approval-file', $approvalPin.CanonicalFilePath,
                '--expected-approval-sha256', $ExpectedApprovalSha256,
                '--expected-public-key-sha256', $ExpectedPublicKeySha256)) {
            $info.ArgumentList.Add($argument)
        }
        $imagePin.Revalidate()
        $approvalPin.Revalidate()
        $native = [Diagnostics.Process]::Start($info)
        $stdout = [CatDeskWakeBootstrapBoundedOutputV1]::Drain($native.StandardOutput, 65536)
        $stderr = [CatDeskWakeBootstrapBoundedOutputV1]::Drain($native.StandardError, 4096)
        if (-not $native.WaitForExit(90000)) {
            # No retry: SCM/file creation may already have happened. The native
            # coordinator starts no service or descendant; partial state is
            # retained for reviewed recovery instead of deletion or overwrite.
            $native.Kill()
            if (-not $native.WaitForExit(5000)) { throw 'WAKE_BOOTSTRAP_NATIVE_EXIT_UNPROVEN_NO_RETRY' }
            throw 'WAKE_BOOTSTRAP_OUTCOME_UNKNOWN_NO_RETRY'
        }
        if (-not $stdout.Wait(2000) -or -not $stderr.Wait(2000)) {
            throw 'WAKE_BOOTSTRAP_READBACK_UNKNOWN_NO_RETRY'
        }
        $output = $stdout.Result.Text
        $errorOutput = $stderr.Result.Text
        if ($stdout.Result.Overflow -or $stderr.Result.Overflow) {
            throw 'WAKE_BOOTSTRAP_READBACK_INVALID_NO_RETRY'
        }
        if ($native.ExitCode -ne 0) {
            if ($errorOutput.Trim() -cmatch '^[A-Z][A-Z0-9_]{1,127}$') { throw $errorOutput.Trim() }
            throw 'WAKE_BOOTSTRAP_NATIVE_REFUSED_NO_RETRY'
        }
        $result = $output | ConvertFrom-Json -ErrorAction Stop
        if ($null -eq $result) { throw 'WAKE_BOOTSTRAP_READBACK_INVALID_NO_RETRY' }
    } finally {
        try {
            if ($null -ne $native) {
                try {
                    if (-not $native.HasExited) {
                        $native.Kill()
                        if (-not $native.WaitForExit(5000)) { throw 'WAKE_BOOTSTRAP_NATIVE_EXIT_UNPROVEN_NO_RETRY' }
                    }
                } finally { $native.Dispose() }
            }
        } finally {
            try { if ($null -ne $approvalPin) { $approvalPin.Dispose() } }
            finally { if ($null -ne $imagePin) { $imagePin.Dispose() } }
        }
    }
    $output.Trim()
    return
}
if ($Execute -or $QualifiedHost -or $ExpectedHostSha256 -or $ApprovalFile -or
    $ExpectedApprovalSha256 -or $ExpectedPublicKeySha256) {
    throw 'WAKE_BOOTSTRAP_EXPLICIT_MODE_REQUIRED'
}

$wakeSource = $PSScriptRoot
$repoSource = Split-Path -Parent $wakeSource
$wakeRoot = Join-Path $env:LOCALAPPDATA 'CatDeskWake'
$versionsRoot = Join-Path $wakeRoot 'versions'

# The current per-user store has no protected install broker. Refuse before
# build or inert staging can write into that namespace; -BuildOnly remains the
# source verification path.
if (-not $BuildOnly) { throw 'WAKE_INSTALL_BROKER_UNPROVISIONED' }

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
# An inert BuildOnly check must refresh both release binaries from current source;
# an older GUI target is not evidence for the current working tree.
if ($BuildOnly -or $RebuildGui -or -not (Test-Path -LiteralPath $guiBuild -PathType Leaf)) {
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
