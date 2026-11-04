# Define-only ordinary cache option. Native entry is explicit, never at load.
function Get-DevLaneLocalKey([string]$Owner,[string]$WorkspaceIdentity) {
    if ($Owner -cnotmatch '\AS-1-[0-9-]{3,180}\z' -or $WorkspaceIdentity -cnotmatch '\A[0-9a-f]{48}\z') { throw 'LOCAL_TARGET_IDENTITY_UNVERIFIED' }
    $hash=[Security.Cryptography.SHA256]::Create()
    try {return ([BitConverter]::ToString($hash.ComputeHash([Text.Encoding]::UTF8.GetBytes('ordinary-worktree-v2|'+$Owner+'|'+$WorkspaceIdentity)))).Replace('-','').ToLowerInvariant()}
    finally {$hash.Dispose()}
}
function Get-DevLaneLocalArguments([string]$Operation,[string]$Target) {
    switch -CaseSensitive ($Operation) {
        'build' {return ,@('build','--locked','--offline','--bin','catdesk','--target-dir',$Target)}
        'fmt' {return ,@('fmt','--all','--','--check')}
        'clippy' {return ,@('clippy','--locked','--offline','--workspace','--all-targets','--all-features','--target-dir',$Target,'--','-D','warnings')}
        'test' {return ,@('test','--locked','--offline','--bin','catdesk','--target-dir',$Target)}
        default {throw 'LOCAL_TARGET_OPERATION_REFUSED'}
    }
}
function Assert-DevLaneLocalState([object]$State,[string]$Key,[string]$TargetIdentity) {
    if ($State.schemaVersion -ne 2 -or $State.workspaceKey -cne $Key -or $State.targetIdentity -cne $TargetIdentity) {throw 'LOCAL_TARGET_REGISTRATION_MISMATCH'}
    if ($State.active -or $State.orphan -or $State.recordCount -ge 256 -or $State.phase -cnotin @('READY','RETURNED','RETURNED_FAILED')) {throw 'LOCAL_TARGET_UNKNOWN_HOLD'}
    if ($State.phase -cne 'READY' -and (!$State.terminalVerified -or !$State.startVerified)) {throw 'LOCAL_TARGET_UNKNOWN_HOLD'}
    if ($State.phase -ceq 'READY' -and $State.recordCount -ne 0) {throw 'LOCAL_TARGET_UNKNOWN_HOLD'}
}
function Get-DevLaneLocalBinding([object]$State,[object]$Journal,[string]$Key,[string]$TargetIdentity) {
    if($State.schemaVersion -ne 2 -or $State.workspaceKey -cne $Key -or $State.targetIdentity -cne $TargetIdentity){throw 'LOCAL_TARGET_REGISTRATION_MISMATCH'}
    $startVerified=$false;$terminalVerified=$false
    if($State.phase -ceq 'READY'){
        if($null -ne $State.operationId -or $Journal.count -ne 0){throw 'LOCAL_TARGET_UNKNOWN_HOLD'}
    }elseif($State.phase -cin @('RETURNED','RETURNED_FAILED')){
        if($State.operationId -isnot [string] -or $State.operationId -cnotmatch '\A[0-9a-f]{32}\z' -or !$Journal.records.ContainsKey($State.operationId)){throw 'LOCAL_TARGET_UNKNOWN_HOLD'}
        $pair=$Journal.records[$State.operationId]
        if(!$pair.ContainsKey('start') -or !$pair.ContainsKey('outcome')){throw 'LOCAL_TARGET_UNKNOWN_HOLD'}
        foreach($record in @($pair.start,$pair.outcome)){
            if($record.schemaVersion -ne 2 -or $record.operationId -cne $State.operationId -or $record.workspaceKey -cne $Key -or $record.targetIdentity -cne $TargetIdentity){throw 'LOCAL_TARGET_UNKNOWN_HOLD'}
        }
        if($pair.outcome.phase -cne $State.phase){throw 'LOCAL_TARGET_UNKNOWN_HOLD'}
        $startVerified=$true;$terminalVerified=$true
    }
    return [pscustomobject]@{startVerified=$startVerified;terminalVerified=$terminalVerified}
}
function Invoke-DevLaneLocalEngine([string]$Command,[string]$SourceRoot,[long]$WarningFreeBytes,[object]$Backend,[string]$LocalTargetBase='') {
    if ($Command -cnotin @('build','verify')) {throw 'LOCAL_TARGET_OPERATION_REFUSED'}
    $session=$null;$facts=$null;$locationPushed=$false
    $prior=Get-Item Env:CARGO_INCREMENTAL -ErrorAction SilentlyContinue
    $hadPrior=$null -ne $prior;$priorValue=if($hadPrior){[string]$prior.Value}else{$null}
    $priorRustc=Get-Item Env:RUSTC -ErrorAction SilentlyContinue
    $hadRustc=$null -ne $priorRustc;$priorRustcValue=if($hadRustc){[string]$priorRustc.Value}else{$null}
    try {
        $facts=& $Backend.Facts $SourceRoot $LocalTargetBase
        $key=Get-DevLaneLocalKey $facts.owner $facts.workspaceIdentity
        $session=& $Backend.Begin $facts $key
        Assert-DevLaneLocalState $session.state $key $session.targetIdentity
        $binary=Join-Path $session.target 'debug\catdesk.exe'
        $id=[Guid]::NewGuid().ToString('N')
        & $Backend.Start $session $id $facts $Command
        Push-Location -LiteralPath $SourceRoot;$locationPushed=$true
        $env:CARGO_INCREMENTAL='0'
        $env:RUSTC=$facts.rustcPath
        & $Backend.Advisory $SourceRoot $WarningFreeBytes
        $operations=if($Command -ceq 'build'){@('build')}else{@('fmt','clippy','test')}
        $codes=[Collections.Generic.List[int]]::new();$failed=$false
        foreach($operation in $operations) {
            $arguments=Get-DevLaneLocalArguments $operation $session.target
            $run=@{returned=$false;exitCode=$null}
            # Stream ordinary output; return status travels separately, not in stdout.
            & $Backend.Run $facts.cargoPath $arguments $run
            if(!$run.returned -or $run.exitCode -isnot [int]) {throw 'LOCAL_TARGET_UNKNOWN_HOLD'}
            $codes.Add($run.exitCode)
            if($run.exitCode -ne 0){$failed=$true;break}
        }
        try {& $Backend.Finish $session $id @($codes) $failed}
        catch {
            $reason=if($failed){'DEVELOPMENT_COMMAND_FAILED_LOCAL_TARGET_OUTCOME_UNKNOWN_HOLD'}else{'LOCAL_TARGET_OUTCOME_UNKNOWN_HOLD'}
            $failure=[InvalidOperationException]::new($reason)
            $failure.Data['observedExitCodes']=[int[]]@($codes)
            $failure.Data['localTargetState']='UNKNOWN_HOLD'
            throw $failure
        }
        if($failed){throw 'DEVELOPMENT_COMMAND_FAILED'}
        # Cold verify need not produce this image; never fall back to source/target.
        $digest=& $Backend.ImageHash $binary ($Command -ceq 'build')
        $dirty=& $Backend.Dirty $SourceRoot
        [pscustomobject][ordered]@{
            schemaVersion=1;mode='DEVELOPMENT_SOURCE_ONLY';operation=$Command.ToUpperInvariant()
            sourceCommit=$facts.sourceCommit;workingTreeClean=(!$dirty);debugImageSha256=$digest
            productionInstallationChanged=$false;daemonOrBrowserStarted=$false;isolatedRuntimeReady=$false
        }|ConvertTo-Json -Compress
    } finally {
        if($hadPrior){Set-Item Env:CARGO_INCREMENTAL $priorValue}else{$env:CARGO_INCREMENTAL=$null}
        if($hadRustc){Set-Item Env:RUSTC $priorRustcValue}else{$env:RUSTC=$null}
        if($locationPushed){Pop-Location}
        if($null -ne $session){& $Backend.Close $session}
        elseif($null -ne $facts -and $null -ne $facts.scope){$facts.scope.Dispose()}
    }
}

