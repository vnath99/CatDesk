$ErrorActionPreference='Stop'
Set-StrictMode -Version Latest
# Reproduce the ordinary lifecycle-first load order without running any
# lifecycle command. Its four-argument native helper must not shadow the
# Secure MCP helper's child-only bearer forwarding overload.
$bootstrap=Join-Path $PSScriptRoot 'start-catdesk-stack.ps1'
$bootstrapTokens=$null;$bootstrapErrors=$null
$bootstrapAst=[Management.Automation.Language.Parser]::ParseFile($bootstrap,[ref]$bootstrapTokens,[ref]$bootstrapErrors)
if($bootstrapErrors.Count){throw 'BOOTSTRAP_PARSE_FAILED'}
$bootstrapDefinitions=@($bootstrapAst.FindAll({param($n)$n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -ceq 'Initialize-BoundedNativeProcessType'},$true))
if($bootstrapDefinitions.Count -ne 1){throw 'BOOTSTRAP_HELPER_IDENTITY_INVALID'}
. ([scriptblock]::Create($bootstrapDefinitions[0].Extent.Text))
Initialize-BoundedNativeProcessType
# Extract helpers only. Never enter a setup mode or touch a real profile/runtime.
$setup=Join-Path $PSScriptRoot 'setup-secure-mcp.ps1'
$tokens=$null;$errors=$null
$ast=[Management.Automation.Language.Parser]::ParseFile($setup,[ref]$tokens,[ref]$errors)
if($errors.Count){throw 'SETUP_PARSE_FAILED'}
foreach($name in @('ConvertTo-NativeCommandLineArgument','Initialize-BoundedNativeProcessType','Invoke-BoundedTunnelClient','Get-LocalMcpRuntimeAuthorization','Get-LocalMcpRuntimeUrl','Test-SafeRuntimeRoute','Connect-Runtime','Get-ReconnectMcpProbeState','Invoke-ReconnectRuntimeTransition','Write-ReconnectEvidence')){
    $nodes=@($ast.FindAll({param($n)$n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -ceq $name},$true))
    if($nodes.Count -ne 1){throw 'HELPER_IDENTITY_INVALID'}
    . ([scriptblock]::Create($nodes[0].Extent.Text))
}
function Need([bool]$condition,[string]$reason){if(!$condition){throw $reason}}
function Refuses([scriptblock]$action,[string]$reason){
    try{$null=& $action;throw 'EXPECTED_REFUSAL_MISSING'}catch{if($_.Exception.Message -cne $reason){throw ('WRONG_REFUSAL case='+$checks+' expected='+$reason+' actual='+$_.Exception.Message)}}
}
$names=@('CATDESK_MCP_AUTH_TOKEN','CATDESK_TUNNEL_MCP_AUTHORIZATION','MCP_EXTRA_HEADERS','MCP_DISCOVERY_EXTRA_HEADERS','CONTROL_PLANE_API_KEY')
$before=@{};foreach($name in $names){$before[$name]=[Environment]::GetEnvironmentVariable($name,'Process')}
$root=Join-Path ([IO.Path]::GetTempPath()) ('catdesk-mcp-bearer-'+[Guid]::NewGuid().ToString('N'))
$null=[IO.Directory]::CreateDirectory($root)
$child=Join-Path $root 'verify-owned-child.ps1'
$token='synthetic-native-child-auth-0123456789'
$reference='Authorization: env:CATDESK_TUNNEL_MCP_AUTHORIZATION'
$checks=0
try{
    foreach($name in $names){[Environment]::SetEnvironmentVariable($name,$null,'Process')}
    Need ((Get-LocalMcpRuntimeAuthorization -ForwardBearer $false) -ceq '') 'DEFAULT_CHANGED';$checks++
    Refuses {Get-LocalMcpRuntimeAuthorization -ForwardBearer $true} 'LOCAL_MCP_AUTH_TOKEN_INVALID';$checks++
    foreach($invalid in @('short',(('x'*24)+"`r`n"),(('x'*24)+' '),(([string][char]233)*24))){
        [Environment]::SetEnvironmentVariable('CATDESK_MCP_AUTH_TOKEN',$invalid,'Process')
        Refuses {Get-LocalMcpRuntimeAuthorization -ForwardBearer $true} 'LOCAL_MCP_AUTH_TOKEN_INVALID';$checks++
    }
    [Environment]::SetEnvironmentVariable('CATDESK_MCP_AUTH_TOKEN',$token,'Process')
    foreach($headerName in @('MCP_EXTRA_HEADERS','MCP_DISCOVERY_EXTRA_HEADERS')){
        [Environment]::SetEnvironmentVariable($headerName,'X-Existing-Policy: preserve','Process')
        Refuses {Get-LocalMcpRuntimeAuthorization -ForwardBearer $true} 'LOCAL_MCP_AUTH_HEADER_CONFLICT';$checks++
        [Environment]::SetEnvironmentVariable($headerName,$null,'Process')
    }
    $authorization=Get-LocalMcpRuntimeAuthorization -ForwardBearer $true
    Need ($authorization -ceq ('Bearer '+$token)) 'AUTHORIZATION_INVALID';$checks++
    $childText=@'
$ErrorActionPreference='Stop'
$expected='Bearer synthetic-native-child-auth-0123456789'
$reference='Authorization: env:CATDESK_TUNNEL_MCP_AUTHORIZATION'
$valid=$env:CATDESK_TUNNEL_MCP_AUTHORIZATION -ceq $expected -and $env:MCP_EXTRA_HEADERS -ceq $reference -and $env:MCP_DISCOVERY_EXTRA_HEADERS -ceq $reference
@{childAuthorizationValid=$valid;secretPrinted=$false}|ConvertTo-Json -Compress
if(!$valid){exit 1}
'@
    [IO.File]::WriteAllText($child,$childText,[Text.UTF8Encoding]::new($false))
    $hostImage=(Get-Process -Id $PID).Path
    $r=Invoke-BoundedTunnelClient -Path $hostImage -CommandArgs @('-NoProfile','-File',$child) -TimeoutSeconds 10 -McpAuthorization $authorization
    $facts=$r.Stdout|ConvertFrom-Json
    Need ($facts.childAuthorizationValid -and !$facts.secretPrinted -and !$r.Stdout.Contains($token) -and !$r.Stderr.Contains($token)) 'NATIVE_CHILD_AUTH_FAILED';$checks++
    foreach($name in @('CATDESK_TUNNEL_MCP_AUTHORIZATION','MCP_EXTRA_HEADERS','MCP_DISCOVERY_EXTRA_HEADERS')){
        Need ([string]::IsNullOrEmpty([Environment]::GetEnvironmentVariable($name,'Process'))) 'PARENT_ENVIRONMENT_CHANGED';$checks++
    }
    # Exercise Connect-Runtime's real validation and closed command shape, but
    # replace only the native connect leaf so this test cannot start a runtime.
    $script:connectCalls=0;$script:expectedAuthorization=''
    function Invoke-BoundedTunnelClient {
        param($Path,$CommandArgs,$TimeoutSeconds,$McpAuthorization)
        Need ($McpAuthorization -ceq $script:expectedAuthorization) 'CONNECT_AUTH_SCOPE_WRONG'
        Need (($CommandArgs -join ' ') -notmatch [regex]::Escape($token)) 'SECRET_IN_ARGUMENTS'
        Need ($CommandArgs[0] -ceq 'runtimes' -and $CommandArgs[1] -ceq 'connect') 'UNEXPECTED_COMMAND'
        $script:connectCalls++
    }
    $TunnelId='tunnel_synthetic_existing';$RuntimeAlias='synthetic-existing';$RuntimeCommandTimeoutSeconds=10
    [Environment]::SetEnvironmentVariable('CONTROL_PLANE_API_KEY','synthetic-existing-runtime-key','Process')
    $config=[pscustomobject]@{Found=$true;BindHost='127.0.0.1';Port=3200;Route='abcdefghijklmnopqrstuvwxyz012345'}
    $ForwardLocalMcpBearer=$false;$null=Connect-Runtime -ClientPath $hostImage -McpConfig $config;$checks++
    $ForwardLocalMcpBearer=$true;$script:expectedAuthorization=$authorization;$null=Connect-Runtime -ClientPath $hostImage -McpConfig $config;$checks++
    Need ($script:connectCalls -eq 2) 'CONNECT_COUNT_WRONG'
    [Environment]::SetEnvironmentVariable('CATDESK_MCP_AUTH_TOKEN','invalid','Process')
    Refuses {Connect-Runtime -ClientPath $hostImage -McpConfig $config} 'LOCAL_MCP_AUTH_TOKEN_INVALID'
    Need ($script:connectCalls -eq 2) 'INVALID_AUTH_STARTED_RUNTIME';$checks++
    [Environment]::SetEnvironmentVariable('CATDESK_MCP_AUTH_TOKEN',$token,'Process')
    $config.BindHost='example.com'
    try{$null=Connect-Runtime -ClientPath $hostImage -McpConfig $config;throw 'REMOTE_TARGET_ACCEPTED'}catch{Need ($_.Exception.Message -ne 'REMOTE_TARGET_ACCEPTED') 'REMOTE_TARGET_ACCEPTED'}
    Need ($script:connectCalls -eq 2) 'REMOTE_TARGET_STARTED_RUNTIME';$checks++
    & {
        $profile = Join-Path $root 'fixture-profile.yaml'
        $configFile = Join-Path $root 'fixture-config.toml'
        $intent = Join-Path $root 'fixture-intent.json'
        [IO.File]::WriteAllText($profile,'immutable fixture profile')
        [IO.File]::WriteAllText($configFile,'immutable fixture config')
        try {
            Write-ReconnectEvidence -Path $intent -State 'SECURE_MCP_RECONNECT_INTENT'
            try {
                Write-ReconnectEvidence -Path $intent -State 'replace'
                throw 'INTENT_OVERWRITE_ACCEPTED'
            } catch [IO.IOException] {}
            Need ((Get-Content -LiteralPath $intent -Raw | ConvertFrom-Json).state -ceq 'SECURE_MCP_RECONNECT_INTENT') 'INTENT_WAS_NOT_PRESERVED'
            $script:transitions = [Collections.Generic.List[string]]::new()
            $script:readIteration = 0
            $original=[pscustomobject]@{Id=23;HasExited=$false}
            $plan=[pscustomobject]@{
                Client=$hostImage;Target='fixture-local-target';Original=$original;Profile=$profile
                ProfileDirectory=$root;ProfileHash=(Get-FileHash -LiteralPath $profile).Hash
                ConfigPath=$configFile;ConfigHash=(Get-FileHash -LiteralPath $configFile).Hash
                Config=$config;Local=$null;Canonical=$null;Backend=$null
            }
            function Test-ReconnectBackendOwner { param($Local,$Canonical,$Expected) return $fault -cne 'backend-changed' }
            function Read-ReconnectRuntimeStatus {
                param($ClientPath,$Target)
                $script:readIteration++
                $running = $script:readIteration -ne 2 -or $fault -ceq 'stop-raced'
                $pidValue = if ($script:readIteration -eq 3 -or $fault -ceq 'owner-changed') { 24 } else { 23 }
                $body = switch ($fault) {
                    'auth-challenge' { 'ready (mcp initialize requires auth: redacted fixture)' }
                    'probe-timeout' { 'ready (mcp startup probe timed out: redacted fixture)' }
                    'unknown-probe' { 'ready (unexpected shape)' }
                    'oversized-probe' { 'x' * 1025 }
                    default { 'ready' }
                }
                $directProbe = if ($fault -cne 'fallback-ready') {
                    [pscustomobject]@{ok=$true;status=200;body=$body}
                }
                [pscustomobject]@{process_running=$running;process=@{pid=$pidValue};profile_path=$profile;healthy=$true;ready=$true
                    local=[pscustomobject]@{
                        health=if ($fault -eq 'missing-direct-health') { [pscustomobject]@{} } else { [pscustomobject]@{readyz=$directProbe} }
                        effective_health=[pscustomobject]@{readyz=[pscustomobject]@{ok=$true;status=200;body='ready'}}
                    }
                }
            }
            function Get-ReconnectRuntimeOwner { param($Status,$ClientPath) [pscustomobject]@{Id=[int]$Status.process.pid} }
            function Invoke-BoundedTunnelClient {
                param($Path,$CommandArgs,$TimeoutSeconds)
                Need (($CommandArgs -join ' ') -ceq "runtimes stop $RuntimeAlias") 'WRONG_RUNTIME_STOP'
                [void]$script:transitions.Add('stop')
                $original.HasExited = $fault -cne 'exit-unproven'
            }
            function Connect-Runtime {
                param($ClientPath,$McpConfig,$ExistingProfileDirectory)
                Need ($ExistingProfileDirectory -ceq $root) 'PROFILE_DIRECTORY_CHANGED'
                [void]$script:transitions.Add('connect')
                if ($fault -ceq 'connect-timeout') { throw 'fixture-uncertain-timeout' }
                if ($fault -ceq 'profile-bytes') { [IO.File]::WriteAllText($profile,'changed fixture profile') }
            }
            foreach ($case in @(
                @('', 'SECURE_MCP_RECONNECT_RUNTIME_READY', 'stop,connect'),
                @('owner-changed', 'SECURE_MCP_RECONNECT_OWNER_CHANGED', ''),
                @('backend-changed', 'SECURE_MCP_RECONNECT_OWNER_CHANGED', ''),
                @('stop-raced', 'SECURE_MCP_RECONNECT_STOP_UNPROVEN_NO_RETRY', 'stop'),
                @('exit-unproven', 'SECURE_MCP_RECONNECT_STOP_UNPROVEN_NO_RETRY', 'stop'),
                @('connect-timeout', 'SECURE_MCP_RECONNECT_RESULT_UNPROVEN_NO_RETRY', 'stop,connect'),
                @('profile-bytes', 'SECURE_MCP_RECONNECT_RESULT_UNPROVEN_NO_RETRY', 'stop,connect'),
                @('auth-challenge', 'SECURE_MCP_RECONNECT_MCP_AUTH_REQUIRED_NO_RETRY', 'stop,connect'),
                @('probe-timeout', 'SECURE_MCP_RECONNECT_MCP_PROBE_TIMED_OUT_NO_RETRY', 'stop,connect'),
                @('unknown-probe', 'SECURE_MCP_RECONNECT_MCP_PROBE_UNVERIFIED_NO_RETRY', 'stop,connect'),
                @('oversized-probe', 'SECURE_MCP_RECONNECT_MCP_PROBE_UNVERIFIED_NO_RETRY', 'stop,connect'),
                @('fallback-ready', 'SECURE_MCP_RECONNECT_MCP_PROBE_UNVERIFIED_NO_RETRY', 'stop,connect'),
                @('missing-direct-health', 'SECURE_MCP_RECONNECT_MCP_PROBE_UNVERIFIED_NO_RETRY', 'stop,connect')
            )) {
                $fault = $case[0]
                $script:transitions.Clear()
                $script:readIteration = 0
                $original.HasExited=$false
                $outcome = Invoke-ReconnectRuntimeTransition -Plan $plan
                Need ($outcome -ceq $case[1] -and ($script:transitions -join ',') -ceq $case[2]) "RECONNECT_CASE_FAILED $fault $outcome"
                [IO.File]::WriteAllText($profile,'immutable fixture profile')
            }
        } finally {
            foreach ($path in @($profile,$configFile,$intent)) { [IO.File]::Delete($path) }
        }
    }
    $checks += 14
    @{state='PASS';checks=$checks;actualOwnedChild=$true;officialRuntimeStarts=0;operatorProfileReads=0;productionAuthChanges=0;parentEnvironmentPreserved=$true}|ConvertTo-Json -Compress
}finally{
    foreach($name in $names){[Environment]::SetEnvironmentVariable($name,$before[$name],'Process')}
    if(Test-Path -LiteralPath $child){Remove-Item -LiteralPath $child -Force}
    Remove-Item -LiteralPath $root -Force
}
