[CmdletBinding()]
param(
    [switch]$CompileDefinitions,
    [ValidateSet('Mock','FilesystemFixture')][string]$Mode='Mock',
    [switch]$PrepareDefinitionsOnly
)
$ErrorActionPreference='Stop';Set-StrictMode -Version Latest
$root=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$helper=Join-Path $PSScriptRoot 'catdesk-dev-lane-local-target.ps1'
$tokens=$null;$errors=$null
$null=[Management.Automation.Language.Parser]::ParseFile($helper,[ref]$tokens,[ref]$errors)
if($errors.Count){throw 'HELPER_PARSE_FAILED'}
if($Mode -ceq 'Mock' -and ('CatDesk.DevLaneCacheV2.HeldFile' -as [type])){throw 'TEST_REQUIRES_FRESH_HOST'}
. $helper

function Initialize-DevLaneFilesystemFixtureBridge {
    $definition=@'
using System;
using System.IO;
using System.Runtime.InteropServices;
namespace CatDesk.DevLaneCacheFixtureV1 {
 public sealed class RootState {
  public string Phase { get; private set; }
  public string RelativeRootName { get; private set; }
  public string ParentPhysicalIdentity { get; private set; }
  public string CreatedRootPhysicalIdentity { get; private set; }
  internal RootState(string name,string parent,string created) {
   RelativeRootName=name;ParentPhysicalIdentity=parent;CreatedRootPhysicalIdentity=created;
   Phase=name==null?"NO_ROOT_INTENT":created==null?"CREATION_OUTCOME_UNKNOWN":"CREATED_IDENTITY_VERIFIED";
  }
 }
 public static class Bridge {
  public const string TemplateSha256="__TEMPLATE__";
  public static int NativeEntries;
  private static int fixtureAttempts;
  private static RootState rootState;
  public static int FixtureAttempts { get { return System.Threading.Volatile.Read(ref fixtureAttempts); } }
  public static void ClaimFixtureAttempt(){if(System.Threading.Interlocked.CompareExchange(ref fixtureAttempts,1,0)!=0)throw new IOException("FS_FIXTURE_ALREADY_ATTEMPTED_NO_RETRY");}
  public static RootState GetRootState(){return System.Threading.Volatile.Read(ref rootState)??new RootState(null,null,null);}
  public static void RecordRootIntent(string name,string parentIdentity) {
   if(FixtureAttempts!=1||name==null||parentIdentity==null||!System.Text.RegularExpressions.Regex.IsMatch(name,"\\Acatdesk-dev-lane-fs-v1-[0-9a-f]{32}\\z")||!System.Text.RegularExpressions.Regex.IsMatch(parentIdentity,"\\A[0-9a-f]{48}\\z"))throw new IOException("FS_FIXTURE_ROOT_INTENT_INVALID");
   var intent=new RootState(name,parentIdentity,null);
   if(System.Threading.Interlocked.CompareExchange(ref rootState,intent,null)!=null)throw new IOException("FS_FIXTURE_ROOT_INTENT_ALREADY_RECORDED");
  }
  public static void RecordCreatedRoot(string identity) {
   RootState prior=System.Threading.Volatile.Read(ref rootState);
   if(prior==null||prior.CreatedRootPhysicalIdentity!=null||identity==null||!System.Text.RegularExpressions.Regex.IsMatch(identity,"\\A[0-9a-f]{48}\\z"))throw new IOException("FS_FIXTURE_CREATED_ROOT_INVALID");
   var created=new RootState(prior.RelativeRootName,prior.ParentPhysicalIdentity,identity);
   if(!Object.ReferenceEquals(System.Threading.Interlocked.CompareExchange(ref rootState,created,prior),prior))throw new IOException("FS_FIXTURE_ROOT_STATE_CHANGED");
  }
  [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern bool CreateHardLink(string link,string existing,IntPtr reserved);
  public static void CreateOwnedHardLink(string root) {
   string full=Path.GetFullPath(root).TrimEnd('\\');
   if(!System.Text.RegularExpressions.Regex.IsMatch(Path.GetFileName(full),"\\Acatdesk-dev-lane-fs-v1-[0-9a-f]{32}\\z"))throw new IOException("FS_FIXTURE_LINK_SCOPE_REFUSED");
   string newPath=Path.Combine(full,"hardlinks","alias.json"),oldPath=Path.Combine(full,"hardlinks","original.json");
   NativeEntries++;
   if(!CreateHardLink(newPath,oldPath,IntPtr.Zero))throw new IOException("FS_FIXTURE_HARDLINK_NOT_CREATED");
  }
 }
}
'@
    $hash=[Security.Cryptography.SHA256]::Create()
    try{$digest=([BitConverter]::ToString($hash.ComputeHash([Text.Encoding]::UTF8.GetBytes($definition)))).Replace('-','').ToLowerInvariant()}finally{$hash.Dispose()}
    if('CatDesk.DevLaneCacheFixtureV1.Bridge' -as [type]){
        if([CatDesk.DevLaneCacheFixtureV1.Bridge]::TemplateSha256 -cne $digest){throw 'FS_FIXTURE_BRIDGE_VERSION_MISMATCH'}
        return
    }
    Add-Type -TypeDefinition $definition.Replace('__TEMPLATE__',$digest)
}
function Initialize-DevLaneFilesystemFixtureDefinitions {
    . (Join-Path $PSScriptRoot 'catdesk-dev-local-target-native.ps1')
    Initialize-DevLocalTargetMetadata
    Initialize-DevLaneLocalFilesystem
    Initialize-DevLaneFilesystemFixtureBridge
    if([CatDesk.DevLaneCacheV2.HeldFile]::NativeEntries -ne 0 -or [CatDesk.DevLaneCacheFixtureV1.Bridge]::NativeEntries -ne 0 -or [CatDesk.DevLaneCacheFixtureV1.Bridge]::FixtureAttempts -ne 0){throw 'FS_FIXTURE_PREPARATION_NOT_PURE'}
    if([CatDesk.DevLaneCacheFixtureV1.Bridge]::GetRootState().Phase -cne 'NO_ROOT_INTENT'){throw 'FS_FIXTURE_PREPARATION_ROOT_STATE_NOT_EMPTY'}
}
function Assert-DevLaneFilesystemFixtureParentPath([string]$Path) {
    if($Path -cnotmatch '\A[A-Za-z]:\\' -or $Path -match '(?i)(^|[\\/])OneDrive(?:[^\\/]*)?([\\/]|$)'){throw 'FS_FIXTURE_PARENT_PATH_REFUSED'}
}
function Write-DevLaneFilesystemFixtureRootMarker {
    $state=[CatDesk.DevLaneCacheFixtureV1.Bridge]::GetRootState()
    $marker=[ordered]@{schemaVersion=1;mode='FILESYSTEM_FIXTURE_ROOT_STATE';phase=$state.Phase;relativeRootName=$state.RelativeRootName;parentPhysicalIdentity=$state.ParentPhysicalIdentity;createdRootPhysicalIdentity=$state.CreatedRootPhysicalIdentity;cleanupAuthorized=$false}
    [Console]::Error.WriteLine(($marker|ConvertTo-Json -Compress))
}
function Invoke-DevLaneFilesystemFixture {
    # This mode is not admission. An independently qualified controller must first
    # initialize definitions, then recheck exact hashes, resource/time and owners.
    if(!('CatDesk.DevLocalTarget.ReadOnlyScope' -as [type]) -or !('CatDesk.DevLaneCacheV2.HeldFile' -as [type]) -or !('CatDesk.DevLaneCacheFixtureV1.Bridge' -as [type])){throw 'FS_FIXTURE_PREINITIALIZATION_REQUIRED'}
    if([CatDesk.DevLaneCacheV2.HeldFile]::NativeEntries -ne 0 -or [CatDesk.DevLaneCacheFixtureV1.Bridge]::NativeEntries -ne 0){throw 'FS_FIXTURE_FRESH_HOST_REQUIRED'}
    # Consume even an attempt that fails during read-only parent verification.
    [CatDesk.DevLaneCacheFixtureV1.Bridge]::ClaimFixtureAttempt()
    $fsFixture=@{
        directories=0;files=0;writtenBytes=[long]0;cases=[Collections.Generic.List[string]]::new()
        sessions=[Collections.Generic.List[object]]::new();scopes=[Collections.Generic.List[object]]::new()
        initialized=@{};clock=[Diagnostics.Stopwatch]::StartNew();root=$null;anchor=$null
        originalDirectory=(Get-Command New-DevLaneLocalDirectory).ScriptBlock
        originalWrite=(Get-Command Write-DevLaneLocalNewJson).ScriptBlock
        originalGuard=(Get-Command New-DevLaneLocalGuard).ScriptBlock
        originalGuardWrite=(Get-Command Write-DevLaneLocalGuard).ScriptBlock
    }
    function Assert-FixtureBudget([int]$Directories,[int]$Files,[long]$Bytes){
        if($fsFixture.clock.ElapsedMilliseconds -gt 20000 -or $Directories -lt 0 -or $Files -lt 0 -or $Bytes -lt 0 -or $fsFixture.directories+$Directories -gt 128 -or $fsFixture.files+$Files -gt 256 -or $fsFixture.writtenBytes+$Bytes -gt 2MB){throw 'FS_FIXTURE_BUDGET_HOLD'}
        $fsFixture.directories+=$Directories;$fsFixture.files+=$Files;$fsFixture.writtenBytes+=$Bytes
    }
    function New-DevLaneLocalDirectory([string]$Path,[object]$Facts){
        Assert-FixtureBudget 1 0 0
        & $fsFixture.originalDirectory $Path $Facts
    }
    function Write-DevLaneLocalNewJson([string]$Path,[string]$Owner,[object]$Value){
        $bytes=[Text.Encoding]::UTF8.GetByteCount(($Value|ConvertTo-Json -Depth 6 -Compress))
        Assert-FixtureBudget 0 1 $bytes
        & $fsFixture.originalWrite $Path $Owner $Value
    }
    function New-DevLaneLocalGuard([object]$Session){
        Assert-FixtureBudget 0 1 0
        & $fsFixture.originalGuard $Session
    }
    function Write-DevLaneLocalGuard([object]$Session,[object]$Value){
        $bytes=[Text.Encoding]::UTF8.GetByteCount(($Value|ConvertTo-Json -Depth 6 -Compress))
        Assert-FixtureBudget 0 0 $bytes
        & $fsFixture.originalGuardWrite $Session $Value
    }
    function Check-FilesystemFixture([bool]$Ok,[string]$Reason){if(!$Ok){throw $Reason}}
    function New-FixtureFacts([string]$Name){
        if($Name -cnotmatch '\A[a-z]{1,16}\z'){throw 'FS_FIXTURE_FACT_SCOPE'}
        $scope=[CatDesk.DevLocalTarget.ReadOnlyScope]::new();$fsFixture.scopes.Add($scope)
        $workspace=Join-Path $fsFixture.root ('workspace-'+$Name)
        $facts=[pscustomobject]@{scope=$scope;owner=$fsFixture.owner;volume=$fsFixture.volume;local=$fsFixture.root;localIdentity=$fsFixture.rootIdentity;workspaceIdentity=$null;sourceCommit=('a'*40);cargoSha256=('b'*64);rustcSha256=('c'*64);lockSha256=('d'*64);cargoPath='NOT_EXECUTABLE_FIXTURE_ONLY';rustcPath='NOT_EXECUTABLE_FIXTURE_ONLY'}
        $id=$scope.Directory($workspace,$true)
        if($null -eq $id){$id=New-DevLaneLocalDirectory $workspace $facts}
        Check-FilesystemFixture ($id.Owner -ceq $facts.owner -and $id.Volume -ceq $facts.volume) 'FS_FIXTURE_WORKSPACE_IDENTITY'
        $facts.workspaceIdentity=$id.Identity
        return $facts
    }
    function Open-FixtureSession([object]$Facts){
        $key=Get-DevLaneLocalKey $Facts.owner $Facts.workspaceIdentity
        # Reserve one additional file conservatively for a newly initialized lease.
        if(!$fsFixture.initialized.ContainsKey($key)){Assert-FixtureBudget 0 1 0}
        $session=Open-DevLaneLocalSession $Facts $key
        $fsFixture.sessions.Add($session);$fsFixture.initialized[$key]=$true
        try{Assert-DevLaneLocalState $session.state $key $session.targetIdentity}
        catch{Close-FixtureSession $session;throw}
        return $session
    }
    function Close-FixtureSession([object]$Session){& (New-DevLaneLocalBackend).Close $Session}
    function Complete-FixtureOperation([object]$Session,[object]$Facts){
        $id=[Guid]::NewGuid().ToString('N')
        Start-DevLaneLocalOperation $Session $id $Facts 'build'
        # Synthetic observed return for state-machine testing; no Cargo was run.
        Complete-DevLaneLocalOperation $Session $id @([int]0) $false
        return $id
    }
    function Expect-FixtureOpenHold([string]$Name){
        $facts=New-FixtureFacts $Name;$refused=$false;$session=$null
        try{$session=Open-FixtureSession $facts}catch{
            if($_.Exception.GetBaseException().Message -cnotin @('LOCAL_TARGET_UNKNOWN_HOLD','LOCAL_TARGET_REGISTRATION_MISMATCH')){throw}
            $refused=$true
        }
        finally{if($null -ne $session){Close-FixtureSession $session};$facts.scope.Dispose()}
        Check-FilesystemFixture $refused 'FS_FIXTURE_EXPECTED_HOLD_NOT_OBSERVED'
    }
    function Write-FixtureFinalizing([object]$Session,[string]$Id){
        $null=Write-DevLaneLocalNewJson (Join-Path $Session.directory ('journal\'+$Id+'.outcome.json')) $Session.owner @{schemaVersion=2;operationId=$Id;workspaceKey=$Session.key;targetIdentity=$Session.targetIdentity;phase='RETURNED';exitCodes=@(0);wholeJobOrOutputEofVerified=$false}
        Set-DevLaneLocalPointer $Session @{schemaVersion=2;operationId=$Id;workspaceKey=$Session.key;targetIdentity=$Session.targetIdentity;phase='FINALIZING'}
    }
    $result=$null
    try{
        $anchor=[CatDesk.DevLocalTarget.ReadOnlyScope]::new();$fsFixture.anchor=$anchor
        $owner=[CatDesk.DevLocalTarget.ReadOnlyScope]::CurrentOwner();$fsFixture.owner=$owner
        $local=[CatDesk.DevLocalTarget.ReadOnlyScope]::KnownFolder($true)
        Assert-DevLaneFilesystemFixtureParentPath $local
        $localId=$anchor.Directory($local,$false)
        $temp=Join-Path $local 'Temp';$tempId=$anchor.Directory($temp,$false)
        Check-FilesystemFixture ($localId.Owner -ceq $owner -and $tempId.Owner -ceq $owner -and $localId.Volume -ceq $tempId.Volume) 'FS_FIXTURE_PARENT_IDENTITY_REFUSED'
        # Both complete existing ancestry chains are physically pinned BEFORE create.
        $rootName='catdesk-dev-lane-fs-v1-'+[Guid]::NewGuid().ToString('N')
        $fsFixture.root=Join-Path $temp $rootName;$fsFixture.volume=$tempId.Volume
        # Monotonic BCL state survives script catch/finally errors. Intent without
        # read-back identity always means UNKNOWN, even if creation was not called.
        [CatDesk.DevLaneCacheFixtureV1.Bridge]::RecordRootIntent($rootName,$tempId.Identity)
        Write-DevLaneFilesystemFixtureRootMarker
        $rootFacts=[pscustomobject]@{scope=$anchor;owner=$owner;volume=$tempId.Volume}
        $rootId=New-DevLaneLocalDirectory $fsFixture.root $rootFacts;$fsFixture.rootIdentity=$rootId.Identity
        [CatDesk.DevLaneCacheFixtureV1.Bridge]::RecordCreatedRoot($rootId.Identity)
        Write-DevLaneFilesystemFixtureRootMarker
        $null=Write-DevLaneLocalNewJson (Join-Path $fsFixture.root 'fixture.json') $owner @{schemaVersion=1;scope='SYNTHETIC_FILESYSTEM_ONLY';maximumDirectories=128;maximumFiles=256;maximumWrittenBytes=2MB;preserved=$true}

        $facts=New-FixtureFacts 'history';$session=Open-FixtureSession $facts
        $targetId=$session.targetIdentity;$leaseId=$session.lease.Identity
        $null=Complete-FixtureOperation $session $facts;Close-FixtureSession $session
        $facts=New-FixtureFacts 'history';$session=Open-FixtureSession $facts
        Check-FilesystemFixture ($session.targetIdentity -ceq $targetId -and $session.lease.Identity -ceq $leaseId -and $session.state.phase -ceq 'RETURNED') 'FS_FIXTURE_REOPEN_IDENTITY'
        $null=Complete-FixtureOperation $session $facts
        $journal=Get-DevLaneLocalJournal $session
        Check-FilesystemFixture ($journal.count -eq 4) 'FS_FIXTURE_IMMUTABLE_HISTORY'
        Close-FixtureSession $session;$fsFixture.cases.Add('creation_reopen_ids_pointer_replacement_and_immutable_history')

        $facts=New-FixtureFacts 'history';$held=Open-FixtureSession $facts
        Expect-FixtureOpenHold 'history'
        $otherFacts=New-FixtureFacts 'independent';$other=Open-FixtureSession $otherFacts
        $null=Complete-FixtureOperation $other $otherFacts;Close-FixtureSession $other
        Close-FixtureSession $held;$fsFixture.cases.Add('same_handle_lease_conflict_and_independent_workspace')

        $facts=New-FixtureFacts 'orphan';$session=Open-FixtureSession $facts
        $null=Complete-FixtureOperation $session $facts
        $id=[Guid]::NewGuid().ToString('N')
        $null=Write-DevLaneLocalNewJson (Join-Path $session.directory ('journal\'+$id+'.start.json')) $owner @{schemaVersion=2;operationId=$id;workspaceKey=$session.key;targetIdentity=$session.targetIdentity;operation='build'}
        Close-FixtureSession $session;Expect-FixtureOpenHold 'orphan';$fsFixture.cases.Add('old_success_pointer_with_orphan_start_holds')

        foreach($name in @('missing','mismatched')){
            $facts=New-FixtureFacts $name;$session=Open-FixtureSession $facts
            $id=Complete-FixtureOperation $session $facts
            $pointerId=if($name -ceq 'missing'){[Guid]::NewGuid().ToString('N')}else{$id}
            $phase=if($name -ceq 'missing'){'RETURNED'}else{'RETURNED_FAILED'}
            Set-DevLaneLocalPointer $session @{schemaVersion=2;operationId=$pointerId;workspaceKey=$session.key;targetIdentity=$session.targetIdentity;phase=$phase}
            Close-FixtureSession $session;Expect-FixtureOpenHold $name;$fsFixture.cases.Add($name+'_pointer_pair_holds')
        }
        $facts=New-FixtureFacts 'partial';$key=Get-DevLaneLocalKey $owner $facts.workspaceIdentity
        $partial=Join-Path $fsFixture.root ('CatDeskDev\ordinary-targets-v2\'+$key)
        $null=New-DevLaneLocalDirectory $partial $facts
        Expect-FixtureOpenHold 'partial';$fsFixture.cases.Add('unregistered_partial_initialization_holds')

        $facts=New-FixtureFacts 'guard';$session=Open-FixtureSession $facts;$id=[Guid]::NewGuid().ToString('N')
        Start-DevLaneLocalOperation $session $id $facts 'build'
        $start=Read-DevLaneLocalJson (Join-Path $session.directory ('journal\'+$id+'.start.json')) $owner
        Check-FilesystemFixture ($start.guardIdentity -ceq $session.guard.Identity) 'FS_FIXTURE_GUARD_IDENTITY'
        Complete-DevLaneLocalOperation $session $id @([int]0) $false
        Check-FilesystemFixture (!$session.scope.FilePresent((Join-Path $session.directory 'active.json'))) 'FS_FIXTURE_GUARD_NOT_FINALIZED'
        Close-FixtureSession $session;$fsFixture.cases.Add('retained_created_guard_identity_and_exact_handle_finalization')

        foreach($name in @('finalheld','finalabsent')){
            $facts=New-FixtureFacts $name;$session=Open-FixtureSession $facts;$id=[Guid]::NewGuid().ToString('N')
            Start-DevLaneLocalOperation $session $id $facts 'build';Write-FixtureFinalizing $session $id
            if($name -ceq 'finalabsent'){Complete-DevLaneLocalGuard $session}
            Close-FixtureSession $session;Expect-FixtureOpenHold $name;$fsFixture.cases.Add($name+'_finalizing_pointer_holds')
        }
        $facts=New-FixtureFacts 'hardlinks';$folder=Join-Path $fsFixture.root 'hardlinks'
        $null=New-DevLaneLocalDirectory $folder $facts
        $original=Join-Path $folder 'original.json';$alias=Join-Path $folder 'alias.json'
        $null=Write-DevLaneLocalNewJson $original $owner @{schemaVersion=1;synthetic=$true}
        Assert-FixtureBudget 0 1 0
        [CatDesk.DevLaneCacheFixtureV1.Bridge]::CreateOwnedHardLink($fsFixture.root)
        foreach($path in @($original,$alias)){
            $refused=$false
            try{$null=Read-DevLaneLocalJson $path $owner}catch{$refused=$_.Exception.GetBaseException().Message -ceq 'LOCAL_TARGET_CONTROL_LINK_OR_STATE_UNSAFE'}
            Check-FilesystemFixture $refused 'FS_FIXTURE_HARDLINK_NOT_REFUSED_BEFORE_JSON_READ'
        }
        $fsFixture.cases.Add('owned_two_link_control_records_refused_before_json_read')
        Check-FilesystemFixture ($fsFixture.cases.Count -eq 10) 'FS_FIXTURE_CASE_COUNT'
        $result=[pscustomobject][ordered]@{schemaVersion=1;classification='BOUNDED_SYNTHETIC_FILESYSTEM_FIXTURE_ONLY';passed=$fsFixture.cases.Count;cases=@($fsFixture.cases);rootName=$rootName;evidencePreserved=$true;maximumDirectories=128;directoriesReserved=$fsFixture.directories;maximumFiles=256;filesReserved=$fsFixture.files;maximumWrittenBytes=2MB;writtenBytesReserved=$fsFixture.writtenBytes;elapsedMilliseconds=$fsFixture.clock.ElapsedMilliseconds;nativeEntries=[CatDesk.DevLaneCacheV2.HeldFile]::NativeEntries;bridgeNativeEntries=[CatDesk.DevLaneCacheFixtureV1.Bridge]::NativeEntries;projectSourceOrToolBodiesInspected=$false;realCargoRuns=0;childProcessesCreated=0;registeredCacheInitialized=$false;productionOrWakeActions=$false;absoluteKernelDeadlineClaimed=$false;controllerMustProveHostExit=$true}
    }catch{
        # Preserve the complete owned evidence tree and fail without retry/replacement.
        throw 'FS_FIXTURE_FAILED_HOLD_PRESERVE_EVIDENCE_NO_RETRY'
    }finally{
        foreach($session in $fsFixture.sessions){Close-FixtureSession $session}
        foreach($scope in $fsFixture.scopes){$scope.Dispose()}
        if($null -ne $fsFixture.anchor){$fsFixture.anchor.Dispose()}
    }
    $result|ConvertTo-Json -Depth 5 -Compress
}
if($Mode -ceq 'FilesystemFixture'){
    if($PrepareDefinitionsOnly){
        Initialize-DevLaneFilesystemFixtureDefinitions
        [pscustomobject]@{schemaVersion=1;mode='FILESYSTEM_DEFINITIONS_PREPARED_ONLY';nativeEntries=0;fixtureRootCreated=$false;fixtureAttempts=[CatDesk.DevLaneCacheFixtureV1.Bridge]::FixtureAttempts;rootState=[CatDesk.DevLaneCacheFixtureV1.Bridge]::GetRootState()}|ConvertTo-Json -Depth 4 -Compress
        return
    }
    Invoke-DevLaneFilesystemFixture
    return
}
if($Mode -cne 'Mock' -or $PrepareDefinitionsOnly){throw 'FS_FIXTURE_MODE_REFUSED'}
$checks=[Collections.Generic.List[string]]::new()
function Check([bool]$Ok,[string]$Name){if(!$Ok){throw $Name};$checks.Add($Name)}
if($Mode -cne 'Mock'){throw 'DEFAULT_TEST_MODE_NOT_MOCK'}
Check (!$PrepareDefinitionsOnly) 'default_mock_never_prepares_or_runs_filesystem_fixture'
foreach($row in @(
    @{path='C:\synthetic\AppData\Local\Temp';allowed=$true},
    @{path='C:\synthetic\NotOneDrive\Temp';allowed=$true},
    @{path='C:\synthetic\OneDrive\Temp';allowed=$false},
    @{path='C:\synthetic\OneDrive - Organization\Temp';allowed=$false},
    @{path='C:\synthetic\onedrive\Temp';allowed=$false},
    @{path='\\server\share\Temp';allowed=$false},
    @{path='relative-temp';allowed=$false}
)){
    $denied=$false;try{Assert-DevLaneFilesystemFixtureParentPath $row.path}catch{$denied=$true}
    Check ($denied -ne $row.allowed) ('fixture_parent_path_policy_'+$checks.Count)
}
Check (!('CatDesk.DevLaneCacheV2.HeldFile' -as [type])) 'define_only_load_does_not_compile_or_call_native'
Check (!('CatDesk.DevLaneCacheFixtureV1.Bridge' -as [type])) 'fixture_bridge_not_initialized_on_default_entry'
if($CompileDefinitions){Initialize-DevLaneLocalFilesystem;Initialize-DevLaneFilesystemFixtureBridge;Check ([CatDesk.DevLaneCacheV2.HeldFile]::NativeEntries -eq 0 -and [CatDesk.DevLaneCacheFixtureV1.Bridge]::NativeEntries -eq 0) 'managed_definitions_compile_without_native_entry'}
if($CompileDefinitions){
    foreach($row in @(
        @{links=[uint32]1;pending=$false;directory=$false;allowed=$true},
        @{links=[uint32]0;pending=$false;directory=$false;allowed=$false},
        @{links=[uint32]2;pending=$false;directory=$false;allowed=$false},
        @{links=[uint32]::MaxValue;pending=$false;directory=$false;allowed=$false},
        @{links=[uint32]1;pending=$true;directory=$false;allowed=$false},
        @{links=[uint32]1;pending=$false;directory=$true;allowed=$false}
    )){
        $denied=$false;try{[CatDesk.DevLaneCacheV2.HeldFile]::ValidateControlFilePolicy($row.links,$row.pending,$row.directory)}catch{$denied=$true}
        Check ($denied -ne $row.allowed) ('control_same_handle_metadata_policy_'+$checks.Count)
    }
}

# Only these filesystem seams are substituted; the real policy, argument,
# START/OUTCOME and journal reconciliation functions run unchanged against memory.
function Write-DevLaneLocalNewJson([string]$Path,[string]$Owner,[object]$Value){
    if($localFixture.Store.ContainsKey($Path)){throw 'MOCK_CREATE_NEW_REFUSED'}
    $json=$Value|ConvertTo-Json -Depth 6 -Compress
    if([Text.Encoding]::UTF8.GetByteCount($json) -gt 8192){throw 'MOCK_MARKER_LIMIT'}
    $localFixture.Store[$Path]=$json
    $localFixture.Events.Add('new:'+([IO.Path]::GetFileName($Path)))
    if($localFixture.FailNewSuffix -and $Path.EndsWith($localFixture.FailNewSuffix,[StringComparison]::Ordinal)){throw 'MOCK_NEW_RECORD_PUBLICATION_FAILURE'}
    return ('a'*48)
}
function Read-DevLaneLocalJson([string]$Path,[string]$Owner){
    if(!$localFixture.Store.ContainsKey($Path)){throw 'MOCK_MISSING_RECORD'}
    return $localFixture.Store[$Path]|ConvertFrom-Json
}
function Set-DevLaneLocalPointer([object]$Session,[object]$Value){
    $localFixture.Events.Add('pointer:'+ $Value.phase)
    if($localFixture.FailPointer -ceq $Value.phase){throw 'MOCK_POINTER_PUBLICATION_FAILURE'}
    $localFixture.Store[(Join-Path $Session.directory 'current.json')]=$Value|ConvertTo-Json -Compress
}
function New-DevLaneLocalGuard([object]$Session){
    $path=Join-Path $Session.directory 'active.json'
    if($localFixture.Store.ContainsKey($path)){throw 'MOCK_CREATE_NEW_REFUSED'}
    $localFixture.Store[$path]='';$localFixture.Events.Add('guard-created-owned')
    return [pscustomobject]@{Identity=('8'*48)}
}
function Write-DevLaneLocalGuard([object]$Session,[object]$Value){
    $path=Join-Path $Session.directory 'active.json'
    $localFixture.Store[$path]=$Value|ConvertTo-Json -Depth 6 -Compress
    if($localFixture.FailNewSuffix -ceq 'active.json'){throw 'MOCK_GUARD_WRITE_FAILURE'}
}
function Complete-DevLaneLocalGuard([object]$Session){
    if($localFixture.FailGuardRemoval){throw 'MOCK_GUARD_REMOVAL_FAILED'}
    $path=Join-Path $Session.directory 'active.json'
    $record=$localFixture.Store[$path]|ConvertFrom-Json
    if($record.guardIdentity -cne $Session.guard.Identity){throw 'MOCK_FOREIGN_LEAF_NOT_REMOVED'}
    $localFixture.Store.Remove($path);$localFixture.Events.Add('guard-finalized-owned')
    if($localFixture.FailGuardAfterRemoval){throw 'MOCK_GUARD_FINALIZATION_UNKNOWN_AFTER_REMOVAL'}
}
function New-Fixture {
    return @{
        Store=@{};Leases=@{};Events=[Collections.Generic.List[string]]::new();Calls=[Collections.Generic.List[object]]::new()
        Owner='S-1-5-21-111-222-333-1001';Identity=('1'*48);Commit=('b'*40);Lock=('2'*64)
        FailPointer='';FailNewSuffix='';FailGuardRemoval=$false;FailGuardAfterRemoval=$false;ForeignGuard=$false;FailAt=0;UnknownAt=0;ThrowAt=0;Hash=$null
        RegistrationMismatch=$false;RecordOverride=-1;PhaseOverride='';Orphan=$false;UnsafeMetadata=$false
        Closed=0;Hashes=[Collections.Generic.List[object]]::new()
    }
}
function New-MockBackend {
    return @{
        Facts={param($source)
            if($localFixture.UnsafeMetadata){throw 'MOCK_METADATA_REFUSED'}
            [pscustomobject]@{owner=$localFixture.Owner;workspaceIdentity=$localFixture.Identity;sourceCommit=$localFixture.Commit;cargoPath='synthetic-cargo.exe';cargoSha256=('3'*64);rustcPath='C:\synthetic toolchain\bin\rustc.exe';rustcSha256=('4'*64);lockSha256=$localFixture.Lock;scope=$null}}
        Begin={param($facts,$key)
            if($localFixture.Leases.ContainsKey($key)){throw 'MOCK_LEASE_BUSY'}
            $directory=Join-Path 'C:\synthetic-cache' $key
            $session=[pscustomobject]@{directory=$directory;target=(Join-Path $directory 'target');targetIdentity=('7'*48);key=$key;owner=$facts.owner;state=$null;guard=$null}
            $current=Join-Path $directory 'current.json'
            $state=if($localFixture.Store.ContainsKey($current)){Read-DevLaneLocalJson $current $facts.owner}else{[pscustomobject]@{schemaVersion=2;workspaceKey=$key;targetIdentity=$session.targetIdentity;phase='READY';operationId=$null}}
            $paths=@($localFixture.Store.Keys|Where-Object{$_ -like ($directory+'\journal\*')})
            $journal=Get-DevLaneLocalJournal $session {$paths}
            $state|Add-Member active ($localFixture.Store.ContainsKey((Join-Path $directory 'active.json')))
            $state|Add-Member orphan $localFixture.Orphan
            $state|Add-Member recordCount $(if($localFixture.RecordOverride -ge 0){$localFixture.RecordOverride}else{$journal.count})
            $binding=Get-DevLaneLocalBinding $state $journal $key $session.targetIdentity
            $state|Add-Member startVerified $binding.startVerified
            $state|Add-Member terminalVerified $binding.terminalVerified
            if($localFixture.RegistrationMismatch){$state.workspaceKey='wrong'}
            if($localFixture.PhaseOverride){$state.phase=$localFixture.PhaseOverride}
            $session.state=$state;$localFixture.Leases[$key]=$true;return $session}
        Start={param($session,$id,$facts,$command) Start-DevLaneLocalOperation $session $id $facts $command}
        Advisory={param($source,$threshold) $localFixture.Events.Add('advisory')}
        Run={param($program,$arguments,$result)
            $localFixture.Calls.Add(@{program=$program;arguments=@($arguments);incremental=$env:CARGO_INCREMENTAL;compiler=$env:RUSTC;cwd=(Get-Location).Path})
            $n=$localFixture.Calls.Count;$localFixture.Events.Add('run')
            if($n -eq $localFixture.ThrowAt){throw 'MOCK_INTERRUPTED_CALL'}
            if($n -eq $localFixture.UnknownAt){return}
            if($localFixture.ForeignGuard){
                $path=@($localFixture.Store.Keys|Where-Object{$_ -like '*\active.json'})[0]
                $localFixture.Store[$path]=@{guardIdentity=('9'*48);foreign=$true}|ConvertTo-Json -Compress
            }
            $result.returned=$true;$result.exitCode=if($n -eq $localFixture.FailAt){[int]77}else{[int]0}}
        Finish={param($session,$id,$codes,$failed) Complete-DevLaneLocalOperation $session $id $codes $failed}
        ImageHash={param($path,$required)
            $localFixture.Hashes.Add(@{path=$path;required=$required})
            if($required -and $null -eq $localFixture.Hash){throw 'MOCK_SELECTED_BUILD_IMAGE_MISSING'}
            return $localFixture.Hash}
        Dirty={param($source) return $false}
        Close={param($session) $localFixture.Leases.Remove($session.key);$localFixture.Closed++}
    }
}
function Run-Fixture([string]$Operation){
    $before=(Get-Location).Path;$beforeOs=[Environment]::CurrentDirectory
    $prior=Get-Item Env:CARGO_INCREMENTAL -ErrorAction SilentlyContinue;$had=$null -ne $prior;$value=if($had){[string]$prior.Value}else{$null}
    $priorRustc=Get-Item Env:RUSTC -ErrorAction SilentlyContinue;$hadRustc=$null -ne $priorRustc;$rustcValue=if($hadRustc){[string]$priorRustc.Value}else{$null}
    $output=@();$failure=$null;$exitEvidence=$null
    try{$output=@(Invoke-DevLaneLocalEngine $Operation $root 0 (New-MockBackend))}catch{$failure=$_.Exception.Message;$exitEvidence=$_.Exception.Data['observedExitCodes']}
    $after=Get-Item Env:CARGO_INCREMENTAL -ErrorAction SilentlyContinue
    Check (((Get-Location).Path -ceq $before) -and [Environment]::CurrentDirectory -ceq $beforeOs) ('cwd_restored_'+$checks.Count)
    Check ((($null -ne $after) -eq $had) -and (!$had -or [string]$after.Value -ceq $value)) ('environment_restored_'+$checks.Count)
    $afterRustc=Get-Item Env:RUSTC -ErrorAction SilentlyContinue
    Check ((($null -ne $afterRustc) -eq $hadRustc) -and (!$hadRustc -or [string]$afterRustc.Value -ceq $rustcValue)) ('compiler_environment_restored_'+$checks.Count)
    return [pscustomobject]@{output=$output;failure=$failure;observedExitCodes=$exitEvidence}
}
$keys=@('schemaVersion','mode','operation','sourceCommit','workingTreeClean','debugImageSha256','productionInstallationChanged','daemonOrBrowserStarted','isolatedRuntimeReady')|Sort-Object
$localFixture=New-Fixture
$r=Run-Fixture verify
Check ($null -eq $r.failure -and $r.output.Count -eq 1) 'cold_verify_succeeds_without_top_level_image'
$receipt=$r.output[0]|ConvertFrom-Json
Check ($null -eq $receipt.debugImageSha256 -and !(Compare-Object $keys @($receipt.PSObject.Properties.Name|Sort-Object))) 'cold_verify_null_exact_nine_keys'
Check ($localFixture.Hashes.Count -eq 1 -and !$localFixture.Hashes[0].required -and $localFixture.Hashes[0].path -like 'C:\synthetic-cache\*\target\debug\catdesk.exe') 'verify_uses_selected_binary_never_legacy'
Check (($localFixture.Calls[0].arguments -join '|') -ceq 'fmt|--all|--|--check') 'fmt_unchanged'
$clippy=$localFixture.Calls[1].arguments
Check (($clippy[0..7] -join '|') -ceq 'clippy|--locked|--offline|--workspace|--all-targets|--all-features|--target-dir|C:\synthetic-cache\'+(Get-DevLaneLocalKey $localFixture.Owner $localFixture.Identity)+'\target' -and ($clippy[8..10] -join '|') -ceq '--|-D|warnings') 'clippy_target_precedes_rustc_separator'
Check ($localFixture.Calls[2].arguments[-2] -ceq '--target-dir') 'test_explicit_target'
Check (@($localFixture.Calls|Where-Object{$_.incremental -cne '0' -or $_.cwd -cne $root}).Count -eq 0) 'all_opted_commands_incremental_zero_and_source_cwd'
Check (@($localFixture.Calls|Where-Object{$_.compiler -cne 'C:\synthetic toolchain\bin\rustc.exe'}).Count -eq 0) 'each_command_uses_exact_observed_compiler_path'
Check ($localFixture.Events.IndexOf('pointer:FINALIZING') -lt $localFixture.Events.IndexOf('guard-finalized-owned') -and $localFixture.Events.IndexOf('guard-finalized-owned') -lt $localFixture.Events.IndexOf('pointer:RETURNED')) 'finalizing_precedes_exact_guard_finalization_and_terminal_pointer'
Check ($localFixture.Store.Values.Where({$_ -match 'wholeJobOrOutputEofVerified'}).Count -eq 1) 'terminal_record_is_persisted'
Check (!$localFixture.Store.Keys.Where({$_ -like '*\active.json'}).Count -and $localFixture.Closed -eq 1) 'success_releases_only_own_guard_and_lease'
$firstKey=Get-DevLaneLocalKey $localFixture.Owner $localFixture.Identity
$localFixture.Commit='c'*40;$localFixture.Lock='d'*64
Check ((Get-DevLaneLocalKey $localFixture.Owner $localFixture.Identity) -ceq $firstKey) 'source_and_lock_change_preserve_workspace_cache_key'
$r=Run-Fixture verify
Check ($null -eq $r.failure -and @($localFixture.Store.Keys|Where-Object{$_ -like '*.start.json'}).Count -eq 2) 'restart_reconciles_complete_immutable_history'
Check ((Get-DevLaneLocalKey $localFixture.Owner ('5'*48)) -cne $firstKey) 'different_physical_workspace_isolated'
Check ((Get-DevLaneLocalKey 'S-1-5-21-111-222-333-1002' $localFixture.Identity) -cne $firstKey) 'different_owner_isolated'
foreach($operation in @('status','storage-status','BUILD','unknown')){
    $localFixture=New-Fixture;$r=Run-Fixture $operation
    Check ($r.failure -ceq 'LOCAL_TARGET_OPERATION_REFUSED' -and !$localFixture.Calls.Count) ('non_optable_operation_refused_'+$operation)
}
$localFixture=New-Fixture;$localFixture.Hash='a'*64;$r=Run-Fixture build
Check ($null -eq $r.failure -and ($r.output[0]|ConvertFrom-Json).debugImageSha256 -ceq ('a'*64)) 'build_hashes_selected_image_after_return'
Check ($localFixture.Hashes[0].required -and $localFixture.Calls.Count -eq 1 -and $localFixture.Calls[0].arguments[-2] -ceq '--target-dir') 'build_requires_selected_image_and_explicit_target'
$localFixture=New-Fixture;$r=Run-Fixture build
Check ($r.failure -ceq 'MOCK_SELECTED_BUILD_IMAGE_MISSING' -and !$r.output.Count) 'missing_selected_build_image_never_uses_legacy'
foreach($failAt in @(1,2,3)){
    $localFixture=New-Fixture;$localFixture.FailAt=$failAt;$r=Run-Fixture verify
    Check ($r.failure -ceq 'DEVELOPMENT_COMMAND_FAILED' -and $localFixture.Calls.Count -eq $failAt -and @($localFixture.Store.Values|Where-Object{$_ -match 'RETURNED_FAILED' -and $_ -match 'exitCodes'}).Count -eq 1) ('nonzero_return_persisted_before_failure_'+$failAt)
}
foreach($kind in @('unknown','throw','active_write','start_write','outcome_write','start_pointer','finalizing_pointer','terminal_pointer','guard_removal','guard_absent_unknown')){
    $localFixture=New-Fixture
    switch($kind){'unknown'{$localFixture.UnknownAt=1};'throw'{$localFixture.ThrowAt=1};'active_write'{$localFixture.FailNewSuffix='active.json'};'start_write'{$localFixture.FailNewSuffix='.start.json'};'outcome_write'{$localFixture.FailNewSuffix='.outcome.json'};'start_pointer'{$localFixture.FailPointer='START'};'finalizing_pointer'{$localFixture.FailPointer='FINALIZING'};'terminal_pointer'{$localFixture.FailPointer='RETURNED'};'guard_removal'{$localFixture.FailGuardRemoval=$true};'guard_absent_unknown'{$localFixture.FailGuardAfterRemoval=$true}}
    $r=Run-Fixture verify
    $activeCount=@($localFixture.Store.Keys|Where-Object{$_ -like '*\active.json'}).Count
    $phases=@($localFixture.Store.Keys|Where-Object{$_ -like '*\current.json'}|ForEach-Object{($localFixture.Store[$_]|ConvertFrom-Json).phase})
    Check ($null -ne $r.failure -and !$r.output.Count -and ($activeCount -eq 1 -or $phases -ccontains 'FINALIZING')) ($kind+'_retains_durable_hold')
    if($kind -cin @('terminal_pointer','guard_absent_unknown')){Check ($activeCount -eq 0 -and $phases -ccontains 'FINALIZING') ($kind+'_holds_even_without_guard_leaf')}
    $calls=$localFixture.Calls.Count;$r=Run-Fixture verify
    Check ($null -ne $r.failure -and $localFixture.Calls.Count -eq $calls) ($kind+'_restart_never_blindly_replays')
}
$localFixture=New-Fixture;$localFixture.FailAt=1;$localFixture.FailPointer='RETURNED_FAILED';$r=Run-Fixture verify
Check ($r.failure -ceq 'DEVELOPMENT_COMMAND_FAILED_LOCAL_TARGET_OUTCOME_UNKNOWN_HOLD' -and @($localFixture.Store.Values|Where-Object{$_ -match 'exitCodes' -and $_ -match '77'}).Count -eq 1 -and $r.observedExitCodes[-1] -eq 77) 'nonzero_and_pointer_failure_preserve_exit_and_hold'
$localFixture=New-Fixture;$localFixture.ForeignGuard=$true;$r=Run-Fixture verify
Check ($r.failure -ceq 'LOCAL_TARGET_OUTCOME_UNKNOWN_HOLD' -and @($localFixture.Store.Values|Where-Object{$_ -match 'foreign'}).Count -eq 1) 'foreign_guard_leaf_is_preserved_and_held'
# Old successful pointer with an orphan START must hold even if active is missing.
$localFixture=New-Fixture;$null=Run-Fixture verify
$directory=[IO.Path]::GetDirectoryName(@($localFixture.Store.Keys|Where-Object{$_ -like '*\current.json'})[0])
$id='f'*32;$targetId='7'*48
$localFixture.Store[(Join-Path $directory ('journal\'+$id+'.start.json'))]=@{schemaVersion=2;operationId=$id;workspaceKey=(Get-DevLaneLocalKey $localFixture.Owner $localFixture.Identity);targetIdentity=$targetId;operation='verify'}|ConvertTo-Json -Compress
$calls=$localFixture.Calls.Count;$r=Run-Fixture verify
Check ($r.failure -ceq 'LOCAL_TARGET_ORPHAN_START_HOLD' -and $localFixture.Calls.Count -eq $calls) 'old_success_pointer_orphan_start_holds'
foreach($row in @(
    @{namespace=$true;names=@('namespace.json',('a'*64));ok=$true},
    @{namespace=$true;names=@('namespace.json','foreign');ok=$false},
    @{namespace=$false;names=@('owner.json','lease','target','journal','current.json');ok=$true},
    @{namespace=$false;names=@('owner.json','pointer-aborted.tmp');ok=$false},
    @{namespace=$false;names=@('owner.json','foreign');ok=$false},
    @{namespace=$false;names=@('lease')*257;ok=$false}
)){
    $denied=$false;try{Assert-DevLaneLocalEntries 'C:\synthetic' $row.namespace {$row.names}}catch{$denied=$true}
    Check ($denied -ne $row.ok) ('bounded_immediate_directory_policy_'+$checks.Count)
}
# Exercise journal limit with valid immutable pairs; entry 257 refuses before read.
$localFixture=New-Fixture;$session=[pscustomobject]@{directory='C:\synthetic';owner=$localFixture.Owner;key=$firstKey;targetIdentity=('7'*48)}
$paths=[Collections.Generic.List[string]]::new()
for($i=0;$i -lt 128;$i++){
    $id=$i.ToString('x32')
    foreach($kind in @('start','outcome')){
        $p=Join-Path $session.directory ('journal\'+$id+'.'+$kind+'.json');$paths.Add($p)
        $record=@{schemaVersion=2;operationId=$id;workspaceKey=$firstKey;targetIdentity=$session.targetIdentity;operation='build';phase='RETURNED';exitCodes=@(0)}
        $localFixture.Store[$p]=$record|ConvertTo-Json -Compress
    }
}
$journal=Get-DevLaneLocalJournal $session {$paths}
Check ($journal.count -eq 256) 'journal_observation_bound_exact_boundary'
$paths.Add('C:\synthetic\journal\never-read')
$denied=$false;try{$null=Get-DevLaneLocalJournal $session {$paths}}catch{$denied=$_.Exception.Message -ceq 'LOCAL_TARGET_RECORD_LIMIT_HOLD'}
Check $denied 'journal_limit_refuses_before_257th_read'
foreach($variant in @('registration','limit','START','FINALIZING','UNKNOWN','metadata')){
    $localFixture=New-Fixture
    switch($variant){'registration'{$localFixture.RegistrationMismatch=$true};'limit'{$localFixture.RecordOverride=256};'metadata'{$localFixture.UnsafeMetadata=$true};default{$localFixture.PhaseOverride=$variant}}
    $r=Run-Fixture verify
    Check ($null -ne $r.failure -and !$localFixture.Calls.Count) ('refused_before_cargo_'+$variant)
}
$localFixture=New-Fixture;$localFixture.Leases[$firstKey]=$true;$r=Run-Fixture verify
Check ($r.failure -ceq 'MOCK_LEASE_BUSY' -and !$localFixture.Calls.Count) 'same_workspace_lease_refused'
$localFixture.Identity='6'*48;$r=Run-Fixture verify
Check ($null -eq $r.failure -and $localFixture.Calls.Count -eq 3 -and $localFixture.Leases.ContainsKey($firstKey)) 'blocked_sibling_does_not_block_independent_workspace'
foreach($value in @($null,'','caller-value','0','1')){
    $prior=Get-Item Env:CARGO_INCREMENTAL -ErrorAction SilentlyContinue;$had=$null -ne $prior;$old=if($had){[string]$prior.Value}else{$null}
    try{if($null -eq $value){$env:CARGO_INCREMENTAL=$null}else{Set-Item Env:CARGO_INCREMENTAL $value};$localFixture=New-Fixture;$null=Run-Fixture verify}finally{if($had){Set-Item Env:CARGO_INCREMENTAL $old}else{$env:CARGO_INCREMENTAL=$null}}
}
foreach($value in @($null,'','C:\caller compiler\rustc.exe')){
    $prior=Get-Item Env:RUSTC -ErrorAction SilentlyContinue;$had=$null -ne $prior;$old=if($had){[string]$prior.Value}else{$null}
    try{
        if($null -eq $value){$env:RUSTC=$null}else{Set-Item Env:RUSTC $value}
        foreach($failure in @('none','native_nonzero','interrupted','terminal_pointer')){
            $localFixture=New-Fixture
            switch($failure){'native_nonzero'{$localFixture.FailAt=1};'interrupted'{$localFixture.ThrowAt=1};'terminal_pointer'{$localFixture.FailPointer='RETURNED'}}
            $null=Run-Fixture verify
            Check (@($localFixture.Calls|Where-Object{$_.compiler -cne 'C:\synthetic toolchain\bin\rustc.exe'}).Count -eq 0) ('selected_compiler_and_restoration_'+$checks.Count)
        }
    }finally{if($had){Set-Item Env:RUSTC $old}else{$env:RUSTC=$null}}
}
# Shared production pointer-to-journal binding, independent of native session open.
foreach($variant in @('valid','missing_pair','missing_start','missing_outcome','wrong_phase','wrong_record_key','wrong_record_target','wrong_record_id','wrong_pointer_id','wrong_pointer_key','wrong_pointer_target','ready_with_history')){
    $id='a'*32;$targetId='7'*48
    $state=[pscustomobject]@{schemaVersion=2;workspaceKey=$firstKey;targetIdentity=$targetId;phase='RETURNED';operationId=$id}
    $start=@{schemaVersion=2;workspaceKey=$firstKey;targetIdentity=$targetId;operationId=$id}
    $outcome=@{schemaVersion=2;workspaceKey=$firstKey;targetIdentity=$targetId;operationId=$id;phase='RETURNED'}
    $pair=@{start=$start;outcome=$outcome};$records=@{};$records[$id]=$pair
    switch($variant){
        'missing_pair'{$records.Clear()}
        'missing_start'{$pair.Remove('start')}
        'missing_outcome'{$pair.Remove('outcome')}
        'wrong_phase'{$outcome.phase='RETURNED_FAILED'}
        'wrong_record_key'{$start.workspaceKey='wrong'}
        'wrong_record_target'{$outcome.targetIdentity='wrong'}
        'wrong_record_id'{$start.operationId='b'*32}
        'wrong_pointer_id'{$state.operationId='c'*32}
        'wrong_pointer_key'{$state.workspaceKey='wrong'}
        'wrong_pointer_target'{$state.targetIdentity='wrong'}
        'ready_with_history'{$state.phase='READY';$state.operationId=$null}
    }
    $denied=$false;try{$null=Get-DevLaneLocalBinding $state ([pscustomobject]@{count=2;records=$records}) $firstKey $targetId}catch{$denied=$true}
    Check ($denied -eq ($variant -cne 'valid')) ('shared_pointer_journal_binding_'+$variant)
}
# Exact added shape removal must reconstruct the actual published fbc facade.
$facade=[IO.File]::ReadAllText((Join-Path $PSScriptRoot 'catdesk-dev-lane.ps1')).Replace([string][char]13,'')
$branch=@'
# Opt-in cache placement; the default path below is preserved byte-for-byte.
if ($UseLocalTarget) {
    if ($Command -cnotin @('build', 'verify')) { throw 'LOCAL_TARGET_OPERATION_REFUSED' }
    $localHelper = Join-Path $PSScriptRoot 'catdesk-dev-lane-local-target.ps1'
    $null = Assert-RegularFile $localHelper
    . $localHelper
    Invoke-DevLaneLocalTarget -Command $Command -SourceRoot ([IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))) -WarningFreeBytes $StorageWarningFreeBytes
    return
}

'@
$branch=$branch.Replace([string][char]13,'')+[char]10
$parameter='    [long]$StorageWarningFreeBytes = 64GB,'+[char]10+'    [switch]$UseLocalTarget'
Check ([regex]::Matches($facade,[regex]::Escape($branch)).Count -eq 1 -and [regex]::Matches($facade,[regex]::Escape($parameter)).Count -eq 1) 'exact_two_optin_addition_shapes'
$legacy=$facade.Replace($branch,'').Replace($parameter,'    [long]$StorageWarningFreeBytes = 64GB')
$h=[Security.Cryptography.SHA256]::Create()
try{$digest=([BitConverter]::ToString($h.ComputeHash([Text.Encoding]::UTF8.GetBytes($legacy)))).Replace('-','').ToLowerInvariant()}finally{$h.Dispose()}
Check ($digest -ceq 'cbe089734a5c7fdedc7951abf19d5210b5ddec81d15e69bf8c85ca656e1356bd') 'false_branch_reconstructs_actual_fbc_facade_no_repin'
foreach($operation in @('status','storage-status')){
    $denied=$false;try{$null=& (Join-Path $PSScriptRoot 'catdesk-dev-lane.ps1') -Command $operation -UseLocalTarget}catch{$denied=$_.Exception.Message -ceq 'LOCAL_TARGET_OPERATION_REFUSED'}
    Check $denied ('public_nonoptable_operation_refused_without_helper_'+$operation)
}
foreach($name in @('CARGO_HOME','RUSTUP_TOOLCHAIN','RUSTC_WRAPPER','CARGO_TARGET_DIR','CARGO_BUILD_BUILD_DIR')){
    $path='Env:'+$name;$prior=Get-Item $path -ErrorAction SilentlyContinue;$had=$null -ne $prior;$old=if($had){[string]$prior.Value}else{$null}
    try{
        Set-Item $path 'synthetic-redirect'
        $denied=$false;try{$null=Get-DevLaneLocalFacts $root}catch{$denied=$_.Exception.Message -ceq 'LOCAL_TARGET_INHERITED_POLICY_REFUSED'}
        Check $denied ('actual_policy_refuses_before_metadata_native_'+$name)
    }finally{if($had){Set-Item $path $old}else{Remove-Item $path -ErrorAction SilentlyContinue}}
}
foreach($row in @(
    @{name='CARGO_BUILD_JOBS';value='1';allowed=$true},
    @{name='CARGO_BUILD_JOBS';value='';allowed=$false},
    @{name='CARGO_BUILD_JOBS';value=$null;allowed=$false},
    @{name='CARGO_BUILD_JOBS';value='0';allowed=$false},
    @{name='CARGO_BUILD_JOBS';value='2';allowed=$false},
    @{name='CARGO_BUILD_JOBS';value='01';allowed=$false},
    @{name='CARGO_BUILD_JOBS';value=' 1';allowed=$false},
    @{name='CARGO_BUILD_JOBS';value='1 ';allowed=$false},
    @{name='CARGO_BUILD_JOBS';value='other';allowed=$false},
    @{name='CARGO_BUILD_JOBS';value=1;allowed=$false},
    @{name='cargo_build_jobs';value='1';allowed=$false},
    @{name='Cargo_BUILD_JOBS';value='1';allowed=$false},
    @{name='CARGO_BUILD_BUILD_DIR';value='';allowed=$false},
    @{name='CARGO_BUILD_BUILD_DIR';value='private';allowed=$false},
    @{name='cargo_build_target';value='';allowed=$false},
    @{name='CARGO_NET_OFFLINE';value='true';allowed=$true}
)){
    $denied=$false;try{Assert-DevLaneLocalEnvironment @([pscustomobject]@{Name=$row.name;Value=$row.value})}catch{$denied=$true}
    Check ($denied -ne $row.allowed) ('optin_environment_exception_'+$checks.Count)
}
if($CompileDefinitions){
    $empty=[CatDesk.DevLaneCacheFixtureV1.Bridge]::GetRootState()
    Check ($empty.Phase -ceq 'NO_ROOT_INTENT' -and $null -eq $empty.RelativeRootName -and $null -eq $empty.ParentPhysicalIdentity -and $null -eq $empty.CreatedRootPhysicalIdentity) 'fresh_bridge_root_state_empty'
    $syntheticName='catdesk-dev-lane-fs-v1-'+('a'*32);$syntheticParent='b'*48
    $denied=$false;try{[CatDesk.DevLaneCacheFixtureV1.Bridge]::RecordRootIntent($syntheticName,$syntheticParent)}catch{$denied=$true}
    Check $denied 'root_intent_requires_prior_attempt_claim'
    $denied=$false;try{[CatDesk.DevLaneCacheFixtureV1.Bridge]::RecordCreatedRoot(('c'*48))}catch{$denied=$true}
    Check $denied 'created_root_without_intent_refused'
    Check ([CatDesk.DevLaneCacheFixtureV1.Bridge]::FixtureAttempts -eq 0) 'fixture_attempt_gate_unused_by_mock_suite'
    [CatDesk.DevLaneCacheFixtureV1.Bridge]::ClaimFixtureAttempt()
    Check ([CatDesk.DevLaneCacheFixtureV1.Bridge]::FixtureAttempts -eq 1) 'pure_fixture_gate_claim_without_filesystem_entry'
    $denied=$false;try{[CatDesk.DevLaneCacheFixtureV1.Bridge]::ClaimFixtureAttempt()}catch{$denied=$true}
    Check $denied 'same_host_fixture_attempt_retry_refused_by_pure_gate'
    foreach($row in @(
        @{name='';parent=$syntheticParent},
        @{name='C:\private\root';parent=$syntheticParent},
        @{name='..\catdesk-dev-lane-fs-v1-'+('a'*32);parent=$syntheticParent},
        @{name=$syntheticName;parent='invalid'},
        @{name=$syntheticName;parent=('B'*48)}
    )){
        $denied=$false;try{[CatDesk.DevLaneCacheFixtureV1.Bridge]::RecordRootIntent($row.name,$row.parent)}catch{$denied=$true}
        Check ($denied -and [CatDesk.DevLaneCacheFixtureV1.Bridge]::GetRootState().Phase -ceq 'NO_ROOT_INTENT') ('invalid_root_intent_has_no_state_effect_'+$checks.Count)
    }
    [CatDesk.DevLaneCacheFixtureV1.Bridge]::RecordRootIntent($syntheticName,$syntheticParent)
    $intent=[CatDesk.DevLaneCacheFixtureV1.Bridge]::GetRootState()
    Check ($intent.Phase -ceq 'CREATION_OUTCOME_UNKNOWN' -and $intent.RelativeRootName -ceq $syntheticName -and $intent.ParentPhysicalIdentity -ceq $syntheticParent -and $null -eq $intent.CreatedRootPhysicalIdentity) 'intent_without_readback_is_creation_unknown'
    try{try{throw 'SYNTHETIC_CASE_FAILURE'}finally{throw 'SYNTHETIC_DISPOSAL_FAILURE'}}catch{
        $state=[CatDesk.DevLaneCacheFixtureV1.Bridge]::GetRootState()
        Check ($state.Phase -ceq 'CREATION_OUTCOME_UNKNOWN' -and $state.RelativeRootName -ceq $syntheticName) 'controller_can_read_intent_after_case_and_finally_throw'
    }
    $stderr=[IO.StringWriter]::new();$priorStderr=[Console]::Error
    try{[Console]::SetError($stderr);Write-DevLaneFilesystemFixtureRootMarker}finally{[Console]::SetError($priorStderr)}
    $intentMarker=$stderr.ToString()|ConvertFrom-Json;$stderr.Dispose()
    Check ($intentMarker.phase -ceq 'CREATION_OUTCOME_UNKNOWN' -and $null -eq $intentMarker.createdRootPhysicalIdentity -and !$intentMarker.cleanupAuthorized) 'precreate_stderr_marker_reports_unknown_without_cleanup_authority'
    foreach($badIdentity in @('','bad',('C'*48),('c'*64))){
        $denied=$false;try{[CatDesk.DevLaneCacheFixtureV1.Bridge]::RecordCreatedRoot($badIdentity)}catch{$denied=$true}
        Check ($denied -and [CatDesk.DevLaneCacheFixtureV1.Bridge]::GetRootState().Phase -ceq 'CREATION_OUTCOME_UNKNOWN') ('unverified_created_identity_cannot_advance_state_'+$checks.Count)
    }
    [CatDesk.DevLaneCacheFixtureV1.Bridge]::RecordCreatedRoot(('c'*48))
    $created=[CatDesk.DevLaneCacheFixtureV1.Bridge]::GetRootState()
    Check ($created.Phase -ceq 'CREATED_IDENTITY_VERIFIED' -and $created.RelativeRootName -ceq $syntheticName -and $created.ParentPhysicalIdentity -ceq $syntheticParent -and $created.CreatedRootPhysicalIdentity -ceq ('c'*48)) 'created_identity_retains_original_name_and_parent'
    Check ($intent.Phase -ceq 'CREATION_OUTCOME_UNKNOWN' -and $null -eq $intent.CreatedRootPhysicalIdentity) 'prior_root_snapshot_is_immutable'
    try{throw 'SYNTHETIC_FINAL_DISPOSAL_FAILURE'}catch{Check ([CatDesk.DevLaneCacheFixtureV1.Bridge]::GetRootState().CreatedRootPhysicalIdentity -ceq ('c'*48)) 'controller_readback_survives_postcreate_finally_failure'}
    $denied=$false;try{[CatDesk.DevLaneCacheFixtureV1.Bridge]::RecordRootIntent(('catdesk-dev-lane-fs-v1-'+('d'*32)),('e'*48))}catch{$denied=$true}
    Check $denied 'root_intent_cannot_be_replaced_or_reset'
    $denied=$false;try{[CatDesk.DevLaneCacheFixtureV1.Bridge]::RecordCreatedRoot(('d'*48))}catch{$denied=$true}
    Check ($denied -and [CatDesk.DevLaneCacheFixtureV1.Bridge]::GetRootState().CreatedRootPhysicalIdentity -ceq ('c'*48)) 'created_root_identity_cannot_be_replaced'
    $stderr=[IO.StringWriter]::new();$priorStderr=[Console]::Error
    try{[Console]::SetError($stderr);Write-DevLaneFilesystemFixtureRootMarker}finally{[Console]::SetError($priorStderr)}
    $markerText=$stderr.ToString();$createdMarker=$markerText|ConvertFrom-Json;$stderr.Dispose()
    Check ($createdMarker.phase -ceq 'CREATED_IDENTITY_VERIFIED' -and $createdMarker.createdRootPhysicalIdentity -ceq ('c'*48) -and $markerText -notmatch 'S-1-|[A-Za-z]:\\|credential|bearer') 'created_stderr_marker_has_no_personal_path_sid_or_content'
    Check ([CatDesk.DevLaneCacheV2.HeldFile]::NativeEntries -eq 0 -and [CatDesk.DevLaneCacheFixtureV1.Bridge]::NativeEntries -eq 0) 'all_mock_tests_native_entries_zero'
}
$bridgeHash=if($CompileDefinitions){[CatDesk.DevLaneCacheFixtureV1.Bridge]::TemplateSha256}else{$null}
[pscustomobject][ordered]@{schemaVersion=1;mode='Mock';passed=$checks.Count;hostVersion=$PSVersionTable.PSVersion.ToString();managedCompilations=(2*[int][bool]$CompileDefinitions);nativeEntries=0;bridgeNativeEntries=0;fixtureBridgeTemplateSha256=$bridgeHash;rootStateInputsSynthetic=[bool]$CompileDefinitions;fixtureRootCreated=$false;filesystemFixtures=0;realCargoRuns=0;registeredTargetsCreated=0;checks=@($checks)}|ConvertTo-Json -Depth 4 -Compress
