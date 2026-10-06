[CmdletBinding()]
param(
    [string]$PythonExecutable = ""
)

$ErrorActionPreference = "Stop"
$workspace = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$wakeRoot = Join-Path $workspace ".catdesk\wake-bridge"
$venvRoot = Join-Path $wakeRoot "venv"
$venvPython = Join-Path $venvRoot "Scripts\python.exe"
$backupRoot = Join-Path $wakeRoot "venv.pre-repair"
$requirements = Join-Path $PSScriptRoot "requirements-wake-bridge.txt"
$tests = Join-Path $workspace "tests\test_wake_bridge.py"

function ConvertTo-NativeArgument {
    param([string]$Value)
    if ($null -eq $Value -or $Value.Length -eq 0) { return '""' }
    if ($Value -notmatch '[\s"]') { return $Value }
    $escaped = [regex]::Replace($Value, '(\\*)"', '$1$1\"')
    $escaped = [regex]::Replace($escaped, '(\\+)$', '$1$1')
    return '"' + $escaped + '"'
}

function Initialize-WakeRepairContainedProcessType {
    if ("CatDesk.WakeRepairContainedProcess" -as [type]) { return }
    Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Diagnostics;
using System.Runtime.InteropServices;

namespace CatDesk {
    public sealed class WakeRepairContainedResult {
        public int ExitCode;
        public bool TimedOut;
        public bool TerminationFailed;
    }

    public static class WakeRepairContainedProcess {
        private const uint JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE = 0x00002000;
        private const int JobObjectExtendedLimitInformation = 9;

        [StructLayout(LayoutKind.Sequential)]
        private struct IO_COUNTERS {
            public UInt64 ReadOperationCount;
            public UInt64 WriteOperationCount;
            public UInt64 OtherOperationCount;
            public UInt64 ReadTransferCount;
            public UInt64 WriteTransferCount;
            public UInt64 OtherTransferCount;
        }

        [StructLayout(LayoutKind.Sequential)]
        private struct JOBOBJECT_BASIC_LIMIT_INFORMATION {
            public Int64 PerProcessUserTimeLimit;
            public Int64 PerJobUserTimeLimit;
            public UInt32 LimitFlags;
            public UIntPtr MinimumWorkingSetSize;
            public UIntPtr MaximumWorkingSetSize;
            public UInt32 ActiveProcessLimit;
            public UIntPtr Affinity;
            public UInt32 PriorityClass;
            public UInt32 SchedulingClass;
        }

        [StructLayout(LayoutKind.Sequential)]
        private struct JOBOBJECT_EXTENDED_LIMIT_INFORMATION {
            public JOBOBJECT_BASIC_LIMIT_INFORMATION BasicLimitInformation;
            public IO_COUNTERS IoInfo;
            public UIntPtr ProcessMemoryLimit;
            public UIntPtr JobMemoryLimit;
            public UIntPtr PeakProcessMemoryUsed;
            public UIntPtr PeakJobMemoryUsed;
        }

        [DllImport("kernel32.dll", CharSet = CharSet.Unicode)]
        private static extern IntPtr CreateJobObject(IntPtr lpJobAttributes, string lpName);

        [DllImport("kernel32.dll", SetLastError = true)]
        private static extern bool SetInformationJobObject(IntPtr hJob, int infoClass, IntPtr lpJobObjectInfo, uint cbJobObjectInfoLength);

        [DllImport("kernel32.dll", SetLastError = true)]
        private static extern bool AssignProcessToJobObject(IntPtr hJob, IntPtr hProcess);

        [DllImport("kernel32.dll", SetLastError = true)]
        private static extern bool CloseHandle(IntPtr hObject);

        private static IntPtr CreateKillOnCloseJob() {
            var job = CreateJobObject(IntPtr.Zero, null);
            if (job == IntPtr.Zero) throw new Win32Exception(Marshal.GetLastWin32Error(), "wake repair containment job could not be created");
            var info = new JOBOBJECT_EXTENDED_LIMIT_INFORMATION();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            var size = Marshal.SizeOf(typeof(JOBOBJECT_EXTENDED_LIMIT_INFORMATION));
            var pointer = Marshal.AllocHGlobal(size);
            try {
                Marshal.StructureToPtr(info, pointer, false);
                if (!SetInformationJobObject(job, JobObjectExtendedLimitInformation, pointer, (uint)size)) {
                    var error = Marshal.GetLastWin32Error();
                    CloseHandle(job);
                    throw new Win32Exception(error, "wake repair containment job could not be configured");
                }
            } finally {
                Marshal.FreeHGlobal(pointer);
            }
            return job;
        }

        public static WakeRepairContainedResult Run(string fileName, string arguments, int timeoutMilliseconds) {
            var start = new ProcessStartInfo(fileName, arguments);
            start.UseShellExecute = false;
            start.CreateNoWindow = true;
            start.RedirectStandardInput = true;
            IntPtr job = IntPtr.Zero;
            Process process = null;
            try {
                job = CreateKillOnCloseJob();
                process = Process.Start(start);
                if (process == null) throw new InvalidOperationException("wake repair native process did not start");
                process.StandardInput.Close();
                if (!AssignProcessToJobObject(job, process.Handle)) {
                    var error = Marshal.GetLastWin32Error();
                    try { process.Kill(); } catch (InvalidOperationException) {}
                    process.WaitForExit(2000);
                    throw new Win32Exception(error, "wake repair native process could not be contained");
                }
                var timedOut = !process.WaitForExit(timeoutMilliseconds);
                if (timedOut) {
                    // Closing a KILL_ON_JOB_CLOSE job is the mutation boundary:
                    // Python/venv/pip descendants are terminated with the parent.
                    CloseHandle(job);
                    job = IntPtr.Zero;
                    var terminated = process.WaitForExit(2000);
                    return new WakeRepairContainedResult {
                        TimedOut = true,
                        TerminationFailed = !terminated,
                        ExitCode = terminated ? process.ExitCode : -1,
                    };
                }
                var exitCode = process.ExitCode;
                // Successful repair children are not allowed to leave detached
                // descendants behind. Closing the job enforces that invariant.
                CloseHandle(job);
                job = IntPtr.Zero;
                return new WakeRepairContainedResult { ExitCode = exitCode };
            } finally {
                if (job != IntPtr.Zero) CloseHandle(job);
                if (process != null) process.Dispose();
            }
        }
    }
}
'@
}

