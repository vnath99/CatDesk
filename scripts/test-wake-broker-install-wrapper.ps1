#requires -Version 7.0
[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$wrapper = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../wake/install.ps1'))
$tokens = $null; $parseErrors = $null
$wrapperAst = [Management.Automation.Language.Parser]::ParseFile($wrapper, [ref]$tokens, [ref]$parseErrors)
if ($parseErrors.Count) { throw 'WRAPPER_PARSE_FAILED' }
$cases = 0; $assertions = 0; $processStarts = 0; $builds = 0; $systemQueries = 0
$pathPinCases = 0
function Assert-That([bool]$Condition, [string]$Code) {
    if (-not $Condition) { throw $Code }
    $script:assertions++
}

# Compile the actual wrapper's one bounded Drain implementation, not a copy.
$definitions = @($wrapperAst.FindAll({param($node)
    $node -is [Management.Automation.Language.CommandAst] -and
    $node.GetCommandName() -ceq 'Add-Type' -and
    @($node.CommandElements | Where-Object {
        $_ -is [Management.Automation.Language.StringConstantExpressionAst] -and
        $_.Value.Contains('public static class CatDeskWakeBootstrapBoundedOutputV1')
    }).Count -eq 1
}, $true))
Assert-That ($definitions.Count -eq 1) 'BOUNDED_DRAIN_DEFINITION_AMBIGUOUS'
$definition = @($definitions[0].CommandElements | Where-Object {
    $_ -is [Management.Automation.Language.StringConstantExpressionAst] -and
    $_.Value.Contains('public static class CatDeskWakeBootstrapBoundedOutputV1')
})[0].Value
Assert-That (-not ('CatDeskWakeBootstrapBoundedOutputV1' -as [type])) 'FRESH_POWERSHELL_TEST_PROCESS_REQUIRED'
Add-Type -TypeDefinition $definition
# Compile the actual independently versioned native path pin, not a test copy.
$pinDefinitions = @($wrapperAst.FindAll({param($node)
    $node -is [Management.Automation.Language.CommandAst] -and $node.GetCommandName() -ceq 'Add-Type' -and
    @($node.CommandElements | Where-Object {
        $_ -is [Management.Automation.Language.StringConstantExpressionAst] -and
        $_.Value.Contains('public sealed class CatDeskWakeBootstrapPathPinV1')
    }).Count -eq 1
}, $true))
Assert-That ($pinDefinitions.Count -eq 1) 'PATH_PIN_DEFINITION_AMBIGUOUS'
$pinDefinition = @($pinDefinitions[0].CommandElements | Where-Object {
    $_ -is [Management.Automation.Language.StringConstantExpressionAst] -and
    $_.Value.Contains('public sealed class CatDeskWakeBootstrapPathPinV1')
})[0].Value
Assert-That (-not ('CatDeskWakeBootstrapPathPinV1' -as [type])) 'FRESH_PATH_PIN_TEST_PROCESS_REQUIRED'
Add-Type -TypeDefinition $pinDefinition
Add-Type -TypeDefinition @'
using System;
using System.IO;
using System.Threading;
using System.Threading.Tasks;
using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;
public static class CatDeskWakeWrapperNativeProbeV1 {
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true, ExactSpelling=true)]
    private static extern SafeFileHandle CreateFileW(string path, uint access, uint share,
        IntPtr security, uint disposition, uint flags, IntPtr template);
    public static int DirectoryWriteError(string path) {
        using (var handle = CreateFileW(path, 0x00120082, 7, IntPtr.Zero, 3, 0x02200000, IntPtr.Zero)) {
            return handle.IsInvalid ? Marshal.GetLastWin32Error() : 0;
        }
    }
}
public sealed class CatDeskWakeWrapperCountingStreamV1 : MemoryStream {
    public int MaximumReadRequest;
    public CatDeskWakeWrapperCountingStreamV1(byte[] bytes) : base(bytes, false) { }
    public override int Read(byte[] buffer, int offset, int count) {
        MaximumReadRequest = Math.Max(MaximumReadRequest, count);
        return base.Read(buffer, offset, count);
    }
    public override Task<int> ReadAsync(byte[] buffer, int offset, int count, CancellationToken token) {
        MaximumReadRequest = Math.Max(MaximumReadRequest, count);
        return base.ReadAsync(buffer, offset, count, token);
    }
    public override ValueTask<int> ReadAsync(Memory<byte> buffer, CancellationToken token = default) {
        MaximumReadRequest = Math.Max(MaximumReadRequest, buffer.Length);
        return base.ReadAsync(buffer, token);
    }
    public static CatDeskWakeWrapperCountingStreamV1 Repeated(int length) {
        var bytes = new byte[length];
        Array.Fill(bytes, (byte)'x');
        return new CatDeskWakeWrapperCountingStreamV1(bytes);
    }
}
public sealed class CatDeskWakeWrapperFaultStreamV1 : MemoryStream {
    public override int Read(byte[] buffer, int offset, int count) {
        throw new IOException("EXPECTED_READ_FAILURE");
    }
    public override Task<int> ReadAsync(byte[] buffer, int offset, int count, CancellationToken token) {
        return Task.FromException<int>(new IOException("EXPECTED_READ_FAILURE"));
    }
    public override ValueTask<int> ReadAsync(Memory<byte> buffer, CancellationToken token = default) {
        return new ValueTask<int>(Task.FromException<int>(new IOException("EXPECTED_READ_FAILURE")));
    }
}
'@

