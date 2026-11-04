[CmdletBinding()]
param()
$ErrorActionPreference='Stop';Set-StrictMode -Version Latest
$helper=Join-Path $PSScriptRoot 'catdesk-dev-lane-local-target.ps1'
$tokens=$null;$errors=$null
$null=[Management.Automation.Language.Parser]::ParseFile($helper,[ref]$tokens,[ref]$errors)
if($errors.Count){throw 'HELPER_PARSE_FAILED'}
if('CatDesk.DevLaneCacheV2.HeldFile' -as [type]){throw 'TEST_REQUIRES_FRESH_HOST'}
. $helper
$dedicatedBaseEntries=(Get-Command Assert-DevLaneLocalDedicatedBaseEntries).ScriptBlock

# Substitute only side effects: no compilation, directories, files or native calls.
# Production initialization control flow and diagnostic constructors run unchanged.
function Initialize-DevLaneLocalFilesystem {}
function Assert-DevLaneLocalDedicatedBaseEntries([string]$Directory) {
    $script:diagnosticFixture.Events.Add('dedicated-base-checked')
}
function New-DevLaneLocalDirectoryEntry([string]$Path) {
    if(!$script:diagnosticFixture.Directories.Add($Path)){throw 'LOCAL_TARGET_DIRECTORY_NOT_CREATED'}
    $script:diagnosticFixture.Events.Add('create:'+ $Path)
}
function Write-DevLaneLocalNewJson([string]$Path,[string]$Owner,[object]$Value) {
    if($script:diagnosticFixture.Records.ContainsKey($Path)){throw 'MOCK_RECORD_EXISTS'}
    $script:diagnosticFixture.Records[$Path]=$Value
    return ('a'*48)
}
function New-DiagnosticFixture {
    $fixture=@{Directories=[Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase);
        Events=[Collections.Generic.List[string]]::new();Records=@{};FailReopen='';FailLookup='';LastOptional=$null;
        ReturnedOwner='S-1-5-21-111-222-333-1001';ReturnedVolume='0000000000000001'}
    $scope=[pscustomobject]@{Fixture=$fixture}
    $scope|Add-Member ScriptMethod Directory {
        param([string]$Path,[bool]$Optional)
        $this.Fixture.Events.Add('lookup:'+ $Path)
        $this.Fixture.LastOptional=$Optional
        if(($Optional -and $Path -ceq $this.Fixture.FailLookup) -or
           (!$Optional -and $Path -ceq $this.Fixture.FailReopen)) {
            throw [InvalidOperationException]::new('PATH_OPEN_UNVERIFIED')
        }
        if($this.Fixture.Directories.Contains($Path)){
            return [pscustomobject]@{Owner=$this.Fixture.ReturnedOwner;Volume=$this.Fixture.ReturnedVolume;Identity=('b'*48)}
        }
        if($Optional){return $null}
        throw 'MOCK_REQUIRED_DIRECTORY_MISSING'
    }
    return @{Fixture=$fixture;Facts=[pscustomobject]@{scope=$scope;local='C:\synthetic-local';
        owner='S-1-5-21-111-222-333-1001';volume='0000000000000001';localIdentity=('c'*48)}}
}
$checks=[Collections.Generic.List[string]]::new()
function Check([bool]$Condition,[string]$Name){if(!$Condition){throw ('CHECK_FAILED:'+ $Name)};$checks.Add($Name)}
function Run-DiagnosticOpen([object]$Facts){
    try{$null=Open-DevLaneLocalSession $Facts ('d'*64);throw 'EXPECTED_HOLD'}
    catch{if($_.Exception.Message -cne 'LOCAL_TARGET_UNKNOWN_HOLD'){throw};return $_.Exception}
}
$parent='C:\synthetic-local\CatDeskDev';$base=Join-Path $parent 'ordinary-targets-v2'

$case=New-DiagnosticFixture;$script:diagnosticFixture=$case.Fixture
$failure=Run-DiagnosticOpen $case.Facts;$detail=Get-DevLaneLocalInitializationDiagnostic $failure
Check ($case.Fixture.Directories.Contains($parent) -and $case.Fixture.Directories.Contains($base)) 'absent_parent_and_namespace_creation_flow'
Check ($case.Fixture.Records.ContainsKey((Join-Path $base 'namespace.json'))) 'namespace_record_follows_verified_create'
Check ($detail.stage -ceq 'NAMESPACE_CONTROL_OPEN' -and $detail.cause -ceq 'UNCLASSIFIED') 'unloaded_native_type_stops_before_control_file_open'
Check ($case.Fixture.Records.Count -eq 1) 'no_workspace_or_operation_records_after_hold'

