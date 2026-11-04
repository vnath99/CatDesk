$ErrorActionPreference = 'Stop'
# Test-only SHA256 helper independent of optional PowerShell module autoload.
function Get-FixtureHash {
    param([string]$LiteralPath, [ValidateSet('SHA256')][string]$Algorithm = 'SHA256')
    $sha = [Security.Cryptography.SHA256]::Create()
    $stream = [IO.File]::OpenRead($LiteralPath)
    try { [pscustomobject]@{ Hash = ([BitConverter]::ToString($sha.ComputeHash($stream)).Replace('-', '')) } }
    finally { $sha.Dispose(); $stream.Dispose() }
}
Set-Alias -Name Get-FileHash -Value Get-FixtureHash -Scope Script

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
$hashFunction = $launcherAst.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Get-RestartSha256' }, $true)
Require ($null -ne $hashFunction) 'restart hashing helper is missing'
Invoke-Expression $hashFunction.Extent.Text
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
$launchArgumentsFunction = $workerAst.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Get-RestartLaunchArguments' }, $true)
Require ($null -ne $launchArgumentsFunction) 'backend profile launch argument selector is missing'
Invoke-Expression $launchArgumentsFunction.Extent.Text
Require ((@(Get-RestartLaunchArguments $true) -join '|') -ceq '--catdesk-daemon|--backend-only') 'local profile launches only the closed native backend tuple'
Require ((@(Get-RestartLaunchArguments $false) -join '|') -ceq '--auto-start-computer') 'legacy default launch tuple changed'
$contextFunction=$workerAst.Find({param($node)$node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Get-RestartBackendOnlyContext'},$true)
Invoke-Expression $contextFunction.Extent.Text
$portRefused=$false;try{[void](Get-RestartBackendOnlyContext 3201)}catch{$portRefused=$_.Exception.Message -ceq 'Backend-only handoff requires the fixed local port 3200.'}
Require $portRefused 'a noncanonical native port must refuse before configuration or process changes'
$mcpFunction=$workerAst.Find({param($node)$node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Test-RestartBackendOnlyMcp'},$true)
Invoke-Expression $mcpFunction.Extent.Text
$McpPort=3200;$script:backendRequests=[Collections.Generic.List[object]]::new()
function Invoke-WebRequest {
    param([switch]$UseBasicParsing,[string]$Uri,[string]$Method,[string]$ContentType,[string]$Body,[int]$MaximumRedirection,[int]$TimeoutSec,[hashtable]$Headers)
    $script:backendRequests.Add(@{uri=$Uri;headers=$Headers;body=$Body;redirects=$MaximumRedirection})
    if(!$Headers){return [pscustomobject]@{StatusCode=401;Content=''}}
    $request=$Body|ConvertFrom-Json
    return [pscustomobject]@{StatusCode=200;Content=$(if($request.method -eq 'initialize'){'{"jsonrpc":"2.0","id":2,"result":{"serverInfo":{"name":"fixture"}}}'}else{'{"jsonrpc":"2.0","id":2,"result":{"tools":[{"name":"fixture"}]}}'})}
}
try {
    $context=@{Uri=('http://127.0.0.1:3200/'+('a'*24)+'/mcp');Token=('t'*24)}
    Require (Test-RestartBackendOnlyMcp $context) 'local backend readiness requires both authorized MCP exchanges'
    Require ($script:backendRequests.Count -eq 3 -and !$script:backendRequests[0].headers -and $script:backendRequests[1].headers.Authorization -ceq ('Bearer '+$context.Token) -and $script:backendRequests[2].redirects -eq 0) 'MCP readiness lost its unauthorized/authenticated/no-redirect checks'
    $script:backendRequests.Clear();$context.Uri='http://example.test/mcp'
    Require (-not (Test-RestartBackendOnlyMcp $context) -and $script:backendRequests.Count -eq 0) 'backend token must never be sent outside the exact loopback scope'
} finally { Remove-Item Function:\Invoke-WebRequest }

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

    # Exercise the actual refusal path with only harmless PowerShell children.
    # PowerShell cannot satisfy CatDesk's closed capability tuple. The selected
    # old process must therefore remain alive, without a stop or replacement.
    $shell = (Get-Process -Id $PID -ErrorAction Stop).Path
    $old = $null; $worker = $null
    try {
        $oldInfo=[Diagnostics.ProcessStartInfo]::new();$oldInfo.FileName=$shell;$oldInfo.Arguments='-NoProfile -NonInteractive -Command "Start-Sleep -Seconds 60"';$oldInfo.UseShellExecute=$false;$oldInfo.CreateNoWindow=$true
        $old=[Diagnostics.Process]::Start($oldInfo);$null=$old.Handle
        $start=$old.StartTime.ToUniversalTime().ToString('o');$shellHash=(Get-FileHash -LiteralPath $shell -Algorithm SHA256).Hash.ToLowerInvariant()
        $workerInfo=[Diagnostics.ProcessStartInfo]::new();$workerInfo.FileName=$shell;$workerInfo.UseShellExecute=$false;$workerInfo.CreateNoWindow=$true;$workerInfo.RedirectStandardOutput=$true;$workerInfo.RedirectStandardError=$true
        $workerInfo.Arguments='-NoProfile -NonInteractive -File "'+$workerPath+'" -OldPid '+$old.Id+' -ExpectedOldProcessStartedAtUtc "'+$start+'" -ExpectedOldProcessPath "'+$shell+'" -BuildPath "'+$shell+'" -Workspace "'+$tempRoot+'" -McpPort 3200 -StartupDelaySeconds 0 -BackendOnly -ExpectedBuildSha256 '+$shellHash
        $worker=[Diagnostics.Process]::Start($workerInfo);$stdout=$worker.StandardOutput.ReadToEndAsync();$stderr=$worker.StandardError.ReadToEndAsync()
        Require ($worker.WaitForExit(15000) -and $stdout.Wait(1000) -and $stderr.Wait(1000) -and $worker.ExitCode -ne 0) 'unsupported replacement must refuse within the bounded preflight'
        Require (-not $old.HasExited) 'unsupported backend target stopped the old process'
        Require (Test-Path -LiteralPath (Join-Path $tempRoot '.catdesk/restart-handoff/latest.json')) ("unsupported-target worker did not reach the recorded gate: " + $stderr.Result)
        $state=Get-Content -LiteralPath (Join-Path $tempRoot '.catdesk/restart-handoff/latest.json') -Raw|ConvertFrom-Json
        Require ($state.status -ceq 'FAILED_OPERATOR_ATTENTION' -and $state.stage -ceq 'backend-only-preflight') 'unsupported target did not stop at the capability gate'
    } finally {
        foreach($owned in @($worker,$old)){if($null -ne $owned){if(-not $owned.HasExited){$owned.Kill();[void]$owned.WaitForExit(5000)};$owned.Dispose()}}
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