function Invoke-Drain([IO.StreamReader]$Reader, [int]$Limit) {
    $task = [CatDeskWakeBootstrapBoundedOutputV1]::Drain($Reader, $Limit)
    try { $finished = $task.Wait(5000) }
    catch { throw $_.Exception.GetBaseException() }
    Assert-That $finished 'DRAIN_DID_NOT_COMPLETE'
    $task.GetAwaiter().GetResult()
}
function Test-Drain([int]$Length, [int]$Limit, [bool]$Overflow) {
    $stream = [CatDeskWakeWrapperCountingStreamV1]::Repeated($Length)
    $reader = [IO.StreamReader]::new($stream, [Text.Encoding]::UTF8, $false, 1024, $true)
    try {
        $result = Invoke-Drain $reader $Limit
        Assert-That ($result.Text -ceq ('x' * [Math]::Min($Length, $Limit))) 'DRAIN_PREFIX_CHANGED'
        Assert-That ($result.Overflow -eq $Overflow) 'DRAIN_OVERFLOW_CLASSIFICATION_WRONG'
        Assert-That ($stream.Position -eq $stream.Length) 'DRAIN_DID_NOT_REACH_EOF'
        Assert-That ($stream.MaximumReadRequest -le 4096) 'DRAIN_READ_REQUEST_UNBOUNDED'
        $script:cases++
    } finally { $reader.Dispose(); $stream.Dispose() }
}
Test-Drain 0 65536 $false
Test-Drain 65536 65536 $false
Test-Drain 65537 65536 $true
Test-Drain 4096 4096 $false
Test-Drain 4097 4096 $true

# An implementation that reads all output and only then truncates must fail.
# Input allocation is excluded; retained output stays bounded while 8 MiB drains.
$large = [CatDeskWakeWrapperCountingStreamV1]::Repeated(8MB)
$largeReader = [IO.StreamReader]::new($large, [Text.Encoding]::UTF8, $false, 1024, $true)
try {
    [GC]::Collect(); [GC]::WaitForPendingFinalizers()
    $allocatedBefore = [GC]::GetTotalAllocatedBytes($true)
    $bounded = Invoke-Drain $largeReader 65536
    $allocatedBytes = [GC]::GetTotalAllocatedBytes($true) - $allocatedBefore
    Assert-That ($bounded.Text.Length -eq 65536 -and $bounded.Overflow) 'LARGE_DRAIN_CAPTURE_NOT_BOUNDED'
    Assert-That ($large.Position -eq $large.Length) 'LARGE_DRAIN_DID_NOT_REACH_EOF'
    Assert-That ($allocatedBytes -lt 8MB) 'DRAIN_ALLOCATED_UNBOUNDED_OUTPUT'
    $cases++
} finally { $largeReader.Dispose(); $large.Dispose() }