$case=New-DiagnosticFixture;$script:diagnosticFixture=$case.Fixture;$case.Fixture.FailReopen=$parent
$failure=Run-DiagnosticOpen $case.Facts;$detail=Get-DevLaneLocalInitializationDiagnostic $failure
Check ($detail.stage -ceq 'DIRECTORY_REOPEN' -and $detail.context -ceq 'PARENT_CREATE' -and $detail.cause -ceq 'PATH_OPEN_UNVERIFIED') 'postcreate_metadata_failure_keeps_specific_stage_and_category'
Check ($case.Fixture.Directories.Contains($parent) -and !$case.Fixture.Directories.Contains($base) -and !$case.Fixture.Records.Count) 'partial_parent_preserved_without_next_write'
Check ($failure.Data['DevLaneLocalPrivateErrorRecord'] -is [Management.Automation.ErrorRecord]) 'original_error_record_retained_in_memory_for_private_controller'

$case=New-DiagnosticFixture;$script:diagnosticFixture=$case.Fixture;$case.Fixture.FailLookup=$base
$failure=Run-DiagnosticOpen $case.Facts;$detail=Get-DevLaneLocalInitializationDiagnostic $failure
Check ($detail.stage -ceq 'NAMESPACE_LOOKUP' -and $detail.cause -ceq 'PATH_OPEN_UNVERIFIED') 'next_child_open_failure_identified'
Check ($case.Fixture.Directories.Contains($parent) -and !$case.Fixture.Directories.Contains($base) -and !$case.Fixture.Records.Count) 'child_open_failure_preserves_only_parent'

$case=New-DiagnosticFixture;$script:diagnosticFixture=$case.Fixture
$case.Facts|Add-Member NoteProperty explicitBase $true
$null=$case.Fixture.Directories.Add($parent)
$failure=Run-DiagnosticOpen $case.Facts;$detail=Get-DevLaneLocalInitializationDiagnostic $failure
Check ($detail.stage -ceq 'NAMESPACE_LOOKUP' -and $detail.cause -ceq 'LOCAL_TARGET_EXPLICIT_PARTIAL_BASE_HOLD') 'explicit_base_never_adopts_preexisting_partial_parent'
Check ($case.Fixture.Directories.Count -eq 1 -and !$case.Fixture.Records.Count) 'explicit_partial_parent_stays_unchanged'
foreach($row in @(@{entries=@();ok=$true},@{entries=@('C:\synthetic\CatDeskDev');ok=$true},
                  @{entries=@('C:\synthetic\foreign');ok=$false},@{entries=@('C:\synthetic\CatDeskDev','C:\synthetic\foreign');ok=$false})){
    $denied=$false;try{& $dedicatedBaseEntries 'C:\synthetic' {$row.entries}}catch{$denied=$true}
    Check ($denied -ne $row.ok) ('dedicated_base_content_policy_'+$checks.Count)
}

try{throw 'C:\private-user\private-cache-secret'}catch{$failure=New-DevLaneLocalInitializationHold 'PARENT_LOOKUP' $_}
$detail=Get-DevLaneLocalInitializationDiagnostic $failure
Check ($detail.cause -ceq 'UNCLASSIFIED' -and ($detail|ConvertTo-Json -Compress) -notmatch 'private-user|private-cache-secret') 'safe_export_does_not_publish_raw_exception'
Check ($failure.Message -ceq 'LOCAL_TARGET_UNKNOWN_HOLD' -and $failure.GetBaseException().Message -ceq 'LOCAL_TARGET_UNKNOWN_HOLD') 'generic_hold_contract_unchanged'
Check (!$detail.cleanupAuthorized -and !$detail.retryAuthorized) 'diagnostic_grants_no_cleanup_or_retry'
Check ($failure.Data['DevLaneLocalPrivateErrorRecord'].Exception.Message -cmatch 'private-cache-secret') 'private_original_preserved_without_public_export'

$tampered=[InvalidOperationException]::new('LOCAL_TARGET_UNKNOWN_HOLD')
$tampered.Data['DevLaneLocalDiagnostic']=@{schemaVersion=1;stage='C:\private-stage';context='PARENT_LOOKUP';cause='UNCLASSIFIED'}
Check ($null -eq (Get-DevLaneLocalInitializationDiagnostic $tampered)) 'unsafe_diagnostic_vocabulary_refused'
$tampered.Data['DevLaneLocalDiagnostic']=@{schemaVersion=1;stage='PARENT_LOOKUP';context='PARENT_LOOKUP';cause='UNCLASSIFIED'}
Check ($null -eq (Get-DevLaneLocalInitializationDiagnostic $tampered)) 'incomplete_diagnostic_shape_refused'

