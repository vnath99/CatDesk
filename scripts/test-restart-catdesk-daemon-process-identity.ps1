$ErrorActionPreference = 'Stop'

function Require([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw $Message }
}

$launcherPath = Join-Path $PSScriptRoot 'restart_catdesk_daemon.ps1'
$workerPath = Join-Path $PSScriptRoot 'restart_catdesk_daemon_worker.ps1'
$promotionPath = Join-Path $PSScriptRoot 'promote-reviewed-catdesk-build.ps1'
$launcherSource = Get-Content -LiteralPath $launcherPath -Raw
$workerSource = Get-Content -LiteralPath $workerPath -Raw
$promotionSource = Get-Content -LiteralPath $promotionPath -Raw

foreach ($path in @($launcherPath, $workerPath, $promotionPath)) {
    $tokens = $null
    $errors = $null
    [void][System.Management.Automation.Language.Parser]::ParseFile($path, [ref]$tokens, [ref]$errors)
    Require ($errors.Count -eq 0) ("PowerShell parser rejected {0}: {1}" -f $path, (($errors | ForEach-Object { $_.Message }) -join '; '))
}

$tokens = $null
$errors = $null
$launcherAst = [System.Management.Automation.Language.Parser]::ParseFile($launcherPath, [ref]$tokens, [ref]$errors)
$promotionIdentityFunction = $launcherAst.Find({
    param($node)
    $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and
        $node.Name -eq 'Test-ExpectedOldProcessIdentity'
}, $true)
Require ($null -ne $promotionIdentityFunction) 'restart launcher is missing the promotion exact-process identity helper'
Invoke-Expression $promotionIdentityFunction.Extent.Text

$trustedPowerShellFunction = $launcherAst.Find({
    param($node)
    $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and
        $node.Name -eq 'Resolve-TrustedWindowsPowerShellPath'
}, $true)
Require ($null -ne $trustedPowerShellFunction) 'restart launcher is missing the trusted Windows PowerShell resolver'
Invoke-Expression $trustedPowerShellFunction.Extent.Text

$trustedWorkerFunction = $launcherAst.Find({
    param($node)
    $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and
        $node.Name -eq 'Resolve-TrustedRestartWorkerScriptPath'
}, $true)
Require ($null -ne $trustedWorkerFunction) 'restart launcher is missing the trusted worker-script resolver'
$trustedWorkerSource = $trustedWorkerFunction.Extent.Text
foreach ($requiredGuard in @('Get-Item', 'FileInfo', 'ReparsePoint', 'GetFullPath', 'OrdinalIgnoreCase')) {
    Require ($trustedWorkerSource -match [regex]::Escape($requiredGuard)) ("restart worker identity resolver lost {0}" -f $requiredGuard)
}

$tokens = $null
$errors = $null
$workerAst = [System.Management.Automation.Language.Parser]::ParseFile($workerPath, [ref]$tokens, [ref]$errors)
$identityFunction = $workerAst.Find({
    param($node)
    $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and
        $node.Name -eq 'Test-PinnedRestartTargetIdentity'
}, $true)
Require ($null -ne $identityFunction) 'restart worker is missing the exact-process identity helper'
Invoke-Expression $identityFunction.Extent.Text

