[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Import-Module Microsoft.PowerShell.Utility -ErrorAction Stop
# Isolated fixture hash probe: avoid depending on PowerShell module autoload.
function Get-FixtureHash {
    param([string]$LiteralPath, [ValidateSet('SHA256')][string]$Algorithm = 'SHA256')
    $sha = [Security.Cryptography.SHA256]::Create()
    $stream = [IO.File]::OpenRead($LiteralPath)
    try { [pscustomobject]@{ Hash = ([BitConverter]::ToString($sha.ComputeHash($stream)).Replace('-', '')) } }
    finally { $sha.Dispose(); $stream.Dispose() }
}
Set-Alias -Name Get-FileHash -Value Get-FixtureHash -Scope Script
$workspace = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$facade = Join-Path $workspace 'catdesk.ps1'
$source = Get-Content -LiteralPath $facade -Raw
$tokens = $null; $errors = $null
[void][System.Management.Automation.Language.Parser]::ParseInput($source, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'consumer lifecycle facade did not parse' }

function Require([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw "assertion failed: $Message" }
}

function Invoke-FixtureFacade(
    [string]$Command,
    [hashtable]$Seams,
    [string]$AutostartAction = '',
    [string]$WakeTerminalTaskId = '',
    [string]$FixtureWorkspace = $workspace,
    [string]$FixtureConfigPath = 'C:\fixture\config.toml',
    [string]$FixtureExpectedBuildSha256 = '',
    [string]$FixtureFingerprintPath = 'C:\fixture\catdesk.exe.sha256',
    [int]$FixtureReadyTimeoutSeconds = 180,
    [switch]$FixtureBackendOnly
) {
    $prior = Get-Location
    try {
        Set-Location ([IO.Path]::GetTempPath())
        return & $facade -Command $Command -AutostartAction $AutostartAction -WakeTerminalTaskId $WakeTerminalTaskId -Workspace $FixtureWorkspace -ConfigPath $FixtureConfigPath -ExpectedBuildSha256 $FixtureExpectedBuildSha256 -BuildFingerprintPath $FixtureFingerprintPath -ReadyTimeoutSeconds $FixtureReadyTimeoutSeconds -BackendOnly:$FixtureBackendOnly -TestSeams $Seams
    } finally {
        Set-Location $prior
    }
}

function New-OwnedTask($Definition) {
    [pscustomobject]@{
        TaskName = $Definition.TaskName
        TaskPath = $Definition.TaskPath
        State = 'Ready'
        Actions = @([pscustomobject]@{ Execute = $Definition.Execute; Arguments = $Definition.Arguments })
        Principal = [pscustomobject]@{ UserId = $Definition.UserId; LogonType = 'Interactive'; RunLevel = 'Limited' }
        Triggers = @([pscustomobject]@{ CimClass = [pscustomobject]@{ CimClassName = 'MSFT_TaskLogonTrigger' } })
        Settings = [pscustomobject]@{ StartWhenAvailable = $true; DisallowStartIfOnBatteries = $false; StopIfGoingOnBatteries = $false; MultipleInstances = 'IgnoreNew'; ExecutionTimeLimit = 'PT0S'; AllowDemandStart = $true }
    }
}

# Exercise the real cross-edition child environment without starting a monitor,
# backend, persistence registration or lifecycle action.
$launchAst = [Management.Automation.Language.Parser]::ParseInput($source, [ref]$tokens, [ref]$errors)
$launchDefinition = @($launchAst.FindAll({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -ceq 'New-AutostartSupervisorProcessStartInfo' }, $true))
Require ($launchDefinition.Count -eq 1) 'one supervisor launch definition exists'
Invoke-Expression $launchDefinition[0].Extent.Text
$parentModulePath = $env:PSModulePath
$childHost = (Get-Command powershell.exe -CommandType Application -ErrorAction Stop).Path
$probe = "`$ProgressPreference='SilentlyContinue'; `$cmd=Get-Command Get-FileHash -ErrorAction Stop; @{hashCommandAvailable=(`$null -ne `$cmd);edition=`$PSVersionTable.PSEdition}|ConvertTo-Json -Compress"
$probeArguments = '-NoProfile -NonInteractive -EncodedCommand ' + [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($probe))
$probeInfo = New-AutostartSupervisorProcessStartInfo -Definition ([pscustomobject]@{ Execute = $childHost; MonitorArguments = $probeArguments })
Require ($probeInfo.FileName -ceq $childHost -and $probeInfo.Arguments -ceq $probeArguments) 'child command identity is unchanged'
Require (-not $probeInfo.UseShellExecute -and $probeInfo.CreateNoWindow) 'child remains hidden without a shell launcher'
Require (-not $probeInfo.EnvironmentVariables.ContainsKey('PSModulePath')) 'child initializes its own module path'
Require ($probeInfo.EnvironmentVariables['PATH'] -ceq $env:PATH) 'unrelated child environment is preserved'
Require ($env:PSModulePath -ceq $parentModulePath) 'parent module environment is unchanged'
$probeInfo.RedirectStandardOutput = $true
$probeInfo.RedirectStandardError = $true
$probeChild = [Diagnostics.Process]::new()
$probeChild.StartInfo = $probeInfo
$probeStarted = $false
try {
    $probeStarted = $probeChild.Start()
    Require $probeStarted 'real Windows PowerShell child starts'
    Require ($probeChild.WaitForExit(10000)) 'read-only child exits within ten seconds'
    $probeOutput = $probeChild.StandardOutput.ReadToEnd()
    $probeErrors = $probeChild.StandardError.ReadToEnd()
    Require ($probeChild.ExitCode -eq 0 -and $probeErrors.Length -eq 0 -and $probeOutput.Length -lt 1024) "child command lookup succeeds without errors (exit=$($probeChild.ExitCode), stderrBytes=$($probeErrors.Length), stdoutBytes=$($probeOutput.Length))"
    $probeResult = $probeOutput | ConvertFrom-Json
    Require ($probeResult.hashCommandAvailable -and $probeResult.edition -ceq 'Desktop') 'Windows PowerShell loads its own hash cmdlet'
} finally {
    if ($probeStarted -and -not $probeChild.HasExited) { $probeChild.Kill(); Require ($probeChild.WaitForExit(5000)) 'owned child cleanup completes' }
    $probeChild.Dispose()
}

$actions = [Collections.Generic.List[string]]::new()
$canonical = [pscustomobject]@{ Path = 'C:\fixture\target\release\catdesk.exe'; Sha256 = 'a' * 64 }
$base = @{
    CanonicalIdentity = { param($root, $expected, $manifest) $canonical }
    LocalMcp = { param($config) [pscustomobject]@{ Port = 43123; Uri = 'http://127.0.0.1:43123/redacted/mcp' } }
    Listener = { param($local, $identity) $null }
    LocalMcpReadiness = { param($local, $identity) $false }
    RuntimeStatus = { param($root, $config) $false }
    Recovery = { param($engine, $root, $config, $expected, $manifest, $timeout, $command) [void]$actions.Add("recover:$command"); '{"State":"CONNECTED_VERIFIED"}' }
    RecoveryReadiness = { param($engine, $root, $config, $expected, $manifest, $timeout) '{"State":"RECOVERY_RELEASE_AUTHORITY_REQUIRED"}' }
    TunnelClient = { $null }
    InstallTunnelClient = { param($setup) [void]$actions.Add('install-client') }
    CodexPrerequisites = { [void]$actions.Add('codex-check') }
    CodexStatus = { $true }
    WakeRuntime = { param($root) $true }
    WakeRepair = { param($root) [void]$actions.Add('wake-repair') }
    StopProcess = { param($local, $identity, $listener) [void]$actions.Add("stop:$([int]$listener.Pid)"); $true }
    CurrentUser = { 'FIXTURE\current-user' }
}
$realCanonicalRoot = $null

try {
    Require ($source -match [regex]::Escape('$PSScriptRoot')) 'facade resolves all internal paths from its own root'
    Require ($source -match "ValidateSet\('install', 'start', 'status', 'diagnose', 'doctor', 'recover', 'stop', 'autostart', 'wake', 'promote'\)") 'exact public command vocabulary is present'
    Require ($source -notmatch '(?i)provision-catdesk-release|cargo\s+(build|run|test)|tunnel-client.*(stop|remove)|runtimes\s+(stop|rm)') 'facade contains no compile/provision or tunnel stop/remove route'
    Require ($source -notmatch 'Stop-Process\s+-Id\s+\$processId') 'public stop never reacquires destructive authority by PID alone'
    Require ($source -match 'Stop-CatDeskListenerProcessForRecovery\s+-LocalMcp\s+\$endpoint\s+-Canonical\s+\$identity\s+-Candidate\s+\$candidate') 'public stop reuses the exact pinned listener process-instance mutation boundary'
    Require ($source -match '\.\s+\$enginePath\s+-Workspace\s+\$facadeWorkspace\s+-ConfigPath\s+\$facadeConfigPath') 'dot-sourced helper receives the facade identity values explicitly'

    # Exercise the real canonical identity path rather than masking it behind a
    # CanonicalIdentity seam. This catches dot-source parameter clobbering where
    # start-catdesk-stack.ps1 defaults replace the facade's bound workspace or
    # fingerprint path before status/start validate target\release\catdesk.exe.
    $realCanonicalRoot = Join-Path ([IO.Path]::GetTempPath()) ("catdesk-lifecycle-real-" + [Guid]::NewGuid().ToString('N'))
    $realRelease = Join-Path $realCanonicalRoot 'target\release'
    [void](New-Item -ItemType Directory -Path $realRelease -Force)
    $realBinary = Join-Path $realRelease 'catdesk.exe'
    [IO.File]::WriteAllBytes($realBinary, [Text.Encoding]::UTF8.GetBytes('fixture-catdesk-binary'))
    $realHash = (Get-FileHash -LiteralPath $realBinary -Algorithm SHA256).Hash.ToLowerInvariant()
    $realFingerprint = Join-Path $realRelease 'catdesk.exe.sha256'
    [IO.File]::WriteAllText($realFingerprint, $realHash)
    $realConfig = Join-Path $realCanonicalRoot 'fixture-config.toml'
    $realTimeout = 17
    $observedPublicInputs = [Collections.Generic.List[string]]::new()
    $realCanonical = $base.Clone()
    [void]$realCanonical.Remove('CanonicalIdentity')
    $realCanonical.LocalMcp = { param($config) [void]$observedPublicInputs.Add("local-mcp:$config"); [pscustomobject]@{ Port = 43123; Uri = 'http://127.0.0.1:43123/redacted/mcp' } }
    $realCanonical.RuntimeStatus = { param($root, $config) [void]$observedPublicInputs.Add("runtime:$root|$config"); $false }
    $realCanonical.Recovery = { param($engine, $root, $config, $expected, $manifest, $timeout, $command) [void]$actions.Add("recover:$command"); [void]$observedPublicInputs.Add("recovery:$root|$config|$expected|$manifest|$timeout|$command"); '{"State":"CONNECTED_VERIFIED"}' }
    $realStatus = Invoke-FixtureFacade -Command 'status' -Seams $realCanonical -FixtureWorkspace $realCanonicalRoot -FixtureConfigPath $realConfig -FixtureExpectedBuildSha256 $realHash -FixtureFingerprintPath $realFingerprint -FixtureReadyTimeoutSeconds $realTimeout | ConvertFrom-Json
    Require ($realStatus.state -eq 'LOCAL_DAEMON_PENDING') 'real canonical status preserves the facade workspace and fingerprint across engine dot-source'
    Require ($observedPublicInputs -contains "local-mcp:$realConfig") 'status preserves the public config identity after helper loading'
    Require ($observedPublicInputs -contains "runtime:$realCanonicalRoot|$realConfig") 'status preserves the public workspace and config identity after helper loading'
    $actions.Clear()
    $observedPublicInputs.Clear()
    $realStart = Invoke-FixtureFacade -Command 'start' -Seams $realCanonical -FixtureWorkspace $realCanonicalRoot -FixtureConfigPath $realConfig -FixtureExpectedBuildSha256 $realHash -FixtureFingerprintPath $realFingerprint -FixtureReadyTimeoutSeconds $realTimeout | ConvertFrom-Json
    Require ($realStart.state -eq 'CONNECTED_VERIFIED') 'real canonical start preserves the facade workspace and fingerprint across engine dot-source'
    Require (([string]::Join(',', $actions)) -eq 'recover:start') 'real canonical start reaches only the bounded recovery seam after identity validation'
    Require ($observedPublicInputs -contains "recovery:$realCanonicalRoot|$realConfig|$realHash|$realFingerprint|$realTimeout|start") 'start preserves every public lifecycle identity and timeout after helper loading'
    $actions.Clear()

    $controlRoot = Join-Path $realCanonicalRoot '.catdesk\release-pair-control'
    [void][IO.Directory]::CreateDirectory($controlRoot)
    $leasePath = Join-Path $controlRoot 'stopped-owner.lock'
    $ownedLease = [IO.File]::Open($leasePath, [IO.FileMode]::CreateNew, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
    $publicRecovery = $realCanonical.Clone()
    [void]$publicRecovery.Remove('Recovery')
    try {
        $busy = Invoke-FixtureFacade -Command 'recover' -Seams $publicRecovery -FixtureWorkspace $realCanonicalRoot -FixtureConfigPath $realConfig -FixtureBackendOnly | ConvertFrom-Json
        Require ($busy.state -ceq 'RELEASE_LIFECYCLE_BUSY' -and $busy.detail -ceq 'gate=RELEASE_LIFECYCLE_BUSY') 'public backend recovery distinguishes a real Windows lease from missing authority'
        $busyStatus = Invoke-FixtureFacade -Command 'status' -Seams $realCanonical -FixtureWorkspace $realCanonicalRoot -FixtureConfigPath $realConfig -FixtureFingerprintPath $realFingerprint | ConvertFrom-Json
        $busyDiagnosis = Invoke-FixtureFacade -Command 'diagnose' -Seams $realCanonical -FixtureWorkspace $realCanonicalRoot -FixtureConfigPath $realConfig -FixtureFingerprintPath $realFingerprint | ConvertFrom-Json
        Require ($busyStatus.state -ceq 'RELEASE_LIFECYCLE_BUSY' -and $busyStatus.detail -ceq 'gate=RELEASE_LIFECYCLE_BUSY') 'read-only status identifies the exact held stopped-owner lease'
        Require ($busyDiagnosis.state -ceq 'DEGRADED' -and $busyDiagnosis.primaryLayer -ceq 'RECOVERY_AUTHORITY' -and $busyDiagnosis.nextAction -ceq 'NONE') 'diagnosis asks neither to restore nor to start under a verified active lease'
        Require (@($busyDiagnosis.layers | Where-Object { $_.gate -ceq 'RELEASE_LIFECYCLE_BUSY' }).Count -eq 1) 'diagnosis exposes the active lifecycle owner gate'
    } finally {
        $ownedLease.Dispose()
    }
    Require ($actions.Count -eq 0 -and -not [IO.File]::Exists($realConfig)) 'lease contention did not invoke another lifecycle action or require a configuration file'

    $promotionDirectory = Join-Path $realCanonicalRoot '.catdesk\promotion-recovery'
    [void][IO.Directory]::CreateDirectory($promotionDirectory)
    $promotionJournal = Join-Path $promotionDirectory 'promotion-transaction.json'
    $journalContents = '{"schemaVersion":2,"phase":"CANONICAL_MUTATION_STARTED"}'
    $readyStatus = $realCanonical.Clone()
    $readyStatus.LocalMcpReadiness = { $true }
    $readyStatus.RuntimeStatus = { $true }
    $statusArgs = @{
        Command = 'status'
        Seams = $readyStatus
        FixtureWorkspace = $realCanonicalRoot
        FixtureConfigPath = $realConfig
        FixtureExpectedBuildSha256 = $realHash
        FixtureFingerprintPath = $realFingerprint
    }
    $unfenced = Invoke-FixtureFacade @statusArgs | ConvertFrom-Json
    Require ($unfenced.state -ceq 'READY') 'matching canonical pair and healthy probes are ready without a journal'

    $stoppedRecord = Join-Path $controlRoot 'qualification.json'
    $stoppedContents = '{"schemaVersion":2,"mode":"QUALIFIED_STOPPED"}'
    [IO.File]::WriteAllText($stoppedRecord,$stoppedContents)
    $observedPublicInputs.Clear()
    $blockedStatus = Invoke-FixtureFacade @statusArgs | ConvertFrom-Json
    $diagnoseArgs = $statusArgs.Clone()
    $diagnoseArgs.Command = 'diagnose'
    $blockedDiagnosis = Invoke-FixtureFacade @diagnoseArgs | ConvertFrom-Json
    Require ($blockedStatus.state -ceq 'RECOVERY_AUTHORITY_REQUIRED' -and $observedPublicInputs.Count -eq 0) 'stopped reconciliation fences status before network probes'
    Require ($blockedDiagnosis.state -ceq 'ACTION_REQUIRED' -and $blockedDiagnosis.primaryLayer -ceq 'RECOVERY_AUTHORITY' -and $blockedDiagnosis.nextAction -ceq 'OPERATOR_ATTENTION') 'stopped reconciliation fences diagnosis'
    Require ([IO.File]::ReadAllText($stoppedRecord) -ceq $stoppedContents) 'readers preserve the original stopped record'
    [IO.File]::Delete($stoppedRecord)

    $originalProfilesProbe = $readyStatus.WakeRuntime
    $readyStatus.WakeRuntime = {
        [IO.File]::WriteAllText($stoppedRecord,$stoppedContents)
        $true
    }
    $lateDiagnosis = Invoke-FixtureFacade @diagnoseArgs | ConvertFrom-Json
    Require ($lateDiagnosis.state -ceq 'ACTION_REQUIRED' -and $lateDiagnosis.primaryLayer -ceq 'RECOVERY_AUTHORITY') 'a stopped record published during diagnosis invalidates readiness'
    [IO.File]::Delete($stoppedRecord)
    $lateStatus = Invoke-FixtureFacade @statusArgs | ConvertFrom-Json
    Require ($lateStatus.state -ceq 'RECOVERY_AUTHORITY_REQUIRED') 'a stopped record published during status invalidates readiness'
    [IO.File]::Delete($stoppedRecord)
    $readyStatus.WakeRuntime = $originalProfilesProbe
    $observedPublicInputs.Clear()
    [IO.File]::WriteAllText($promotionJournal, $journalContents)
    $alreadyFenced = Invoke-FixtureFacade @statusArgs | ConvertFrom-Json
    Require ($alreadyFenced.state -ceq 'RECOVERY_AUTHORITY_REQUIRED') 'matching canonical pair must not hide an unfinished promotion'
    Require ($observedPublicInputs.Count -eq 0) 'existing promotion journal prevents status probes'
    Require ([IO.File]::ReadAllText($promotionJournal) -ceq $journalContents) 'status preserves the existing journal'
    [IO.File]::Delete($promotionJournal)

    $readyStatus.WakeRuntime = {
        [IO.File]::WriteAllText($promotionJournal, $journalContents)
        $true
    }
    $newlyFenced = Invoke-FixtureFacade @statusArgs | ConvertFrom-Json
    Require ($newlyFenced.state -ceq 'RECOVERY_AUTHORITY_REQUIRED') 'journal published during readiness checks invalidates the ready response'
    Require ([IO.File]::ReadAllText($promotionJournal) -ceq $journalContents) 'status preserves the newly published journal'
    Require ((Get-FileHash -LiteralPath $realBinary).Hash -ieq $realHash -and [IO.File]::ReadAllText($realFingerprint) -ceq $realHash) 'status preserves both release files'
    [IO.File]::Delete($promotionJournal)

    # Use real file identity and hashing, with only the native child seam
    # simulated. This never launches a release controller or a backend.
    $attemptId = 'a' * 32
    $candidate = Join-Path $realCanonicalRoot "target\reviewed-builds\$attemptId\catdesk.exe"
    [void][IO.Directory]::CreateDirectory((Split-Path -Parent $candidate))
    [IO.File]::WriteAllText($candidate,'fixture-attested-controller')
    $digest = (Get-FileHash -LiteralPath $candidate).Hash.ToLowerInvariant()
    $resolver = $realCanonical.Clone()
    $resolver.PromotionInvokeNative = {
        param($image,$root,$verb,$hash)
        Require ($root -ceq $realCanonicalRoot -and $hash -ceq $digest) 'promotion stays within the same workspace and artifact'
        [void]$actions.Add($verb)
        if ($verb -ceq 'promote-resolve') {
            Require ($image -ceq $realBinary) 'only the canonical image resolves the attested release'
            return [pscustomobject]@{state='PROMOTION_CANDIDATE_RESOLVED';candidateSha256=$hash;buildAttemptId=$attemptId}
        }
        Require ($verb -ceq 'promote' -and $image -ceq $candidate) 'only the exact resolved controller starts promotion'
        [pscustomobject]@{state='PROMOTION_COMPLETED';candidateSha256=$hash;authorizationGeneration=7;servingState='STOPPED_AFTER_BOUNDED_OWNER'}
    }
    $invalid = & $facade promote '56aa9912' -Workspace $realCanonicalRoot -TestSeams $resolver | ConvertFrom-Json
    Require ($invalid.state -ceq 'REVIEWED_PROMOTION_ARTIFACT_SHA256_REQUIRED' -and $actions.Count -eq 0) 'promotion rejects short and Git source hashes before resolving'
    $retiredResolver = $resolver.Clone()
    $retiredResolver.PromotionInvokeNative = {
        param($image,$root,$verb,$hash)
        [void]$actions.Add($verb)
        throw 'REVIEWED_PROMOTION_ALREADY_RETIRED_INSPECT_RESULT'
    }
    $retired = & $facade promote $digest -Workspace $realCanonicalRoot -TestSeams $retiredResolver | ConvertFrom-Json
    Require ($retired.state -ceq 'REVIEWED_PROMOTION_ALREADY_RETIRED_INSPECT_RESULT' -and $retired.detail -ceq 'RESOLVE_CANDIDATE' -and ($actions -join ',') -ceq 'promote-resolve') 'retired promotion refusal stops before launching a candidate'
    $actions.Clear()
    $staleReceipt = & $facade promote $digest.ToUpperInvariant() -Workspace $realCanonicalRoot -TestSeams $resolver | ConvertFrom-Json
    Require ($staleReceipt.state -ceq 'REVIEWED_PROMOTION_CANONICAL_READBACK_UNVERIFIED') 'a matching terminal receipt cannot prove an unchanged installed pair is the candidate'
    Require (($actions -join ',') -ceq 'promote-resolve,promote') 'the one-argument facade invokes each native phase once'
    $actions.Clear()
    $servingResolver = $resolver.Clone()
    $nativeFixture = $resolver.PromotionInvokeNative
    $servingResolver.PromotionInvokeNative = {
        param($image,$root,$verb,$hash)
        $receipt = & $nativeFixture $image $root $verb $hash
        if ($verb -ceq 'promote') { [IO.File]::Copy($candidate, $realBinary, $true) }
        $receipt
    }
    $splitPair = & $facade promote $digest -Workspace $realCanonicalRoot -TestSeams $servingResolver | ConvertFrom-Json
    Require ($splitPair.state -ceq 'REVIEWED_PROMOTION_CANONICAL_READBACK_UNVERIFIED') 'a candidate image with the old fingerprint cannot prove selection'
    Require (($actions -join ',') -ceq 'promote-resolve,promote') 'unverified readback does not initiate another promotion'
    $actions.Clear()
    [IO.File]::WriteAllBytes($realBinary, [Text.Encoding]::UTF8.GetBytes('fixture-catdesk-binary'))
    $promotedResolver = $resolver.Clone()
    $promotedResolver.PromotionInvokeNative = {
        param($image,$root,$verb,$hash)
        $receipt = & $nativeFixture $image $root $verb $hash
        if ($verb -ceq 'promote') {
            [IO.File]::Copy($candidate, $realBinary, $true)
            [IO.File]::WriteAllText($realFingerprint, $hash)
        }
        $receipt
    }
    $promoted = & $facade promote $digest -Workspace $realCanonicalRoot -TestSeams $promotedResolver | ConvertFrom-Json
    Require ($promoted.state -ceq 'PROMOTION_COMPLETED' -and $promoted.detail -ceq 'STOPPED_AFTER_BOUNDED_OWNER') 'promotion confirms the exact installed pair without claiming running health'
    Require (($actions -join ',') -ceq 'promote-resolve,promote') 'verified readback does not initiate another promotion'
    $actions.Clear()

    $servingObservation = $promotedResolver.Clone()
    $servingObservation.ScheduledTaskGet = { $null }
    $servingObservation.AutostartRunKeyGet = { param($definition) $definition.RunKeyValue + ' -BackendOnly' }
    $servingObservation.AutostartSupervisorRunning = { $true }
    $servingObservation.Listener = {
        param($endpoint,$identity)
        Require ($identity.Path -ceq $realBinary -and $identity.Sha256 -ceq $digest) 'observer binds the actual new canonical pair'
        [pscustomobject]@{Pid=3001;CreationTimeUtc='fixture-owner';MatchesCanonical=$true;Sha256=$digest}
    }
    $authProbed = [Collections.Generic.List[bool]]::new()
    $servingObservation.LocalMcpReadiness = {
        param($endpoint,$identity,$requireAuth)
        [void]$authProbed.Add($requireAuth)
        $true
    }
    $observed = & $facade promote $digest -Workspace $realCanonicalRoot -ReadyTimeoutSeconds 0 -TestSeams $servingObservation | ConvertFrom-Json
    Require ($observed.state -ceq 'PROMOTION_COMPLETED' -and $observed.detail -ceq 'LOCAL_BACKEND_READY') 'one command reports exact authenticated observed serving'
    Require ($authProbed.Count -eq 1 -and $authProbed[0] -and ($actions -join ',') -ceq 'promote-resolve,promote') 'serving proof requires bearer and calls no other lifecycle operation'
    $actions.Clear()
    $authProbed.Clear()
    $inactiveMonitor = $servingObservation.Clone()
    $inactiveMonitor.AutostartSupervisorRunning = { $false }
    $observed = & $facade promote $digest -Workspace $realCanonicalRoot -ReadyTimeoutSeconds 0 -TestSeams $inactiveMonitor | ConvertFrom-Json
    Require ($observed.state -ceq 'PROMOTION_COMPLETED' -and $observed.detail -ceq 'LOCAL_BACKEND_READY') "the exact serving backend remains ready after the monitor exits (observed $($observed.detail))"
    Require ($authProbed.Count -eq 1 -and $authProbed[0] -and ($actions -join ',') -ceq 'promote-resolve,promote') 'monitor exit cannot skip authenticated serving proof or advance another operation'
    $actions.Clear()
    $authProbed.Clear()

    $coldTask = $inactiveMonitor.Clone()
    $fixtureScheduledMode = 'backend'
    $coldTask.AutostartRunKeyGet = { $null }
    $coldTask.ScheduledTaskGet = {
        param($definition)
        $task = New-OwnedTask $definition
        if ($fixtureScheduledMode -ne 'full-stack') { $task.Actions[0].Arguments += ' -BackendOnly' }
        if ($fixtureScheduledMode -eq 'disabled') { $task.State = 'Disabled' }
        if ($fixtureScheduledMode -eq 'foreign') { $task.Principal.UserId = 'another-user' }
        if ($fixtureScheduledMode -eq 'battery-blocked') { $task.Settings.DisallowStartIfOnBatteries = $true }
        if ($fixtureScheduledMode -eq 'battery-policy-missing') { $task.Settings.PSObject.Properties.Remove('StopIfGoingOnBatteries') }
        if ($fixtureScheduledMode -eq 'finite-duration') { $task.Settings.ExecutionTimeLimit = 'PT1M' }
        if ($fixtureScheduledMode -eq 'wrong-logon') { $task.Principal.LogonType = 'InteractiveOrPassword' }
        if ($fixtureScheduledMode -eq 'other-sid') { $task.Principal.UserId = 'S-1-5-18' }
        if ($fixtureScheduledMode -eq 'matching-sid') { $task.Principal.UserId = $fixtureActualSid }
        $task
    }
    $coldTask.ScheduledTaskStart = {
        param($task)
        Require ($task.State -ceq 'Ready' -and $task.Actions[0].Arguments.EndsWith(' -BackendOnly')) 'only the exact ready backend-only task can be started'
        [void]$actions.Add('scheduled-backend-start')
    }
    $coldTask.Listener = {
        if ($actions -contains 'scheduled-backend-start') {
            [pscustomobject]@{Pid=3001;CreationTimeUtc='fixture-scheduled-owner';MatchesCanonical=$true;Sha256=$digest}
        }
    }
    $observed = & $facade promote $digest -Workspace $realCanonicalRoot -ReadyTimeoutSeconds 1 -TestSeams $coldTask | ConvertFrom-Json
    Require ($observed.state -ceq 'PROMOTION_COMPLETED' -and $observed.detail -ceq 'LOCAL_BACKEND_READY' -and
        ($actions -join ',') -ceq 'promote-resolve,promote,scheduled-backend-start' -and $authProbed.Count -eq 1 -and
        $authProbed[0]) 'fresh promotion starts the exact stopped backend scheduler once and verifies its authenticated listener'
    $actions.Clear()
    $authProbed.Clear()
    foreach ($fixtureScheduledMode in @('disabled', 'full-stack', 'foreign', 'battery-blocked', 'battery-policy-missing', 'finite-duration', 'wrong-logon', 'other-sid')) {
        $observed = & $facade promote $digest -Workspace $realCanonicalRoot -ReadyTimeoutSeconds 1 -TestSeams $coldTask | ConvertFrom-Json
        Require ($observed.state -ceq 'PROMOTION_COMPLETED' -and $observed.detail -ceq 'STOPPED_AFTER_BOUNDED_OWNER' -and
            ($actions -join ',') -ceq 'promote-resolve,promote') "fresh promotion leaves the $fixtureScheduledMode task untouched"
        $actions.Clear()
    }
    $sidTask = $coldTask.Clone()
    $actualUser = [Security.Principal.WindowsIdentity]::GetCurrent()
    $fixtureActualSid = $actualUser.User.Value
    $fixtureActualUser = $actualUser.Name
    $actualUser.Dispose()
    $sidTask.CurrentUser = { $fixtureActualUser }
    $fixtureScheduledMode = 'matching-sid'
    $observed = & $facade promote $digest -Workspace $realCanonicalRoot -ReadyTimeoutSeconds 1 -TestSeams $sidTask | ConvertFrom-Json
    Require ($observed.detail -ceq 'LOCAL_BACKEND_READY' -and ($actions -join ',') -ceq 'promote-resolve,promote,scheduled-backend-start') 'task account text and SID are equivalent only after exact Windows SID resolution'
    $actions.Clear()
    $authProbed.Clear()
    $fixtureScheduledMode = 'backend'
    $startUnobserved = $coldTask.Clone()
    $startUnobserved.Listener = { $null }
    $observed = & $facade promote $digest -Workspace $realCanonicalRoot -ReadyTimeoutSeconds 1 -TestSeams $startUnobserved | ConvertFrom-Json
    Require ($observed.state -ceq 'PROMOTION_COMPLETED' -and $observed.detail -ceq 'LOCAL_BACKEND_STARTUP_PENDING' -and
        ($actions -join ',') -ceq 'promote-resolve,promote,scheduled-backend-start') 'a delayed owner leaves serving pending without retry'
    $actions.Clear()
    $startUnobserved.ScheduledTaskStart = { [void]$actions.Add('attempted-start'); throw 'fixture start ambiguous' }
    $observed = & $facade promote $digest -Workspace $realCanonicalRoot -ReadyTimeoutSeconds 1 -TestSeams $startUnobserved | ConvertFrom-Json
    Require ($observed.state -ceq 'PROMOTION_COMPLETED' -and $observed.detail -ceq 'LOCAL_BACKEND_READBACK_UNVERIFIED' -and
        ($actions -join ',') -ceq 'promote-resolve,promote,attempted-start') 'an ambiguous start does not cause a second request or a health claim'
    $actions.Clear()

    $retainedCold = $coldTask.Clone()
    $retainedCold.PromotionInvokeNative = {
        param($image,$root,$verb,$hash)
        [void]$actions.Add($verb)
        [pscustomobject]@{state='PROMOTION_COMPLETED';candidateSha256=$hash;existingResult=$true;authorizationGeneration=7;servingState='STOPPED_AFTER_BOUNDED_OWNER'}
    }
    $observed = & $facade promote $digest -Workspace $realCanonicalRoot -ReadyTimeoutSeconds 1 -TestSeams $retainedCold | ConvertFrom-Json
    Require ($observed.state -ceq 'PROMOTION_COMPLETED' -and $observed.detail -ceq 'STOPPED_AFTER_BOUNDED_OWNER' -and
        ($actions -join ',') -ceq 'promote-resolve') 'a retained receipt remains observation-only even with an eligible stopped task'
    $actions.Clear()

    $runningFullStack = $coldTask.Clone()
    $runningFullStack.ScheduledTaskGet = { $null }
    $runningFullStack.AutostartRunKeyGet = { param($definition) $definition.RunKeyValue }
    $runningFullStack.AutostartSupervisorRunning = { $true }
    $runningFullStack.Listener = { $null }
    $observed = & $facade promote $digest -Workspace $realCanonicalRoot -ReadyTimeoutSeconds 0 -TestSeams $runningFullStack | ConvertFrom-Json
    Require ($observed.state -ceq 'PROMOTION_COMPLETED' -and $observed.detail -ceq 'LOCAL_BACKEND_STARTUP_PENDING' -and
        ($actions -join ',') -ceq 'promote-resolve,promote') 'an exact already-running full-stack owner is observed without a startup request'
    $actions.Clear()

    $uncertainHandoff = $inactiveMonitor.Clone()
    $promoteFixture = $promotedResolver.PromotionInvokeNative
    $uncertainHandoff.PromotionInvokeNative = {
        param($image,$root,$verb,$hash)
        $receipt = & $promoteFixture $image $root $verb $hash
        if ($verb -ceq 'promote') { $receipt.servingState = 'UNVERIFIED' }
        $receipt
    }
    $observed = & $facade promote $digest -Workspace $realCanonicalRoot -ReadyTimeoutSeconds 0 -TestSeams $uncertainHandoff | ConvertFrom-Json
    Require ($observed.state -ceq 'PROMOTION_COMPLETED' -and $observed.detail -ceq 'LOCAL_BACKEND_READY') "an initially unverified handoff can prove exact current serving (observed $($observed.detail))"
    Require ($authProbed.Count -eq 1 -and $authProbed[0] -and ($actions -join ',') -ceq 'promote-resolve,promote') 'unverified receipt readback requires authentication and introduces no launch'
    $actions.Clear()
    $authProbed.Clear()

    $uncertainHandoff.Listener = { $null }
    $observed = & $facade promote $digest -Workspace $realCanonicalRoot -ReadyTimeoutSeconds 0 -TestSeams $uncertainHandoff | ConvertFrom-Json
    Require ($observed.state -ceq 'PROMOTION_COMPLETED' -and $observed.detail -ceq 'LOCAL_BACKEND_READBACK_UNVERIFIED' -and $authProbed.Count -eq 0) 'no listener after an ambiguous handoff cannot be projected as a confirmed stopped owner'
    $actions.Clear()

    $unregistered = $inactiveMonitor.Clone()
    $unregistered.AutostartRunKeyGet = { $null }
    $unregistered.LocalMcpReadiness = { $false }
    $observed = & $facade promote $digest -Workspace $realCanonicalRoot -ReadyTimeoutSeconds 0 -TestSeams $unregistered | ConvertFrom-Json
    Require ($observed.state -ceq 'PROMOTION_COMPLETED' -and $observed.detail -ceq 'LOCAL_BACKEND_READBACK_UNVERIFIED') 'an unauthenticated existing listener does not read back as stopped or ready'
    $actions.Clear()

    $servingObservation.Listener = { $null }
    $pendingBackend = & $facade promote $digest -Workspace $realCanonicalRoot -ReadyTimeoutSeconds 0 -TestSeams $servingObservation | ConvertFrom-Json
    Require ($pendingBackend.state -ceq 'PROMOTION_COMPLETED' -and $pendingBackend.detail -ceq 'LOCAL_BACKEND_STARTUP_PENDING' -and $authProbed.Count -eq 0 -and ($actions -join ',') -ceq 'promote-resolve,promote') 'unavailable listener leaves disk completion separate from serving'
    $actions.Clear()

    $driftObservations = [Collections.Generic.List[int]]::new()
    $servingObservation.Listener = {
        [void]$driftObservations.Add(1)
        [pscustomobject]@{Pid=3001;CreationTimeUtc=[string]$driftObservations.Count;MatchesCanonical=$true;Sha256=$digest}
    }
    $driftedBackend = & $facade promote $digest -Workspace $realCanonicalRoot -ReadyTimeoutSeconds 0 -TestSeams $servingObservation | ConvertFrom-Json
    Require ($driftedBackend.state -ceq 'PROMOTION_COMPLETED' -and $driftedBackend.detail -ceq 'LOCAL_BACKEND_READBACK_UNVERIFIED') 'same PID at a new creation time cannot confirm readiness'
    Require ($driftObservations.Count -eq 2 -and $authProbed.Count -eq 1 -and ($actions -join ',') -ceq 'promote-resolve,promote') 'owner drift produces no retry or another lifecycle operation'
    $actions.Clear()

    # A terminal retained transaction is readable after the staging path goes
    # away. The facade must use only the pinned canonical resolver in this case.
    [IO.File]::Delete($candidate)
    try {
        $retained = $promotedResolver.Clone()
        $retained.ScheduledTaskGet = { $null }
        $retained.AutostartRunKeyGet = { $null }
        $retained.PromotionInvokeNative = {
            param($image,$root,$verb,$hash)
            [void]$actions.Add($verb)
            Require ($verb -ceq 'promote-resolve' -and $image -ceq $realBinary) 'a retained result is read only through the canonical resolver'
            [pscustomobject]@{state=$retainedState;candidateSha256=$hash;existingResult=$retainedMarker
                authorizationGeneration=7;servingState=$retainedServingState}
        }
        $retainedState = 'PROMOTION_COMPLETED'
        $retainedMarker = $true
        $retainedServingState = 'STOPPED_AFTER_BOUNDED_OWNER'
        $done = & $facade promote $digest -Workspace $realCanonicalRoot -TestSeams $retained | ConvertFrom-Json
        Require ($done.state -ceq 'PROMOTION_COMPLETED' -and $actions.Count -eq 1) 'retained completion needs no staged candidate launch'
        $actions.Clear()
        $retainedServingState = 'UNVERIFIED'
        $retained.Listener = $inactiveMonitor.Listener
        $retained.LocalMcpReadiness = $inactiveMonitor.LocalMcpReadiness
        $authProbed.Clear()
        $done = & $facade promote $digest -Workspace $realCanonicalRoot -ReadyTimeoutSeconds 0 -TestSeams $retained | ConvertFrom-Json
        Require ($done.state -ceq 'PROMOTION_COMPLETED' -and $done.detail -ceq 'LOCAL_BACKEND_READY' -and
            ($actions -join ',') -ceq 'promote-resolve' -and $authProbed.Count -eq 1 -and $authProbed[0]) 'a retained ambiguous serving receipt is reconciled with live evidence without rerunning the candidate'
        $actions.Clear()
        $retainedMarker = 'true'
        $malformed = & $facade promote $digest -Workspace $realCanonicalRoot -TestSeams $retained | ConvertFrom-Json
        Require ($malformed.state -ceq 'REVIEWED_PROMOTION_RECONCILIATION_REQUIRED' -and $actions.Count -eq 1) 'an unbound terminal projection refuses before another launch'
        $actions.Clear()
        $retainedMarker = $true
        $retainedState = 'PROMOTION_FAILED_OR_AMBIGUOUS'
        $retainedServingState = $null
        $ambiguous = & $facade promote $digest -Workspace $realCanonicalRoot -TestSeams $retained | ConvertFrom-Json
        Require ($ambiguous.state -ceq 'PROMOTION_FAILED_OR_AMBIGUOUS' -and $actions.Count -eq 1) 'a retained ambiguous outcome never starts another promotion'
        $actions.Clear()
        $retainedState = 'PROMOTION_COMPLETED'
        $retainedServingState = 'STOPPED_AFTER_BOUNDED_OWNER'
        $prior = [IO.File]::ReadAllBytes($realBinary)
        try {
            [IO.File]::WriteAllText($realBinary, 'already-restored-previous-release')
            [IO.File]::WriteAllText($realFingerprint, (Get-FileHash -LiteralPath $realBinary).Hash.ToLowerInvariant())
            $rolledBack = & $facade promote $digest -Workspace $realCanonicalRoot -TestSeams $retained | ConvertFrom-Json
            Require ($rolledBack.state -ceq 'REVIEWED_PROMOTION_CANONICAL_READBACK_UNVERIFIED' -and $actions.Count -eq 1) 'completed history cannot certify a different current selection'
        } finally {
            [IO.File]::WriteAllBytes($realBinary,$prior)
            [IO.File]::WriteAllText($realFingerprint,$digest)
        }
    } finally {
        [IO.File]::Copy($realBinary,$candidate,$false)
    }
    $actions.Clear()

    $invalidResolver = $resolver.Clone()
    $invalidResolver.PromotionInvokeNative = {
        param($image,$root,$verb,$hash)
        [void]$actions.Add($verb)
        [pscustomobject]@{state='PROMOTION_CANDIDATE_RESOLVED';candidateSha256=$hash;buildAttemptId='..\escape'}
    }
    $unsafe = & $facade promote $digest -Workspace $realCanonicalRoot -TestSeams $invalidResolver | ConvertFrom-Json
    Require ($unsafe.state -ceq 'REVIEWED_PROMOTION_RESOLUTION_UNPROVEN' -and ($actions -join ',') -ceq 'promote-resolve') 'untrusted candidate path never reaches launch'
    $actions.Clear()
    [IO.File]::WriteAllText($candidate,'drifted-fixture-controller')
    $drifted = & $facade promote $digest -Workspace $realCanonicalRoot -TestSeams $resolver | ConvertFrom-Json
    Require ($drifted.state -ceq 'REVIEWED_PROMOTION_CONTROLLER_CHANGED' -and ($actions -join ',') -ceq 'promote-resolve') 'changed candidate bytes never reach launch'
    $actions.Clear()

    # Exercise the facade's actual process, pipe and exit-code path.
    $fixtureSource = Join-Path $realCanonicalRoot 'promotion-refusal.cs'
    [IO.File]::WriteAllText($fixtureSource, @'
using System;
using System.IO;

static class PromotionRefusal {
    static int Main(string[] args) {
        if (args.Length == 1 && args[0] == "--fixture-hold-promotion-pipes") {
            File.WriteAllText("pipe-owner.pid", System.Diagnostics.Process.GetCurrentProcess().Id.ToString());
            var budget = System.Diagnostics.Stopwatch.StartNew();
            while (budget.ElapsedMilliseconds < 12000 && !File.Exists("release-pipe-owner"))
                System.Threading.Thread.Sleep(25);
            return 0;
        }
        string mode = File.ReadAllText("refusal-mode.txt");
        File.AppendAllText("refusal-invocations.txt", args[1] + "\n");
        if ((mode == "candidate" || mode == "stopped-source" || mode == "listener-owner" || mode == "source-pair" || mode == "pipe-held") && args[1] == "promote-resolve") {
            Console.WriteLine("{\"state\":\"PROMOTION_CANDIDATE_RESOLVED\",\"candidateSha256\":\"" + args[2] + "\",\"buildAttemptId\":\"" + new string('a', 32) + "\"}");
            return 0;
        }
        if (mode == "pipe-held") {
            var childInfo = new System.Diagnostics.ProcessStartInfo(
                System.Reflection.Assembly.GetExecutingAssembly().Location, "--fixture-hold-promotion-pipes");
            childInfo.UseShellExecute = false;
            childInfo.CreateNoWindow = true;
            using (var owner = System.Diagnostics.Process.Start(childInfo)) {
                Console.WriteLine("{\"state\":\"PROMOTION_IN_PROGRESS\",\"candidateSha256\":\"" + args[2] + "\"}");
                return 0;
            }
        }
        string code = mode == "candidate" ? "REVIEWED_PROMOTION_REVIEW_REQUIRED" : "REVIEWED_PROMOTION_NO_ACTIVE_ATTESTED_BUILD";
        if (mode == "stopped-source") code = "REVIEWED_PROMOTION_STOPPED_SOURCE_UNQUALIFIED";
        if (mode == "listener-owner") code = "REVIEWED_PROMOTION_SOURCE_LISTENER_UNPROVEN";
        if (mode == "source-pair") code = "REVIEWED_PROMOTION_SOURCE_PAIR_UNPROVEN";
        if (mode == "unknown") code = "FIXTURE_PRIVATE_PATH";
        string extra = mode == "extra" ? ",\"message\":\"FIXTURE_PRIVATE_TEXT\"" : "";
        if (mode == "stderr") Console.Error.WriteLine("FIXTURE_PRIVATE_TEXT");
        string refusal = "{\"status\":\"attention\",\"errorCode\":\"" + code + "\"" + extra + "}";
        Console.WriteLine(mode == "array" ? "[" + refusal + "]" : refusal);
        return mode == "other-exit" ? 1 : 2;
    }
}
'@)
    $compiler = Join-Path $env:WINDIR 'Microsoft.NET\Framework64\v4.0.30319\csc.exe'
    & $compiler /nologo /target:exe "/out:$realBinary" $fixtureSource
    Require ($LASTEXITCODE -eq 0) 'bounded refusal executable compiles'
    [IO.File]::Copy($realBinary, $candidate, $true)
    $digest = (Get-FileHash -LiteralPath $candidate).Hash.ToLowerInvariant()
    [IO.File]::WriteAllText($realFingerprint, $digest)
    $invocations = Join-Path $realCanonicalRoot 'refusal-invocations.txt'
    $cases = @(
        @{ Mode = 'resolver'; State = 'REVIEWED_PROMOTION_NO_ACTIVE_ATTESTED_BUILD'; Phase = 'RESOLVE_CANDIDATE'; Actions = 'promote-resolve' }
        @{ Mode = 'candidate'; State = 'REVIEWED_PROMOTION_REVIEW_REQUIRED'; Phase = 'RUN_CANDIDATE'; Actions = 'promote-resolve,promote' }
        @{ Mode = 'stopped-source'; State = 'REVIEWED_PROMOTION_STOPPED_SOURCE_UNQUALIFIED'; Phase = 'RUN_CANDIDATE'; Actions = 'promote-resolve,promote' }
        @{ Mode = 'listener-owner'; State = 'REVIEWED_PROMOTION_SOURCE_LISTENER_UNPROVEN'; Phase = 'RUN_CANDIDATE'; Actions = 'promote-resolve,promote' }
        @{ Mode = 'source-pair'; State = 'REVIEWED_PROMOTION_SOURCE_PAIR_UNPROVEN'; Phase = 'RUN_CANDIDATE'; Actions = 'promote-resolve,promote' }
        @{ Mode = 'unknown'; State = 'REVIEWED_PROMOTION_REFUSED_INSPECT_RESULT'; Actions = 'promote-resolve' }
        @{ Mode = 'extra'; State = 'REVIEWED_PROMOTION_REFUSED_INSPECT_RESULT'; Actions = 'promote-resolve' }
        @{ Mode = 'array'; State = 'REVIEWED_PROMOTION_REFUSED_INSPECT_RESULT'; Actions = 'promote-resolve' }
        @{ Mode = 'stderr'; State = 'REVIEWED_PROMOTION_REFUSED_INSPECT_RESULT'; Actions = 'promote-resolve' }
        @{ Mode = 'other-exit'; State = 'REVIEWED_PROMOTION_REFUSED_INSPECT_RESULT'; Actions = 'promote-resolve' }
    )
    foreach ($case in $cases) {
        [IO.File]::WriteAllText((Join-Path $realCanonicalRoot 'refusal-mode.txt'), $case.Mode)
        [IO.File]::WriteAllText($invocations, '')
        $wire = & $facade promote $digest -Workspace $realCanonicalRoot -TestSeams $realCanonical
        $refusal = $wire | ConvertFrom-Json
        Require ($refusal.state -ceq $case.State) "expected native refusal for $($case.Mode), got $($refusal.state)"
        if ($case.ContainsKey('Phase')) {
            Require ($refusal.detail -ceq $case.Phase) "native refusal identifies its phase for $($case.Mode)"
        }
        Require ($wire -cnotmatch 'FIXTURE_PRIVATE') 'refusal does not disclose child stderr or arbitrary stdout'
        Require (([IO.File]::ReadAllLines($invocations) -join ',') -ceq $case.Actions) 'refusal ends the operation without advancing or retrying'
    }

    [IO.File]::WriteAllText((Join-Path $realCanonicalRoot 'refusal-mode.txt'), 'pipe-held')
    [IO.File]::WriteAllText($invocations, '')
    $pipeOwnerPath = Join-Path $realCanonicalRoot 'pipe-owner.pid'
    $releasePipeOwner = Join-Path $realCanonicalRoot 'release-pipe-owner'
    $pipeOwner = $null
    try {
        $pipeResult = & $facade promote $digest -Workspace $realCanonicalRoot -TestSeams $realCanonical | ConvertFrom-Json
        Require ([IO.File]::Exists($pipeOwnerPath)) 'the real native descendant inherited the controller pipes'
        Require ($pipeResult.state -ceq 'REVIEWED_PROMOTION_RECONCILIATION_REQUIRED' -and $pipeResult.detail -ceq 'RUN_CANDIDATE') 'a retained output pipe produces exact candidate reconciliation instead of waiting for EOF'
        $pipeOwner = [Diagnostics.Process]::GetProcessById([int][IO.File]::ReadAllText($pipeOwnerPath))
        Require (-not $pipeOwner.HasExited) 'the facade returns without terminating the descendant that owns the pipe'
        Require (([IO.File]::ReadAllLines($invocations) -join ',') -ceq 'promote-resolve,promote') 'ambiguous output does not restart the controller'
    } finally {
        [IO.File]::WriteAllText($releasePipeOwner, '')
        if ($null -ne $pipeOwner) {
            try { Require ($pipeOwner.WaitForExit(5000)) 'test-owned pipe descendant exits after release' }
            finally { $pipeOwner.Dispose() }
        }
    }

    $status = Invoke-FixtureFacade -Command 'status' -Seams $base | ConvertFrom-Json
    Require ($status.state -eq 'LOCAL_DAEMON_PENDING') 'status reports a bounded non-mutating pending state'
    Require ($actions.Count -eq 0) 'status did not invoke any lifecycle side effect'
    Require (($status | ConvertTo-Json -Compress) -notmatch '(?i)secret|token|route_|https?://') 'status output is redacted'

    $coreReadyWakePending = $base.Clone()
    $coreReadyWakePending.LocalMcpReadiness = { param($local, $identity) $true }
    $coreReadyWakePending.RuntimeStatus = { param($root, $config) $true }
    $coreReadyWakePending.WakeRuntime = { param($root) $false }
    $partialStatus = Invoke-FixtureFacade -Command 'status' -Seams $coreReadyWakePending | ConvertFrom-Json
    Require ($partialStatus.state -eq 'CORE_CONNECTED_WAKE_PENDING' -and $partialStatus.detail -eq 'gate=WAKE_RUNTIME_NOT_READY') 'healthy core status keeps unavailable Wake as a separate fixed gate'
    Require ($actions.Count -eq 0) 'partial status did not repair Wake or mutate the daemon'
    $coreReadyWakePending.WakeRuntime = { param($root) throw 'fixture Wake probe unavailable' }
    $failedWakeProbe = Invoke-FixtureFacade -Command 'status' -Seams $coreReadyWakePending | ConvertFrom-Json
    Require ($failedWakeProbe.state -eq 'CORE_CONNECTED_WAKE_PENDING' -and $failedWakeProbe.detail -eq 'gate=WAKE_RUNTIME_NOT_READY') 'Wake probe error does not hide verified core status'
    $coreReadyWakePending.WakeRuntime = { param($root) $true }
    $fullStatus = Invoke-FixtureFacade -Command 'status' -Seams $coreReadyWakePending | ConvertFrom-Json
    Require ($fullStatus.state -eq 'READY') 'available Wake and healthy core can report READY'

    $diagnose = Invoke-FixtureFacade -Command 'diagnose' -Seams $base | ConvertFrom-Json
    Require ($diagnose.state -eq 'DEGRADED' -and $diagnose.primaryLayer -eq 'LOCAL_DAEMON' -and $diagnose.nextAction -eq 'RUN_RECOVER') 'diagnose identifies the first broken local-daemon layer and recommends one-command recovery'
    Require (@($diagnose.layers).Count -eq 9) 'diagnose returns the fixed nine-layer model including Codex CLI'
    Require ((@($diagnose.layers) | Where-Object { $_.layer -eq 'LOCAL_DAEMON' }).gate -eq 'LOCAL_MCP_LISTENER_MISSING') 'diagnose preserves the fixed local-daemon failure gate'
    Require ((@($diagnose.layers) | Where-Object { $_.layer -eq 'OFFICIAL_RUNTIME' }).gate -eq 'RUNTIME_STATUS_NOT_READY') 'diagnose independently reports downstream external-runtime state'
    Require ($actions.Count -eq 0) 'diagnose is non-mutating'
    Require (($diagnose | ConvertTo-Json -Compress -Depth 5) -notmatch '(?i)secret|token|route_|https?://') 'diagnose output is redacted'

    [void](Invoke-FixtureFacade -Command 'start' -Seams $base)
    [void](Invoke-FixtureFacade -Command 'recover' -Seams $base)
    Require (([string]::Join(',', $actions)) -eq 'recover:start,recover:recover') 'start and recover route only through canonical recovery'
    Require ($actions -notcontains 'compile') 'normal recovery never compiles'

    $actions.Clear()
    $backend = $base.Clone()
    $backend.Recovery = { param($engine,$root,$config,$expected,$manifest,$timeout,$command,$backendOnly)
        Require ([bool]$backendOnly) 'backend-only flag survives helper import and recovery dispatch'
        [void]$actions.Add("backend:$command")
        '{"State":"LOCAL_BACKEND_READY"}'
    }
    foreach ($command in @('start','recover')) {
        $localOnly = Invoke-FixtureFacade -Command $command -Seams $backend -FixtureBackendOnly | ConvertFrom-Json
        Require ($localOnly.state -ceq 'LOCAL_BACKEND_READY' -and $localOnly.detail -match 'LOCAL_BACKEND_ONLY') 'backend-only readiness has an explicit limited scope'
    }
    Require (([string]::Join(',', $actions)) -ceq 'backend:start,backend:recover') 'backend-only calls no other lifecycle layer'
    $actions.Clear()
    $wrongScope = Invoke-FixtureFacade -Command 'wake' -Seams $backend -FixtureBackendOnly | ConvertFrom-Json
    Require ($wrongScope.state -ceq 'ACTION_REQUIRED' -and $actions.Count -eq 0) 'backend-only cannot authorize unrelated lifecycle commands'
    $falseFull = Invoke-FixtureFacade -Command 'start' -Seams $base -FixtureBackendOnly | ConvertFrom-Json
    Require ($falseFull.state -ceq 'TRANSPORT_VERIFICATION_FAILED') 'backend-only refuses a full-stack result masquerading as scoped evidence'
    $withoutOptIn = Invoke-FixtureFacade -Command 'start' -Seams $backend | ConvertFrom-Json
    Require ($withoutOptIn.state -ceq 'ACTION_REQUIRED') 'backend flag cannot be silently enabled by a recovery result'
    $actions.Clear()

    $redactedRecovery = $base.Clone()
    $redactedRecovery.Recovery = { param($engine, $root, $config, $expected, $manifest, $timeout, $command) 'https://example.invalid/route_fixture?token=credential_fixture' }
    $redacted = Invoke-FixtureFacade -Command 'recover' -Seams $redactedRecovery | ConvertFrom-Json
    Require ($redacted.state -eq 'TRANSPORT_VERIFICATION_FAILED') 'unexpected internal recovery output reduces to a fixed transport state'
    Require (($redacted | ConvertTo-Json -Compress) -notmatch 'route_fixture|credential_fixture|https?://') 'recovery output is redacted against route and credential fixtures'

    $runtimeGate = $base.Clone()
    $runtimeGate.Recovery = { param($engine, $root, $config, $expected, $manifest, $timeout, $command) '{"State":"TRANSPORT_VERIFICATION_FAILED","Gate":"RUNTIME_STATUS_TIMEOUT"}' }
    $runtimeGateResult = Invoke-FixtureFacade -Command 'recover' -Seams $runtimeGate | ConvertFrom-Json
    Require ($runtimeGateResult.state -eq 'TRANSPORT_VERIFICATION_FAILED' -and $runtimeGateResult.detail -eq 'gate=RUNTIME_STATUS_TIMEOUT') 'recovery preserves the fixed runtime verification gate'

    $localGate = $base.Clone()
    $localGate.Recovery = { param($engine, $root, $config, $expected, $manifest, $timeout, $command) '{"State":"TRANSPORT_VERIFICATION_FAILED","Gate":"LOCAL_MCP_RESPONSE_TIMEOUT"}' }
    $localGateResult = Invoke-FixtureFacade -Command 'recover' -Seams $localGate | ConvertFrom-Json
    Require ($localGateResult.state -eq 'TRANSPORT_VERIFICATION_FAILED' -and $localGateResult.detail -eq 'gate=LOCAL_MCP_RESPONSE_TIMEOUT') 'recovery preserves the same fixed local-MCP layer vocabulary used by diagnose'
    $localGate.Recovery = { '{"State":"TRANSPORT_VERIFICATION_FAILED","Gate":"LOCAL_MCP_LAUNCHED_PROCESS_EXITED"}' }
    $ownedExit = Invoke-FixtureFacade -Command 'recover' -Seams $localGate -FixtureBackendOnly | ConvertFrom-Json
    Require ($ownedExit.state -ceq 'TRANSPORT_VERIFICATION_FAILED' -and $ownedExit.detail -ceq 'gate=LOCAL_MCP_LAUNCHED_PROCESS_EXITED') 'backend-only recovery preserves terminal child exit'
    $wrongOwnerScope = Invoke-FixtureFacade -Command 'recover' -Seams $localGate | ConvertFrom-Json
    Require ($wrongOwnerScope.detail -cne 'gate=LOCAL_MCP_LAUNCHED_PROCESS_EXITED') 'full-stack recovery cannot claim a backend-only child exit'

    $partialRecovery = $base.Clone()
    $partialRecovery.Recovery = { param($engine, $root, $config, $expected, $manifest, $timeout, $command) '{"State":"CORE_CONNECTED_WAKE_PENDING","Gate":"WAKE_RUNTIME_NOT_READY"}' }
    $partialRecoverResult = Invoke-FixtureFacade -Command 'recover' -Seams $partialRecovery | ConvertFrom-Json
    Require ($partialRecoverResult.state -eq 'CORE_CONNECTED_WAKE_PENDING' -and $partialRecoverResult.detail -eq 'gate=WAKE_RUNTIME_NOT_READY') 'public recovery preserves verified core with a separate Wake gate'
    $partialStartResult = Invoke-FixtureFacade -Command 'start' -Seams $partialRecovery | ConvertFrom-Json
    Require ($partialStartResult.state -eq 'CORE_CONNECTED_WAKE_PENDING') 'public start does not promote partial Wake success to CONNECTED_VERIFIED'
    foreach ($invalidPartial in @(
        '{"State":"CORE_CONNECTED_WAKE_PENDING","Gate":"READY"}',
        '{"State":"CORE_CONNECTED_WAKE_PENDING","Gate":["WAKE_RUNTIME_NOT_READY"]}',
        '{"State":"CORE_CONNECTED_WAKE_PENDING","Gate":"WAKE_RUNTIME_NOT_READY","RecoverySource":"LAST_KNOWN_GOOD"}',
        '{"State":["CORE_CONNECTED_WAKE_PENDING"],"Gate":"WAKE_RUNTIME_NOT_READY"}'
    )) {
        $invalidWakeRecovery = $base.Clone()
        $invalidWakeRecovery.Recovery = { param($engine, $root, $config, $expected, $manifest, $timeout, $command) $invalidPartial }
        $invalidWakeResult = Invoke-FixtureFacade -Command 'recover' -Seams $invalidWakeRecovery | ConvertFrom-Json
        Require ($invalidWakeResult.state -eq 'TRANSPORT_VERIFICATION_FAILED' -and $invalidWakeResult.detail -eq 'redacted') 'malformed partial Wake result fails closed'
    }

    $invalidRuntimeGate = $base.Clone()
    $invalidRuntimeGate.Recovery = { param($engine, $root, $config, $expected, $manifest, $timeout, $command) '{"State":"TRANSPORT_VERIFICATION_FAILED","Gate":"https://example.invalid/secret"}' }
    $invalidRuntimeGateResult = Invoke-FixtureFacade -Command 'recover' -Seams $invalidRuntimeGate | ConvertFrom-Json
    Require ($invalidRuntimeGateResult.state -eq 'TRANSPORT_VERIFICATION_FAILED' -and $invalidRuntimeGateResult.detail -eq 'redacted') 'unrecognized runtime gate remains redacted'

    $actions.Clear()
    [void](Invoke-FixtureFacade -Command 'install' -Seams $base)
    Require (([string]::Join(',', $actions)) -eq 'install-client,codex-check') 'install performs bounded prerequisite preparation only'
    Require ($source -notmatch '(?i)(Start-Process\s+.*login|Invoke-WebRequest\s+.*login|platform.*create|Set-Content.*credential)') 'install facade does not expose login, platform, or credential flow'

    $actions.Clear()
    $verified = $base.Clone()
    $verified.Listener = { param($local, $identity) [pscustomobject]@{ Pid = 47; MatchesCanonical = $true } }
    $stop = Invoke-FixtureFacade -Command 'stop' -Seams $verified | ConvertFrom-Json
    Require ($stop.state -eq 'STOPPED') 'verified CatDesk listener can be stopped'
    Require (([string]::Join(',', $actions)) -eq 'stop:47') 'stop targets only the verified exact CatDesk listener instance'

    # T-0334: the public stop facade must fail closed if exact process-instance
    # acquisition/continuity rejects a same-path PID replacement. The lifecycle
    # seam models the reviewed pinned-process helper refusing before Kill().
    $actions.Clear()
    $replaced = $base.Clone()
    $replaced.Listener = { param($local, $identity) [pscustomobject]@{ Pid = 47; MatchesCanonical = $true } }
    $replaced.StopProcess = { param($local, $identity, $listener) throw 'fixture exact process instance changed' }
    $replacedStop = Invoke-FixtureFacade -Command 'stop' -Seams $replaced | ConvertFrom-Json
    Require ($replacedStop.state -eq 'STOP_REFUSED') 'public stop refuses a replaced process instance'
    Require ($actions.Count -eq 0) 'replaced process instance cannot cause PID-directed mutation'

    $actions.Clear()
    $mismatch = $base.Clone()
    $mismatch.Listener = { param($local, $identity) [pscustomobject]@{ Pid = 48; MatchesCanonical = $false } }
    $refusal = Invoke-FixtureFacade -Command 'stop' -Seams $mismatch | ConvertFrom-Json
    Require ($refusal.state -eq 'STOP_REFUSED') 'mismatched listener fails closed'
    Require ($actions.Count -eq 0) 'mismatched listener never receives a stop request'

    $missingRelease = $base.Clone()
    $missingRelease.CanonicalIdentity = { throw 'release missing' }
    $releaseState = Invoke-FixtureFacade -Command 'status' -Seams $missingRelease | ConvertFrom-Json
    Require ($releaseState.state -eq 'RECOVERY_AUTHORITY_REQUIRED') 'missing release without durable authority fails closed without mutation'
    Require ($actions.Count -eq 0) 'missing release does not invoke any side effect'
    [void](Invoke-FixtureFacade -Command 'start' -Seams $missingRelease)
    [void](Invoke-FixtureFacade -Command 'recover' -Seams $missingRelease)
    Require (([string]::Join(',', $actions)) -eq 'recover:start,recover:recover') 'broken canonical identity still reaches the bounded trusted recovery engine for start/recover'
    $actions.Clear()

    $recoverableRelease = $base.Clone()
    $recoverableRelease.CanonicalIdentity = { throw 'release fingerprint mismatch' }
    $recoverableRelease.RecoveryReadiness = { param($engine, $root, $config, $expected, $manifest, $timeout) '{"State":"RECOVERY_READY","RecoverySource":"LAST_KNOWN_GOOD"}' }
    $recoverableStatus = Invoke-FixtureFacade -Command 'status' -Seams $recoverableRelease | ConvertFrom-Json
    Require ($recoverableStatus.state -eq 'RECOVERY_AVAILABLE' -and $recoverableStatus.detail -eq 'source=LAST_KNOWN_GOOD') 'canonical mismatch with exact LKG authority reports bounded recovery availability'
    Require ($actions.Count -eq 0) 'read-only recovery assessment never starts recovery or changes tunnel ownership'

    $recoverableDiagnosis = Invoke-FixtureFacade -Command 'diagnose' -Seams $recoverableRelease | ConvertFrom-Json
    Require ($recoverableDiagnosis.state -eq 'RECOVERY_AVAILABLE' -and $recoverableDiagnosis.primaryLayer -eq 'CANONICAL_RELEASE' -and $recoverableDiagnosis.nextAction -eq 'RUN_RECOVER') 'diagnose turns a broken canonical pair with reviewed LKG into one clear recovery action'
    Require ((@($recoverableDiagnosis.layers) | Where-Object { $_.layer -eq 'RECOVERY_AUTHORITY' }).gate -eq 'LAST_KNOWN_GOOD') 'diagnose reports the bounded reviewed recovery source without paths'
    Require ((@($recoverableDiagnosis.layers) | Where-Object { $_.layer -eq 'CODEX_CLI' }).state -eq 'READY') 'faulted canonical release still reports independent authenticated Codex readiness'
    Require ($actions.Count -eq 0) 'recoverable diagnosis remains non-mutating'

    $forbiddenActions = [Collections.Generic.List[string]]::new()
    $unavailableCodexRelease = $missingRelease.Clone()
    $unavailableCodexRelease.RecoveryReadiness = { param($engine, $root, $config, $expected, $manifest, $timeout) '{"State":"LKG_AUTHORITY_MISSING"}' }
    $unavailableCodexRelease.CodexStatus = { [pscustomobject]@{ Ready = $false; Gate = 'CODEX_CLI_UNAVAILABLE' } }
    $unavailableCodexRelease.Recovery = { [void]$forbiddenActions.Add('recover'); throw 'recovery must not execute during diagnosis' }
    $unavailableCodexRelease.StopProcess = { [void]$forbiddenActions.Add('stop'); throw 'stop must not execute during diagnosis' }
    $unavailableCodexRelease.WakeRuntime = { [void]$forbiddenActions.Add('wake'); throw 'Wake must not execute during canonical failure diagnosis' }
    $unavailableCodexDiagnosis = Invoke-FixtureFacade -Command 'diagnose' -Seams $unavailableCodexRelease | ConvertFrom-Json
    Require ($unavailableCodexDiagnosis.state -eq 'ACTION_REQUIRED' -and $unavailableCodexDiagnosis.primaryLayer -eq 'RECOVERY_AUTHORITY' -and $unavailableCodexDiagnosis.nextAction -eq 'OPERATOR_ATTENTION') 'Codex unavailability does not weaken the failed release authority gate'
    Require ((@($unavailableCodexDiagnosis.layers) | Where-Object { $_.layer -eq 'CANONICAL_RELEASE' }).state -eq 'FAILED') 'diagnosis retains the canonical release failure'
    Require ((@($unavailableCodexDiagnosis.layers) | Where-Object { $_.layer -eq 'RECOVERY_AUTHORITY' }).gate -eq 'LKG_AUTHORITY_MISSING') 'diagnosis retains missing reviewed recovery authority'
    Require ((@($unavailableCodexDiagnosis.layers) | Where-Object { $_.layer -eq 'CODEX_CLI' }).gate -eq 'CODEX_CLI_UNAVAILABLE') 'faulted canonical release reports the fixed independent Codex failure gate'
    Require ($forbiddenActions.Count -eq 0 -and $actions.Count -eq 0) 'diagnosis does not recover, stop, or start Wake while release authority is missing'
    Require (($unavailableCodexDiagnosis | ConvertTo-Json -Compress -Depth 5) -notmatch '(?i)secret|token|route_|https?://') 'faulted-release diagnosis remains redacted'

    $interruptedRelease = $recoverableRelease.Clone()
    $interruptedRelease.RecoveryReadiness = { param($engine, $root, $config, $expected, $manifest, $timeout) '{"State":"RECOVERY_READY","RecoverySource":"INTERRUPTED_PROMOTION"}' }
    $interruptedStatus = Invoke-FixtureFacade -Command 'status' -Seams $interruptedRelease | ConvertFrom-Json
    Require ($interruptedStatus.state -eq 'RECOVERY_AVAILABLE' -and $interruptedStatus.detail -eq 'source=INTERRUPTED_PROMOTION') 'exact interrupted promotion authority reports bounded recovery availability'
    Require ($actions.Count -eq 0) 'interrupted promotion assessment remains non-mutating'

    $ambiguousRelease = $base.Clone()
    $ambiguousRelease.CanonicalIdentity = { throw 'release fingerprint mismatch' }
    $ambiguousRelease.RecoveryReadiness = { param($engine, $root, $config, $expected, $manifest, $timeout) '{"State":"LKG_AUTHORITY_AMBIGUOUS_OR_DAMAGED"}' }
    $ambiguousStatus = Invoke-FixtureFacade -Command 'status' -Seams $ambiguousRelease | ConvertFrom-Json
    Require ($ambiguousStatus.state -eq 'LKG_AUTHORITY_AMBIGUOUS_OR_DAMAGED') 'ambiguous recovery authority remains fail-closed'
    Require ($actions.Count -eq 0) 'ambiguous recovery authority does not trigger lifecycle work'

    $ambiguousDiagnosis = Invoke-FixtureFacade -Command 'diagnose' -Seams $ambiguousRelease | ConvertFrom-Json
    Require ($ambiguousDiagnosis.state -eq 'ACTION_REQUIRED' -and $ambiguousDiagnosis.primaryLayer -eq 'RECOVERY_AUTHORITY' -and $ambiguousDiagnosis.nextAction -eq 'OPERATOR_ATTENTION') 'diagnose isolates damaged recovery authority instead of suggesting blind recovery'
    Require ((@($ambiguousDiagnosis.layers) | Where-Object { $_.layer -eq 'RECOVERY_AUTHORITY' }).gate -eq 'LKG_AUTHORITY_AMBIGUOUS_OR_DAMAGED') 'diagnose keeps the exact bounded recovery-authority failure class'

    $badLocalConfig = $base.Clone()
    $badLocalConfig.LocalMcp = { param($config) throw 'fixture endpoint secret must never escape' }
    $localConfigState = Invoke-FixtureFacade -Command 'status' -Seams $badLocalConfig | ConvertFrom-Json
    Require ($localConfigState.state -eq 'STATUS_UNAVAILABLE' -and $localConfigState.detail -eq 'stage=LOCAL_MCP_CONFIG') 'status reports only the fixed local-MCP-config failure stage'

    $localConfigDiagnosis = Invoke-FixtureFacade -Command 'diagnose' -Seams $badLocalConfig | ConvertFrom-Json
    Require ($localConfigDiagnosis.state -eq 'ACTION_REQUIRED' -and $localConfigDiagnosis.primaryLayer -eq 'LOCAL_MCP_CONFIG' -and $localConfigDiagnosis.nextAction -eq 'OPERATOR_ATTENTION') 'diagnose isolates local MCP configuration failure without suggesting blind daemon recovery'
    Require ((@($localConfigDiagnosis.layers) | Where-Object { $_.layer -eq 'LOCAL_MCP_CONFIG' }).gate -eq 'LOCAL_MCP_CONFIG_UNAVAILABLE') 'diagnose emits the fixed local MCP configuration gate'

    $badLocalReadiness = $base.Clone()
    $badLocalReadiness.LocalMcpReadiness = { param($local, $identity) throw 'fixture route secret must never escape' }
    $localReadinessState = Invoke-FixtureFacade -Command 'status' -Seams $badLocalReadiness | ConvertFrom-Json
    Require ($localReadinessState.state -eq 'STATUS_UNAVAILABLE' -and $localReadinessState.detail -eq 'stage=LOCAL_MCP_READINESS') 'status reports only the fixed local-MCP-readiness failure stage'

    $badRuntime = $base.Clone()
    $badRuntime.RuntimeStatus = { param($root, $config) throw 'fixture tunnel secret must never escape' }
    $runtimeState = Invoke-FixtureFacade -Command 'status' -Seams $badRuntime | ConvertFrom-Json
    Require ($runtimeState.state -eq 'STATUS_UNAVAILABLE' -and $runtimeState.detail -eq 'stage=OFFICIAL_RUNTIME') 'status reports only the fixed official-runtime failure stage'
    Require ((@($releaseState, $localConfigState, $localReadinessState, $runtimeState) | ConvertTo-Json -Compress) -notmatch '(?i)fixture endpoint secret|fixture route secret|fixture tunnel secret|https?://') 'stage diagnostics never project internal exception text or endpoints'

    $ready = $base.Clone()
    $ready.Listener = { param($local, $identity) [pscustomobject]@{ Pid = 49; MatchesCanonical = $true } }
    $ready.LocalMcpReadiness = { param($local, $identity) $true }
    $ready.RuntimeStatus = { param($root, $config) $true }
    $first = Invoke-FixtureFacade -Command 'status' -Seams $ready | ConvertFrom-Json
    $second = Invoke-FixtureFacade -Command 'status' -Seams $ready | ConvertFrom-Json
    Require ($first.state -eq 'READY' -and $second.state -eq 'READY') 'repeated healthy status is idempotent'
    Require ($actions.Count -eq 0) 'repeated healthy status remains non-mutating'

    $readyDiagnosis = Invoke-FixtureFacade -Command 'diagnose' -Seams $ready | ConvertFrom-Json
    Require ($readyDiagnosis.state -eq 'HEALTHY' -and $readyDiagnosis.primaryLayer -eq 'NONE' -and $readyDiagnosis.nextAction -eq 'NONE') 'healthy diagnosis reports no failing layer or recovery action'
    Require ((@($readyDiagnosis.layers) | Where-Object { $_.state -eq 'FAILED' }).Count -eq 0) 'healthy diagnosis has no failed layers'
    Require ($actions.Count -eq 0) 'healthy diagnosis remains non-mutating'

    $listenerMismatchDiagnosisSeams = $ready.Clone()
    $listenerMismatchDiagnosisSeams.Listener = { param($local, $identity) [pscustomobject]@{ Pid = 52; MatchesCanonical = $false } }
    $listenerMismatchDiagnosis = Invoke-FixtureFacade -Command 'diagnose' -Seams $listenerMismatchDiagnosisSeams | ConvertFrom-Json
    Require ($listenerMismatchDiagnosis.state -eq 'DEGRADED' -and $listenerMismatchDiagnosis.primaryLayer -eq 'LOCAL_DAEMON' -and $listenerMismatchDiagnosis.nextAction -eq 'RUN_RECOVER') 'diagnose identifies listener identity mismatch as the local-daemon repair layer'
    Require ((@($listenerMismatchDiagnosis.layers) | Where-Object { $_.layer -eq 'LOCAL_DAEMON' }).gate -eq 'LOCAL_MCP_LISTENER_IDENTITY_MISMATCH') 'diagnose preserves listener identity mismatch gate'

    $protocolFailure = $ready.Clone()
    $protocolFailure.LocalMcpReadiness = { param($local, $identity) [pscustomobject]@{ Ready = $false; Gate = 'LOCAL_MCP_RESPONSE_TIMEOUT' } }
    $protocolDiagnosis = Invoke-FixtureFacade -Command 'diagnose' -Seams $protocolFailure | ConvertFrom-Json
    Require ($protocolDiagnosis.state -eq 'DEGRADED' -and $protocolDiagnosis.primaryLayer -eq 'LOCAL_MCP_PROTOCOL' -and $protocolDiagnosis.nextAction -eq 'RUN_RECOVER') 'diagnose identifies MCP protocol readiness failure independently from listener identity'
    Require ((@($protocolDiagnosis.layers) | Where-Object { $_.layer -eq 'LOCAL_MCP_PROTOCOL' }).gate -eq 'LOCAL_MCP_RESPONSE_TIMEOUT') 'diagnose preserves the fixed MCP protocol failure gate'

    $wakeFailure = $ready.Clone()
    $wakeFailure.WakeRuntime = { param($root) $false }
    $wakeDiagnosis = Invoke-FixtureFacade -Command 'diagnose' -Seams $wakeFailure | ConvertFrom-Json
    Require ($wakeDiagnosis.state -eq 'DEGRADED' -and $wakeDiagnosis.primaryLayer -eq 'WAKE_RUNTIME' -and $wakeDiagnosis.nextAction -eq 'RUN_RECOVER') 'diagnose isolates Wake runtime failure after local MCP readiness'
    Require ((@($wakeDiagnosis.layers) | Where-Object { $_.layer -eq 'WAKE_RUNTIME' }).gate -eq 'WAKE_RUNTIME_NOT_READY') 'diagnose preserves the fixed Wake runtime gate'

    $runtimeTimeout = $ready.Clone()
    $runtimeTimeout.RuntimeStatus = { param($root, $config) [pscustomobject]@{ Verified = $false; Gate = 'RUNTIME_STATUS_TIMEOUT' } }
    $runtimeTimeoutDiagnosis = Invoke-FixtureFacade -Command 'diagnose' -Seams $runtimeTimeout | ConvertFrom-Json
    Require ($runtimeTimeoutDiagnosis.state -eq 'DEGRADED' -and $runtimeTimeoutDiagnosis.primaryLayer -eq 'OFFICIAL_RUNTIME' -and $runtimeTimeoutDiagnosis.nextAction -eq 'RUN_RECOVER') 'diagnose maps recoverable official-runtime timeout to one-command recovery'
    Require ((@($runtimeTimeoutDiagnosis.layers) | Where-Object { $_.layer -eq 'OFFICIAL_RUNTIME' }).gate -eq 'RUNTIME_STATUS_TIMEOUT') 'diagnose preserves the fixed official-runtime timeout gate'

    $runtimeUnavailableDiagnosisSeams = $ready.Clone()
    $runtimeUnavailableDiagnosisSeams.RuntimeStatus = { param($root, $config) throw 'fixture runtime internals must never escape' }
    $runtimeUnavailableDiagnosis = Invoke-FixtureFacade -Command 'diagnose' -Seams $runtimeUnavailableDiagnosisSeams | ConvertFrom-Json
    Require ($runtimeUnavailableDiagnosis.state -eq 'DEGRADED' -and $runtimeUnavailableDiagnosis.primaryLayer -eq 'OFFICIAL_RUNTIME' -and $runtimeUnavailableDiagnosis.nextAction -eq 'OPERATOR_ATTENTION') 'diagnose reserves operator attention for unclassified official-runtime verification failure'
    Require ((@($runtimeUnavailableDiagnosis.layers) | Where-Object { $_.layer -eq 'OFFICIAL_RUNTIME' }).gate -eq 'RUNTIME_VERIFICATION_UNAVAILABLE') 'diagnose redacts unclassified runtime failure behind one fixed gate'

    $codexUnavailable = $ready.Clone()
    $codexUnavailable.CodexStatus = { [pscustomobject]@{ Ready = $false; Gate = 'CODEX_CLI_UNAVAILABLE' } }
    $codexDiagnosis = Invoke-FixtureFacade -Command 'diagnose' -Seams $codexUnavailable | ConvertFrom-Json
    Require ($codexDiagnosis.state -eq 'DEGRADED' -and $codexDiagnosis.primaryLayer -eq 'CODEX_CLI' -and $codexDiagnosis.nextAction -eq 'OPERATOR_ATTENTION') 'diagnose isolates Codex CLI availability only after CatDesk runtime layers are healthy'
    Require ((@($codexDiagnosis.layers) | Where-Object { $_.layer -eq 'CODEX_CLI' }).gate -eq 'CODEX_CLI_UNAVAILABLE') 'diagnose preserves the fixed Codex CLI availability gate'

    Require ((@($listenerMismatchDiagnosis, $protocolDiagnosis, $wakeDiagnosis, $runtimeTimeoutDiagnosis, $runtimeUnavailableDiagnosis, $codexDiagnosis) | ConvertTo-Json -Compress -Depth 5) -notmatch '(?i)fixture runtime internals|https?://') 'failure-matrix diagnosis does not project internal runtime details'
    Require ($actions.Count -eq 0) 'failure-matrix diagnosis remains non-mutating'

    $canonicalButUnready = $base.Clone()
    $canonicalButUnready.Listener = { param($local, $identity) [pscustomobject]@{ Pid = 50; MatchesCanonical = $true } }
    $canonicalButUnready.LocalMcpReadiness = { param($local, $identity) $false }
    $canonicalButUnready.RuntimeStatus = { param($root, $config) $true }
    $unready = Invoke-FixtureFacade -Command 'status' -Seams $canonicalButUnready | ConvertFrom-Json
    Require ($unready.state -eq 'LOCAL_DAEMON_PENDING') 'canonical listener without JSON-RPC readiness is never READY'
    Require ($actions.Count -eq 0) 'unready local MCP status remains non-mutating'

    $localOnly = $base.Clone()
    $localOnly.LocalMcpReadiness = { param($local, $identity) $true }
    $localOnly.RuntimeStatus = { param($root, $config) $false }
    $localPending = Invoke-FixtureFacade -Command 'status' -Seams $localOnly | ConvertFrom-Json
    Require ($localPending.state -eq 'LOCAL_READY_EXTERNAL_RUNTIME_PENDING') 'verified local JSON-RPC readiness with pending runtime is explicit'
    Require ($actions.Count -eq 0) 'local-only readiness status remains non-mutating'

    $clientMissing = $ready.Clone()
    $clientMissing.RuntimeStatus = { param($root, $config) [pscustomobject]@{ Verified = $false; Gate = 'RUNTIME_CLIENT_UNAVAILABLE' } }
    $clientMissingDiagnosis = Invoke-FixtureFacade -Command 'diagnose' -Seams $clientMissing | ConvertFrom-Json
    Require ($clientMissingDiagnosis.state -eq 'DEGRADED' -and $clientMissingDiagnosis.primaryLayer -eq 'OFFICIAL_RUNTIME' -and $clientMissingDiagnosis.nextAction -eq 'RUN_INSTALL') 'diagnose distinguishes missing official runtime client from daemon recovery'
    Require ((@($clientMissingDiagnosis.layers) | Where-Object { $_.layer -eq 'OFFICIAL_RUNTIME' }).gate -eq 'RUNTIME_CLIENT_UNAVAILABLE') 'diagnose preserves missing runtime client gate'

    $wake = $base.Clone()
    $wake.Listener = { param($local, $identity) [pscustomobject]@{ Pid = 51; MatchesCanonical = $true } }
    $wake.WakeMcpCall = {
        param($uri, $body)
        $request = $body | ConvertFrom-Json -ErrorAction Stop
        $tool = [string]$request.params.name
        [void]$actions.Add("wake:$tool")
        $mode = if ($tool -eq 'autonomy_wake_policy_set') { [string]$request.params.arguments.mode } else { 'INDEFINITE' }
        $terminal = if ($tool -eq 'autonomy_wake_policy_set') { [string]$request.params.arguments.terminalTaskId } else { '' }
        $policy = @{ schema_version = 1; generation = if ($tool -eq 'autonomy_wake_policy_set') { 8 } else { 7 }; mode = $mode; terminal_task_id = if ($terminal) { $terminal } else { $null }; set_at_unix = 1; stopped_reason = $null }
        $structured = if ($tool -eq 'autonomy_wake_policy_get') { @{ policy = $policy; readiness = 'READY' } } else { @{ policy = $policy } }
        [pscustomobject]@{ StatusCode = 200; Content = (@{ jsonrpc = '2.0'; id = 'catdesk-wake-policy'; result = @{ structuredContent = $structured } } | ConvertTo-Json -Compress -Depth 8) }
    }
    $actions.Clear()
    $wakeStatus = Invoke-FixtureFacade -Command 'wake' -AutostartAction 'status' -Seams $wake | ConvertFrom-Json
    Require ($wakeStatus.state -eq 'WAKE_POLICY' -and $wakeStatus.mode -eq 'INDEFINITE' -and $wakeStatus.readiness -eq 'READY') 'wake status exposes bounded durable policy state'
    Require (([string]::Join(',', $actions)) -eq 'wake:autonomy_wake_policy_get') 'wake status uses only the read control-plane operation'
    $actions.Clear()
    $wakeThrough = Invoke-FixtureFacade -Command 'wake' -AutostartAction 'through' -WakeTerminalTaskId 'T-0054-AUTOWAKE-CONTROL-MODES' -Seams $wake | ConvertFrom-Json
    Require ($wakeThrough.state -eq 'WAKE_POLICY_UPDATED' -and $wakeThrough.mode -eq 'THROUGH_TASK' -and $wakeThrough.stoppingCondition -eq 'EXACT_TASK:T-0054-AUTOWAKE-CONTROL-MODES') 'wake through uses an exact task-id control shape'
    Require (([string]::Join(',', $actions)) -eq 'wake:autonomy_wake_policy_get,wake:autonomy_wake_policy_set') 'wake change performs a bounded read then CAS-safe set'
    $actions.Clear()
    $wakeInvalid = Invoke-FixtureFacade -Command 'wake' -AutostartAction 'through' -WakeTerminalTaskId 'invalid/task' -Seams $wake | ConvertFrom-Json
    Require ($wakeInvalid.state -eq 'ACTION_REQUIRED' -and $actions.Count -eq 0) 'wake facade rejects non-task-id control input before local MCP access'

    $Global:CatDeskAutostartFixtureTask = $null
    $Global:CatDeskAutostartRunValue = $null
    $Global:CatDeskAutostartSupervisorRunning = $false
    $Global:CatDeskAutostartDefinitions = [Collections.Generic.List[object]]::new()
    $autostart = $base.Clone()
    $autostart.ScheduledTaskGet = { param($definition) [void]$Global:CatDeskAutostartDefinitions.Add($definition); $Global:CatDeskAutostartFixtureTask }
    $autostart.ScheduledTaskRegister = { param($definition) [void]$actions.Add('task-register'); $Global:CatDeskAutostartFixtureTask = New-OwnedTask $definition }
    $autostart.ScheduledTaskUnregister = { param($definition) [void]$actions.Add('task-unregister'); $Global:CatDeskAutostartFixtureTask = $null }
    $autostart.AutostartRunKeyGet = { param($definition) $Global:CatDeskAutostartRunValue }
    $autostart.AutostartRunKeyRegister = { param($definition) [void]$actions.Add('runkey-register'); $Global:CatDeskAutostartRunValue = $definition.RunKeyValue }
    $autostart.AutostartRunKeyUnregister = { param($definition) [void]$actions.Add('runkey-unregister'); $Global:CatDeskAutostartRunValue = $null }
    $autostart.AutostartSupervisorRunning = { param($definition) $Global:CatDeskAutostartSupervisorRunning }
    $autostart.AutostartSupervisorStart = { param($definition) [void]$actions.Add('supervisor-start'); $Global:CatDeskAutostartSupervisorRunning = $true }
    $autostart.AutostartPowerShellHost = { 'C:\fixture\powershell.exe' }
    $disabled = Invoke-FixtureFacade -Command 'autostart' -AutostartAction 'status' -Seams $autostart | ConvertFrom-Json
    Require ($disabled.state -eq 'AUTOSTART_DISABLED') 'absent autostart task is reported disabled without mutation'
    Require ($actions.Count -eq 0) 'autostart status is non-mutating'
    $normalDefinition = $Global:CatDeskAutostartDefinitions[$Global:CatDeskAutostartDefinitions.Count - 1]
    $caseStatus = Invoke-FixtureFacade -Command 'autostart' -AutostartAction 'status' -Seams $autostart -FixtureWorkspace $workspace.ToUpperInvariant() | ConvertFrom-Json
    $caseDefinition = $Global:CatDeskAutostartDefinitions[$Global:CatDeskAutostartDefinitions.Count - 1]
    Require ($caseStatus.state -eq 'AUTOSTART_DISABLED' -and $actions.Count -eq 0) 'case-alias status never registers or starts a monitor'
    Require ($normalDefinition.SupervisorWorkspaceId -ceq $caseDefinition.SupervisorWorkspaceId) 'public facade and supervisor use one physical directory spelling for their singleton'
    $caseRoot = (Resolve-Path -LiteralPath $workspace.ToUpperInvariant()).Path
    $caseHash = [Security.Cryptography.SHA256]::Create()
    try { $legacyCaseId = (([BitConverter]::ToString($caseHash.ComputeHash([Text.Encoding]::UTF8.GetBytes($caseRoot)))).Replace('-', '').ToLowerInvariant()).Substring(0, 16) } finally { $caseHash.Dispose() }
    Require ($caseDefinition.WorkspaceId -ceq $legacyCaseId -and $caseDefinition.TaskName -ceq "CatDesk.Autostart.$legacyCaseId" -and $caseDefinition.RunKeyName -ceq $caseDefinition.TaskName) 'case-alias persistence retains its exact legacy names'
    Require ($caseDefinition.Arguments.Contains((' -Workspace "{0}"' -f $caseRoot))) 'case-alias persistence retains original launch arguments'
    $priorDirectory = Get-Location
    try {
        Set-Location ([IO.Path]::GetTempPath())
        $positional = & $facade autostart status -Workspace $workspace -ConfigPath 'C:\fixture\config.toml' -BuildFingerprintPath 'C:\fixture\catdesk.exe.sha256' -TestSeams $autostart | ConvertFrom-Json
    } finally { Set-Location $priorDirectory }
    Require ($positional.state -eq 'AUTOSTART_DISABLED') 'public autostart status positional syntax resolves from arbitrary CWD'
    $enabled = Invoke-FixtureFacade -Command 'autostart' -AutostartAction 'enable' -Seams $autostart | ConvertFrom-Json
    Require ($enabled.state -eq 'AUTOSTART_ENABLED' -and $actions -contains 'task-register') 'enable registers one owned task'
    $definition = $Global:CatDeskAutostartDefinitions[0]
    Require ($definition.TaskName -match '^CatDesk\.Autostart\.[0-9a-f]{16}$') 'task identity is deterministic and workspace-bound'
    Require ($definition.Execute -eq 'C:\fixture\powershell.exe' -and $definition.RunKeyValue -match '^"C:\\fixture\\powershell\.exe"') 'autostart persistence uses a resolved PowerShell executable identity'
    Require ($definition.UserId -eq 'FIXTURE\current-user') 'task identity binds the exact current user'
    Require ($definition.Arguments -match 'catdesk-autostart-supervisor\.ps1' -and $definition.Arguments -match '-Workspace') 'task action invokes the project-local supervisor with workspace identity'
    Require ($definition.Arguments -notmatch '(?i)credential|token|route|tunnel') 'task action contains no credential, route, or tunnel material'
    Require ((Get-Content -LiteralPath $facade -Raw) -notmatch 'Register-ScheduledTask.*-Force') 'task registration never force-overwrites a race winner'
    Require ($source -match 'ExecutionTimeLimit \(\[TimeSpan\]::Zero\)' -and $source -match 'AllowStartIfOnBatteries' -and $source -match 'DontStopIfGoingOnBatteries') 'persistent task settings have no finite cutoff and support laptop continuation'
    Require ($source -notmatch '\$powershellHost\s*=\s*''powershell\.exe''') 'autostart never falls back to an unresolved PowerShell executable name'
    $actions.Clear()
    $again = Invoke-FixtureFacade -Command 'autostart' -AutostartAction 'enable' -Seams $autostart | ConvertFrom-Json
    Require ($again.state -eq 'AUTOSTART_ENABLED' -and $actions.Count -eq 0) 'enable is idempotent for exact owned task'
    $Global:CatDeskAutostartSupervisorRunning = $false
    $stoppedMonitor = Invoke-FixtureFacade -Command 'autostart' -AutostartAction 'status' -Seams $autostart | ConvertFrom-Json
    Require ($stoppedMonitor.state -eq 'AUTOSTART_REGISTERED_MONITOR_STOPPED' -and $actions.Count -eq 0) 'owned registration without its monitor is not enabled live recovery and status starts nothing'
    $unavailableMonitor = $autostart.Clone()
    $unavailableMonitor.AutostartSupervisorRunning = { param($definition) throw 'private fixture monitor query failure' }
    $unavailableStatus = Invoke-FixtureFacade -Command 'autostart' -AutostartAction 'status' -Seams $unavailableMonitor | ConvertFrom-Json
    Require ($unavailableStatus.state -eq 'AUTOSTART_UNAVAILABLE' -and $actions.Count -eq 0 -and ($unavailableStatus | ConvertTo-Json) -notmatch 'private fixture') 'unavailable monitor observation fails closed without exposing query details or starting anything'
    $Global:CatDeskAutostartSupervisorRunning = $true
    $actions.Clear()
    $conflict = $autostart.Clone()
    $conflict.ScheduledTaskGet = { param($definition) $task = New-OwnedTask $definition; $task.Actions[0].Arguments = 'mismatch'; $task }
    $conflictStatus = Invoke-FixtureFacade -Command 'autostart' -AutostartAction 'status' -Seams $conflict | ConvertFrom-Json
    $conflictEnable = Invoke-FixtureFacade -Command 'autostart' -AutostartAction 'enable' -Seams $conflict | ConvertFrom-Json
    $conflictDisable = Invoke-FixtureFacade -Command 'autostart' -AutostartAction 'disable' -Seams $conflict | ConvertFrom-Json
    Require ($conflictStatus.state -eq 'AUTOSTART_CONFLICT' -and $conflictEnable.state -eq 'AUTOSTART_CONFLICT' -and $conflictDisable.state -eq 'AUTOSTART_CONFLICT') 'mismatched same-name task fails closed for all actions'
    Require ($actions.Count -eq 0) 'conflicting task is never overwritten or removed'
    $foreign = $autostart.Clone()
    $foreign.ScheduledTaskGet = { param($definition) $task = New-OwnedTask $definition; $task.Principal.UserId = 'FIXTURE\other-user'; $task }
    $foreignStatus = Invoke-FixtureFacade -Command 'autostart' -AutostartAction 'status' -Seams $foreign | ConvertFrom-Json
    $foreignDisable = Invoke-FixtureFacade -Command 'autostart' -AutostartAction 'disable' -Seams $foreign | ConvertFrom-Json
    Require ($foreignStatus.state -eq 'AUTOSTART_CONFLICT' -and $foreignDisable.state -eq 'AUTOSTART_CONFLICT') 'foreign-principal task is refused'
    Require ($actions.Count -eq 0) 'foreign-principal task is never removed'
    $disabledAgain = Invoke-FixtureFacade -Command 'autostart' -AutostartAction 'disable' -Seams $autostart | ConvertFrom-Json
    Require ($disabledAgain.state -eq 'AUTOSTART_UNREGISTERED_MONITOR_ACTIVE' -and ($actions -join ',') -ceq 'task-unregister') 'disable removes only exact owned task and reports the surviving monitor'
    $actions.Clear()

    # Standard-user estates may refuse root Task Scheduler registration. The
    # exact user-scoped Run value is the bounded fallback, and enable must also
    # arm the singleton monitor immediately rather than waiting for next logon.
    $Global:CatDeskAutostartFixtureTask = $null
    $Global:CatDeskAutostartRunValue = $null
    $Global:CatDeskAutostartSupervisorRunning = $false
    $fallback = $autostart.Clone()
    $fallback.ScheduledTaskRegister = { param($definition) [void]$actions.Add('task-register-failed'); throw 'fixture scheduler unavailable' }
    $fallbackEnabled = Invoke-FixtureFacade -Command 'autostart' -AutostartAction 'enable' -Seams $fallback | ConvertFrom-Json
    Require ($fallbackEnabled.state -eq 'AUTOSTART_ENABLED') 'scheduler-unavailable enable falls back to exact current-user persistence'
    Require (([string]::Join(',', $actions)) -eq 'task-register-failed,runkey-register,supervisor-start') 'fallback registers only the user-scoped persistence and arms the supervisor immediately'
    $actions.Clear()
    $fallbackStatus = Invoke-FixtureFacade -Command 'autostart' -AutostartAction 'status' -Seams $fallback | ConvertFrom-Json
    Require ($fallbackStatus.state -eq 'AUTOSTART_ENABLED' -and $actions.Count -eq 0) 'run-key fallback status is owned and non-mutating'
    $Global:CatDeskAutostartSupervisorRunning = $false
    $stoppedFallback = Invoke-FixtureFacade -Command 'autostart' -AutostartAction 'status' -Seams $fallback | ConvertFrom-Json
    Require ($stoppedFallback.state -eq 'AUTOSTART_REGISTERED_MONITOR_STOPPED' -and $actions.Count -eq 0) 'logon Run persistence does not imply a live same-session monitor or restart it during status'
    $fallbackDisable = Invoke-FixtureFacade -Command 'autostart' -AutostartAction 'disable' -Seams $fallback | ConvertFrom-Json
    Require ($fallbackDisable.state -eq 'AUTOSTART_DISABLED' -and ([string]::Join(',', $actions)) -eq 'runkey-unregister') 'disable removes only the exact owned run-key fallback'
    $actions.Clear()

    # Scope is part of the exact persisted command, not permission to rewrite
    # an existing owner. All registration/process effects below are fixtures.
    $backendEnabled = Invoke-FixtureFacade -Command 'autostart' -AutostartAction 'enable' -Seams $autostart -FixtureBackendOnly | ConvertFrom-Json
    Require ($backendEnabled.state -ceq 'AUTOSTART_ENABLED' -and $backendEnabled.detail -ceq 'scope=BACKEND_ONLY') 'backend-only scope is selectable through the public facade'
    $backendDefinition = $Global:CatDeskAutostartDefinitions[$Global:CatDeskAutostartDefinitions.Count - 1]
    Require ($backendDefinition.BackendOnly -and $backendDefinition.Arguments.EndsWith(' -BackendOnly') -and $backendDefinition.MonitorArguments.EndsWith(' -BackendOnly')) 'logon and immediate monitor commands retain backend-only scope'
    Require ($backendDefinition.TaskName -ceq $normalDefinition.TaskName -and $backendDefinition.Execute -ceq $normalDefinition.Execute) 'backend-only mode reuses the existing owner identity'
    $actions.Clear()
    $backendStatus = Invoke-FixtureFacade -Command 'autostart' -AutostartAction 'status' -Seams $autostart | ConvertFrom-Json
    Require ($backendStatus.state -ceq 'AUTOSTART_ENABLED' -and $backendStatus.detail -ceq 'scope=BACKEND_ONLY' -and $actions.Count -eq 0) 'ordinary status recognizes the exact backend-only registration without starting anything'
    $widen = Invoke-FixtureFacade -Command 'autostart' -AutostartAction 'enable' -Seams $autostart | ConvertFrom-Json
    Require ($widen.state -ceq 'AUTOSTART_SCOPE_CONFLICT' -and $actions.Count -eq 0) 'default enable cannot widen an existing backend-only owner'
    $backendDoctor = Invoke-FixtureFacade -Command 'doctor' -Seams $autostart | ConvertFrom-Json
    Require ((@($backendDoctor.layers | Where-Object layer -CEQ 'PERSISTENT_SUPERVISOR')[0]).code -ceq 'SUPERVISOR_MONITOR_SIGNAL_PRESENT') 'doctor recognizes the same exact backend-only owner as status'
    Require ($actions.Count -eq 0) 'doctor never starts or edits the registered owner'

    $Global:CatDeskAutostartRunValue = $normalDefinition.RunKeyValue
    foreach ($action in @('status','enable','disable')) {
        $mixed = Invoke-FixtureFacade -Command 'autostart' -AutostartAction $action -Seams $autostart -FixtureBackendOnly | ConvertFrom-Json
        Require ($mixed.state -ceq 'AUTOSTART_SCOPE_CONFLICT' -and $actions.Count -eq 0) 'different exact task/Run scopes fail closed without merging owners'
    }
    $mixedDoctor = Invoke-FixtureFacade -Command 'doctor' -Seams $autostart | ConvertFrom-Json
    Require ((@($mixedDoctor.layers | Where-Object layer -CEQ 'PERSISTENT_SUPERVISOR')[0]).code -ceq 'SUPERVISOR_REGISTRATION_CONFLICT') 'doctor reports mixed registration scopes as a conflict'
    $Global:CatDeskAutostartRunValue = $null
    $Global:CatDeskAutostartFixtureTask = New-OwnedTask $normalDefinition
    foreach ($action in @('enable','disable')) {
        $narrow = Invoke-FixtureFacade -Command 'autostart' -AutostartAction $action -Seams $autostart -FixtureBackendOnly | ConvertFrom-Json
        Require ($narrow.state -ceq 'AUTOSTART_SCOPE_CONFLICT' -and $actions.Count -eq 0) 'backend-only mutation cannot replace or remove an existing full-stack owner'
    }
    $fullStatus = Invoke-FixtureFacade -Command 'autostart' -AutostartAction 'status' -Seams $autostart -FixtureBackendOnly | ConvertFrom-Json
    Require ($fullStatus.detail -ceq 'scope=FULL_STACK' -and $actions.Count -eq 0) 'status reports actual existing scope rather than the requested flag'
    $Global:CatDeskAutostartFixtureTask = $null
    $Global:CatDeskAutostartSupervisorRunning = $false
    $backendFallback = Invoke-FixtureFacade -Command 'autostart' -AutostartAction 'enable' -Seams $fallback -FixtureBackendOnly | ConvertFrom-Json
    Require ($backendFallback.state -ceq 'AUTOSTART_ENABLED' -and $Global:CatDeskAutostartRunValue.EndsWith(' -BackendOnly')) 'existing Run fallback preserves backend-only scope'
    $actions.Clear()
    $backendFallbackStatus = Invoke-FixtureFacade -Command 'autostart' -AutostartAction 'status' -Seams $fallback | ConvertFrom-Json
    Require ($backendFallbackStatus.detail -ceq 'scope=BACKEND_ONLY' -and $actions.Count -eq 0) 'ordinary status recognizes backend-only Run fallback'
    $backendFallbackDisable = Invoke-FixtureFacade -Command 'autostart' -AutostartAction 'disable' -Seams $fallback | ConvertFrom-Json
    Require ($backendFallbackDisable.state -ceq 'AUTOSTART_UNREGISTERED_MONITOR_ACTIVE' -and ($actions -join ',') -ceq 'runkey-unregister') 'default disable removes only the exact existing known backend-only Run value and reports the surviving monitor'
    $actions.Clear()
    foreach ($localOnly in @($false,$true)) {
        foreach ($readOrDisable in @('status','disable')) {
            $survivingStatus = Invoke-FixtureFacade -Command 'autostart' -AutostartAction $readOrDisable -Seams $autostart -FixtureBackendOnly:$localOnly | ConvertFrom-Json
            Require ($survivingStatus.state -ceq 'AUTOSTART_UNREGISTERED_MONITOR_ACTIVE' -and $actions.Count -eq 0) 'an unregistered active monitor is not reported disabled and no process or registration is changed'
        }
        $survivingMonitor = Invoke-FixtureFacade -Command 'autostart' -AutostartAction 'enable' -Seams $autostart -FixtureBackendOnly:$localOnly | ConvertFrom-Json
        Require ($survivingMonitor.state -ceq 'AUTOSTART_UNREGISTERED_MONITOR_ACTIVE' -and $actions.Count -eq 0) 'an unregistered surviving monitor is never relabeled or reused as a new scope'
        Require ($null -eq $Global:CatDeskAutostartFixtureTask -and $null -eq $Global:CatDeskAutostartRunValue) 'surviving monitor refusal creates no persistence'
    }
    # Exercise the actual Windows named guard without starting any supervisor
    # or changing persistence. The monitor seam is removed, not faked true.
    $realGuardSeams = $autostart.Clone()
    $realGuardSeams.Remove('AutostartSupervisorRunning')
    $guard = $null
    try {
        $guard = [Threading.Mutex]::new($false, "Local\CatDesk.Autostart.$($normalDefinition.SupervisorWorkspaceId)")
        foreach ($action in @('status','disable','enable')) {
            $guarded = Invoke-FixtureFacade -Command 'autostart' -AutostartAction $action -Seams $realGuardSeams | ConvertFrom-Json
            Require ($guarded.state -ceq 'AUTOSTART_UNREGISTERED_MONITOR_ACTIVE' -and $actions.Count -eq 0) 'real surviving kernel guard is reported without starting, removing or killing an owner'
        }
        $guardedDoctor = Invoke-FixtureFacade -Command 'doctor' -Seams $realGuardSeams | ConvertFrom-Json
        $guardedLayer = @($guardedDoctor.layers | Where-Object layer -CEQ 'PERSISTENT_SUPERVISOR')[0]
        Require ($guardedLayer.code -ceq 'SUPERVISOR_UNREGISTERED_MONITOR_ACTIVE' -and $guardedLayer.status -ceq 'FAIL' -and $guardedLayer.nextDiagnostic -ceq 'RECONCILE_EXISTING_MONITOR_OWNER_BEFORE_MIGRATION_OR_SCOPE_CHANGE') 'doctor observes the same real unregistered guard and requests owner reconciliation rather than restart or recovery readiness'
        Require ($actions.Count -eq 0) 'real-guard doctor is non-mutating'
    } finally {
        if ($null -ne $guard) { $guard.Dispose() }
    }
    $afterGuard = Invoke-FixtureFacade -Command 'autostart' -AutostartAction 'status' -Seams $realGuardSeams | ConvertFrom-Json
    Require ($afterGuard.state -ceq 'AUTOSTART_DISABLED' -and $actions.Count -eq 0) 'disabled is reported only after the real unregistered guard is absent'
    foreach ($command in @('install','status','diagnose','doctor','stop','wake')) {
        $invalidScope = Invoke-FixtureFacade -Command $command -Seams $autostart -FixtureBackendOnly | ConvertFrom-Json
        Require ($invalidScope.state -ceq 'ACTION_REQUIRED' -and $actions.Count -eq 0) 'backend-only switch remains refused outside its explicit lifecycle commands'
    }
    $missingAction = Invoke-FixtureFacade -Command 'autostart' -Seams $autostart | ConvertFrom-Json
    Require ($missingAction.state -eq 'ACTION_REQUIRED') 'autostart requires an explicit action'
    Require ($actions.Count -eq 0) 'missing autostart action is non-mutating'
} finally {
    $actions.Clear()
    if ($realCanonicalRoot -and (Test-Path -LiteralPath $realCanonicalRoot)) {
        Remove-Item -LiteralPath $realCanonicalRoot -Recurse -Force -ErrorAction SilentlyContinue
    }
    Remove-Variable -Name CatDeskAutostartFixtureTask -Scope Global -ErrorAction SilentlyContinue
    Remove-Variable -Name CatDeskAutostartRunValue -Scope Global -ErrorAction SilentlyContinue
    Remove-Variable -Name CatDeskAutostartSupervisorRunning -Scope Global -ErrorAction SilentlyContinue
    Remove-Variable -Name CatDeskAutostartDefinitions -Scope Global -ErrorAction SilentlyContinue
}

Write-Output 'consumer lifecycle fixture tests passed'