function Invoke-NativeQuiet {
    param(
        [Parameter(Mandatory = $true)][string]$Executable,
        [Parameter(Mandatory = $true)][string[]]$Arguments,
        [int]$TimeoutMilliseconds = 120000
    )
    if ($TimeoutMilliseconds -lt 1) { throw "wake repair native timeout is invalid" }
    $argumentLine = [string]::Join(' ', @($Arguments | ForEach-Object { ConvertTo-NativeArgument -Value $_ }))
    Initialize-WakeRepairContainedProcessType
    $result = [CatDesk.WakeRepairContainedProcess]::Run($Executable, $argumentLine, $TimeoutMilliseconds)
    if ($result.TimedOut) {
        if ($result.TerminationFailed) {
            throw "wake repair native process exceeded bounded timeout and its contained process tree did not terminate"
        }
        throw "wake repair native process exceeded bounded timeout and its contained process tree was killed"
    }
    return [pscustomobject]@{ ExitCode = $result.ExitCode }
}

function Test-Python3 {
    param([string]$Candidate)
    if ([string]::IsNullOrWhiteSpace($Candidate) -or -not (Test-Path -LiteralPath $Candidate -PathType Leaf)) {
        return $false
    }
    try {
        $result = Invoke-NativeQuiet -Executable $Candidate -Arguments @("-c", "import sys; raise SystemExit(0 if sys.version_info >= (3,10) else 1)")
        return $result.ExitCode -eq 0
    } catch {
        return $false
    }
}