$tempRoot = Join-Path ([IO.Path]::GetTempPath()) ("catdesk-restart-identity-{0}" -f [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $tempRoot -Force | Out-Null
try {
    $expectedPath = Join-Path $tempRoot 'catdesk.exe'
    [IO.File]::WriteAllText($expectedPath, 'fixture')
    $started = [DateTime]::Parse('2026-09-07T12:00:00.1234567Z').ToUniversalTime()
    $expectedStarted = $started.ToString('o')

    $same = [pscustomobject]@{
        Handle = [IntPtr]1
        ProcessName = 'catdesk'
        StartTime = $started
        Path = $expectedPath
    }
    Require (Test-PinnedRestartTargetIdentity -Process $same -ExpectedStartedAtUtc $expectedStarted -ExpectedPath $expectedPath) 'exact pinned restart target was rejected'
    $expectedHash = (Get-FileHash -LiteralPath $expectedPath -Algorithm SHA256).Hash.ToLowerInvariant()
    Require (Test-ExpectedOldProcessIdentity -Process $same -ExpectedStartedAtUtc $expectedStarted -ExpectedPath $expectedPath -ExpectedSha256 $expectedHash) 'promotion handoff rejected the exact reviewed listener instance'

    $reusedPid = [pscustomobject]@{
        Handle = [IntPtr]2
        ProcessName = 'catdesk'
        StartTime = $started.AddTicks(1)
        Path = $expectedPath
    }
    Require (-not (Test-PinnedRestartTargetIdentity -Process $reusedPid -ExpectedStartedAtUtc $expectedStarted -ExpectedPath $expectedPath)) 'same-path replacement process instance was accepted'
    Require (-not (Test-ExpectedOldProcessIdentity -Process $reusedPid -ExpectedStartedAtUtc $expectedStarted -ExpectedPath $expectedPath -ExpectedSha256 $expectedHash)) 'promotion handoff accepted a same-path different process instance'
    Require (-not (Test-ExpectedOldProcessIdentity -Process $same -ExpectedStartedAtUtc $expectedStarted -ExpectedPath $expectedPath -ExpectedSha256 ('0' * 64))) 'promotion handoff accepted the wrong reviewed executable hash'

    $otherPath = Join-Path $tempRoot 'other-catdesk.exe'
    [IO.File]::WriteAllText($otherPath, 'other')
    $wrongPath = [pscustomobject]@{
        Handle = [IntPtr]3
        ProcessName = 'catdesk'
        StartTime = $started
        Path = $otherPath
    }
    Require (-not (Test-PinnedRestartTargetIdentity -Process $wrongPath -ExpectedStartedAtUtc $expectedStarted -ExpectedPath $expectedPath)) 'wrong-path process instance was accepted'

    $pathHijackRoot = Join-Path $tempRoot 'path-hijack'
    New-Item -ItemType Directory -Path $pathHijackRoot -Force | Out-Null
    [IO.File]::WriteAllText((Join-Path $pathHijackRoot 'powershell.exe'), 'not powershell')
    $priorPath = $env:PATH
    try {
        $env:PATH = "$pathHijackRoot;$priorPath"
        $trustedPowerShell = Resolve-TrustedWindowsPowerShellPath
        $expectedPowerShell = [IO.Path]::GetFullPath((Join-Path ([Environment]::SystemDirectory) 'WindowsPowerShell\v1.0\powershell.exe'))
        Require ($trustedPowerShell.Equals($expectedPowerShell, [StringComparison]::OrdinalIgnoreCase)) 'trusted PowerShell resolver was influenced by PATH search'
        $trustedItem = Get-Item -LiteralPath $trustedPowerShell -Force -ErrorAction Stop
        Require (($trustedItem.Attributes -band [IO.FileAttributes]::ReparsePoint) -eq 0) 'trusted PowerShell resolver accepted a reparse point'
    } finally {
        $env:PATH = $priorPath
    }
} finally {
    Remove-Item -LiteralPath $tempRoot -Recurse -Force -ErrorAction SilentlyContinue
}

Require ($launcherSource -match '\$null\s*=\s*\$oldProcess\.Handle') 'launcher does not pin the selected old process before recording identity'
Require ($launcherSource -match 'Test-ExpectedOldProcessIdentity\s+-Process\s+\$oldProcess') 'launcher does not re-prove promotion-supplied old-process identity before handoff'
Require ($promotionSource -match '\$null\s*=\s*\$process\.Handle') 'promotion does not pin the reviewed listener process before recording identity'
Require ($promotionSource -match 'ProcessStartedAtUtc\s*=\s*\$actualStartedAtUtc') 'promotion listener evidence omits exact process start identity'
Require ($promotionSource -match 'BuildPath\s*=\s*\$actualPath') 'promotion listener evidence omits executable path identity'
Require ($promotionSource -match 'BuildHash\s*=\s*\$actualHash') 'promotion listener evidence omits verified executable hash identity'
Require ($promotionSource -match '-ExpectedOldProcessStartedAtUtc\s+\(\[string\]\$source\.ProcessStartedAtUtc\)') 'promotion does not transfer reviewed process start identity to restart launcher'
Require ($promotionSource -match '-ExpectedOldProcessPath\s+\(\[string\]\$source\.BuildPath\)') 'promotion does not transfer reviewed process path identity to restart launcher'
Require ($promotionSource -match '-ExpectedOldProcessSha256\s+\(\[string\]\$source\.BuildHash\)') 'promotion does not transfer reviewed process hash identity to restart launcher'
Require ($launcherSource -match '"-ExpectedOldProcessStartedAtUtc",\s*\$oldProcessStartedAtUtc') 'launcher does not transfer exact old-process creation identity to worker'
Require ($launcherSource -match '"-ExpectedOldProcessPath",\s*\$oldProcessPath') 'launcher does not transfer exact old-process path identity to worker'
Require ($launcherSource -match '\[Environment\]::SystemDirectory') 'restart launcher does not derive PowerShell from the OS system directory'
Require ($launcherSource -match 'Resolve-TrustedWindowsPowerShellPath') 'restart launcher does not resolve a trusted Windows PowerShell executable'
Require ($launcherSource -match '\$worker\s*=\s*Resolve-TrustedRestartWorkerScriptPath') 'restart launcher does not resolve exact worker-script identity before handoff'
Require ($launcherSource -notmatch '\$worker\s*=\s*Join-Path\s+\$PSScriptRoot\s+["'']restart_catdesk_daemon_worker\.ps1["''][\s\S]{0,200}Test-Path\s+-LiteralPath\s+\$worker\s+-PathType\s+Leaf') 'restart launcher regressed to leaf-existence-only worker trust'
Require ($launcherSource -match 'Start-Process\s+-FilePath\s+\$trustedPowerShell') 'restart launcher does not launch through the trusted PowerShell path'
Require ($launcherSource -notmatch 'Start-Process\s+-FilePath\s+["'']powershell\.exe["'']') 'restart launcher still grants executable-search authority to a powershell.exe basename'
Require ($workerSource -match '\$oldProcess\s*=\s*Get-Process\s+-Id\s+\$OldPid[\s\S]*Test-PinnedRestartTargetIdentity') 'worker does not bind its one PID lookup to exact-process revalidation'
Require ($workerSource -notmatch '(?im)^\s*Stop-Process\s+-Id\s+\$OldPid\b') 'worker still performs destructive PID-only stop'
Require ($workerSource -notmatch '(?im)^\s*Wait-Process\s+-Id\s+\$OldPid\b') 'worker still performs PID-only wait'
Require ($workerSource -match '\$oldProcess\.Kill\(\)') 'worker does not terminate through the exact pinned process object'
Require ($workerSource -match '\$oldProcess\.WaitForExit\(') 'worker does not wait through the exact pinned process object'
Require ($workerSource -notmatch 'Get-Process\s+-Id\s+\$newProcess\.Id') 'replacement readiness still reacquires the launched process by PID'
Require ($workerSource -match '\$null\s*=\s*\$newProcess\.Handle') 'replacement process is not pinned immediately after launch'
Require ($workerSource -match '\$newProcess\.HasExited') 'replacement readiness does not observe the exact launched process object'

Write-Output 'restart handoff exact-process identity tests passed'
