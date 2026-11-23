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
 DoctorAuthConfiguration={'MCP_BEARER_NOT_CONFIGURED'}
 DoctorSupervisorPolicy={'SUPERVISOR_REGISTERED_MONITOR_STOPPED'}
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
 Require ($report.layers.Count -eq 19) 'doctor omitted a layer'
 Require ((Layer $report 'RELEASE_PAIR').status -ceq 'PASS') 'valid release not observed'
 Require ((Layer $report 'PROJECT_REGISTRY').facts.pairedWakeTargets -eq 1) 'paired metadata not observed'
 Require ((Layer $report 'PROJECT_REGISTRY').facts.workReadiness[0].workState -ceq 'NO_WORK_LEDGER' -and -not (Layer $report 'PROJECT_REGISTRY').facts.workReadiness[0].executionAuthorized) 'missing ledger implied a task grant'
 $registryFixture=Join-Path $projectRoot '.catdesk/projects/projects.json'
 Save $registryFixture @{schemaVersion=1;projects=@(@{projectId='alpha';workspace=$projectRoot;chatgptTargetUrl=$url;chatgptTargetSha256=$digest},@{projectId='beta';workspace=$projectRoot})}
 $unbound=Layer (Invoke-Doctor) 'PROJECT_REGISTRY'
 Require ($unbound.code -ceq 'PROJECT_WEB_TARGET_UNBOUND' -and $unbound.status -ceq 'UNVERIFIED' -and $unbound.facts.unboundProjects -eq 1 -and $unbound.facts.unpairedWebBindings -eq 0) 'unbound project disappeared behind healthy paired rows'
 [IO.File]::WriteAllText($registryFixture,$before[$registryFixture])
 Require ((Layer $report 'WAKE_QUEUE').code -ceq 'WAKE_UNKNOWN_OUTCOME') 'unknown send was hidden'
 Require ((Layer $report 'LOCAL_MCP').status -ceq 'NOT_RUN' -and (Layer $report 'CONNECTOR').status -ceq 'NOT_RUN') 'unexecuted probes claimed success'
 Require ((Layer $report 'MCP_AUTH_CONFIGURATION').code -ceq 'MCP_BEARER_NOT_CONFIGURED' -and (Layer $report 'MCP_AUTH_CONFIGURATION').status -ceq 'FAIL') 'missing bearer did not identify central authentication gap'
 Require ((Layer $report 'PERSISTENT_SUPERVISOR').code -ceq 'SUPERVISOR_REGISTERED_MONITOR_STOPPED' -and (Layer $report 'PERSISTENT_SUPERVISOR').status -ceq 'FAIL') 'registered stopped monitor reported ready'
 foreach($code in @('MCP_BEARER_PROCESS_CONFIGURED','MCP_BEARER_USER_PRESENT_PROCESS_UNSET')){
  $current=$code;$authReport=Invoke-Doctor @{DoctorAuthConfiguration={ $current }.GetNewClosure()};$authLayer=Layer $authReport 'MCP_AUTH_CONFIGURATION'
  Require ($authLayer.status -ceq 'UNVERIFIED' -and $authLayer.code -ceq $code -and -not $authLayer.facts.runningDaemonAuthVerified -and -not $authLayer.facts.clientCatalogVerified -and -not $authLayer.facts.projectTaskGrantVerified -and -not $authLayer.facts.environmentChanged) 'environment configuration claimed daemon, Web or task acceptance'
 }
 foreach($code in @('MCP_BEARER_INVALID','MCP_BEARER_ENV_CONFLICT')){
  $current=$code;Require ((Layer (Invoke-Doctor @{DoctorAuthConfiguration={ $current }.GetNewClosure()}) 'MCP_AUTH_CONFIGURATION').status -ceq 'FAIL') 'invalid or conflicting bearer accepted'
 }
 foreach($code in @('SUPERVISOR_NOT_REGISTERED','SUPERVISOR_REGISTRATION_CONFLICT')){
  $current=$code;Require ((Layer (Invoke-Doctor @{DoctorSupervisorPolicy={ $current }.GetNewClosure()}) 'PERSISTENT_SUPERVISOR').status -ceq 'FAIL') 'missing or foreign supervisor registration accepted'
 }
 $monitor=Layer (Invoke-Doctor @{DoctorSupervisorPolicy={'SUPERVISOR_MONITOR_SIGNAL_PRESENT'}}) 'PERSISTENT_SUPERVISOR'
 Require ($monitor.status -ceq 'UNVERIFIED' -and -not $monitor.facts.rebootRecoveryVerified -and -not $monitor.facts.externalOwnerRecoveryVerified -and -not $monitor.facts.processLivenessVerified) 'mutex signal claimed supervisor recovery acceptance'
 Require ((Layer (Invoke-Doctor @{DoctorSupervisorPolicy={'PRIVATE_MARKER'}}) 'PERSISTENT_SUPERVISOR').code -ceq 'SUPERVISOR_POLICY_UNVERIFIED') 'unknown supervisor metadata projected or accepted'
 $dead=Invoke-Doctor @{Listener={$null}}
 Require ((Layer $dead 'LOCAL_DAEMON').code -ceq 'LOCAL_MCP_LISTENER_MISSING') 'dead backend classification'
 $wrong=Invoke-Doctor @{Listener={[pscustomobject]@{MatchesCanonical=$false}}}
 Require ((Layer $wrong 'LOCAL_DAEMON').code -ceq 'LOCAL_MCP_LISTENER_IDENTITY_MISMATCH') 'wrong owner classification'
 $wrongType=Invoke-Doctor @{Listener={[pscustomobject]@{MatchesCanonical='True'}}}
 Require ((Layer $wrongType 'LOCAL_DAEMON').status -ceq 'FAIL') 'truthy owner proof was accepted'
 $broken=Invoke-Doctor @{CanonicalIdentity={throw 'CATDESK_CANONICAL_HASH_MISMATCH'};DoctorAuthority={[pscustomobject]@{State='LKG_AUTHORITY_MISSING'}}}
 Require ((Layer $broken 'RELEASE_PAIR').code -ceq 'CANONICAL_HASH_MISMATCH') 'hash mismatch classification'
 Require ((Layer $broken 'RECOVERY_AUTHORITY').code -ceq 'LKG_AUTHORITY_MISSING') 'authority classification'
 $busy=Invoke-Doctor @{DoctorAuthority={[pscustomobject]@{State='RELEASE_LIFECYCLE_BUSY'}}}
 $busyAuthority=Layer $busy 'RECOVERY_AUTHORITY'
 Require ($busyAuthority.code -ceq 'RELEASE_LIFECYCLE_BUSY' -and $busyAuthority.status -ceq 'UNVERIFIED') 'busy authority is a separate transient gate'
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
 $import.observations=@(@{layer='CONNECTOR';code='CONNECTOR_TARGET_HTTP_401'},@{layer='LOCAL_MCP';code='LOCAL_MCP_READY'});Save $observation $import
 $refusalReport=Invoke-Doctor -Import;$refusal=Layer $refusalReport 'CONNECTOR'
 Require ($refusal.code -ceq 'CONNECTOR_TARGET_HTTP_401' -and $refusal.status -ceq 'UNVERIFIED' -and $refusal.provenance -ceq 'IMPORTED_ADVISORY') 'target auth refusal was conflated with an upstream outage or live probe'
 Require ($refusal.nextDiagnostic -ceq 'VERIFY_CONFIGURED_LOCAL_BEARER_AND_EXISTING_TUNNEL_OWNER' -and $refusal.facts.reportedTargetHttpStatus -eq 401 -and $refusal.facts.runtimeBearerForwarding -ceq 'UNVERIFIED') 'target auth refusal asserted a missing bearer or suggested a retry'
 Require ((Layer $refusalReport 'LOCAL_MCP').code -ceq 'LOCAL_MCP_READY' -and (Layer $refusalReport 'LOCAL_MCP').status -ceq 'UNVERIFIED' -and (Layer $refusalReport 'WAKE_QUEUE').code -ceq 'WAKE_UNKNOWN_OUTCOME' -and $refusalReport.runtimeStarts -eq 0 -and $refusalReport.networkRequests -eq 0 -and $refusalReport.repairs -eq 0) 'target auth refusal changed independent layers or performed an action'
 $import.observations=@(@{layer='CONNECTOR';code='CONNECTOR_RESPONSE_VERIFIED'});Save $observation $import
 $connectedReport=Invoke-Doctor -Import;$connected=Layer $connectedReport 'CONNECTOR'
 Require ($connected.status -ceq 'UNVERIFIED' -and $connected.provenance -ceq 'IMPORTED_ADVISORY' -and $connected.nextDiagnostic -ceq 'VERIFY_AUTHENTICATED_PROJECT_TASK_SCOPE') 'retained connector response requested polling or claimed current availability'
 Require ($connected.facts.projectTaskAccess -ceq 'UNVERIFIED' -and $connected.facts.providerConversationIdentity -ceq 'UNVERIFIED' -and $connected.facts.taskGrantRequired -and $connected.facts.bearerAndExecutionModeRequired -and (Layer $connectedReport 'LOCAL_MCP').status -ceq 'NOT_RUN' -and (Layer $connectedReport 'WAKE_QUEUE').code -ceq 'WAKE_UNKNOWN_OUTCOME') 'connector success implied task authority or hid independent gates'
 foreach($case in @(
  @{code='CONNECTOR_TOOL_DISCOVERY_UNVERIFIED';next='COMPARE_AFFECTED_CALLER_TOOL_CATALOG_WITHOUT_TASK_REPLAY'},
  @{code='CONNECTOR_TASK_CALL_REPORTED_BLOCKED';next='OBTAIN_ATTRIBUTABLE_PLATFORM_OR_TASK_CALL_EVIDENCE_WITHOUT_REPLAY'})){
  $import.observations=@(@{layer='CONNECTOR';code=$case.code});Save $observation $import
  $capabilityReport=Invoke-Doctor -Import;$capability=Layer $capabilityReport 'CONNECTOR'
  Require ($capability.code -ceq $case.code -and $capability.nextDiagnostic -ceq $case.next) 'connector capability gap collapsed into transport retry or task invocation'
  Require ($capability.status -ceq 'UNVERIFIED' -and $capability.provenance -ceq 'IMPORTED_ADVISORY') 'reported connector capability became current authoritative evidence'
  Require ($capability.facts.reportBasis -ceq 'ADVISORY_NOT_PLATFORM_REJECTION_RECEIPT' -and $capability.facts.projectTaskAccess -ceq 'UNVERIFIED' -and $capability.facts.affectedWebCallerCatalog -ceq 'UNVERIFIED' -and $capability.facts.taskCallOutcome -ceq 'UNESTABLISHED') 'different caller inventory or assistant report asserted task-call cause/outcome'
  Require (-not $capability.facts.appPermissionEstablishesTaskAuthority -and -not $capability.facts.unknownCallReplayAllowed -and -not $capability.facts.alternateTransportRetryAllowed -and $capability.facts.independentProjectsMayContinue) 'app setting authorized retry or blocked independent projects'
  Require ((Layer $capabilityReport 'LOCAL_MCP').status -ceq 'NOT_RUN' -and (Layer $capabilityReport 'WAKE_QUEUE').code -ceq 'WAKE_UNKNOWN_OUTCOME' -and $capabilityReport.runtimeStarts -eq 0 -and $capabilityReport.networkRequests -eq 0 -and $capabilityReport.repairs -eq 0) 'connector report hid existing gates or started active diagnosis'
 }
 foreach($invalidCapability in @(
  @{layer='CONNECTOR';code='CONNECTOR_TASK_CALL_REPORTED_BLOCKED';status='PASS'},
  @{layer='CONNECTOR';code='CONNECTOR_TOOL_DISCOVERY_UNVERIFIED';facts=@{unknownCallReplayAllowed=$true}},
  @{layer='LOCAL_MCP';code='CONNECTOR_TOOL_DISCOVERY_UNVERIFIED'},
  @{layer='LOCAL_MCP';code='CONNECTOR_TARGET_HTTP_401'})){
  $import.observations=@($invalidCapability);Save $observation $import
  $invalidReport=Invoke-Doctor -Import
  Require ((Layer $invalidReport 'OBSERVATIONS').code -ceq 'OBSERVATIONS_INVALID' -and (Layer $invalidReport 'CONNECTOR').code -ceq 'CONNECTOR_NOT_PROBED') 'invalid capability observation partially applied or overrode authority'
 }
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
 Require (Test-DoctorFacadeWorkspaceScope 'C:\fixture\owner' 'c:\FIXTURE\owner\') 'equivalent facade spelling rejected'
 Require (-not (Test-DoctorFacadeWorkspaceScope 'C:\fixture\source' 'C:\fixture\installed')) 'foreign facade treated as installed owner'
 $mismatched=Layer (Invoke-Doctor @{DoctorSupervisorPolicy={'SUPERVISOR_OBSERVER_SCOPE_MISMATCH'}}) 'PERSISTENT_SUPERVISOR'
 Require ($mismatched.status -ceq 'UNVERIFIED' -and $mismatched.code -ceq 'SUPERVISOR_OBSERVER_SCOPE_MISMATCH') 'observer mismatch falsely claimed registration conflict'
 Require ($mismatched.nextDiagnostic -ceq 'RUN_CANONICAL_FACADE_DOCTOR') 'observer mismatch lacks canonical owner diagnostic'
 Require (-not $mismatched.facts.processLivenessVerified -and -not $mismatched.facts.rebootRecoveryVerified) 'observer mismatch granted runtime evidence'
 $conflicting=Layer (Invoke-Doctor @{DoctorSupervisorPolicy={'SUPERVISOR_REGISTRATION_CONFLICT'}}) 'PERSISTENT_SUPERVISOR'
 Require ($conflicting.status -ceq 'FAIL' -and $conflicting.code -ceq 'SUPERVISOR_REGISTRATION_CONFLICT') 'real same-owner registration conflict was weakened'
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
 Require ($human.Count -eq 19 -and ($human -join "`n") -notmatch 'PRIVATE_MARKER|C:\\|https://') 'human summary not safe'
 $doctorTokens=$null;$doctorErrors=$null
 $doctorAst=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot 'catdesk-doctor.ps1'),[ref]$doctorTokens,[ref]$doctorErrors)
 Require ($doctorErrors.Count -eq 0) 'doctor parse failed'
 $authFunction=@($doctorAst.FindAll({param($n)$n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -ceq 'Get-DoctorBearerConfiguration'},$true))
 . ([scriptblock]::Create($authFunction[0].Extent.Text))
 foreach($case in @(
  @('','','MCP_BEARER_NOT_CONFIGURED'),@(('a'*24),'','MCP_BEARER_PROCESS_CONFIGURED'),
  @('',('a'*24),'MCP_BEARER_USER_PRESENT_PROCESS_UNSET'),@(('a'*24),('a'*24),'MCP_BEARER_PROCESS_CONFIGURED'),
  @(('a'*24),('b'*24),'MCP_BEARER_ENV_CONFLICT'),@('short','','MCP_BEARER_INVALID'),
  @('',(('a'*24)+"`n"),'MCP_BEARER_INVALID'),@((([string][char]233)*24),'','MCP_BEARER_INVALID'))){
  Require ((Get-DoctorBearerConfiguration $case[0] $case[1]) -ceq $case[2]) 'bearer configuration classification failed'
 }
 # Distinct projects keep independent source/ownership diagnostics. These are
 # disposable metadata fixtures, never native grant or receipt authority.
 [void][IO.Directory]::CreateDirectory((Join-Path $projectRoot '.catdesk/web-work'))
 $targetAlpha=@{url=$url;digest=$digest;generation=1}
 $urlBeta='https://chatgpt.com/c/doctor-beta';$targetBeta=@{url=$urlBeta;digest=(Digest $urlBeta);generation=1}
 function Work-Fixture([string]$Id,$Target,[string]$Phase){
  $eventId='web-work-'+(Digest ($Id+[char]0+'request-'+$Id));$messageHash=Digest 'PRIVATE_MARKER'
  $record=@{requestId=('request-'+$Id);eventId=$eventId;messageSha256=$messageHash;target=$Target;phase='OPEN';taskAssignment=@{taskId=('task-'+$Id)}}
  Save (Join-Path $projectRoot ('.catdesk/web-work/'+$Id+'.json')) @{schemaVersion=1;projectId=$Id;records=@($record)}
  $receipt=@{schemaVersion=1;evidence='EXACT_USER_MESSAGE_APPENDED';eventId=$eventId;messageDigest=$messageHash;targetGeneration=1;targetDigest=$Target.digest}
  $delivery=@{schemaVersion=1;event=@{schemaVersion=1;projectId=$Id;eventId=$eventId;eventType='engineering_attention';targetGeneration=1;message='PRIVATE_MARKER'};
   target=$Target;phase=$Phase;messageDigest=$messageHash;receipt=if($Phase -ceq 'SENT'){$receipt}else{$null}}
  Save (Join-Path $wakeRoot ('deliveries/'+$eventId+'.json')) $delivery
  return $delivery
 }
 $null=Work-Fixture 'alpha' $targetAlpha 'SUBMITTING'
 $betaDelivery=Work-Fixture 'beta' $targetBeta 'SENT'
 $betaGrantDirectory=Join-Path $projectRoot '.catdesk/project-mcp-tasks/beta/task-beta'
 [void][IO.Directory]::CreateDirectory($betaGrantDirectory)
 # An intentionally unreadable-as-JSON body proves that mere presence does not
 # parse a grant, export its contents, or establish an execution permission.
 [IO.File]::WriteAllText((Join-Path $betaGrantDirectory 'execution-grant.json'),'PRIVATE_MARKER')
 Save (Join-Path $projectRoot '.catdesk/projects/projects.json') @{schemaVersion=1;projects=@(@{projectId='alpha';workspace=$projectRoot;chatgptTargetUrl=$url;chatgptTargetSha256=$digest},@{projectId='beta';workspace=$projectRoot;chatgptTargetUrl=$urlBeta;chatgptTargetSha256=$targetBeta.digest})}
 Save (Join-Path $wakeRoot 'config.json') @{schemaVersion=1;targets=@{alpha=$targetAlpha;beta=$targetBeta}}
 function Read-WorkFixtureRow([string]$Id){
  $layer=Layer (Invoke-Doctor) 'PROJECT_REGISTRY'
  return @($layer.facts.workReadiness|Where-Object{$_.projectId -ceq $Id})[0]
 }
 $alpha=Read-WorkFixtureRow 'alpha'
 $beta=Read-WorkFixtureRow 'beta'
 Require ($alpha.sourceEvidence -ceq 'SUBMITTING_UNKNOWN' -and -not $alpha.taskPreflightCandidate -and -not $alpha.retryAuthorized) 'unknown project gained replay or task admission'
 Require ($beta.sourceEvidence -ceq 'SENT_METADATA_MATCHED' -and $beta.executionGrant -ceq 'PRESENT_UNVERIFIED' -and $beta.taskPreflightCandidate -and -not $beta.executionAuthorized) 'matching source metadata claimed execution authority or hid independent preflight'
 Require ($beta.nextDiagnostic -ceq 'VERIFY_ORIGINAL_ASSIGNMENT_GRANT_AND_OWNER_WITH_EXISTING_LOCAL_PREFLIGHT') 'direct Web diagnostic implied native-model execution was a prerequisite'
 $noCodex=Invoke-Doctor @{DoctorCodexCommand={$null}}
 $withoutNative=@((Layer $noCodex 'PROJECT_REGISTRY').facts.workReadiness|Where-Object{$_.projectId -ceq 'beta'})[0]
 Require ((Layer $noCodex 'CODEX_CLI').code -ceq 'CODEX_CLI_UNAVAILABLE' -and $withoutNative.taskPreflightCandidate -and -not $withoutNative.executionAuthorized -and -not $withoutNative.retryAuthorized) 'optional Codex absence globally blocked direct Web preflight or granted execution'
 $betaLedgerPath=Join-Path $projectRoot '.catdesk/web-work/beta.json';$betaLedgerBytes=[IO.File]::ReadAllBytes($betaLedgerPath)
 foreach($case in @('empty','multiple','schema','limit','assignment','request','missing')){
  $ledger=[Text.Encoding]::UTF8.GetString($betaLedgerBytes)|ConvertFrom-Json
  switch($case){
   'empty' {$ledger.records=@()}
   'multiple' {$ledger.records=@($ledger.records[0],$ledger.records[0])}
   'schema' {$ledger.schemaVersion='1'}
   'limit' {$ledger.records=@(1..257|ForEach-Object{$ledger.records[0]})}
   'assignment' {$ledger.records[0].taskAssignment=$null}
   'request' {$ledger.records[0].requestId='foreign-request'}
  }
  Save $betaLedgerPath $ledger
  if($case -ceq 'missing'){[IO.File]::Delete($betaLedgerPath)}
  $row=Read-WorkFixtureRow 'beta'
  Require (-not $row.taskPreflightCandidate -and -not $row.executionAuthorized -and -not $row.retryAuthorized) ('ledger edge case admitted: '+$case)
  if($case -ceq 'empty'){Require ($row.nextDiagnostic -ceq 'PREPARE_NEW_SCOPED_ASSIGNMENT_WITH_EXISTING_LOCAL_PREFLIGHT') 'fresh direct Web task implied native-model execution was required'}
  if($case -ceq 'missing'){Require ($row.workState -ceq 'NO_WORK_LEDGER' -and $row.nextDiagnostic -ceq 'PREPARE_NEW_SCOPED_ASSIGNMENT_WITH_EXISTING_LOCAL_PREFLIGHT') 'missing ledger selected a native-model-only path'}
  if($case -ceq 'limit'){Require ($row.workState -ceq 'LEDGER_OBSERVATION_LIMIT') 'ledger cap claimed corruption or complete coverage'}
 }
 [IO.File]::WriteAllBytes($betaLedgerPath,$betaLedgerBytes)
 $fixturePins=@{};foreach($file in Get-ChildItem -LiteralPath $projectRoot -File -Recurse){$fixturePins[$file.FullName]=[IO.File]::ReadAllBytes($file.FullName)}
 Save (Join-Path $projectRoot '.catdesk/projects/projects.json') @{schemaVersion=1;projects=@(@{projectId='alpha';workspace=$projectRoot;chatgptTargetUrl=$url;chatgptTargetSha256=$digest},@{projectId='beta';workspace=$projectRoot;chatgptTargetUrl=$urlBeta;chatgptTargetSha256=$targetBeta.digest})}
 Save (Join-Path $wakeRoot 'config.json') @{schemaVersion=1;targets=@{alpha=$targetAlpha;beta=$targetBeta}}
 $independent=Layer (Invoke-Doctor) 'PROJECT_REGISTRY';$rows=@($independent.facts.workReadiness)
 Require ($rows.Count -eq 2 -and $rows[0].sourceEvidence -ceq 'SUBMITTING_UNKNOWN' -and $rows[1].taskPreflightCandidate) 'global unknown project blocked independent metadata preflight'
 Require ($independent.facts.observedRootIsNotServingOwnerProof -and $independent.facts.independentProjectPreflightsNotGloballyBlocked) 'root view or independent scope was overstated'
 foreach($case in @('receipt','project','generation','event-message','schema','missing','malformed')){
  $changed=$betaDelivery|ConvertTo-Json -Depth 15|ConvertFrom-Json
  switch($case){
   'receipt' {$changed.receipt.messageDigest='f'*64}
   'project' {$changed.event.projectId='alpha'}
   'generation' {$changed.target.generation='1'}
   'event-message' {$changed.event.message='changed private body'}
   'schema' {$changed.schemaVersion=$true}
  }
  $path=Join-Path $wakeRoot ('deliveries/'+$changed.event.eventId+'.json')
  Save $path $changed
  if($case -ceq 'missing'){[IO.File]::Delete($path)}
  if($case -ceq 'malformed'){[IO.File]::WriteAllText($path,'PRIVATE_MARKER')}
  $refusal=Read-WorkFixtureRow 'beta'
  Require (-not $refusal.taskPreflightCandidate -and -not $refusal.executionAuthorized -and -not $refusal.retryAuthorized) ('invalid task source accepted: '+$case)
 }
 Save (Join-Path $wakeRoot ('deliveries/'+$betaDelivery.event.eventId+'.json')) $betaDelivery
 foreach($state in @('ATTENTION','CLAIMED','PRIVATE_MARKER')){
  $changed=$betaDelivery|ConvertTo-Json -Depth 15|ConvertFrom-Json;$changed.phase=$state
  Save (Join-Path $wakeRoot ('deliveries/'+$changed.event.eventId+'.json')) $changed
  $refusal=Read-WorkFixtureRow 'beta'
  Require (-not $refusal.taskPreflightCandidate -and -not $refusal.retryAuthorized) 'unresolved delivery enabled task admission'
 }
 Save (Join-Path $wakeRoot ('deliveries/'+$betaDelivery.event.eventId+'.json')) $betaDelivery
 $alphaLedger=Join-Path $projectRoot '.catdesk/web-work/alpha.json'
 [IO.File]::WriteAllText($alphaLedger,'PRIVATE_MARKER')
 $invalidSibling=Layer (Invoke-Doctor) 'PROJECT_REGISTRY'
 Require ($invalidSibling.facts.workReadiness[0].workState -ceq 'METADATA_UNAVAILABLE_OR_INVALID' -and $invalidSibling.facts.workReadiness[1].taskPreflightCandidate) 'malformed sibling ledger disabled the valid project'
 $workHuman=@(& $facade doctor -Workspace $testRoot -ProjectWorkspace $projectRoot -WakeRoot $wakeRoot -TestSeams $seams -Human)
 Require (($workHuman -join "`n") -match 'work=alpha=' -and ($workHuman -join "`n") -notmatch 'PRIVATE_MARKER|https://|C:\\') 'human project-work diagnostics missing or private'
 $many=@();$manyTargets=@{}
 foreach($i in 1..33){
  $id='bounded-'+$i;$u='https://chatgpt.com/c/doctor-bounded-'+$i;$d=Digest $u
  $many+=@{projectId=$id;workspace=$projectRoot;chatgptTargetUrl=$u;chatgptTargetSha256=$d};$manyTargets[$id]=@{url=$u;digest=$d;generation=1}
 }
 Save (Join-Path $projectRoot '.catdesk/projects/projects.json') @{schemaVersion=1;projects=$many}
 Save (Join-Path $wakeRoot 'config.json') @{schemaVersion=1;targets=$manyTargets}
 $bounded=Layer (Invoke-Doctor) 'PROJECT_REGISTRY'
 Require ($bounded.facts.workReadiness.Count -eq 32 -and $bounded.facts.workReadinessCapped -and $bounded.facts.registeredProjects -eq 33) 'bounded project diagnostics claimed full coverage or changed pairing inventory'
 foreach($file in $fixturePins.Keys){[IO.File]::WriteAllBytes($file,$fixturePins[$file])}
 [IO.File]::WriteAllText((Join-Path $wakeRoot 'config.json'),$before[(Join-Path $wakeRoot 'config.json')])
 foreach($file in $fixturePins.Keys){
  $bytes=[IO.File]::ReadAllBytes($file)
  $null=Read-WorkFixtureRow 'beta'
  Require ([Convert]::ToBase64String([IO.File]::ReadAllBytes($file)) -ceq [Convert]::ToBase64String($bytes)) 'project-work diagnostic mutated a fixture'
 }
 $source=[IO.File]::ReadAllText((Join-Path $PSScriptRoot 'catdesk-doctor.ps1'))
 Require ($source -notmatch '(?im)^\s*(Start-Process|Stop-Process|Set-Content|Add-Content|Remove-Item|Invoke-WebRequest|Invoke-RestMethod)\b|\.WriteAll|Store::open\(') 'doctor gained active operations'
 "doctor tests passed: $checks assertions"
}finally{
 $resolved=[IO.Path]::GetFullPath($testRoot)
 if(-not $resolved.StartsWith($tempParent+[IO.Path]::DirectorySeparatorChar,[StringComparison]::OrdinalIgnoreCase) -or [IO.Path]::GetFileName($resolved) -notmatch '^catdesk-doctor-test-[a-f0-9]{32}$'){throw 'fixture cleanup path refused'}
 if(Test-Path -LiteralPath (Join-Path $resolved 'linked-observations')){[IO.Directory]::Delete((Join-Path $resolved 'linked-observations'))}
 if(Test-Path -LiteralPath $resolved){Remove-Item -LiteralPath $resolved -Recurse -Force}
}
