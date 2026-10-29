# Additive passive diagnostic library for catdesk.ps1. No startup, repair,
# authentication command, MCP POST, browser inspection or report-file writer.

function Get-DoctorProperty($Value, [string]$Name, $Default = $null) {
    if ($null -eq $Value) { return $Default }
    if ($Value -is [Collections.IDictionary]) {
        if ($Value.Contains($Name)) { return $Value[$Name] }
    } elseif ($Value.PSObject.Properties[$Name]) { return $Value.$Name }
    return $Default
}

function Assert-DoctorLocalPath([string]$Path) {
    if($Path -notmatch '\A[A-Za-z]:[\\/]' -or $Path.Length -gt 4096){throw 'DOCTOR_LOCAL_PATH_REQUIRED'}
    $full=[IO.Path]::GetFullPath($Path);$volume=[IO.Path]::GetPathRoot($full)
    if($full.Substring($volume.Length).Contains(':') -or ([IO.DriveInfo]::new($volume)).DriveType -notin @([IO.DriveType]::Fixed,[IO.DriveType]::Removable)){throw 'DOCTOR_LOCAL_PATH_REQUIRED'}
    # Check ancestors before touching descendants so a junction cannot turn a
    # local metadata read into a remote share/authentication request.
    $cursor=$volume
    foreach($component in ($full.Substring($volume.Length) -split '[\\/]')){
        if(-not $component){continue};$cursor=Join-Path $cursor $component
        try{$entry=Get-Item -LiteralPath $cursor -Force -ErrorAction Stop}
        catch [System.Management.Automation.ItemNotFoundException]{break}
        if(($entry.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0){throw 'DOCTOR_LOCAL_PATH_REQUIRED'}
    }
}

function Test-DoctorWorkspaceSpelling($Path) {
    # Rust canonicalize persists local Windows workspaces with a verbatim drive
    # prefix. Inspect spelling only: do not open another project's directory or
    # interpret a registry row as current filesystem/ownership authority.
    if($Path -isnot [string] -or $Path.Length -gt 4096){return $false}
    $ordinary=$Path
    if($ordinary.StartsWith('\\?\',[StringComparison]::Ordinal)){$ordinary=$ordinary.Substring(4)}
    if($ordinary -notmatch '\A[A-Za-z]:[\\/]' -or $ordinary -match '(?:\A|[\\/])\.\.?(?:[\\/]|\z)'){return $false}
    if($ordinary.IndexOfAny([IO.Path]::GetInvalidPathChars()) -ge 0 -or $ordinary.Substring(2).Contains(':')){return $false}
    try{return [string]::Equals([IO.Path]::GetFullPath($ordinary),$ordinary.Replace('/','\'),[StringComparison]::OrdinalIgnoreCase)}catch{return $false}
}

function Read-DoctorJson([string]$Path, [int]$MaximumBytes = 1048576) {
    Assert-DoctorLocalPath $Path
    $item = Get-Item -LiteralPath $Path -Force -ErrorAction Stop
    if ($item -isnot [IO.FileInfo] -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0 -or $item.Length -gt $MaximumBytes) { throw 'DOCTOR_METADATA_UNAVAILABLE' }
    $stream=[IO.File]::Open($item.FullName,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::ReadWrite)
    try{
        $bytes=New-Object byte[] ($MaximumBytes+1);$count=0
        while($count -le $MaximumBytes){$read=$stream.Read($bytes,$count,$bytes.Length-$count);if($read -eq 0){break};$count+=$read}
        if($count -gt $MaximumBytes){throw 'DOCTOR_METADATA_UNAVAILABLE'}
        $text=[Text.UTF8Encoding]::new($false,$true).GetString($bytes,0,$count).TrimStart([char]0xfeff)
    }finally{$stream.Dispose()}
    return (ConvertFrom-CatDeskLiteralJson $text)
}

function Get-DoctorDefinitions {
    @(
        @('PROCESS_OWNER','CURRENT_OPERATOR','CURRENT_PROCESS','RUN_SUPPORTED_OWNER_DIAGNOSTIC',@('OWNER_NOT_QUALIFIED','OWNER_QUALIFIED')),
        @('RELEASE_PAIR','CANONICAL_RELEASE','target/release/catdesk.exe.sha256','catdesk.ps1 diagnose',@('RELEASE_PAIR_VERIFIED','CANONICAL_BINARY_MISSING','CANONICAL_FINGERPRINT_MISSING','CANONICAL_FINGERPRINT_INVALID','CANONICAL_HASH_MISMATCH','CANONICAL_WORKSPACE_RESOLVE','CANONICAL_IDENTITY_INTERNAL','CANONICAL_IDENTITY')),
        @('RECOVERY_AUTHORITY','RELEASE_MANAGER','.catdesk/release-recovery/current.json','catdesk.ps1 diagnose',@('RECOVERY_AUTHORITY_AVAILABLE','LKG_AUTHORITY_MISSING','LKG_AUTHORITY_AMBIGUOUS_OR_DAMAGED','RECOVERY_RELEASE_AUTHORITY_REQUIRED','RECOVERY_AUTHORITY_UNVERIFIED')),
        @('LOCAL_CONFIG','LOCAL_OPERATOR','CONFIGURED_LOCAL_MCP','catdesk.ps1 diagnose',@('LOCAL_CONFIG_READABLE','LOCAL_CONFIG_UNAVAILABLE')),
        @('LOCAL_DAEMON','CANONICAL_DAEMON','BOUNDED_WINDOWS_LISTENER_INVENTORY','catdesk.ps1 diagnose',@('CANONICAL_LISTENER_PRESENT','LOCAL_MCP_LISTENER_MISSING','LOCAL_MCP_LISTENER_IDENTITY_MISMATCH','LOCAL_OWNER_UNVERIFIED')),
        @('LOCAL_HEALTH','CANONICAL_DAEMON','LOCAL_HEALTH_ENDPOINTS','RUN_BOUNDED_LOCAL_HEALTH_TEST',@('LOCAL_HEALTH_NOT_PROBED','LOCAL_HEALTH_READY','LOCAL_HEALTH_FAILED')),
        @('LOCAL_MCP','CANONICAL_DAEMON','LOCAL_MCP_INITIALIZE_AND_CATALOG','catdesk.ps1 diagnose',@('LOCAL_MCP_NOT_PROBED','LOCAL_MCP_READY','LOCAL_MCP_AUTH_REJECTED','LOCAL_MCP_PROTOCOL_UNREADY')),
        @('OFFICIAL_RUNTIME','EXISTING_OFFICIAL_OWNER','CONFIGURED_NATIVE_RUNTIME_PROFILE','RUN_EXISTING_OWNER_READONLY_STATUS',@('RUNTIME_CLIENT_UNAVAILABLE','RUNTIME_OWNER_UNVERIFIED','RUNTIME_OWNER_ABSENT','RUNTIME_PROFILE_ROUTE_MISMATCH','RUNTIME_LOCAL_READY')),
        @('UPSTREAM_CONTROL_PLANE','EXTERNAL_SERVICE','OFFICIAL_OWNER_POLL_OBSERVATION','WAIT_FOR_CHANGED_UPSTREAM_EVIDENCE',@('UPSTREAM_NOT_PROBED','UPSTREAM_RATE_OR_CONNECTOR_UNAVAILABLE','UPSTREAM_READY','UPSTREAM_UNAVAILABLE')),
        @('CONNECTOR','CHATGPT_CONNECTOR','CONNECTOR_RESPONSE_OBSERVATION','WAIT_FOR_CHANGED_CONNECTOR_EVIDENCE',@('CONNECTOR_NOT_PROBED','UPSTREAM_RATE_OR_CONNECTOR_UNAVAILABLE','CONNECTOR_TRANSPORT_NO_HTTP_STATUS','CONNECTOR_RESPONSE_VERIFIED')),
        @('BROWSER','EXISTING_BROWSER_OWNER','APPROVED_PROFILE_READINESS_OBSERVATION','operator web-dev-runtime inspect-readiness --project-id <registered-project>',@('BROWSER_NOT_INSPECTED','HOME_EDITOR_MISSING','BROWSER_DRAFT_UNKNOWN','BROWSER_TARGET_MISSING','BROWSER_EXACT_READY')),
        @('PROJECT_REGISTRY','PROJECT_REGISTRY','.catdesk/projects/projects.json','operator web projects',@('REGISTRY_METADATA_PAIRED','PROJECT_WAKE_TARGET_MISMATCH','REGISTRY_UNAVAILABLE','REGISTRY_INVALID','WAKE_CONFIG_INVALID')),
        @('WAKE_QUEUE','EXISTING_WAKE_OWNER','WAKE_CONFIG_CONTROL_AND_DELIVERIES','operator web projects',@('WAKE_METADATA_OBSERVED','WAKE_METADATA_UNAVAILABLE','WAKE_METADATA_PARTIAL','WAKE_UNKNOWN_OUTCOME')),
        @('CODEX_CLI','CURRENT_USER_CODEX','TRUSTED_CODEX_COMMAND_DISCOVERY','operator codex diagnose --project-id <registered-project>',@('CODEX_PRESENT_AUTH_UNVERIFIED','CODEX_CLI_UNAVAILABLE')),
        @('NATIVE_SESSION','EXISTING_NATIVE_PROJECT_OWNER','PROJECT_OWNER_LEASE_OBSERVATION','operator codex diagnose --project-id <registered-project>',@('WORKER_OWNER_NOT_PROBED','WORKER_GONE','WORKER_OWNER_VERIFIED')),
        @('LOCAL_PROVIDER','LOCAL_MODEL_OWNER','LOCAL_OLLAMA_EXECUTABLE_DISCOVERY','RUN_BOUNDED_LOCAL_PROVIDER_STATUS',@('LOCAL_PROVIDER_NOT_PROBED','LOCAL_PROVIDER_UNAVAILABLE','LOCAL_PROVIDER_READY')),
        @('OBSERVATIONS','CALLER','IMPORTED_ADVISORY_DOCUMENT','SUPPLY_VALID_REDACTED_OBSERVATIONS',@('OBSERVATIONS_NOT_SUPPLIED','OBSERVATIONS_IMPORTED_ADVISORY','OBSERVATIONS_INVALID'))
    )
}

function Invoke-CatDeskDoctor([string]$ObservationFile = '', [string]$ProjectWorkspace = '', [string]$WakeRoot = '', [switch]$Human) {
    if (-not $ProjectWorkspace) { $ProjectWorkspace = $Workspace }
    if (-not $WakeRoot) { $WakeRoot = Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) 'CatDeskWake' }
    foreach($localPath in @($Workspace,$ProjectWorkspace,$WakeRoot,$ConfigPath,$BuildFingerprintPath)){Assert-DoctorLocalPath $localPath}
    $observedUtc = [DateTimeOffset]::UtcNow.ToString('o')
    $definitions = Get-DoctorDefinitions
    $layers = [ordered]@{}
    foreach ($definition in $definitions) {
        $layers[$definition[0]] = [pscustomobject][ordered]@{
            layer=$definition[0]; status='NOT_RUN'; code=$definition[4][0]; observedUtc=$observedUtc;
            provenance='NOT_RUN'; owner=$definition[1]; evidenceRef=$definition[2]; nextDiagnostic=$definition[3]; facts=[ordered]@{}; retained=@()
        }
    }
    function Set-DoctorLayer([string]$Id,[string]$State,[string]$Code,$Facts = $null) {
        $definition = @($definitions | Where-Object { $_[0] -ceq $Id })[0]
        if ($State -cnotin @('PASS','FAIL','UNVERIFIED','NOT_RUN') -or $Code -cnotin $definition[4]) { throw 'DOCTOR_CODE_INVALID' }
        $layers[$Id].status=$State; $layers[$Id].code=$Code
        $layers[$Id].provenance=if($State -ceq 'NOT_RUN'){'NOT_RUN'}else{'CURRENT_PASSIVE_METADATA'}
        if ($null -ne $Facts) { $layers[$Id].facts=$Facts }
    }
    Set-DoctorLayer 'PROCESS_OWNER' 'UNVERIFIED' 'OWNER_NOT_QUALIFIED'
    Set-DoctorLayer 'LOCAL_HEALTH' 'NOT_RUN' 'LOCAL_HEALTH_NOT_PROBED'
    Set-DoctorLayer 'LOCAL_MCP' 'NOT_RUN' 'LOCAL_MCP_NOT_PROBED'
    Set-DoctorLayer 'UPSTREAM_CONTROL_PLANE' 'NOT_RUN' 'UPSTREAM_NOT_PROBED'
    Set-DoctorLayer 'CONNECTOR' 'NOT_RUN' 'CONNECTOR_NOT_PROBED'
    Set-DoctorLayer 'BROWSER' 'NOT_RUN' 'BROWSER_NOT_INSPECTED'
    Set-DoctorLayer 'NATIVE_SESSION' 'NOT_RUN' 'WORKER_OWNER_NOT_PROBED'
    $identity=$null;$local=$null
    try {
        $identity=Get-LifecycleCanonicalIdentity
        if ((Get-DoctorProperty $identity 'Path') -isnot [string] -or (Get-DoctorProperty $identity 'Sha256') -cnotmatch '\A[a-fA-F0-9]{64}\z') { throw 'CATDESK_CANONICAL_IDENTITY_INTERNAL' }
        Set-DoctorLayer 'RELEASE_PAIR' 'PASS' 'RELEASE_PAIR_VERIFIED'
    }
    catch { $identity=$null; Set-DoctorLayer 'RELEASE_PAIR' 'FAIL' (Get-LifecycleCanonicalFailureGate $_.Exception.Message) }
    try {
        $authority=Invoke-LifecycleSeam -Name 'DoctorAuthority' -Arguments @($Workspace,$ExpectedBuildSha256) -Default {param($root,$hash) Get-CanonicalReleaseRecoveryAssessment -Root $root -ExpectedHash $hash}
        $code=[string](Get-DoctorProperty $authority 'State')
        if($code -ceq 'RECOVERY_READY'){Set-DoctorLayer 'RECOVERY_AUTHORITY' 'PASS' 'RECOVERY_AUTHORITY_AVAILABLE'}
        elseif($code -cin @('LKG_AUTHORITY_MISSING','LKG_AUTHORITY_AMBIGUOUS_OR_DAMAGED','RECOVERY_RELEASE_AUTHORITY_REQUIRED')){Set-DoctorLayer 'RECOVERY_AUTHORITY' 'FAIL' $code}
        else{Set-DoctorLayer 'RECOVERY_AUTHORITY' 'UNVERIFIED' 'RECOVERY_AUTHORITY_UNVERIFIED'}
    } catch { Set-DoctorLayer 'RECOVERY_AUTHORITY' 'UNVERIFIED' 'RECOVERY_AUTHORITY_UNVERIFIED' }
    try { $local=Get-LifecycleLocalMcp; Set-DoctorLayer 'LOCAL_CONFIG' 'PASS' 'LOCAL_CONFIG_READABLE' }
    catch { Set-DoctorLayer 'LOCAL_CONFIG' 'FAIL' 'LOCAL_CONFIG_UNAVAILABLE' }
    if($identity -and $local){
        try {
            $listener=Get-LifecycleListener -LocalMcp $local -Canonical $identity
            if($null -eq $listener){Set-DoctorLayer 'LOCAL_DAEMON' 'FAIL' 'LOCAL_MCP_LISTENER_MISSING'}
            elseif((Get-DoctorProperty $listener 'MatchesCanonical') -is [bool] -and (Get-DoctorProperty $listener 'MatchesCanonical')){Set-DoctorLayer 'LOCAL_DAEMON' 'PASS' 'CANONICAL_LISTENER_PRESENT'}
            else{Set-DoctorLayer 'LOCAL_DAEMON' 'FAIL' 'LOCAL_MCP_LISTENER_IDENTITY_MISMATCH'}
        }catch{Set-DoctorLayer 'LOCAL_DAEMON' 'UNVERIFIED' 'LOCAL_OWNER_UNVERIFIED'}
    }else{Set-DoctorLayer 'LOCAL_DAEMON' 'NOT_RUN' 'LOCAL_OWNER_UNVERIFIED'}
    try {
        $client=Invoke-LifecycleSeam -Name 'DoctorRuntimeClient' -Arguments @() -Default {Find-TunnelClient -ExplicitPath ''}
        if($client){Set-DoctorLayer 'OFFICIAL_RUNTIME' 'UNVERIFIED' 'RUNTIME_OWNER_UNVERIFIED'}
        else{Set-DoctorLayer 'OFFICIAL_RUNTIME' 'FAIL' 'RUNTIME_CLIENT_UNAVAILABLE'}
    }catch{Set-DoctorLayer 'OFFICIAL_RUNTIME' 'UNVERIFIED' 'RUNTIME_OWNER_UNVERIFIED'}
    try{
        $registry=Read-DoctorJson (Join-Path $ProjectWorkspace '.catdesk/projects/projects.json')
        $version=Get-DoctorProperty $registry 'schemaVersion'
        if(($version -isnot [int] -and $version -isnot [long]) -or $version -notin @(1,2)){throw 'REGISTRY_INVALID'}
        if(-not $registry.PSObject.Properties['projects'] -or $registry.projects -isnot [array]){throw 'REGISTRY_INVALID'}
        $projects=$registry.projects;$seen=@{};$web=0;$paired=0;$native=0
        $wake=$null;$invalidWake=$false
        try{
            $wake=Read-DoctorJson (Join-Path $WakeRoot 'config.json');$wakeVersion=Get-DoctorProperty $wake 'schemaVersion'
            $retired=Get-DoctorProperty $wake 'retiredTargets';$retiredCount=0
            if($null -ne $retired){
                if($retired -isnot [pscustomobject]){throw 'WAKE_CONFIG_INVALID'}
                $retiredCount=@($retired.PSObject.Properties).Count
            }
            if(($wakeVersion -isnot [int] -and $wakeVersion -isnot [long]) -or $wakeVersion -notin @(1,2) -or
                ($wakeVersion -eq 1 -and $retiredCount -ne 0) -or ($wakeVersion -eq 2 -and $retiredCount -eq 0) -or $retiredCount -gt 256){$wake=$null;$invalidWake=$true}
        }catch{if($_.Exception.Message -ceq 'WAKE_CONFIG_INVALID'){$wake=$null;$invalidWake=$true}}
        foreach($project in $projects){
            $id=Get-DoctorProperty $project 'projectId'
            if($id -isnot [string] -or $id -cnotmatch '\A[A-Za-z0-9_-]{1,200}\z' -or $seen.ContainsKey($id)){throw 'REGISTRY_INVALID'}
            $projectPath=Get-DoctorProperty $project 'workspace'
            if(-not (Test-DoctorWorkspaceSpelling $projectPath)){throw 'REGISTRY_INVALID'}
            $seen[$id]=$true
            if(Get-DoctorProperty $project 'codexThreadId'){$native++}
            $url=Get-DoctorProperty $project 'chatgptTargetUrl'
            if(-not $url){continue};$web++
            $digest=Get-DoctorProperty $project 'chatgptTargetSha256'
            $target=Get-DoctorProperty (Get-DoctorProperty $wake 'targets') $id
            $generation=Get-DoctorProperty $target 'generation' 0
            if($url -is [string] -and $url.Length -le 512 -and $url -cmatch '\Ahttps://(?:chatgpt\.com|chat\.openai\.com)/(?:c/(?:WEB:)?[A-Za-z0-9_-]{1,200}|g/[A-Za-z0-9_-]{1,200}/c/(?:WEB:)?[A-Za-z0-9_-]{1,200})\z' -and
                $digest -is [string] -and $digest -cmatch '\A[a-f0-9]{64}\z' -and $digest -ceq (Get-LifecycleSha256Hex $url) -and
                (Get-DoctorProperty $target 'url') -ceq $url -and (Get-DoctorProperty $target 'digest') -ceq $digest -and ($generation -is [int] -or $generation -is [long]) -and $generation -gt 0){$paired++}
        }
        $facts=@{registeredProjects=$projects.Count;webBindings=$web;pairedWakeTargets=$paired;nativeOwnerRegistrations=$native;metadataOnly=$true;nativeOwnerLivenessVerified=$false}
        if($invalidWake -and $web -gt 0){Set-DoctorLayer 'PROJECT_REGISTRY' 'FAIL' 'WAKE_CONFIG_INVALID' $facts}
        elseif($paired -eq $web){Set-DoctorLayer 'PROJECT_REGISTRY' 'PASS' 'REGISTRY_METADATA_PAIRED' $facts}
        else{Set-DoctorLayer 'PROJECT_REGISTRY' 'FAIL' 'PROJECT_WAKE_TARGET_MISMATCH' $facts}
    }catch{
        if($_.Exception.Message -ceq 'REGISTRY_INVALID'){Set-DoctorLayer 'PROJECT_REGISTRY' 'FAIL' 'REGISTRY_INVALID'}
        else{Set-DoctorLayer 'PROJECT_REGISTRY' 'UNVERIFIED' 'REGISTRY_UNAVAILABLE'}
    }
    try{
        $control=Read-DoctorJson (Join-Path $WakeRoot 'control.json') 4096
        $desired=Get-DoctorProperty $control 'desired';if($desired -cnotin @('RUNNING','STOPPED','PAUSED')){throw 'WAKE_INVALID'}
        $queued=0;$unknown=0;$sampled=0
        $queue=Join-Path $WakeRoot 'queue';if(Test-Path -LiteralPath $queue){foreach($file in [IO.Directory]::EnumerateFiles($queue,'*.json')){$queued++;if($queued -ge 4096){break}}}
        $deliveries=Join-Path $WakeRoot 'deliveries';if(Test-Path -LiteralPath $deliveries){foreach($file in [IO.Directory]::EnumerateFiles($deliveries,'*.json')){
            if($sampled -ge 64){break};$sampled++;$delivery=Read-DoctorJson $file 65536
            if((Get-DoctorProperty $delivery 'phase') -ceq 'SUBMITTING'){$unknown++}
        }}
        $facts=@{desired=$desired;queuedObserved=$queued;queueCountCapped=($queued -ge 4096);deliverySamples=$sampled;submittingSamples=$unknown;recordedOwnerIsNotLivenessProof=$true}
        $code=if($unknown){'WAKE_UNKNOWN_OUTCOME'}elseif($sampled -ge 64 -or $queued -ge 4096){'WAKE_METADATA_PARTIAL'}else{'WAKE_METADATA_OBSERVED'}
        Set-DoctorLayer 'WAKE_QUEUE' 'UNVERIFIED' $code $facts
    }catch{Set-DoctorLayer 'WAKE_QUEUE' 'UNVERIFIED' 'WAKE_METADATA_UNAVAILABLE'}
    try{
        $codex=Invoke-LifecycleSeam -Name 'DoctorCodexCommand' -Arguments @() -Default {Resolve-TrustedCurrentUserCodexCommand}
        if($codex){Set-DoctorLayer 'CODEX_CLI' 'UNVERIFIED' 'CODEX_PRESENT_AUTH_UNVERIFIED'}else{Set-DoctorLayer 'CODEX_CLI' 'FAIL' 'CODEX_CLI_UNAVAILABLE'}
    }catch{Set-DoctorLayer 'CODEX_CLI' 'FAIL' 'CODEX_CLI_UNAVAILABLE'}
    $ollama=Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) 'Programs/Ollama/ollama.exe'
    if(Test-Path -LiteralPath $ollama -PathType Leaf){Set-DoctorLayer 'LOCAL_PROVIDER' 'UNVERIFIED' 'LOCAL_PROVIDER_NOT_PROBED'}else{Set-DoctorLayer 'LOCAL_PROVIDER' 'UNVERIFIED' 'LOCAL_PROVIDER_UNAVAILABLE'}
    if($ObservationFile){
        try{
            $document=Read-DoctorJson $ObservationFile 16384
            $names=@($document.PSObject.Properties.Name|Sort-Object)
            if(($names -join ',') -cne 'observations,observedUtc,schemaVersion,source' -or ($document.schemaVersion -isnot [int] -and $document.schemaVersion -isnot [long]) -or $document.schemaVersion -ne 1 -or $document.source -cnotin @('OPERATOR_REPORTED','RETAINED_REPORT')){throw 'INVALID_OBSERVATION'}
            $stamp=$document.observedUtc;$instant=[DateTimeOffset]::MinValue
            if($stamp -isnot [string] -or $stamp.Length -gt 40 -or $stamp -cnotmatch '\A\d{4}-\d{2}-\d{2}(?:T[0-9:.]+(?:Z|[+-][0-9:]+))?\z' -or
                -not [DateTimeOffset]::TryParse($stamp,[Globalization.CultureInfo]::InvariantCulture,[Globalization.DateTimeStyles]::AssumeUniversal,[ref]$instant) -or $instant -gt [DateTimeOffset]::UtcNow.AddMinutes(5)){throw 'INVALID_OBSERVATION'}
            if($document.observations -isnot [array]){throw 'INVALID_OBSERVATION'}
            $observations=$document.observations;if($observations.Count -gt 32){throw 'INVALID_OBSERVATION'};$imported=@{};$pending=@()
            foreach($observation in $observations){
                if((@($observation.PSObject.Properties.Name|Sort-Object) -join ',') -cne 'code,layer'){throw 'INVALID_OBSERVATION'}
                $id=$observation.layer;$code=$observation.code
                if($id -isnot [string] -or $code -isnot [string] -or -not $layers.Contains($id) -or $id -ceq 'OBSERVATIONS' -or $imported.ContainsKey($id)){throw 'INVALID_OBSERVATION'}
                $definition=@($definitions|Where-Object{$_[0] -ceq $id})[0];if($code -cnotin $definition[4]){throw 'INVALID_OBSERVATION'}
                $imported[$id]=$true;$pending+=@{layer=$id;code=$code}
            }
            # Validate the whole document before importing any item. Retained
            # evidence can explain an unprobed layer, never supersede live facts.
            foreach($observation in $pending){
                $layer=$layers[$observation.layer];$layer.retained=@([pscustomobject]@{code=$observation.code;observedUtc=$stamp;source=$document.source})
                if($layer.provenance -ceq 'NOT_RUN'){$layer.status='UNVERIFIED';$layer.code=$observation.code;$layer.observedUtc=$stamp;$layer.provenance='IMPORTED_ADVISORY'}
            }
            Set-DoctorLayer 'OBSERVATIONS' 'UNVERIFIED' 'OBSERVATIONS_IMPORTED_ADVISORY'
        }catch{Set-DoctorLayer 'OBSERVATIONS' 'FAIL' 'OBSERVATIONS_INVALID'}
    }else{Set-DoctorLayer 'OBSERVATIONS' 'NOT_RUN' 'OBSERVATIONS_NOT_SUPPLIED'}
    $report=[pscustomobject][ordered]@{
        schemaVersion=1;command='doctor';readOnly=$true;observedUtc=$observedUtc;networkRequests=0;runtimeStarts=0;repairs=0;
        projectScope=if($ProjectWorkspace -ceq $Workspace){'RELEASE_WORKSPACE'}else{'EXPLICIT_OBSERVATION_WORKSPACE'};
        installedWebAcceptance='UNVERIFIED';layers=@($layers.Values)
    }
    if($Human){foreach($layer in $report.layers){'{0}: {1} {2}; next={3}' -f $layer.layer,$layer.status,$layer.code,$layer.nextDiagnostic}}
    else{$report|ConvertTo-Json -Depth 12 -Compress}
}