function Get-TrustedPythonExecutableCandidate {
    param([string]$Candidate)

    if ([string]::IsNullOrWhiteSpace($Candidate)) { return $null }
    try {
        $full = [IO.Path]::GetFullPath($Candidate)
        if (-not (Test-Path -LiteralPath $full -PathType Leaf)) { return $null }
        $item = Get-Item -LiteralPath $full -Force -ErrorAction Stop
        if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) { return $null }
        $resolved = [IO.Path]::GetFullPath($item.FullName)
        if (-not [string]::Equals($full, $resolved, [StringComparison]::OrdinalIgnoreCase)) { return $null }
        return $resolved
    } catch {
        return $null
    }
}

function Resolve-Python3 {
    if (-not [string]::IsNullOrWhiteSpace($PythonExecutable)) {
        # An explicit path is intentional operator authority. Keep accepting it
        # independently of the default discovery policy, while retaining the
        # existing bounded Python-version validation before use.
        $candidate = [IO.Path]::GetFullPath($PythonExecutable)
        if (-not (Test-Python3 -Candidate $candidate)) {
            throw "PYTHON3_INVALID: -PythonExecutable must identify a working Python 3.10+ executable."
        }
        return $candidate
    }

    # Default recovery discovery must not inherit executable authority from
    # caller PATH, LOCALAPPDATA, USERPROFILE, or command-resolution state.
    $localAppData = [System.Environment]::GetFolderPath([System.Environment+SpecialFolder]::LocalApplicationData)
    $userProfile = [System.Environment]::GetFolderPath([System.Environment+SpecialFolder]::UserProfile)
    $windowsRoot = [IO.Directory]::GetParent([System.Environment]::SystemDirectory).FullName
    $launcherCandidates = @(
        (Join-Path $localAppData "Programs\Python\Launcher\py.exe"),
        (Join-Path $windowsRoot "py.exe")
    )

    foreach ($launcherCandidate in $launcherCandidates) {
        $py = Get-TrustedPythonExecutableCandidate -Candidate $launcherCandidate
        if ([string]::IsNullOrWhiteSpace($py)) { continue }
        foreach ($selector in @("-3.12", "-3")) {
            $candidateFile = [IO.Path]::GetTempFileName()
            try {
                # Launcher discovery is itself a native child of one-command
                # recovery. Keep it inside the same bounded process primitive
                # as every subsequent Python validation/repair operation. Use
                # a temporary file rather than redirected stdout so discovery
                # cannot introduce a separate unbounded pipe-drain boundary.
                $candidateResult = Invoke-NativeQuiet -Executable $py -Arguments @(
                    $selector,
                    "-c",
                    "import pathlib, sys; pathlib.Path(sys.argv[1]).write_text(sys.executable, encoding='utf-8')",
                    $candidateFile
                ) -TimeoutMilliseconds 5000
                if ($candidateResult.ExitCode -eq 0 -and (Test-Path -LiteralPath $candidateFile -PathType Leaf)) {
                    $reportedCandidate = ([IO.File]::ReadAllText($candidateFile)).Trim()
                    $candidate = Get-TrustedPythonExecutableCandidate -Candidate $reportedCandidate
                    if ($candidate -and (Test-Python3 -Candidate $candidate)) { return $candidate }
                }
            } catch {
            } finally {
                Remove-Item -LiteralPath $candidateFile -Force -ErrorAction SilentlyContinue
            }
        }
    }

    $standardCandidates = @(
        (Join-Path $localAppData "Programs\Python\Python313\python.exe"),
        (Join-Path $localAppData "Programs\Python\Python312\python.exe"),
        (Join-Path $localAppData "Programs\Python\Python311\python.exe"),
        (Join-Path $userProfile "miniconda3\python.exe"),
        (Join-Path $userProfile "anaconda3\python.exe")
    )
    foreach ($standardCandidate in $standardCandidates) {
        $candidate = Get-TrustedPythonExecutableCandidate -Candidate $standardCandidate
        if ($candidate -and (Test-Python3 -Candidate $candidate)) { return $candidate }
    }

    throw "PYTHON3_NOT_FOUND: install Python 3.12 for the current user, then rerun this script."
}

