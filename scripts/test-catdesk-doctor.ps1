$ErrorActionPreference='Stop'
Set-StrictMode -Version Latest
$facade=Join-Path $PSScriptRoot '../catdesk.ps1'
$tempParent=[IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd([IO.Path]::DirectorySeparatorChar)
$testRoot=Join-Path $tempParent ('catdesk-doctor-test-'+[Guid]::NewGuid().ToString('N'))
$projectRoot=Join-Path $testRoot 'project';$wakeRoot=Join-Path $testRoot 'wake';$observation=Join-Path $testRoot 'observation.json'
$script:forbidden=0;$checks=0
function Require([bool]$Condition,[string]$Why){if(!$Condition){throw $Why};$script:checks++}
function Save([string]$Path,$Value){[IO.File]::WriteAllText($Path,($Value|ConvertTo-Json -Depth 15 -Compress),[Text.UTF8Encoding]::new($false))}
function Digest([string]$Text){$sha=[Security.Cryptography.SHA256]::Create();try{([BitConverter]::ToString($sha.ComputeHash([Text.Encoding]::UTF8.GetBytes($Text))).Replace('-','')).ToLowerInvariant()}finally{$sha.Dispose()}}
$seams=@{
 CanonicalIdentity={param($root,$hash,$fingerprint) if($fingerprint -cne (Join-Path $root 'target/release/catdesk.exe.sha256')){throw 'wrong doctor fingerprint root'};[pscustomobject]@{Path=(Join-Path $root 'target/release/catdesk.exe');Sha256=('a'*64)}}
 DoctorAuthority={[pscustomobject]@{State='RECOVERY_READY'}}
 LocalMcp={[pscustomobject]@{Port=3200}}
 Listener={[pscustomobject]@{MatchesCanonical=$true}}
 DoctorRuntimeClient={'fixture-client'}
 DoctorCodexCommand={'fixture-command'}
}
$forbid={ $script:forbidden++;throw 'DOCTOR_FORBIDDEN_OPERATION' }
foreach($name in @('Recovery','RecoveryReadiness','LocalMcpReadiness','RuntimeStatus','CodexStatus','StopProcess','WakeRepair','WakeRuntime','WakeMcp','AutostartTasks')){$seams[$name]=$forbid}
function Invoke-Doctor([hashtable]$Overrides=@{},[switch]$Import){
 $parameters=@{Workspace=$testRoot;ProjectWorkspace=$projectRoot;WakeRoot=$wakeRoot;TestSeams=$seams.Clone()}
 foreach($key in $Overrides.Keys){$parameters.TestSeams[$key]=$Overrides[$key]}
 if($Import){$parameters.ObservationFile=$observation}
 $text=@(& $facade doctor @parameters) -join "`n"
 Require ($text -notmatch 'PRIVATE_MARKER|chatgpt\.com/c/|C:\\|fixture-client|fixture-command') 'private data leaked'
 return ($text|ConvertFrom-Json)
}
function Layer($Report,[string]$Name){@($Report.layers|Where-Object{$_.layer -ceq $Name})[0]}
try{
 [void][IO.Directory]::CreateDirectory((Join-Path $projectRoot '.catdesk/projects'))
 [void][IO.Directory]::CreateDirectory((Join-Path $wakeRoot 'queue'))
 [void][IO.Directory]::CreateDirectory((Join-Path $wakeRoot 'deliveries'))
 $url='https://chatgpt.com/c/doctor-fixture';$digest=Digest $url
 Save (Join-Path $projectRoot '.catdesk/projects/projects.json') @{schemaVersion=1;projects=@(@{projectId='alpha';workspace=$projectRoot;chatgptTargetUrl=$url;chatgptTargetSha256=$digest;codexThreadId='PRIVATE_MARKER'})}
 Save (Join-Path $wakeRoot 'config.json') @{schemaVersion=1;targets=@{alpha=@{url=$url;digest=$digest;generation=1}}}
 Save (Join-Path $wakeRoot 'control.json') @{schemaVersion=1;desired='STOPPED'}
 Save (Join-Path $wakeRoot 'deliveries/unknown.json') @{phase='SUBMITTING';message='PRIVATE_MARKER'}
 $before=@{};foreach($path in [IO.Directory]::EnumerateFiles($testRoot,'*',[IO.SearchOption]::AllDirectories)){$before[$path]=[IO.File]::ReadAllText($path)}
 $report=Invoke-Doctor
 Require ($report.command -ceq 'doctor' -and $report.readOnly -and $report.networkRequests -eq 0 -and $report.repairs -eq 0) 'doctor contract invalid'
 Require ($report.layers.Count -eq 17) 'doctor omitted a layer'
 Require ((Layer $report 'RELEASE_PAIR').status -ceq 'PASS') 'valid release not observed'
 Require ((Layer $report 'PROJECT_REGISTRY').facts.pairedWakeTargets -eq 1) 'paired metadata not observed'
 Require ((Layer $report 'WAKE_QUEUE').code -ceq 'WAKE_UNKNOWN_OUTCOME') 'unknown send was hidden'
 Require ((Layer $report 'LOCAL_MCP').status -ceq 'NOT_RUN' -and (Layer $report 'CONNECTOR').status -ceq 'NOT_RUN') 'unexecuted probes claimed success'
 $dead=Invoke-Doctor @{Listener={$null}}
 Require ((Layer $dead 'LOCAL_DAEMON').code -ceq 'LOCAL_MCP_LISTENER_MISSING') 'dead backend classification'
 $wrong=Invoke-Doctor @{Listener={[pscustomobject]@{MatchesCanonical=$false}}}
 Require ((Layer $wrong 'LOCAL_DAEMON').code -ceq 'LOCAL_MCP_LISTENER_IDENTITY_MISMATCH') 'wrong owner classification'
 $wrongType=Invoke-Doctor @{Listener={[pscustomobject]@{MatchesCanonical='True'}}}
 Require ((Layer $wrongType 'LOCAL_DAEMON').status -ceq 'FAIL') 'truthy owner proof was accepted'
 $broken=Invoke-Doctor @{CanonicalIdentity={throw 'CATDESK_CANONICAL_HASH_MISMATCH'};DoctorAuthority={[pscustomobject]@{State='LKG_AUTHORITY_MISSING'}}}
 Require ((Layer $broken 'RELEASE_PAIR').code -ceq 'CANONICAL_HASH_MISMATCH') 'hash mismatch classification'
 Require ((Layer $broken 'RECOVERY_AUTHORITY').code -ceq 'LKG_AUTHORITY_MISSING') 'authority classification'
 Require ((Layer $broken 'PROJECT_REGISTRY').facts.pairedWakeTargets -eq 1 -and (Layer $broken 'CODEX_CLI').status -ceq 'UNVERIFIED') 'release failure stalled independent layers'
 $import=@{schemaVersion=1;source='RETAINED_REPORT';observedUtc='2020-01-01T00:00:00Z';observations=@(
  @{layer='CONNECTOR';code='UPSTREAM_RATE_OR_CONNECTOR_UNAVAILABLE'},@{layer='BROWSER';code='HOME_EDITOR_MISSING'},
  @{layer='LOCAL_MCP';code='LOCAL_MCP_AUTH_REJECTED'},@{layer='NATIVE_SESSION';code='WORKER_GONE'},
  @{layer='LOCAL_PROVIDER';code='LOCAL_PROVIDER_UNAVAILABLE'},@{layer='LOCAL_HEALTH';code='LOCAL_HEALTH_READY'},
  @{layer='WAKE_QUEUE';code='WAKE_METADATA_OBSERVED'},@{layer='CODEX_CLI';code='CODEX_CLI_UNAVAILABLE'})}
 Save $observation $import;$recorded=Invoke-Doctor -Import
 Require ((Layer $recorded 'CONNECTOR').code -ceq 'UPSTREAM_RATE_OR_CONNECTOR_UNAVAILABLE' -and (Layer $recorded 'CONNECTOR').provenance -ceq 'IMPORTED_ADVISORY') '429 conflated with backend failure'
 Require ((Layer $recorded 'LOCAL_HEALTH').status -ceq 'UNVERIFIED') 'retained health became current PASS'
 Require ((Layer $recorded 'BROWSER').code -ceq 'HOME_EDITOR_MISSING' -and (Layer $recorded 'NATIVE_SESSION').code -ceq 'WORKER_GONE') 'independent retained gates lost'
 Require ((Layer $recorded 'WAKE_QUEUE').code -ceq 'WAKE_UNKNOWN_OUTCOME' -and (Layer $recorded 'WAKE_QUEUE').provenance -ceq 'CURRENT_PASSIVE_METADATA' -and (Layer $recorded 'WAKE_QUEUE').observedUtc -ceq $recorded.observedUtc) 'retained positive hid a current unknown send'
 Require ((Layer $recorded 'CODEX_CLI').code -ceq 'CODEX_PRESENT_AUTH_UNVERIFIED') 'retained absence hid current command discovery'
 Require ((Layer $report 'BROWSER').nextDiagnostic -ceq 'operator web-dev-runtime inspect-readiness --project-id <registered-project>') 'uninspected browser lost its first diagnostic'
 foreach($browserCode in @('HOME_EDITOR_MISSING','BROWSER_TARGET_MISSING','BROWSER_DRAFT_UNKNOWN')){
  $import.observations=@(@{layer='BROWSER';code=$browserCode});Save $observation $import
  $browserReport=Invoke-Doctor -Import;$browserLayer=Layer $browserReport 'BROWSER'
  Require ($browserLayer.nextDiagnostic -ceq 'QUALIFIED_READONLY_PROFILE_OBSERVATION_AFTER_OWNER_RECONCILIATION') 'coarse browser refusal requested human classification or repeated automation'
  Require ($browserLayer.code -ceq $browserCode -and $browserLayer.status -ceq 'UNVERIFIED' -and $browserLayer.provenance -ceq 'IMPORTED_ADVISORY' -and $browserReport.runtimeStarts -eq 0 -and $browserReport.networkRequests -eq 0 -and $browserReport.repairs -eq 0) 'operator advice changed evidence or performed an action'
 }
 $import.observations=@(@{layer='BROWSER';code='BROWSER_EXACT_READY'});Save $observation $import
 Require ((Layer (Invoke-Doctor -Import) 'BROWSER').nextDiagnostic -cne 'operator web-dev-runtime login --project-id <registered-project>') 'retained readiness unnecessarily requested operator login'
 foreach($case in @(
  @{code='BROWSER_AUTH_REQUIRED';next='operator web-dev-runtime login --project-id <registered-project>'},
  @{code='BROWSER_OWNER_CLEANUP_UNVERIFIED';next='RECONCILE_EXISTING_BROWSER_OWNER_WITHOUT_MARKER_CLEAR_OR_RELAUNCH'},
  @{code='BROWSER_RESPONSE_LOAD_ERROR';next='ATTRIBUTE_EXACT_WAKE_RESPONSE_WITHOUT_USER_RESEND'})){
  $import.observations=@(@{layer='BROWSER';code=$case.code});Save $observation $import
  $browserReport=Invoke-Doctor -Import;$browserLayer=Layer $browserReport 'BROWSER'
  Require ($browserLayer.nextDiagnostic -ceq $case.next -and $browserLayer.code -ceq $case.code) 'browser failure layers conflated'
  Require ($browserLayer.status -ceq 'UNVERIFIED' -and $browserLayer.provenance -ceq 'IMPORTED_ADVISORY' -and $browserReport.runtimeStarts -eq 0 -and $browserReport.networkRequests -eq 0 -and $browserReport.repairs -eq 0) 'retained browser failure gained authority or performed an action'
 }
 Require ($browserLayer.facts.exchange -ceq 'UNATTRIBUTED' -and -not $browserLayer.facts.userResendAllowed -and -not $browserLayer.facts.genericLoadErrorRetryQualified -and $browserLayer.facts.independentProjectsMayContinue) 'assistant failure authorized USER resend or globally blocked projects'
 $import.observations=@(@{layer='CONNECTOR';code='CONNECTOR_TRANSPORT_NO_HTTP_STATUS'});Save $observation $import
 Require ((Layer (Invoke-Doctor -Import) 'CONNECTOR').code -ceq 'CONNECTOR_TRANSPORT_NO_HTTP_STATUS') 'no-status failure relabeled 429'
 $import.observations=@(@{layer='CONNECTOR';code='CONNECTOR_RESPONSE_VERIFIED'});Save $observation $import
 $connectedReport=Invoke-Doctor -Import;$connected=Layer $connectedReport 'CONNECTOR'
 Require ($connected.status -ceq 'UNVERIFIED' -and $connected.provenance -ceq 'IMPORTED_ADVISORY' -and $connected.nextDiagnostic -ceq 'VERIFY_AUTHENTICATED_PROJECT_TASK_SCOPE') 'retained connector response requested polling or claimed current availability'
 Require ($connected.facts.projectTaskAccess -ceq 'UNVERIFIED' -and $connected.facts.providerConversationIdentity -ceq 'UNVERIFIED' -and $connected.facts.taskGrantRequired -and $connected.facts.bearerAndExecutionModeRequired -and (Layer $connectedReport 'LOCAL_MCP').status -ceq 'NOT_RUN' -and (Layer $connectedReport 'WAKE_QUEUE').code -ceq 'WAKE_UNKNOWN_OUTCOME') 'connector success implied task authority or hid independent gates'
 foreach($invalid in @('PRIVATE_MARKER',('x'*16385),'{"schemaVersion":true}',('{"schemaVersion":1,"source":"RETAINED_REPORT","observedUtc":"2999-01-01","observations":[]}'))){
  [IO.File]::WriteAllText($observation,$invalid)
  Require ((Layer (Invoke-Doctor -Import) 'OBSERVATIONS').code -ceq 'OBSERVATIONS_INVALID') 'invalid imported evidence accepted'
 }
 Save (Join-Path $wakeRoot 'config.json') @{schemaVersion=1;targets=@{alpha=@{url=$url;digest=('b'*64);generation=1}}}
 Require ((Layer (Invoke-Doctor) 'PROJECT_REGISTRY').code -ceq 'PROJECT_WAKE_TARGET_MISMATCH') 'stale pair not observed'
 Save (Join-Path $wakeRoot 'config.json') @{schemaVersion=1;targets=@{alpha=@{url=$url;digest=$digest;generation=1}}}
 Save (Join-Path $wakeRoot 'config.json') @{schemaVersion=999;targets=@{alpha=@{url=$url;digest=$digest;generation=1}}}
 Require ((Layer (Invoke-Doctor) 'PROJECT_REGISTRY').code -ceq 'WAKE_CONFIG_INVALID') 'unknown Wake schema was accepted as paired'
 Save (Join-Path $wakeRoot 'config.json') @{schemaVersion=2;targets=@{alpha=@{url=$url;digest=$digest;generation=1}};retiredTargets=@{retired=@{retirementId='retired-fixture';lastTarget=$null;retiredUtc=1}}}
 Require ((Layer (Invoke-Doctor) 'PROJECT_REGISTRY').facts.pairedWakeTargets -eq 1) 'supported retired-target schema refused'
 Save (Join-Path $wakeRoot 'config.json') @{schemaVersion=2;targets=@{alpha=@{url=$url;digest=$digest;generation=1}};retiredTargets=@{}}
 Require ((Layer (Invoke-Doctor) 'PROJECT_REGISTRY').code -ceq 'WAKE_CONFIG_INVALID') 'schema2 without retirement accepted'
 Save (Join-Path $wakeRoot 'config.json') @{schemaVersion=1;targets=@{alpha=@{url=$url;digest=$digest;generation=1}};retiredTargets=@{retired=@{}}}
 Require ((Layer (Invoke-Doctor) 'PROJECT_REGISTRY').code -ceq 'WAKE_CONFIG_INVALID') 'retirement allowed in legacy schema'
 Save (Join-Path $wakeRoot 'config.json') @{schemaVersion=1;targets=@{alpha=@{url=$url;digest=$digest;generation=1}}}
 Save (Join-Path $projectRoot '.catdesk/projects/projects.json') @{schemaVersion=1;projects=@(@{projectId='alpha';chatgptTargetUrl=$url;chatgptTargetSha256=$digest})}
 Require ((Layer (Invoke-Doctor) 'PROJECT_REGISTRY').code -ceq 'REGISTRY_INVALID') 'missing workspace was accepted as paired'
 Save (Join-Path $projectRoot '.catdesk/projects/projects.json') @{schemaVersion=1;projects=@(@{projectId='invalid.id';workspace=$projectRoot;chatgptTargetUrl=$url;chatgptTargetSha256=$digest})}
 Require ((Layer (Invoke-Doctor) 'PROJECT_REGISTRY').code -ceq 'REGISTRY_INVALID') 'invalid Wake project ID was accepted'
 Save (Join-Path $projectRoot '.catdesk/projects/projects.json') @{schemaVersion=1;projects=@(@{projectId='alpha';workspace=(Join-Path $projectRoot '../elsewhere');chatgptTargetUrl=$url;chatgptTargetSha256=$digest})}
 Require ((Layer (Invoke-Doctor) 'PROJECT_REGISTRY').status -cne 'PASS') 'noncanonical workspace was accepted'
 [IO.File]::WriteAllText((Join-Path $projectRoot '.catdesk/projects/projects.json'),$before[(Join-Path $projectRoot '.catdesk/projects/projects.json')])
 foreach($path in $before.Keys){Require ([IO.File]::ReadAllText($path) -ceq $before[$path]) 'doctor mutated protected fixture data'}
 Require ($script:forbidden -eq 0) 'doctor performed a forbidden probe or mutation'
 . (Join-Path $PSScriptRoot 'catdesk-doctor.ps1')
 foreach($local in @($projectRoot,('\\?\'+$projectRoot),$projectRoot.Replace('\','/'))){
  Require (Test-DoctorWorkspaceSpelling $local) 'canonical local workspace spelling refused'
 }
 foreach($invalidPath in @('relative','\\server\share','\\?\UNC\server\share','\\.\C:\project','C:\project\..\outside','\\?\C:\project\..\outside','C:\project:stream')){
  Require (-not (Test-DoctorWorkspaceSpelling $invalidPath)) 'unsupported workspace spelling accepted'
 }
 Save (Join-Path $projectRoot '.catdesk/projects/projects.json') @{schemaVersion=1;projects=@(@{projectId='alpha';workspace=('\\?\'+$projectRoot);chatgptTargetUrl=$url;chatgptTargetSha256=$digest})}
 Require ((Layer (Invoke-Doctor) 'PROJECT_REGISTRY').facts.pairedWakeTargets -eq 1) 'Rust canonical registry workspace refused'
 [IO.File]::WriteAllText((Join-Path $projectRoot '.catdesk/projects/projects.json'),$before[(Join-Path $projectRoot '.catdesk/projects/projects.json')])
 foreach($nonlocal in @('relative.json','\\server\share\observation.json','https://invalid.example/report')){
  $refused=$false;try{Assert-DoctorLocalPath $nonlocal}catch{$refused=$_.Exception.Message -ceq 'DOCTOR_LOCAL_PATH_REQUIRED'}
  Require $refused 'nonlocal metadata path was not refused before filesystem access'
 }
 $link=Join-Path $testRoot 'linked-observations'
 [void](New-Item -ItemType Junction -Path $link -Target $wakeRoot)
 try{$refused=$false;try{Assert-DoctorLocalPath (Join-Path $link 'control.json')}catch{$refused=$true};Require $refused 'redirected metadata ancestor accepted'}finally{[IO.Directory]::Delete($link)}
 $human=@(& $facade doctor -Workspace $testRoot -ProjectWorkspace $projectRoot -WakeRoot $wakeRoot -Human -TestSeams $seams)
 Require ($human.Count -eq 17 -and ($human -join "`n") -notmatch 'PRIVATE_MARKER|C:\\|https://') 'human summary not safe'
 $source=[IO.File]::ReadAllText((Join-Path $PSScriptRoot 'catdesk-doctor.ps1'))
 Require ($source -notmatch '(?im)^\s*(Start-Process|Stop-Process|Set-Content|Add-Content|Remove-Item|Invoke-WebRequest|Invoke-RestMethod)\b|\.WriteAll|Store::open\(') 'doctor gained active operations'
 "doctor tests passed: $checks assertions"
}finally{
 $resolved=[IO.Path]::GetFullPath($testRoot)
 if(-not $resolved.StartsWith($tempParent+[IO.Path]::DirectorySeparatorChar,[StringComparison]::OrdinalIgnoreCase) -or [IO.Path]::GetFileName($resolved) -notmatch '^catdesk-doctor-test-[a-f0-9]{32}$'){throw 'fixture cleanup path refused'}
 if(Test-Path -LiteralPath (Join-Path $resolved 'linked-observations')){[IO.Directory]::Delete((Join-Path $resolved 'linked-observations'))}
 if(Test-Path -LiteralPath $resolved){Remove-Item -LiteralPath $resolved -Recurse -Force}
}