$profile='C:\synthetic-user';$base=Join-Path $profile 'CatDeskBuildCache-diagnostic';$source='C:\synthetic-project'
foreach($path in @($base,(Join-Path $profile 'CatDeskBuildCache'))){
    $rejected=$false;try{Assert-DevLaneLocalExplicitBasePath $path $profile $source}catch{$rejected=$true}
    Check (!$rejected) ('dedicated_direct_child_path_allowed_'+$checks.Count)
}
foreach($path in @('C:\', $profile, 'C:\other-user\CatDeskBuildCache',
                   'C:\synthetic-user\AppData\Local\CatDeskBuildCache',
                   'C:\synthetic-user\OneDrive\CatDeskBuildCache',
                   'C:\synthetic-user\Projects\CatDeskBuildCache',
                   'C:\synthetic-user\CatDeskBuildCache\nested',
                   'C:\synthetic-user\CatDeskBuildCache\',
                   'C:\synthetic-user\other\..\CatDeskBuildCache',
                   'C:/synthetic-user/CatDeskBuildCache')){
    $rejected=$false;try{Assert-DevLaneLocalExplicitBasePath $path $profile $source}catch{$rejected=$true}
    Check $rejected ('non_dedicated_or_noncanonical_base_refused_'+$checks.Count)
}
$rejected=$false;try{Assert-DevLaneLocalExplicitBasePath $base $profile (Join-Path $base 'project')}catch{$rejected=$true}
Check $rejected 'explicit_base_cannot_contain_source'
$case=New-DiagnosticFixture;$script:diagnosticFixture=$case.Fixture;$null=$case.Fixture.Directories.Add($base)
$selected=Get-DevLaneLocalExplicitBase $base $profile $source $case.Facts.scope $case.Facts.owner ([pscustomobject]@{Volume=$case.Facts.volume})
Check ($selected.Path -ceq $base -and $selected.Observation.Identity -ceq ('b'*48) -and !$case.Fixture.LastOptional) 'explicit_base_requires_existing_metadata_identity'
Check ($case.Fixture.Directories.Count -eq 1 -and !$case.Fixture.Records.Count) 'explicit_base_selection_has_no_creation_or_adoption'
foreach($variant in @('missing','owner','volume','alias')){
    $case=New-DiagnosticFixture;$script:diagnosticFixture=$case.Fixture
    if($variant -cne 'missing'){$null=$case.Fixture.Directories.Add($base)}
    if($variant -ceq 'owner'){$case.Fixture.ReturnedOwner='S-1-5-21-111-222-333-1002'}
    if($variant -ceq 'volume'){$case.Fixture.ReturnedVolume='0000000000000002'}
    if($variant -ceq 'alias'){$case.Facts.scope|Add-Member ScriptMethod Directory {param($p,$optional) throw 'PATH_ALIAS_UNVERIFIED'} -Force}
    $rejected=$false;try{$null=Get-DevLaneLocalExplicitBase $base $profile $source $case.Facts.scope $case.Facts.owner ([pscustomobject]@{Volume=$case.Facts.volume})}catch{$rejected=$true}
    Check $rejected ('explicit_base_metadata_refused_'+$variant)
    Check (!$case.Fixture.Records.Count) ('explicit_base_refusal_has_no_control_writes_'+$variant)
}
$rejected=$false
try{& (Join-Path $PSScriptRoot 'catdesk-dev-lane.ps1') -Command status -LocalTargetBase $base}catch{$rejected=$_.Exception.Message -ceq 'LOCAL_TARGET_BASE_REQUIRES_OPT_IN'}
Check $rejected 'explicit_base_requires_opted_in_facade_branch'
$script:observedBase=$null
$backend=@{Facts={param($root,$selectedBase) $script:observedBase=$selectedBase;throw 'STOP_AFTER_BASE_CAPTURE'}}
try{Invoke-DevLaneLocalEngine 'build' $source 0 $backend $base}catch{if($_.Exception.Message -cne 'STOP_AFTER_BASE_CAPTURE'){throw}}
Check ($script:observedBase -ceq $base) 'explicit_base_reaches_metadata_backend_without_global_configuration'
Check (!('CatDesk.DevLaneCacheV2.HeldFile' -as [type])) 'native_type_remains_unloaded'

[pscustomobject][ordered]@{schemaVersion=1;mode='MockInitializationDiagnostics';passed=$checks.Count;
    managedCompilations=0;nativeEntries=0;filesystemWrites=0;realCargoRuns=0;checks=@($checks)}|ConvertTo-Json -Depth 4 -Compress
