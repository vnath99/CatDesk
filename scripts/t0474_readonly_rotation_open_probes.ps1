[CmdletBinding()]
param()

# T-0474: fixed-path, non-elevated, read-only Windows sharing/access preflight.
# No file move, deletion, rename, replacement, elevation or installer call.
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$Root = Split-Path -Parent $PSScriptRoot
$Report = Join-Path $Root '.catdesk\t0474-rotation-open-probes.json'
$InstalledPath = 'C:\Program Files\CatDesk\CatDesk.exe'
$StagingPath = 'C:\Program Files\CatDesk\CatDesk.rotation-next.exe'
$IncomingPath = 'C:\ProgramData\CatDesk\reviewed-main-image\rotation\incoming\CatDesk.exe'
$PendingPath = 'C:\Program Files\CatDesk.reviewed-main-image-rotation-pending.v1'
$IncomingEnvelopePath = 'C:\ProgramData\CatDesk\reviewed-main-image\rotation\incoming\review-envelope.v1'
$ExpectedEpoch1 = '2421a90aeb9ad7775ec9927f8dfbe294faa3cbfe11ec7a071a1ed554eee28459'
$ExpectedEpoch2 = 'd09c677f8ce5f14b3600a5435929aff31b4128d9a041cdaceae3213b5f0af36f'
$ExpectedEnvelope = '42ababb6dcd47ec20db97775d7d441eb9fcadc806d3f304b7f221809bfda892c'

function Assert-ExactRegularFile([string] $Path, [long] $Length, [string] $Sha, [string] $Label) {
    $item = Get-Item -LiteralPath $Path -Force -ErrorAction Stop
    if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0 -or -not ($item -is [IO.FileInfo])) {
        throw "$Label is a reparse point or not a regular file; stopped."
    }
    if ($item.Length -ne $Length) { throw "$Label length mismatch; stopped." }
    if ((Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant() -ne $Sha) {
        throw "$Label digest mismatch; stopped."
    }
}

# Fail closed before even probing the fixed file handles. The previously
# executed signed public-root diagnostic, not this script, establishes trust.
Assert-ExactRegularFile $InstalledPath 25134592 $ExpectedEpoch1 'Installed epoch1 image'
Assert-ExactRegularFile $StagingPath 26302464 $ExpectedEpoch2 'Pending T0366 staging image'
Assert-ExactRegularFile $IncomingPath 26302464 $ExpectedEpoch2 'Incoming T0366 image'
Assert-ExactRegularFile $PendingPath 507 $ExpectedEnvelope 'Pending signed envelope'
Assert-ExactRegularFile $IncomingEnvelopePath 507 $ExpectedEnvelope 'Incoming signed envelope'

$Native = @'
using System;
using System.Runtime.InteropServices;
public static class T0474ReadOnlyWin32HandleProbe {
    [DllImport("kernel32.dll", EntryPoint="CreateFileW", CharSet=CharSet.Unicode, ExactSpelling=true, SetLastError=true)]
    static extern IntPtr CreateFileW(string path, uint desiredAccess, uint shareMode, IntPtr securityAttributes,
                                    uint creationDisposition, uint flagsAndAttributes, IntPtr templateFile);
    [DllImport("kernel32.dll", SetLastError=true)]
    static extern bool CloseHandle(IntPtr handle);
    // Positive error code means the non-elevated open failed; zero means success.
    public static int Check(string path, uint desiredAccess, uint shareMode) {
        IntPtr h = CreateFileW(path, desiredAccess, shareMode, IntPtr.Zero, 3u, 0x00200000u, IntPtr.Zero);
        if (h == new IntPtr(-1) || h == IntPtr.Zero) return Marshal.GetLastWin32Error();
        try { return 0; } finally { CloseHandle(h); }
    }
}
'@
Add-Type -TypeDefinition $Native -ErrorAction Stop

function Get-OpenCodeClass([int] $Code) {
    if ($Code -eq 0) { return 'OPEN_OK' }
    if ($Code -eq 5) { return 'ACCESS_DENIED' }
    if ($Code -eq 32) { return 'SHARING_VIOLATION' }
    return "WIN32_$Code"
}

function Get-OpenProbe([string] $Name, [string] $Path) {
    # First test the exact GENERIC_READ + SHARE_READ|SHARE_DELETE combination
    # used by the rotation state machine. Then request DELETE *access only* with
    # maximum sharing; this does not mark or delete the object.
    $rotateRead = [T0474ReadOnlyWin32HandleProbe]::Check($Path, [uint32]2147483648, [uint32]5)
    $deleteRights = [T0474ReadOnlyWin32HandleProbe]::Check($Path, [uint32]65536, [uint32]7)
    [ordered]@{
        name = $Name
        rotationReadOpenWin32 = $rotateRead
        deleteAccessOpenWin32 = $deleteRights
        # Win32 5 = ACCESS_DENIED; 32 = SHARING_VIOLATION. These are
        # non-elevated observations, not proof of elevated MoveFileEx outcome.
        rotationReadClass = (Get-OpenCodeClass $rotateRead)
        deleteAccessClass = (Get-OpenCodeClass $deleteRights)
    }
}

$results = @(
    (Get-OpenProbe 'installedEpoch1' $InstalledPath)
    (Get-OpenProbe 'stagingT0366' $StagingPath)
    (Get-OpenProbe 'incomingT0366' $IncomingPath)
)
$principal = [Security.Principal.WindowsPrincipal]::new([Security.Principal.WindowsIdentity]::GetCurrent())
$tokenIsAdmin = $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
# Recheck exact public identity to detect an intervening host change.
Assert-ExactRegularFile $InstalledPath 25134592 $ExpectedEpoch1 'Installed epoch1 image (second read)'
Assert-ExactRegularFile $StagingPath 26302464 $ExpectedEpoch2 'Pending T0366 staging image (second read)'
Assert-ExactRegularFile $IncomingPath 26302464 $ExpectedEpoch2 'Incoming T0366 image (second read)'
Assert-ExactRegularFile $PendingPath 507 $ExpectedEnvelope 'Pending signed envelope (second read)'
Assert-ExactRegularFile $IncomingEnvelopePath 507 $ExpectedEnvelope 'Incoming signed envelope (second read)'

$payload = [ordered]@{
    schemaVersion = 1
    task = 'T-0474'
    state = 'READ_ONLY_OPEN_PROBES_COMPLETE'
    timestampUtc = [DateTime]::UtcNow.ToString('o')
    tokenIsAdministrator = [bool]$tokenIsAdmin
    note = 'Access-only CreateFileW probes, no DeleteFile, MoveFileEx, files/receipts rewritten or UAC. Non-elevated access-denied is not elevated-install proof.'
    probes = $results
}
$payload | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $Report -Encoding UTF8
Write-Output 'T0474_READ_ONLY_OPEN_PROBES_COMPLETE'
$payload | ConvertTo-Json -Depth 5
