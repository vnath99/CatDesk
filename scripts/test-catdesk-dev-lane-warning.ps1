[CmdletBinding()]
param()
$ErrorActionPreference='Stop'; Set-StrictMode -Version Latest
$sourceRoot=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$facade=Join-Path $PSScriptRoot 'catdesk-dev-lane.ps1'
$manifest=Microsoft.PowerShell.Management\Get-Item -LiteralPath (Join-Path $sourceRoot 'Cargo.toml')
$binary=Join-Path $sourceRoot 'target\debug\catdesk.exe'
$warningFixtureState=$null
$cases=[Collections.Generic.List[string]]::new()
function Check([bool]$Ok,[string]$Name){if(!$Ok){throw $Name};$cases.Add($Name)}

# Scoped mocks: execute the actual facade, including its original control flow.
# No native Git/Cargo, real volume query, build output or temporary tree.
function git {
    $global:LASTEXITCODE=0
    switch ($args -join ' ') {
        'rev-parse --show-toplevel' { return $sourceRoot }
        'rev-parse --verify HEAD' { return ('b'*40) }
        'status --porcelain' { return }
        default { throw 'UNEXPECTED_GIT_INVOCATION' }
    }
}
function cargo {
    $warningFixtureState.Cargo.Add(($args -join ' '))
    $warningFixtureState.Events.Add('cargo')
    $warningFixtureState.Incremental.Add([string]$env:CARGO_INCREMENTAL)
    $global:LASTEXITCODE=if($warningFixtureState.Cargo.Count -eq $warningFixtureState.FailAt){77}else{0}
}
function Get-CimInstance {
    param($ClassName,$Filter,$OperationTimeoutSec,$ErrorAction)
    $warningFixtureState.Queries++
    $warningFixtureState.Events.Add('query')
    if($ClassName -cne 'Win32_Volume' -or $Filter -cne "DriveLetter='C:'" -or $OperationTimeoutSec -ne 3 -or $ErrorAction -cne 'Stop'){
        $warningFixtureState.UnexpectedQuery=$true;throw 'UNEXPECTED_COUNTER_QUERY'
    }
    if($warningFixtureState.CounterFailure){throw 'PRIVATE_PROVIDER_FAILURE_WITH_CREDENTIAL'}
    return [pscustomobject]@{FreeSpace=[uint64]$warningFixtureState.Free;Capacity=[uint64](128GB);DeviceID='\\?\Volume{00000000-0000-0000-0000-000000000001}\'}
}
function Get-Item {
    [CmdletBinding()]param([string]$LiteralPath,[switch]$Force)
    if($LiteralPath -ceq $binary){return $manifest}
    Microsoft.PowerShell.Management\Get-Item @PSBoundParameters
}
function Test-Path {
    [CmdletBinding()]param([string]$LiteralPath,[string]$PathType)
    if($LiteralPath -ceq $binary){return $false}
    Microsoft.PowerShell.Management\Test-Path @PSBoundParameters
}
function Get-FileHash {
    [CmdletBinding()]param([string]$LiteralPath,[string]$Algorithm)
    if($LiteralPath -ceq $binary){return [pscustomobject]@{Hash=('a'*64)}}
    Microsoft.PowerShell.Utility\Get-FileHash @PSBoundParameters
}

function Invoke-Fixture([string]$Operation,[long]$Free,[long]$Threshold,[bool]$CounterFailure=$false,[bool]$StderrFailure=$false,[int]$FailAt=0,[bool]$UseDefault=$false){
    $warningFixtureState=@{Cargo=[Collections.Generic.List[string]]::new();Events=[Collections.Generic.List[string]]::new();Incremental=[Collections.Generic.List[string]]::new();Queries=0;UnexpectedQuery=$false;Free=$Free;CounterFailure=$CounterFailure;FailAt=$FailAt}
    $beforeLocation=(Get-Location).Path;$beforeOs=[Environment]::CurrentDirectory
    $prior=Microsoft.PowerShell.Management\Get-Item Env:CARGO_INCREMENTAL -ErrorAction SilentlyContinue
    $hadPrior=$null -ne $prior;$priorValue=if($hadPrior){[string]$prior.Value}else{$null}
    $writer=[IO.StringWriter]::new();$priorStderr=[Console]::Error
    $output=@();$errorText=$null;$exitCode=$null;$restored=$false
    try{
        $env:CARGO_INCREMENTAL='caller-value'
        if($StderrFailure){$writer.Dispose()}
        [Console]::SetError($writer)
        $parameters=@{Command=$Operation}
        if(!$UseDefault){$parameters.StorageWarningFreeBytes=$Threshold}
        try{$output=@(& $facade @parameters)}catch{$errorText=$_.Exception.Message}
        $exitCode=$global:LASTEXITCODE
        $restored=$env:CARGO_INCREMENTAL -ceq 'caller-value'
    }finally{
        [Console]::SetError($priorStderr)
        if($hadPrior){$env:CARGO_INCREMENTAL=$priorValue}else{$env:CARGO_INCREMENTAL=$null}
    }
    $stderr=if($StderrFailure){''}else{$writer.ToString()};$writer.Dispose()
    [pscustomobject]@{Output=$output;ErrorText=$errorText;ExitCode=$exitCode;Stderr=$stderr;Cargo=@($warningFixtureState.Cargo);Events=@($warningFixtureState.Events);Incremental=@($warningFixtureState.Incremental);Queries=$warningFixtureState.Queries;UnexpectedQuery=$warningFixtureState.UnexpectedQuery;IncrementalRestored=$restored;LocationRestored=((Get-Location).Path -ceq $beforeLocation -and [Environment]::CurrentDirectory -ceq $beforeOs)}
}
$keys=@('schemaVersion','mode','operation','sourceCommit','workingTreeClean','debugImageSha256','productionInstallationChanged','daemonOrBrowserStarted','isolatedRuntimeReady')|Sort-Object
foreach($operation in @('build','verify')){
    $expected=if($operation -ceq 'build'){@('build --locked --offline --bin catdesk')}else{@('fmt --all -- --check','clippy --locked --offline --workspace --all-targets --all-features -- -D warnings','test --locked --offline --bin catdesk')}
    foreach($variant in @('default','low','healthy','boundary','custom','zero','counter_failure','stderr_failure')){
        $free=if($variant -ceq 'healthy'){65GB}elseif($variant -ceq 'boundary'){64GB}elseif($variant -ceq 'custom'){3GB}else{1GB}
        $threshold=if($variant -ceq 'zero'){0}elseif($variant -ceq 'custom'){2GB}else{64GB}
        $r=Invoke-Fixture $operation $free $threshold ($variant -ceq 'counter_failure') ($variant -ceq 'stderr_failure') 0 ($variant -ceq 'default')
        Check ($null -eq $r.ErrorText -and ($r.Cargo -join '|') -ceq ($expected -join '|') -and !$r.UnexpectedQuery) ($operation+'_'+$variant+'_original_commands')
        Check ($r.Output.Count -eq 1 -and !(Compare-Object $keys @((($r.Output[0]|ConvertFrom-Json).PSObject.Properties.Name)|Sort-Object))) ($operation+'_'+$variant+'_exact_nine_stdout_keys')
        Check ($r.Queries -eq $(if($variant -ceq 'zero'){0}else{1})) ($operation+'_'+$variant+'_query_count')
        Check ($r.IncrementalRestored -and $r.LocationRestored -and $r.ExitCode -eq 0) ($operation+'_'+$variant+'_ambient_behavior')
        if($variant -cin @('healthy','boundary','custom','zero','stderr_failure')){
            Check ([string]::IsNullOrEmpty($r.Stderr)) ($operation+'_'+$variant+'_no_notice')
        }else{
            $notice=$r.Stderr.Trim()|ConvertFrom-Json
            $reason=if($variant -ceq 'counter_failure'){'REFERENCE_VOLUME_COUNTER_UNAVAILABLE'}else{'REFERENCE_VOLUME_LOW_FREE_SPACE'}
            Check ($notice.reason -ceq $reason -and $notice.availableFreeBytes -eq $(if($variant -ceq 'counter_failure'){$null}else{$free}) -and $notice.warningFreeBytes -eq $threshold -and $notice.referenceOnly -and !$notice.admissionGate -and !$notice.effectiveCargoTargetVerified) ($operation+'_'+$variant+'_nonfatal_notice')
            Check ($r.Stderr -notmatch 'PRIVATE_PROVIDER|Credential|Volume\{|referenceTargetPath|[A-Za-z]:\\') ($operation+'_'+$variant+'_private_data_excluded')
        }
        Check (($r.Incremental -join '|') -ceq $(if($operation -ceq 'build'){'caller-value'}else{'0|0|0'})) ($operation+'_'+$variant+'_incremental_semantics_unchanged')
        if($variant -cne 'zero'){Check ($r.Events[0] -ceq 'query') ($operation+'_'+$variant+'_advisory_before_cargo')}
    }
    foreach($variant in @('low','counter_failure','stderr_failure')){
        $r=Invoke-Fixture $operation 1GB 64GB ($variant -ceq 'counter_failure') ($variant -ceq 'stderr_failure') 1
        Check ($r.ErrorText -ceq 'DEVELOPMENT_COMMAND_FAILED' -and $r.ExitCode -eq 77 -and $r.Output.Count -eq 0 -and $r.Cargo.Count -eq 1) ($operation+'_'+$variant+'_cargo_failure_preserved')
        Check ($r.IncrementalRestored -and $r.LocationRestored) ($operation+'_'+$variant+'_failure_restoration')
    }
}
$r=Invoke-Fixture status 1GB 64GB
Check ($r.Queries -eq 0 -and $r.Cargo.Count -eq 0 -and !$r.Stderr -and $r.Output.Count -eq 1) 'status_remains_readonly_without_automatic_query'
$r=Invoke-Fixture storage-status 1GB 0
Check ($r.Queries -eq 1 -and $r.Cargo.Count -eq 0 -and !$r.Stderr -and ($r.Output[0]|ConvertFrom-Json).mode -ceq 'DEVELOPMENT_STORAGE_ADVISORY') 'explicit_storage_status_keeps_original_observation_semantics'
[pscustomobject][ordered]@{schemaVersion=1;hostVersion=$PSVersionTable.PSVersion.ToString();hostEdition=$PSVersionTable.PSEdition;passed=$cases.Count;realCargoRuns=0;realVolumeQueries=0;nativeFixtures=0;cases=@($cases)}|ConvertTo-Json -Depth 4 -Compress