function Initialize-DevLaneLocalFilesystem {
    $definition=@'
using System;
using System.IO;
using System.Text;
using System.Runtime.InteropServices;
using System.Security.Principal;
using Microsoft.Win32.SafeHandles;
namespace CatDesk.DevLaneCacheV2 {
 public sealed class HeldFile : IDisposable {
  public const string TemplateSha256="__TEMPLATE__";
  public static int NativeEntries;
  public readonly SafeFileHandle Handle;
  public readonly string Identity;
  private readonly bool ownedGuard;
  private bool guardFinalizationAttempted;
  [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern SafeFileHandle CreateFile(string p,uint a,uint s,IntPtr sec,uint c,uint f,IntPtr t);
  [DllImport("kernel32.dll",SetLastError=true)] static extern bool GetFileInformationByHandleEx(SafeFileHandle h,int k,byte[] b,uint n);
  [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern uint GetFinalPathNameByHandle(SafeFileHandle h,StringBuilder b,uint n,uint f);
  [DllImport("advapi32.dll")] static extern uint GetSecurityInfo(SafeFileHandle h,int t,uint i,out IntPtr o,IntPtr g,IntPtr d,IntPtr s,out IntPtr sd);
  [DllImport("kernel32.dll")] static extern IntPtr LocalFree(IntPtr p);
  [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern bool CreateDirectory(string p,IntPtr s);
  [DllImport("kernel32.dll",SetLastError=true)] static extern bool SetFileInformationByHandle(SafeFileHandle h,int k,ref byte b,uint n);
  [DllImport("kernel32.dll",SetLastError=true)] static extern bool CloseHandle(IntPtr h);
  public static void CreateEmptyDirectory(string path) {
   NativeEntries++;
   if(!CreateDirectory(path,IntPtr.Zero)) throw new IOException("LOCAL_TARGET_DIRECTORY_NOT_CREATED");
  }
  public static HeldFile CreateGuard(string path,string owner){return new HeldFile(path,owner,true,true,true);}
  public HeldFile(string path,string owner,bool create,bool exclusive):this(path,owner,create,exclusive,false){}
  private HeldFile(string path,string owner,bool create,bool exclusive,bool guard) {
   NativeEntries++;
   ownedGuard=guard;
   string full=Path.GetFullPath(path);
   Handle=CreateFile(full,0x80020080u | (create?0x40000000u:0u) | (guard?0x10000u:0u),exclusive?0u:1u,IntPtr.Zero,create?1u:3u,0x00300000u,IntPtr.Zero);
   if(Handle.IsInvalid){Handle.Dispose();throw new IOException("LOCAL_TARGET_FILE_OPEN_REFUSED");}
   try {
    byte[] a=new byte[8];
    if(!GetFileInformationByHandleEx(Handle,9,a,8)||(BitConverter.ToUInt32(a,0)&0x541410u)!=0)throw new IOException("LOCAL_TARGET_FILE_KIND_UNSAFE");
    AssertControlFile();
    var final=new StringBuilder(32768);uint n=GetFinalPathNameByHandle(Handle,final,32768,0);
    if(n==0||n>=32768||!String.Equals(final.ToString(),"\\\\?\\"+full,StringComparison.OrdinalIgnoreCase))throw new IOException("LOCAL_TARGET_FILE_ALIAS");
    IntPtr o,sd;if(GetSecurityInfo(Handle,1,1,out o,IntPtr.Zero,IntPtr.Zero,IntPtr.Zero,out sd)!=0)throw new IOException("LOCAL_TARGET_FILE_OWNER");
    try{if(new SecurityIdentifier(o).Value!=owner)throw new IOException("LOCAL_TARGET_FILE_OWNER");}finally{LocalFree(sd);}
    byte[] id=new byte[24];if(!GetFileInformationByHandleEx(Handle,18,id,24))throw new IOException("LOCAL_TARGET_FILE_IDENTITY");
    Identity=BitConverter.ToString(id).Replace("-","").ToLowerInvariant();
   }catch{Handle.Dispose();throw;}
  }
  public string Read() {
   AssertControlFile();
   using(var borrowed=new SafeFileHandle(Handle.DangerousGetHandle(),false))
   using(var stream=new FileStream(borrowed,FileAccess.Read)) {
    if(stream.Length>8192)throw new IOException("LOCAL_TARGET_MARKER_TOO_LARGE");
    stream.Position=0;byte[] bytes=new byte[(int)stream.Length];int at=0,n;
    while(at<bytes.Length&&(n=stream.Read(bytes,at,bytes.Length-at))>0)at+=n;
    if(at!=bytes.Length)throw new IOException("LOCAL_TARGET_MARKER_INCOMPLETE");
    return new UTF8Encoding(false,true).GetString(bytes);
   }
  }
  public void WriteNew(string value) {
   AssertControlFile();
   byte[] bytes=new UTF8Encoding(false,true).GetBytes(value);
   if(bytes.Length>8192)throw new IOException("LOCAL_TARGET_MARKER_TOO_LARGE");
   using(var borrowed=new SafeFileHandle(Handle.DangerousGetHandle(),false))
   using(var stream=new FileStream(borrowed,FileAccess.Write)) {if(stream.Length!=0)throw new IOException("LOCAL_TARGET_NOT_EMPTY");stream.Write(bytes,0,bytes.Length);stream.Flush(true);}
  }
  public void FinalizeOwnedGuard() {
   if(!ownedGuard||guardFinalizationAttempted||Handle.IsClosed||Handle.IsInvalid)throw new IOException("LOCAL_TARGET_GUARD_FINALIZATION_UNVERIFIED");
   AssertControlFile();
   guardFinalizationAttempted=true;NativeEntries++;
   byte disposition=1;
   // FileDispositionInfo applies to the retained CREATE_NEW handle, never a path.
   if(!SetFileInformationByHandle(Handle,4,ref disposition,1))throw new IOException("LOCAL_TARGET_GUARD_FINALIZATION_UNVERIFIED");
   IntPtr raw=Handle.DangerousGetHandle();
   // Do not retry an uncertain raw close through SafeHandle/finalization.
   Handle.SetHandleAsInvalid();
   if(!CloseHandle(raw))throw new IOException("LOCAL_TARGET_GUARD_CLOSE_UNKNOWN");
  }
  public static void ValidateControlFilePolicy(uint links,bool deletePending,bool directory) {
   if(links!=1||deletePending||directory)throw new IOException("LOCAL_TARGET_CONTROL_LINK_OR_STATE_UNSAFE");
  }
  private void AssertControlFile() {
   NativeEntries++;
   byte[] standard=new byte[24];
   if(!GetFileInformationByHandleEx(Handle,1,standard,24))throw new IOException("LOCAL_TARGET_CONTROL_INFO_UNVERIFIED");
   ValidateControlFilePolicy(BitConverter.ToUInt32(standard,16),standard[20]!=0,standard[21]!=0);
  }
  public void Dispose(){Handle.Dispose();}
 }
}
'@
    $hash=[Security.Cryptography.SHA256]::Create()
    try{$digest=([BitConverter]::ToString($hash.ComputeHash([Text.Encoding]::UTF8.GetBytes($definition)))).Replace('-','').ToLowerInvariant()}finally{$hash.Dispose()}
    if('CatDesk.DevLaneCacheV2.HeldFile' -as [type]){
        if([CatDesk.DevLaneCacheV2.HeldFile]::TemplateSha256 -cne $digest){throw 'LOCAL_TARGET_FILESYSTEM_VERSION_MISMATCH'}
        return
    }
    Add-Type -TypeDefinition $definition.Replace('__TEMPLATE__',$digest)
}
function Read-DevLaneLocalJson([string]$Path,[string]$Owner) {
    $file=[CatDesk.DevLaneCacheV2.HeldFile]::new($Path,$Owner,$false,$false)
    try{return $file.Read()|ConvertFrom-Json}finally{$file.Dispose()}
}
function Write-DevLaneLocalNewJson([string]$Path,[string]$Owner,[object]$Value) {
    $file=[CatDesk.DevLaneCacheV2.HeldFile]::new($Path,$Owner,$true,$true)
    try{$file.WriteNew(($Value|ConvertTo-Json -Depth 6 -Compress));return $file.Identity}finally{$file.Dispose()}
}
function Set-DevLaneLocalPointer([object]$Session,[object]$Value) {
    $temp=Join-Path $Session.directory ('pointer-'+[Guid]::NewGuid().ToString('N')+'.tmp')
    $null=Write-DevLaneLocalNewJson $temp $Session.owner $Value
    [IO.File]::Move($temp,(Join-Path $Session.directory 'current.json'),$true)
}
function Assert-DevLaneLocalEnvironment([object[]]$Entries) {
    foreach($item in $Entries){
        if($item.Name -imatch '\ACARGO_BUILD_JOBS\z'){
            if($item.Name -cne 'CARGO_BUILD_JOBS' -or $item.Value -isnot [string] -or $item.Value -cne '1'){throw 'LOCAL_TARGET_INHERITED_POLICY_REFUSED'}
            continue
        }
        if($item.Name -imatch '\ACARGO_BUILD_' -or ($item.Name -match '^(CARGO_HOME|RUSTUP_HOME|RUSTUP_TOOLCHAIN|RUSTC|RUSTC_WRAPPER|RUSTC_WORKSPACE_WRAPPER|RUSTFLAGS|CARGO_ENCODED_RUSTFLAGS|CARGO_TARGET_DIR|CARGO_TARGET_.*|CARGO_PROFILE_.*)$' -and $item.Value)){throw 'LOCAL_TARGET_INHERITED_POLICY_REFUSED'}
    }
}
function Assert-DevLaneLocalExplicitBasePath([string]$Base,[string]$Profile,[string]$SourceRoot) {
    # Deliberately narrow: one existing dedicated direct child of the user profile.
    # No AppData/OneDrive/project/toolchain path or automatic fallback is accepted.
    $profileFull=[IO.Path]::GetFullPath($Profile).TrimEnd('\')
    $full=[IO.Path]::GetFullPath($Base)
    if($full -cne $Base -or $Base.Contains('/') -or $Base.EndsWith('\') -or
       $Base.Length -gt 300 -or $Base -match '[\x00-\x1f*?]' -or
       $profileFull -match '(?i)(^|[\\/])OneDrive(?:[^\\/]*)?([\\/]|$)' -or
       ![string]::Equals([IO.Path]::GetDirectoryName($Base),$profileFull,[StringComparison]::OrdinalIgnoreCase) -or
       [IO.Path]::GetFileName($Base) -cnotmatch '\ACatDeskBuildCache(?:-[A-Za-z0-9_-]{1,32})?\z') {
        throw 'LOCAL_TARGET_EXPLICIT_BASE_REFUSED'
    }
    $source=[IO.Path]::GetFullPath($SourceRoot).TrimEnd('\')
    if([string]::Equals($full,$source,[StringComparison]::OrdinalIgnoreCase) -or
       $full.StartsWith($source+'\',[StringComparison]::OrdinalIgnoreCase) -or
       $source.StartsWith($full+'\',[StringComparison]::OrdinalIgnoreCase)) {
        throw 'LOCAL_TARGET_EXPLICIT_BASE_OVERLAP_REFUSED'
    }
}
function Get-DevLaneLocalExplicitBase([string]$Base,[string]$Profile,[string]$SourceRoot,[object]$Scope,[string]$Owner,[object]$LocalIdentity) {
    Assert-DevLaneLocalExplicitBasePath $Base $Profile $SourceRoot
    # Required-existing lookup retains the adapter's exact final-path, owner,
    # resident/reparse and volume checks. This function never creates the base.
    $identity=$Scope.Directory($Base,$false)
    if($null -eq $identity -or $identity.Owner -cne $Owner -or $identity.Volume -cne $LocalIdentity.Volume){
        throw 'LOCAL_TARGET_EXPLICIT_BASE_IDENTITY_REFUSED'
    }
    return [pscustomobject]@{Path=$Base;Observation=$identity}
}
function Get-DevLaneLocalFacts([string]$SourceRoot,[string]$LocalTargetBase='') {
    if($env:OS -cne 'Windows_NT' -or $PSVersionTable.PSEdition -cne 'Core' -or ![Environment]::Is64BitProcess){throw 'LOCAL_TARGET_CORE_WINDOWS_REQUIRED'}
    Assert-DevLaneLocalEnvironment @(Get-ChildItem Env:)
    . (Join-Path $PSScriptRoot 'catdesk-dev-local-target-native.ps1')
    Initialize-DevLocalTargetMetadata
    $scope=[CatDesk.DevLocalTarget.ReadOnlyScope]::new()
    try {
        if(![CatDesk.DevLocalTarget.ReadOnlyScope]::Amd64Host()){throw 'LOCAL_TARGET_HOST_REFUSED'}
        $owner=[CatDesk.DevLocalTarget.ReadOnlyScope]::CurrentOwner()
        $profile=[CatDesk.DevLocalTarget.ReadOnlyScope]::KnownFolder($false)
        $local=[CatDesk.DevLocalTarget.ReadOnlyScope]::KnownFolder($true)
        if(![string]::Equals($local.TrimEnd('\'),(Join-Path $profile 'AppData\Local'),[StringComparison]::OrdinalIgnoreCase) -or $local -match '(?i)(^|[\\/])OneDrive(?:[^\\/]*)?([\\/]|$)'){throw 'LOCAL_TARGET_BASE_REFUSED'}
        $localId=$scope.Directory($local,$false);$sourceId=$scope.Directory($SourceRoot,$false)
        if($localId.Owner -cne $owner -or $sourceId.Owner -cne $owner){throw 'LOCAL_TARGET_OWNER_REFUSED'}
        if($LocalTargetBase){
            $selection=Get-DevLaneLocalExplicitBase $LocalTargetBase $profile $SourceRoot $scope $owner $localId
            $local=$selection.Path;$localId=$selection.Observation
        }
        $managed=Join-Path $local 'CatDeskDev'
        if($SourceRoot.TrimEnd('\').StartsWith($managed+'\',[StringComparison]::OrdinalIgnoreCase) -or $managed.StartsWith($SourceRoot.TrimEnd('\')+'\',[StringComparison]::OrdinalIgnoreCase) -or [string]::Equals($managed,$SourceRoot.TrimEnd('\'),[StringComparison]::OrdinalIgnoreCase)){throw 'LOCAL_TARGET_OVERLAP_REFUSED'}
        $cursor=$SourceRoot
        while($cursor){foreach($n in @('config','config.toml')){if($scope.FilePresent((Join-Path $cursor ('.cargo\'+$n)))){throw 'LOCAL_TARGET_CONFIG_REFUSED'}};$parent=[IO.Directory]::GetParent($cursor);$cursor=if($null -eq $parent){$null}else{$parent.FullName}}
        foreach($n in @('config','config.toml')){if($scope.FilePresent((Join-Path $profile ('.cargo\'+$n)))){throw 'LOCAL_TARGET_CONFIG_REFUSED'}}
        $tools=Join-Path $profile '.rustup\toolchains\stable-x86_64-pc-windows-msvc\bin'
        $cargoPath=Join-Path $tools 'cargo.exe';$cargo=$scope.File($cargoPath,134217728,$false)
        $rustcPath=Join-Path $tools 'rustc.exe';$rustc=$scope.File($rustcPath,134217728,$false)
        $lock=$scope.File((Join-Path $SourceRoot 'Cargo.lock'),16777216,$false)
        $null=$scope.File((Join-Path $SourceRoot 'Cargo.toml'),16777216,$false);$null=$scope.File((Join-Path $SourceRoot 'src\main.rs'),16777216,$false)
        if($cargo.Owner -cne $owner -or $rustc.Owner -cne $owner -or $lock.Owner -cne $owner){throw 'LOCAL_TARGET_OWNER_REFUSED'}
        $gitRoot=& git --no-optional-locks -c core.fsmonitor=false -C $SourceRoot rev-parse --show-toplevel
        if($LASTEXITCODE -ne 0 -or ![string]::Equals([IO.Path]::GetFullPath(([string]$gitRoot).Replace('/','\')),$SourceRoot,[StringComparison]::OrdinalIgnoreCase)){throw 'LOCAL_TARGET_GIT_ROOT_REFUSED'}
        $head=& git --no-optional-locks -c core.fsmonitor=false -C $SourceRoot rev-parse --verify HEAD
        if($LASTEXITCODE -ne 0 -or [string]$head -cnotmatch '\A[0-9a-f]{40}\z'){throw 'LOCAL_TARGET_COMMIT_REFUSED'}
        return [pscustomobject]@{owner=$owner;workspaceIdentity=$sourceId.Identity;sourceCommit=[string]$head;local=$local;localIdentity=$localId.Identity;explicitBase=[bool]$LocalTargetBase;volume=$localId.Volume;cargoPath=$cargoPath;cargoSha256=$cargo.Sha256;rustcPath=$rustcPath;rustcSha256=$rustc.Sha256;lockSha256=$lock.Sha256;scope=$scope}
    }catch{$scope.Dispose();throw 'LOCAL_TARGET_METADATA_REFUSED'}
}
function Get-DevLaneLocalJournal([object]$Session,[scriptblock]$Entries) {
    $ids=@{};$count=0
    # Only bounded immediate journal names, never the Cargo target tree.
    $iterator=$null
    if($null -ne $Entries){$iterator=@(& $Entries).GetEnumerator()}
    else{$iterator=([IO.Directory]::EnumerateFileSystemEntries((Join-Path $Session.directory 'journal'))).GetEnumerator()}
    try {while($iterator.MoveNext()) {
        $path=[string]$iterator.Current
        $count++;if($count -gt 256){throw 'LOCAL_TARGET_RECORD_LIMIT_HOLD'}
        $name=[IO.Path]::GetFileName($path)
        if($name -cnotmatch '\A([0-9a-f]{32})\.(start|outcome)\.json\z'){throw 'LOCAL_TARGET_UNKNOWN_HOLD'}
        $id=$Matches[1];$kind=$Matches[2]
        $record=Read-DevLaneLocalJson $path $Session.owner
        if($record.schemaVersion -ne 2 -or $record.operationId -cne $id -or $record.workspaceKey -cne $Session.key -or $record.targetIdentity -cne $Session.targetIdentity){throw 'LOCAL_TARGET_UNKNOWN_HOLD'}
        if(!$ids.ContainsKey($id)){$ids[$id]=@{}}
        $ids[$id][$kind]=$record
    }} finally {if($iterator -is [IDisposable]){$iterator.Dispose()}}
    foreach($id in $ids.Keys){
        if(!$ids[$id].ContainsKey('start') -or !$ids[$id].ContainsKey('outcome')){throw 'LOCAL_TARGET_ORPHAN_START_HOLD'}
        $start=$ids[$id].start;$outcome=$ids[$id].outcome
        if($start.operation -cnotin @('build','verify') -or $outcome.phase -cnotin @('RETURNED','RETURNED_FAILED')){throw 'LOCAL_TARGET_UNKNOWN_HOLD'}
        $codes=@($outcome.exitCodes);$maximum=if($start.operation -ceq 'build'){1}else{3}
        if(!$codes.Count -or $codes.Count -gt $maximum){throw 'LOCAL_TARGET_UNKNOWN_HOLD'}
        for($i=0;$i -lt $codes.Count;$i++){if($codes[$i] -isnot [int] -and $codes[$i] -isnot [long]){throw 'LOCAL_TARGET_UNKNOWN_HOLD'};if($i -lt $codes.Count-1 -and $codes[$i] -ne 0){throw 'LOCAL_TARGET_UNKNOWN_HOLD'}}
        if(($outcome.phase -ceq 'RETURNED' -and ($codes.Count -ne $maximum -or $codes[-1] -ne 0)) -or ($outcome.phase -ceq 'RETURNED_FAILED' -and $codes[-1] -eq 0)){throw 'LOCAL_TARGET_UNKNOWN_HOLD'}
    }
    return [pscustomobject]@{count=$count;records=$ids}
}
function Assert-DevLaneLocalEntries([string]$Directory,[bool]$Namespace,[scriptblock]$Entries) {
    $iterator=$null;$count=0
    if($null -ne $Entries){$iterator=@(& $Entries).GetEnumerator()}
    else{$iterator=([IO.Directory]::EnumerateFileSystemEntries($Directory)).GetEnumerator()}
    try {while($iterator.MoveNext()){
        $count++;if($count -gt 256){throw 'LOCAL_TARGET_ENTRY_LIMIT_HOLD'}
        $name=[IO.Path]::GetFileName([string]$iterator.Current)
        if($Namespace){if($name -cne 'namespace.json' -and $name -cnotmatch '\A[0-9a-f]{64}\z'){throw 'LOCAL_TARGET_UNREGISTERED_CONTENT_HOLD'}}
        elseif($name -cnotin @('owner.json','lease','target','journal','current.json','active.json')){throw 'LOCAL_TARGET_UNREGISTERED_CONTENT_HOLD'}
    }}finally{if($iterator -is [IDisposable]){$iterator.Dispose()}}
}
function Assert-DevLaneLocalDedicatedBaseEntries([string]$Directory,[scriptblock]$Entries) {
    $iterator=$null;$count=0
    if($null -ne $Entries){$iterator=@(& $Entries).GetEnumerator()}
    else{$iterator=([IO.Directory]::EnumerateFileSystemEntries($Directory)).GetEnumerator()}
    try{while($iterator.MoveNext()){
        $count++
        if($count -gt 1 -or [IO.Path]::GetFileName([string]$iterator.Current) -cne 'CatDeskDev'){
            throw 'LOCAL_TARGET_EXPLICIT_BASE_CONTENT_HOLD'
        }
    }}finally{if($iterator -is [IDisposable]){$iterator.Dispose()}}
}
function Get-DevLaneLocalInitializationStages {
    @('UNKNOWN','FILESYSTEM_INITIALIZE','EXPLICIT_BASE_ENTRIES','DIRECTORY_CREATE','DIRECTORY_REOPEN','DIRECTORY_IDENTITY',
      'PARENT_LOOKUP','PARENT_CREATE','PARENT_IDENTITY','NAMESPACE_LOOKUP','NAMESPACE_CREATE',
      'NAMESPACE_CONTROL_WRITE','NAMESPACE_IDENTITY','NAMESPACE_CONTROL_OPEN','NAMESPACE_CONTROL_READ',
      'NAMESPACE_CONTROL_VALIDATE','NAMESPACE_ENTRIES','WORKSPACE_LOOKUP','WORKSPACE_CREATE',
      'TARGET_CREATE','JOURNAL_CREATE','LEASE_CREATE','CURRENT_CREATE','OWNER_CREATE',
      'WORKSPACE_IDENTITY','OWNER_OPEN','OWNER_READ','WORKSPACE_ENTRIES','LEASE_OPEN',
      'TARGET_REOPEN','JOURNAL_REOPEN','REGISTRATION_VALIDATE','CURRENT_READ','ACTIVE_LOOKUP',
      'JOURNAL_READ','BINDING_VALIDATE','STATE_RETURN')
}
function Get-DevLaneLocalInitializationCauses {
    @('UNCLASSIFIED','PATH_OPEN_UNVERIFIED','ATTRIBUTES_UNAVAILABLE','PATH_REPARSE_CLOUD_OR_KIND_UNSAFE',
      'PATH_ALIAS_UNVERIFIED','PHYSICAL_ID_UNAVAILABLE','OWNER_UNAVAILABLE',
      'LOCAL_TARGET_DIRECTORY_NOT_CREATED','LOCAL_TARGET_CREATED_DIRECTORY_UNVERIFIED',
      'LOCAL_TARGET_PARENT_REFUSED','LOCAL_TARGET_NAMESPACE_REFUSED','LOCAL_TARGET_FILE_OPEN_REFUSED',
      'LOCAL_TARGET_FILE_KIND_UNSAFE','LOCAL_TARGET_FILE_ALIAS','LOCAL_TARGET_FILE_OWNER',
      'LOCAL_TARGET_FILE_IDENTITY','LOCAL_TARGET_CONTROL_INFO_UNVERIFIED',
      'LOCAL_TARGET_CONTROL_LINK_OR_STATE_UNSAFE','LOCAL_TARGET_MARKER_TOO_LARGE',
      'LOCAL_TARGET_MARKER_INCOMPLETE','LOCAL_TARGET_REGISTRATION_MISMATCH',
      'LOCAL_TARGET_UNREGISTERED_CONTENT_HOLD','LOCAL_TARGET_ENTRY_LIMIT_HOLD',
      'LOCAL_TARGET_RECORD_LIMIT_HOLD','LOCAL_TARGET_ORPHAN_START_HOLD',
      'LOCAL_TARGET_EXPLICIT_BASE_CONTENT_HOLD','LOCAL_TARGET_EXPLICIT_PARTIAL_BASE_HOLD')
}
function Get-DevLaneLocalInitializationDiagnostic([Exception]$Failure) {
    # Export only fixed vocabulary. The private original ErrorRecord is deliberately
    # not returned here: it can contain user paths and belongs only in private receipts.
    $cursor=$Failure
    for($depth=0;$null -ne $cursor -and $depth -lt 8;$depth++) {
        $candidate=$cursor.Data['DevLaneLocalDiagnostic']
        if($candidate -is [Collections.IDictionary] -and
           $candidate.Contains('schemaVersion') -and $candidate.Contains('stage') -and
           $candidate.Contains('context') -and $candidate.Contains('cause') -and
           $candidate.Contains('handleDisposeFailed') -and
           $candidate.schemaVersion -eq 1 -and $candidate.stage -is [string] -and
           $candidate.context -is [string] -and $candidate.cause -is [string] -and
           $candidate.handleDisposeFailed -is [bool] -and
           $candidate.stage -cin @(Get-DevLaneLocalInitializationStages) -and
           $candidate.context -cin @(Get-DevLaneLocalInitializationStages) -and
           $candidate.cause -cin @(Get-DevLaneLocalInitializationCauses)) {
            return [pscustomobject][ordered]@{schemaVersion=1;hold='LOCAL_TARGET_UNKNOWN_HOLD';
                stage=[string]$candidate.stage;context=[string]$candidate.context;cause=[string]$candidate.cause;
                handleDisposeFailed=[bool]$candidate.handleDisposeFailed;cleanupAuthorized=$false;retryAuthorized=$false}
        }
        $cursor=$cursor.InnerException
    }
    return $null
}
function New-DevLaneLocalInitializationHold([string]$Stage,[Management.Automation.ErrorRecord]$Failure) {
    if($Stage -cnotin @(Get-DevLaneLocalInitializationStages)){$Stage='UNKNOWN'}
    $prior=Get-DevLaneLocalInitializationDiagnostic $Failure.Exception
    $cause='UNCLASSIFIED';$specificStage=$Stage;$private=$Failure
    if($null -ne $prior){
        $cause=$prior.cause;$specificStage=$prior.stage
        $cursor=$Failure.Exception
        for($depth=0;$null -ne $cursor -and $depth -lt 8;$depth++){
            if($cursor.Data.Contains('DevLaneLocalPrivateErrorRecord')){$private=$cursor.Data['DevLaneLocalPrivateErrorRecord'];break}
            $cursor=$cursor.InnerException
        }
    }else{
        $cursor=$Failure.Exception
        for($depth=0;$null -ne $cursor -and $depth -lt 8;$depth++){
            if($cursor.Message -cin @(Get-DevLaneLocalInitializationCauses)){$cause=$cursor.Message;break}
            $cursor=$cursor.InnerException
        }
    }
    $hold=[InvalidOperationException]::new('LOCAL_TARGET_UNKNOWN_HOLD')
    $hold.Data['DevLaneLocalDiagnostic']=[ordered]@{schemaVersion=1;stage=$specificStage;context=$Stage;
        cause=$cause;handleDisposeFailed=$false}
    $hold.Data['DevLaneLocalPrivateErrorRecord']=$private
    return $hold
}
function New-DevLaneLocalDirectoryEntry([string]$Path) {
    [CatDesk.DevLaneCacheV2.HeldFile]::CreateEmptyDirectory($Path)
}
function New-DevLaneLocalDirectory([string]$Path,[object]$Facts) {
    $stage='DIRECTORY_CREATE'
    try {
        New-DevLaneLocalDirectoryEntry $Path
        $stage='DIRECTORY_REOPEN';$id=$Facts.scope.Directory($Path,$false)
        $stage='DIRECTORY_IDENTITY'
        if($id.Owner -cne $Facts.owner -or $id.Volume -cne $Facts.volume){throw 'LOCAL_TARGET_CREATED_DIRECTORY_UNVERIFIED'}
        return $id
    }catch{throw (New-DevLaneLocalInitializationHold $stage $_)}
}
function Open-DevLaneLocalSession([object]$Facts,[string]$Key) {
    $scope=$Facts.scope;$lease=$null;$marker=$null;$namespaceFile=$null;$stage='FILESYSTEM_INITIALIZE'
    $explicit=$null -ne $Facts.PSObject.Properties['explicitBase'] -and [bool]$Facts.explicitBase
    $createdParent=$false
    try {
        Initialize-DevLaneLocalFilesystem
        if($explicit){$stage='EXPLICIT_BASE_ENTRIES';Assert-DevLaneLocalDedicatedBaseEntries $Facts.local}
        $stage='PARENT_LOOKUP';$parent=Join-Path $Facts.local 'CatDeskDev';$id=$scope.Directory($parent,$true)
        if($null -eq $id){$stage='PARENT_CREATE';$id=New-DevLaneLocalDirectory $parent $Facts;$createdParent=$true}
        $stage='PARENT_IDENTITY'
        if($id.Owner -cne $Facts.owner -or $id.Volume -cne $Facts.volume){throw 'LOCAL_TARGET_PARENT_REFUSED'}
        $stage='NAMESPACE_LOOKUP';$base=Join-Path $parent 'ordinary-targets-v2';$baseId=$scope.Directory($base,$true)
        if($null -eq $baseId){
            if($explicit -and !$createdParent){throw 'LOCAL_TARGET_EXPLICIT_PARTIAL_BASE_HOLD'}
            $stage='NAMESPACE_CREATE';$baseId=New-DevLaneLocalDirectory $base $Facts
            $stage='NAMESPACE_CONTROL_WRITE'
            $null=Write-DevLaneLocalNewJson (Join-Path $base 'namespace.json') $Facts.owner @{schemaVersion=2;identity=$baseId.Identity;localIdentity=$Facts.localIdentity}
        }
        $stage='NAMESPACE_IDENTITY'
        if($baseId.Owner -cne $Facts.owner -or $baseId.Volume -cne $Facts.volume){throw 'LOCAL_TARGET_NAMESPACE_REFUSED'}
        $stage='NAMESPACE_CONTROL_OPEN'
        $namespaceFile=[CatDesk.DevLaneCacheV2.HeldFile]::new((Join-Path $base 'namespace.json'),$Facts.owner,$false,$false)
        $stage='NAMESPACE_CONTROL_READ'
        $namespace=$namespaceFile.Read()|ConvertFrom-Json
        $stage='NAMESPACE_CONTROL_VALIDATE'
        if($namespace.schemaVersion -ne 2 -or $namespace.identity -cne $baseId.Identity -or $namespace.localIdentity -cne $Facts.localIdentity){throw 'LOCAL_TARGET_NAMESPACE_REFUSED'}
        $stage='NAMESPACE_ENTRIES'
        Assert-DevLaneLocalEntries $base $true
        $stage='WORKSPACE_LOOKUP';$directory=Join-Path $base $Key;$workspace=$scope.Directory($directory,$true)
        if($null -eq $workspace){
            $stage='WORKSPACE_CREATE';$workspace=New-DevLaneLocalDirectory $directory $Facts
            $stage='TARGET_CREATE';$target=Join-Path $directory 'target';$targetId=New-DevLaneLocalDirectory $target $Facts
            $stage='JOURNAL_CREATE';$null=New-DevLaneLocalDirectory (Join-Path $directory 'journal') $Facts
            $stage='LEASE_CREATE'
            $lease=[CatDesk.DevLaneCacheV2.HeldFile]::new((Join-Path $directory 'lease'),$Facts.owner,$true,$true)
            $stage='CURRENT_CREATE'
            $null=Write-DevLaneLocalNewJson (Join-Path $directory 'current.json') $Facts.owner @{schemaVersion=2;workspaceKey=$Key;targetIdentity=$targetId.Identity;phase='READY';operationId=$null}
            $stage='OWNER_CREATE'
            $null=Write-DevLaneLocalNewJson (Join-Path $directory 'owner.json') $Facts.owner @{schemaVersion=2;workspaceKey=$Key;workspaceIdentity=$Facts.workspaceIdentity;directoryIdentity=$workspace.Identity;targetIdentity=$targetId.Identity;leaseIdentity=$lease.Identity}
        }
        $stage='WORKSPACE_IDENTITY'
        if($workspace.Owner -cne $Facts.owner -or $workspace.Volume -cne $Facts.volume){throw 'LOCAL_TARGET_REGISTRATION_MISMATCH'}
        $stage='OWNER_OPEN'
        $marker=[CatDesk.DevLaneCacheV2.HeldFile]::new((Join-Path $directory 'owner.json'),$Facts.owner,$false,$false)
        $stage='OWNER_READ'
        $registration=$marker.Read()|ConvertFrom-Json
        $stage='WORKSPACE_ENTRIES'
        Assert-DevLaneLocalEntries $directory $false
        # Describe/verify the SAME exclusive lease handle, with no second metadata open.
        $stage='LEASE_OPEN'
        if($null -eq $lease){$lease=[CatDesk.DevLaneCacheV2.HeldFile]::new((Join-Path $directory 'lease'),$Facts.owner,$false,$true)}
        $stage='TARGET_REOPEN';$target=Join-Path $directory 'target';$targetId=$scope.Directory($target,$false)
        $stage='JOURNAL_REOPEN';$journalId=$scope.Directory((Join-Path $directory 'journal'),$false)
        $stage='REGISTRATION_VALIDATE'
        if($registration.schemaVersion -ne 2 -or $registration.workspaceKey -cne $Key -or $registration.workspaceIdentity -cne $Facts.workspaceIdentity -or $registration.directoryIdentity -cne $workspace.Identity -or $registration.targetIdentity -cne $targetId.Identity -or $registration.leaseIdentity -cne $lease.Identity -or $targetId.Owner -cne $Facts.owner -or $journalId.Owner -cne $Facts.owner){throw 'LOCAL_TARGET_REGISTRATION_MISMATCH'}
        $session=[pscustomobject]@{state=$null;target=$target;targetIdentity=$targetId.Identity;directory=$directory;owner=$Facts.owner;key=$Key;lease=$lease;marker=$marker;namespaceFile=$namespaceFile;scope=$scope;guard=$null}
        $stage='CURRENT_READ'
        $state=Read-DevLaneLocalJson (Join-Path $directory 'current.json') $Facts.owner
        $stage='ACTIVE_LOOKUP'
        $active=$scope.FilePresent((Join-Path $directory 'active.json'))
        $stage='JOURNAL_READ'
        $journal=Get-DevLaneLocalJournal $session
        $stage='BINDING_VALIDATE'
        $binding=Get-DevLaneLocalBinding $state $journal $Key $targetId.Identity
        $stage='STATE_RETURN'
        $state|Add-Member active $active;$state|Add-Member orphan $false;$state|Add-Member recordCount $journal.count
        $state|Add-Member terminalVerified $binding.terminalVerified;$state|Add-Member startVerified $binding.startVerified
        $session.state=$state;return $session
    }catch{
        $hold=New-DevLaneLocalInitializationHold $stage $_
        foreach($handle in @($marker,$namespaceFile,$lease)){
            if($null -ne $handle){try{$handle.Dispose()}catch{$hold.Data['DevLaneLocalDiagnostic'].handleDisposeFailed=$true}}
        }
        throw $hold
    }
}
function Start-DevLaneLocalOperation([object]$Session,[string]$Id,[object]$Facts,[string]$Command) {
    $start=@{schemaVersion=2;operationId=$Id;workspaceKey=$Session.key;targetIdentity=$Session.targetIdentity;operation=$Command;sourceCommit=$Facts.sourceCommit;cargoSha256=$Facts.cargoSha256;rustcSha256=$Facts.rustcSha256;lockSha256=$Facts.lockSha256;observedUtc=[DateTimeOffset]::UtcNow.ToString('o')}
    # Create-new guard precedes immutable START and pointer; never auto-clear it.
    $Session.guard=New-DevLaneLocalGuard $Session
    $start.guardIdentity=$Session.guard.Identity
    Write-DevLaneLocalGuard $Session $start
    $null=Write-DevLaneLocalNewJson (Join-Path $Session.directory ('journal\'+$Id+'.start.json')) $Session.owner $start
    Set-DevLaneLocalPointer $Session @{schemaVersion=2;operationId=$Id;workspaceKey=$Session.key;targetIdentity=$Session.targetIdentity;phase='START'}
}
function Complete-DevLaneLocalOperation([object]$Session,[string]$Id,[int[]]$Codes,[bool]$Failed) {
    $phase=if($Failed){'RETURNED_FAILED'}else{'RETURNED'}
    $outcome=@{schemaVersion=2;operationId=$Id;workspaceKey=$Session.key;targetIdentity=$Session.targetIdentity;phase=$phase;exitCodes=$Codes;observedUtc=[DateTimeOffset]::UtcNow.ToString('o');wholeJobOrOutputEofVerified=$false}
    $null=Write-DevLaneLocalNewJson (Join-Path $Session.directory ('journal\'+$Id+'.outcome.json')) $Session.owner $outcome
    # The terminal outcome is immutable, but the pointer is not yet terminal.
    Set-DevLaneLocalPointer $Session @{schemaVersion=2;operationId=$Id;workspaceKey=$Session.key;targetIdentity=$Session.targetIdentity;phase='FINALIZING'}
    Complete-DevLaneLocalGuard $Session
    Set-DevLaneLocalPointer $Session @{schemaVersion=2;operationId=$Id;workspaceKey=$Session.key;targetIdentity=$Session.targetIdentity;phase=$phase}
}
function New-DevLaneLocalGuard([object]$Session) {return [CatDesk.DevLaneCacheV2.HeldFile]::CreateGuard((Join-Path $Session.directory 'active.json'),$Session.owner)}
function Write-DevLaneLocalGuard([object]$Session,[object]$Value) {$Session.guard.WriteNew(($Value|ConvertTo-Json -Depth 6 -Compress))}
function Complete-DevLaneLocalGuard([object]$Session) {$Session.guard.FinalizeOwnedGuard()}
function New-DevLaneLocalBackend {
    return @{
        Facts={param($root,$base) Get-DevLaneLocalFacts $root $base}
        Begin={param($facts,$key) Open-DevLaneLocalSession $facts $key}
        Start={param($session,$id,$facts,$command) Start-DevLaneLocalOperation $session $id $facts $command}
        Run={param($program,$arguments,$result) $PSNativeCommandUseErrorActionPreference=$false;& $program @arguments;$result.exitCode=[int]$LASTEXITCODE;$result.returned=$true}
        Finish={param($session,$id,$codes,$failed) Complete-DevLaneLocalOperation $session $id $codes $failed}
        ImageHash={param($binary,$required) $scope=[CatDesk.DevLocalTarget.ReadOnlyScope]::new();try{$image=$scope.File($binary,1073741824,(!$required));if($null -eq $image){return $null};return $image.Sha256}finally{$scope.Dispose()}}
        Dirty={param($root) $dirty=& git --no-optional-locks -c core.fsmonitor=false -C $root status --porcelain;if($LASTEXITCODE -ne 0){throw 'DEVELOPMENT_GIT_STATUS_UNAVAILABLE'};return @($dirty|Where-Object{![string]::IsNullOrWhiteSpace([string]$_)}).Count -ne 0}
        Advisory={param($root,$threshold) Write-DevLaneStorageAdvisory $root $threshold}
        Close={param($session) if($null -ne $session.guard){$session.guard.Dispose()};$session.marker.Dispose();$session.namespaceFile.Dispose();$session.lease.Dispose();$session.scope.Dispose()}
    }
}
function Invoke-DevLaneLocalTarget([string]$Command,[string]$SourceRoot,[long]$WarningFreeBytes,[string]$LocalTargetBase='') {
    Invoke-DevLaneLocalEngine $Command $SourceRoot $WarningFreeBytes (New-DevLaneLocalBackend) $LocalTargetBase
}