if (-not (Test-Path -LiteralPath $requirements -PathType Leaf)) {
    throw "Pinned wake-bridge requirements file is missing."
}
if (-not (Test-Path -LiteralPath $tests -PathType Leaf)) {
    throw "Wake-bridge deterministic test suite is missing."
}

$python3 = Resolve-Python3
Write-Host "PYTHON3_READY"

# Prove the selected interpreter can create venvs before moving anything.
$probeRoot = Join-Path $wakeRoot "venv-repair-probe"
if (Test-Path -LiteralPath $probeRoot) {
    Remove-Item -LiteralPath $probeRoot -Recurse -Force
}
try {
    $probe = Invoke-NativeQuiet -Executable $python3 -Arguments @("-m", "venv", $probeRoot)
    if ($probe.ExitCode -ne 0 -or -not (Test-Path -LiteralPath (Join-Path $probeRoot "Scripts\python.exe") -PathType Leaf)) {
        throw "Selected Python cannot create a virtual environment."
    }
} finally {
    Remove-Item -LiteralPath $probeRoot -Recurse -Force -ErrorAction SilentlyContinue
}

if (Test-Path -LiteralPath $backupRoot) {
    throw "WAKE_VENV_BACKUP_EXISTS: remove or inspect .catdesk\wake-bridge\venv.pre-repair before retrying."
}

$hadExistingVenv = Test-Path -LiteralPath $venvRoot
if ($hadExistingVenv) {
    Move-Item -LiteralPath $venvRoot -Destination $backupRoot
}

$repairSucceeded = $false
try {
    New-Item -ItemType Directory -Force -Path $wakeRoot | Out-Null
    $create = Invoke-NativeQuiet -Executable $python3 -Arguments @("-m", "venv", $venvRoot)
    if ($create.ExitCode -ne 0 -or -not (Test-Path -LiteralPath $venvPython -PathType Leaf)) {
        throw "Wake-bridge virtual-environment creation failed."
    }

    # Dependency installation is the slowest legitimate repair child. It is
    # still finite so one-command recovery cannot wait forever on pip/network.
    $pip = Invoke-NativeQuiet -Executable $venvPython -Arguments @("-m", "pip", "install", "--disable-pip-version-check", "--no-input", "-r", $requirements) -TimeoutMilliseconds 600000
    if ($pip.ExitCode -ne 0) {
        throw "Pinned SeleniumBase installation failed."
    }

    $importCheck = Invoke-NativeQuiet -Executable $venvPython -Arguments @("-c", "import seleniumbase")
    if ($importCheck.ExitCode -ne 0) {
        throw "SeleniumBase import verification failed."
    }

    $testRun = Invoke-NativeQuiet -Executable $venvPython -Arguments @("-m", "unittest", "tests/test_wake_bridge.py", "-v")
    if ($testRun.ExitCode -ne 0) {
        throw "Wake-bridge deterministic Python tests failed."
    }

    $repairSucceeded = $true
} finally {
    if (-not $repairSucceeded) {
        Remove-Item -LiteralPath $venvRoot -Recurse -Force -ErrorAction SilentlyContinue
        if ($hadExistingVenv -and (Test-Path -LiteralPath $backupRoot)) {
            Move-Item -LiteralPath $backupRoot -Destination $venvRoot
        }
    }
}

if (Test-Path -LiteralPath $backupRoot) {
    Remove-Item -LiteralPath $backupRoot -Recurse -Force
}

Write-Host "WAKE_BRIDGE_ENV_REPAIRED"
Write-Host "Pinned SeleniumBase import succeeded and deterministic wake tests passed. Browser profile/config were not opened or changed."
