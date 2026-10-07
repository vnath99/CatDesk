[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Import-Module Microsoft.PowerShell.Utility -ErrorAction Stop
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
    [int]$FixtureReadyTimeoutSeconds = 180
) {
    $prior = Get-Location
    try {
        Set-Location ([IO.Path]::GetTempPath())
        return & $facade -Command $Command -AutostartAction $AutostartAction -WakeTerminalTaskId $WakeTerminalTaskId -Workspace $FixtureWorkspace -ConfigPath $FixtureConfigPath -ExpectedBuildSha256 $FixtureExpectedBuildSha256 -BuildFingerprintPath $FixtureFingerprintPath -ReadyTimeoutSeconds $FixtureReadyTimeoutSeconds -TestSeams $Seams
    } finally {
        Set-Location $prior
    }
}

function New-OwnedTask($Definition) {
    [pscustomobject]@{
        Actions = @([pscustomobject]@{ Execute = $Definition.Execute; Arguments = $Definition.Arguments })
        Principal = [pscustomobject]@{ UserId = $Definition.UserId; LogonType = 'InteractiveToken'; RunLevel = 'Limited' }
        Triggers = @([pscustomobject]@{ CimClass = [pscustomobject]@{ CimClassName = 'MSFT_TaskLogonTrigger' } })
        Settings = [pscustomobject]@{ StartWhenAvailable = $true; AllowStartIfOnBatteries = $true; DontStopIfGoingOnBatteries = $true; MultipleInstances = 'IgnoreNew'; ExecutionTimeLimit = [TimeSpan]::Zero }
    }
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
    Require ($source -match "ValidateSet\('install', 'start', 'status', 'diagnose', 'recover', 'stop', 'autostart', 'wake'\)") 'exact public command vocabulary is present'
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

    $status = Invoke-FixtureFacade -Command 'status' -Seams $base | ConvertFrom-Json
    Require ($status.state -eq 'LOCAL_DAEMON_PENDING') 'status reports a bounded non-mutating pending state'
    Require ($actions.Count -eq 0) 'status did not invoke any lifecycle side effect'
    Require (($status | ConvertTo-Json -Compress) -notmatch '(?i)secret|token|route_|https?://') 'status output is redacted'

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
    Require ($actions.Count -eq 0) 'recoverable diagnosis remains non-mutating'

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
    Require ($disabledAgain.state -eq 'AUTOSTART_DISABLED' -and $actions -contains 'task-unregister') 'disable removes only exact owned task'
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
    $fallbackDisable = Invoke-FixtureFacade -Command 'autostart' -AutostartAction 'disable' -Seams $fallback | ConvertFrom-Json
    Require ($fallbackDisable.state -eq 'AUTOSTART_DISABLED' -and ([string]::Join(',', $actions)) -eq 'runkey-unregister') 'disable removes only the exact owned run-key fallback'
    $actions.Clear()
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