$fault = [CatDeskWakeWrapperFaultStreamV1]::new()
$faultReader = [IO.StreamReader]::new($fault)
try {
    $failure = $null
    try { $null = Invoke-Drain $faultReader 4096 } catch { $failure = $_.Exception.GetBaseException().Message }
    Assert-That ($failure -ceq 'EXPECTED_READ_FAILURE') 'DRAIN_READ_EXCEPTION_WAS_LOST'
    $cases++
} finally { $faultReader.Dispose(); $fault.Dispose() }

# Execute the real admission statements, substituting only effect boundaries.
# A guard regression fails with a sentinel instead of starting a process/build.
$safeSource = $wrapperAst.Extent.Text
$starts = @($wrapperAst.FindAll({param($node)
    $node -is [Management.Automation.Language.InvokeMemberExpressionAst] -and $node.Static -and
    $node.Expression -is [Management.Automation.Language.TypeExpressionAst] -and
    $node.Expression.TypeName.FullName -in @('Diagnostics.Process', 'System.Diagnostics.Process') -and
    $node.Member -is [Management.Automation.Language.StringConstantExpressionAst] -and $node.Member.Value -ceq 'Start'
}, $true))
Assert-That ($starts.Count -ge 1) 'PROCESS_EFFECT_BOUNDARY_NOT_FOUND'
foreach ($start in ($starts | Sort-Object {$_.Extent.StartOffset} -Descending)) {
    $offset = $start.Extent.StartOffset - $wrapperAst.Extent.StartOffset
    $safeSource = $safeSource.Substring(0, $offset) + '$($effects.ProcessStarts++; throw ''TEST_PROCESS_START_REACHED'')' +
        $safeSource.Substring($offset + $start.Extent.Text.Length)
}
$safeTokens = $null; $safeErrors = $null
$safeAst = [Management.Automation.Language.Parser]::ParseInput($safeSource, $wrapper, [ref]$safeTokens, [ref]$safeErrors)
Assert-That ($safeErrors.Count -eq 0) 'GUARDED_ADMISSION_PARSE_FAILED'
$guardedWrapper = $safeAst.GetScriptBlock()

function Test-Refusal([hashtable]$Arguments, [string]$Expected, [bool]$NoPathProbe = $false) {
    $effects = @{ProcessStarts=0;Builds=0;SystemQueries=0;PathProbes=0}
    function cargo { $effects.Builds++; throw 'TEST_BUILD_REACHED' }
    function Get-CimInstance { $effects.SystemQueries++; throw 'TEST_SYSTEM_QUERY_REACHED' }
    function Test-Path {
        [CmdletBinding()] param([string]$LiteralPath, [string]$PathType)
        $effects.PathProbes++
        if ($LiteralPath.StartsWith('\\')) { throw 'TEST_UNC_PATH_PROBED' }
        Microsoft.PowerShell.Management\Test-Path @PSBoundParameters
    }
    $failure = $null
    try { $null = & $guardedWrapper @Arguments } catch { $failure = $_.Exception.Message }
    Assert-That ($failure -ceq $Expected) 'WRAPPER_EARLY_REFUSAL_CHANGED'
    Assert-That ($effects.ProcessStarts -eq 0 -and $effects.Builds -eq 0 -and $effects.SystemQueries -eq 0) 'WRAPPER_REACHED_EFFECT_BOUNDARY'
    if ($NoPathProbe) { Assert-That ($effects.PathProbes -eq 0) 'WRAPPER_PROBED_PATH_BEFORE_ADMISSION' }
    $script:processStarts += $effects.ProcessStarts; $script:builds += $effects.Builds; $script:systemQueries += $effects.SystemQueries
    $script:cases++
}

$temporaryBase = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
$temporaryRoot = Join-Path $temporaryBase ('catdesk-wake-wrapper-test-' + [Guid]::NewGuid().ToString('N'))
$candidate = Join-Path $temporaryRoot 'candidate.bin'; $approval = Join-Path $temporaryRoot 'approval.json'
$priorLocalAppData = [Environment]::GetEnvironmentVariable('LOCALAPPDATA', 'Process')
$cleanupVerified = $false
$temporaryCreated = $false
$createdFiles = [Collections.Generic.List[string]]::new()
$createdDirectories = [Collections.Generic.List[string]]::new()
$createdJunctions = [Collections.Generic.List[string]]::new()
try {
    Assert-That (-not [IO.Directory]::Exists($temporaryRoot)) 'TEST_DIRECTORY_ALREADY_EXISTS'
    $null = New-Item -ItemType Directory -Path $temporaryRoot -ErrorAction Stop
    $temporaryCreated = $true
    foreach ($entry in @(@($candidate, 'inert fixture image'), @($approval, '{}'))) {
        $file = [IO.File]::Open($entry[0], [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
        $createdFiles.Add($entry[0])
        try { $bytes = [Text.Encoding]::UTF8.GetBytes($entry[1]); $file.Write($bytes); $file.Flush($true) } finally { $file.Dispose() }
    }
    [Environment]::SetEnvironmentVariable('LOCALAPPDATA', $temporaryRoot, 'Process')
    Test-Refusal @{} 'WAKE_INSTALL_BROKER_UNPROVISIONED'
    Test-Refusal @{Execute=$true} 'WAKE_BOOTSTRAP_EXPLICIT_MODE_REQUIRED'
    Test-Refusal @{Bootstrap=$true;BuildOnly=$true} 'WAKE_BOOTSTRAP_POWERSHELL_7_AND_EXCLUSIVE_MODE_REQUIRED'
    Test-Refusal @{Bootstrap=$true;RebuildGui=$true} 'WAKE_BOOTSTRAP_POWERSHELL_7_AND_EXCLUSIVE_MODE_REQUIRED'
    Test-Refusal @{Bootstrap=$true} 'WAKE_BOOTSTRAP_FROZEN_IDENTITY_REQUIRED' $true
    $frozen = @{Bootstrap=$true;QualifiedHost=$candidate;ApprovalFile=$approval;ExpectedHostSha256=('0'*64);
        ExpectedApprovalSha256=[Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([IO.File]::ReadAllBytes($approval))).ToLowerInvariant();ExpectedPublicKeySha256=('1'*64)}
    $missing = $frozen.Clone(); $missing.Remove('ExpectedPublicKeySha256')
    Test-Refusal $missing 'WAKE_BOOTSTRAP_FROZEN_IDENTITY_REQUIRED' $true
    $unc = $frozen.Clone(); $unc.QualifiedHost='\\not-a-real-host\share\fixture.exe'
    Test-Refusal $unc 'WAKE_BOOTSTRAP_FROZEN_FILE_REQUIRED' $true
    if ($IsWindows) {
        $pin = [CatDeskWakeBootstrapPathPinV1]::Open($candidate)
        try {
            Assert-That ([string]::Equals($pin.CanonicalFilePath, [IO.Path]::GetFullPath($candidate), [StringComparison]::OrdinalIgnoreCase)) 'PATH_PIN_CANONICAL_PATH_CHANGED'
            $actual = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($pin.Stream)).ToLowerInvariant()
            Assert-That ($actual -ceq [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes('inert fixture image'))).ToLowerInvariant()) 'PATH_PIN_HASH_NOT_FROM_RETAINED_HANDLE'
            $pin.Revalidate()
            $cases++; $pathPinCases++
            $writer = $null; $writeRefused = $false
            try { $writer = [IO.File]::Open($candidate, [IO.FileMode]::Open, [IO.FileAccess]::Write, [IO.FileShare]::ReadWrite) }
            catch { $writeRefused = ($_.Exception.GetBaseException().HResult -band 65535) -in @(32,33) }
            finally { if ($null -ne $writer) { $writer.Dispose() } }
            Assert-That $writeRefused 'PATH_PIN_DID_NOT_EXCLUDE_LEAF_WRITER'
            Assert-That ([CatDeskWakeWrapperNativeProbeV1]::DirectoryWriteError($temporaryRoot) -in @(32,33)) 'PATH_PIN_DID_NOT_EXCLUDE_PARENT_WRITER'
            $cases++; $pathPinCases++
        } finally { $pin.Dispose() }
        Assert-That ([CatDeskWakeWrapperNativeProbeV1]::DirectoryWriteError($temporaryRoot) -eq 0) 'PATH_PIN_PARENT_HANDLE_NOT_RELEASED'
        $target = Join-Path $temporaryRoot 'junction-target'
        [void][IO.Directory]::CreateDirectory($target); $createdDirectories.Add($target)
        $junctionLeaf = Join-Path $target 'candidate.bin'
        [IO.File]::WriteAllBytes($junctionLeaf, [Text.Encoding]::UTF8.GetBytes('inert fixture image')); $createdFiles.Add($junctionLeaf)
        $junction = Join-Path $temporaryRoot 'junction-parent'
        [void](New-Item -ItemType Junction -Path $junction -Target $target); $createdJunctions.Add($junction)
        $throughJunction = Join-Path $junction 'candidate.bin'
        Assert-That (((Get-Item -LiteralPath $throughJunction).Attributes -band [IO.FileAttributes]::ReparsePoint) -eq 0) 'JUNCTION_FIXTURE_LEAF_MUST_BE_PLAIN'
        $junctionArgs = $frozen.Clone(); $junctionArgs.QualifiedHost = $throughJunction; $junctionArgs.ExpectedHostSha256 = $actual
        Test-Refusal $junctionArgs 'WAKE_BOOTSTRAP_FROZEN_REPARSE_POINT_REFUSED'
        $pathPinCases++
        [IO.Directory]::Delete($junction, $false)
        $hardlink = Join-Path $temporaryRoot 'candidate-hardlink.bin'
        [void](New-Item -ItemType HardLink -Path $hardlink -Target $candidate); $createdFiles.Add($hardlink)
        $failure = $null; $unexpectedPin = $null
        try { $unexpectedPin = [CatDeskWakeBootstrapPathPinV1]::Open($candidate) }
        catch { $failure = $_.Exception.GetBaseException().Message }
        finally { if ($null -ne $unexpectedPin) { $unexpectedPin.Dispose() } }
        Assert-That ($failure -ceq 'WAKE_BOOTSTRAP_FROZEN_FILE_REQUIRED') 'PATH_PIN_MULTILINK_LEAF_NOT_REFUSED'
        $cases++; $pathPinCases++
        [IO.File]::Delete($hardlink)
        Test-Refusal $frozen 'WAKE_BOOTSTRAP_FROZEN_FILE_CHANGED'
        $hashRefusalScope = 'ACTUAL_WRAPPER_ADMISSION'
    } else {
        # Windows drive-path admission correctly rejects native POSIX paths.
        # On other PS7 hosts exercise the actual subsequent hash phase directly.
        $tryBlocks = @($wrapperAst.FindAll({param($node)
            $node -is [Management.Automation.Language.TryStatementAst] -and
            @($node.Body.Statements | Where-Object {$_.Extent.Text.Contains("throw 'WAKE_BOOTSTRAP_FROZEN_FILE_CHANGED'")}).Count -eq 1
        }, $true))
        Assert-That ($tryBlocks.Count -eq 1) 'FROZEN_HASH_PHASE_AMBIGUOUS'
        $prefix = @($tryBlocks[0].Body.Statements | Where-Object {
            ($_ -is [Management.Automation.Language.AssignmentStatementAst] -and $_.Left.Extent.Text -in @('$imageHash','$approvalHash')) -or
            ($_ -is [Management.Automation.Language.IfStatementAst] -and $_.Extent.Text.Contains("throw 'WAKE_BOOTSTRAP_FROZEN_FILE_CHANGED'"))
        } | ForEach-Object {$_.Extent.Text})
        Assert-That ($prefix.Count -eq 3) 'PORTABLE_HASH_PHASE_AMBIGUOUS'
        $QualifiedHost=$candidate; $ApprovalFile=$approval; $ExpectedHostSha256=$frozen.ExpectedHostSha256; $ExpectedApprovalSha256=$frozen.ExpectedApprovalSha256
        $imageStream=[IO.File]::OpenRead($candidate); $approvalStream=[IO.File]::OpenRead($approval)
        $imagePin=@{Stream=$imageStream}; $approvalPin=@{Stream=$approvalStream}; $failure=$null
        try { . ([scriptblock]::Create(($prefix -join "`n"))) } catch { $failure=$_.Exception.Message }
        finally { $approvalStream.Dispose(); $imageStream.Dispose() }
        Assert-That ($failure -ceq 'WAKE_BOOTSTRAP_FROZEN_FILE_CHANGED') 'PORTABLE_HASH_PHASE_REFUSAL_CHANGED'
        $cases++; $hashRefusalScope='ACTUAL_HASH_PHASE_NON_WINDOWS'
    }
} finally {
    [Environment]::SetEnvironmentVariable('LOCALAPPDATA', $priorLocalAppData, 'Process')
    # Only literal known files, junction entries and empty owned directories.
    Assert-That ([IO.Path]::GetDirectoryName([IO.Path]::GetFullPath($temporaryRoot)) -ceq [IO.Path]::TrimEndingDirectorySeparator($temporaryBase)) 'TEST_CLEANUP_ROOT_MISMATCH'
    if ($temporaryCreated) {
        Assert-That (([IO.File]::GetAttributes($temporaryRoot) -band [IO.FileAttributes]::ReparsePoint) -eq 0) 'TEST_CLEANUP_DIRECTORY_REPLACED'
        foreach ($path in $createdJunctions) {
            if (Test-Path -LiteralPath $path) {
                Assert-That ([IO.Path]::GetDirectoryName($path) -ceq $temporaryRoot) 'TEST_JUNCTION_CLEANUP_PATH_MISMATCH'
                Assert-That (((Get-Item -LiteralPath $path).Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) 'TEST_JUNCTION_REPLACED'
                [IO.Directory]::Delete($path,$false)
            }
        }
        foreach ($path in $createdFiles) {
            if ($createdFiles.Contains($path) -and [IO.File]::Exists($path)) { [IO.File]::Delete($path) }
        }
        foreach ($path in $createdDirectories) {
            Assert-That ([IO.Path]::GetDirectoryName($path) -ceq $temporaryRoot) 'TEST_CHILD_CLEANUP_PATH_MISMATCH'
            Assert-That (([IO.File]::GetAttributes($path) -band [IO.FileAttributes]::ReparsePoint) -eq 0) 'TEST_CHILD_DIRECTORY_REPLACED'
            [IO.Directory]::Delete($path,$false)
        }
        [IO.Directory]::Delete($temporaryRoot, $false)
    }
    $cleanupVerified = -not [IO.Directory]::Exists($temporaryRoot)
    Assert-That $cleanupVerified 'TEST_FILES_NOT_CLEANED'
}
[ordered]@{passed=$true;totalPassCount=$cases;assertionCount=$assertions;retainedOutputUnits='UTF16_CHARACTERS';
    largeDrainAllocatedBytes=$allocatedBytes;hashRefusalScope=$hashRefusalScope;processStarts=$processStarts;builds=$builds;
    systemQueries=$systemQueries;temporaryCleanupVerified=$cleanupVerified;nativeBootstrapCalls=0;serviceChanges=0;
    nativePathPinCases=$pathPinCases;nativePathPinScope='OWNED_TEMP_INERT_FILES_ONLY'} | ConvertTo-Json -Compress
