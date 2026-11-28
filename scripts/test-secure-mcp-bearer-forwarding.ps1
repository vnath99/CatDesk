$ErrorActionPreference='Stop'
Set-StrictMode -Version Latest
# Reproduce the ordinary lifecycle-first load order without running any
# lifecycle command. Its four-argument native helper must not shadow the
# Secure MCP helper's child-only bearer forwarding overload.
$bootstrap=Join-Path $PSScriptRoot 'start-catdesk-stack.ps1'
$lifecycleEngine=$bootstrap
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
foreach($name in @('ConvertTo-NativeCommandLineArgument','Initialize-BoundedNativeProcessType','Invoke-BoundedTunnelClient','Get-LocalMcpRuntimeAuthorization','Get-LocalMcpRuntimeUrl','Open-LocalMcpBearerFile','Open-LocalMcpBearerClient','Open-LocalMcpBearerConfig','Test-SafeRuntimeRoute','Connect-Runtime','Get-ReconnectMcpProbeState','Get-ReconnectDisposition')){
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
$ConfigPath=Join-Path $root 'bearer-config.toml'
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
        if ($ForwardLocalMcpBearer) {
            $targetIndex = [array]::IndexOf($CommandArgs, '--mcp-server-url')
            Need ($targetIndex -ge 0 -and $CommandArgs[$targetIndex + 1] -ceq $expectedTarget) 'CONNECT_TARGET_CHANGED'
            $denied = $false
            try {
                $writer = [IO.File]::Open($ConfigPath, [IO.FileMode]::Open, [IO.FileAccess]::Write, [IO.FileShare]::ReadWrite)
                $writer.Dispose()
            } catch [IO.IOException] { $denied = $true }
            Need $denied 'CONFIG_UNPINNED_DURING_CONNECT'
        }
        $script:connectCalls++
    }
    $TunnelId='tunnel_synthetic_existing';$RuntimeAlias='synthetic-existing';$RuntimeCommandTimeoutSeconds=10
    [Environment]::SetEnvironmentVariable('CONTROL_PLANE_API_KEY','synthetic-existing-runtime-key','Process')
    $config=[pscustomobject]@{Found=$true;BindHost='127.0.0.1';Port=3200;Route='abcdefghijklmnopqrstuvwxyz012345'}
    $ForwardLocalMcpBearer=$false;$null=Connect-Runtime -ClientPath $hostImage -McpConfig $config;$checks++
    $validConfig = @'
[mcp] # local endpoint
bind_host = "127.0.0.1"
port = 43123
route_id = "abcdefghijklmnopqrstuvwxyz012345"
[other] # unrelated fixture
port = 43124
route_id = "other-other-other-other-other"
'@
    $checks = & {
    $UserProfile = $root
    $fixtureFiles = @()
    $fixtureDirectories = @()
    try {
    foreach ($relative in @('.catdesk','.catdesk\tools','.catdesk\tools\tunnel-client','.catdesk\tools\tunnel-client\current','target','target\release')) {
        $path = Join-Path $root $relative
        $null = [IO.Directory]::CreateDirectory($path)
        $fixtureDirectories += $path
    }
    $ConfigPath = Join-Path $root '.catdesk\config.toml'
    $fixtureClient = Join-Path $root '.catdesk\tools\tunnel-client\current\tunnel-client.exe'
    $fixtureImage = Join-Path $root 'target\release\catdesk.exe'
    $fixtureEngine = Join-Path $root 'fixture-engine.ps1'
    $alternateConfig = Join-Path $root 'other-config.toml'
    $fixtureFiles = @($ConfigPath,$fixtureClient,$fixtureImage,"$fixtureImage.sha256",$fixtureEngine,$alternateConfig)
    [IO.File]::WriteAllText($fixtureImage,'synthetic CatDesk image')
    $fixtureImageHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $fixtureImage).Hash
    [IO.File]::WriteAllText("$fixtureImage.sha256",$fixtureImageHash)
    [IO.File]::WriteAllText($fixtureClient,'synthetic pinned tunnel client')
    $fixtureDigest = 'fcc85a69ec0ad82518e4f8964f60c45e31787957782a0fc9c1b0c44e82d61b9b'
    function Get-FileHash {
        param($Algorithm,$LiteralPath,$InputStream)
        if ($null -ne $InputStream) {
            Need ($InputStream.Name -ceq $fixtureClient) 'UNEXPECTED_CLIENT_PIN'
            return [pscustomobject]@{Hash=$fixtureDigest}
        }
        Microsoft.PowerShell.Utility\Get-FileHash -Algorithm $Algorithm -LiteralPath $LiteralPath
    }
    # Retain the real endpoint parser, canonical executable and fingerprint reader.
    # Only the Windows listener inventory and the native client hash are synthetic.
    $fixtureLifecycle = @'
param($ConfigPath)
. $bootstrap -Workspace $root -ConfigPath $ConfigPath
$script:BootstrapSeams.Listener = {
    param($port,$canonical)
    if ($port -ne 43123) { return $null }
    [pscustomobject]@{MatchesCanonical=$fixtureMatches;OwnerMode='BACKEND_ONLY';ListenAddresses=@($fixtureAddress)}
}
'@
    [IO.File]::WriteAllText($fixtureEngine,$fixtureLifecycle)
    $lifecycleEngine = $fixtureEngine
    $fixtureMatches = $true
    $fixtureAddress = '127.0.0.1'
    $expectedTarget = 'http://127.0.0.1:43123/abcdefghijklmnopqrstuvwxyz012345/mcp'
    [IO.File]::WriteAllText($ConfigPath, $validConfig)
    $ForwardLocalMcpBearer=$true;$script:expectedAuthorization=$authorization
    $null = Connect-Runtime -ClientPath $fixtureClient
    $checks++
    Need ($script:connectCalls -eq 2) 'CONNECT_COUNT_WRONG'
    [Environment]::SetEnvironmentVariable('CATDESK_MCP_AUTH_TOKEN','invalid','Process')
    Refuses {Connect-Runtime -ClientPath $fixtureClient -McpConfig $config} 'LOCAL_MCP_AUTH_TOKEN_INVALID'
    Need ($script:connectCalls -eq 2) 'INVALID_AUTH_STARTED_RUNTIME';$checks++
    [Environment]::SetEnvironmentVariable('CATDESK_MCP_AUTH_TOKEN',$token,'Process')
    Refuses { Connect-Runtime -ClientPath $hostImage } 'LOCAL_MCP_AUTH_SCOPE_INVALID'
    [IO.File]::WriteAllText($alternateConfig,$validConfig)
    foreach ($case in @(
        @{Mode='connect';ConfigPath=$alternateConfig;TunnelClientPath=$fixtureClient},
        @{Mode='start_all';TunnelClientPath=$fixtureClient}
    )) {
        $wizard = (& $setup @case -ForwardLocalMcpBearer) | ConvertFrom-Json
        Need ($wizard.Wizard -ceq 'catdesk-secure-mcp-setup' -and $wizard.Mode -ceq $case.Mode -and
            $wizard.Errors.Count -eq 1 -and $wizard.Errors[0] -ceq 'LOCAL_MCP_AUTH_SCOPE_INVALID' -and
            $wizard.MutationsPerformed.Count -eq 0 -and -not $wizard.Client.Found -and $null -eq $wizard.LocalMcp.ConfigFound) 'PUBLIC_REFUSAL_LOST_WIZARD_RESULT'
        $checks++
    }
    $canonicalConfig = $ConfigPath
    try { $ConfigPath = $alternateConfig; Refuses { Connect-Runtime -ClientPath $fixtureClient } 'LOCAL_MCP_AUTH_SCOPE_INVALID' }
    finally { $ConfigPath = $canonicalConfig }
    $acceptedDigest = $fixtureDigest
    $fixtureDigest = '0' * 64
    Refuses { Connect-Runtime -ClientPath $fixtureClient } 'LOCAL_MCP_AUTH_SCOPE_INVALID'
    $fixtureDigest = $acceptedDigest
    [IO.File]::WriteAllText("$fixtureImage.sha256",'0' * 64)
    Refuses { Connect-Runtime -ClientPath $fixtureClient } 'LOCAL_MCP_AUTH_SCOPE_INVALID'
    [IO.File]::WriteAllText("$fixtureImage.sha256",$fixtureImageHash)
    $fixtureMatches = $false
    Refuses { Connect-Runtime -ClientPath $fixtureClient } 'LOCAL_MCP_AUTH_SCOPE_INVALID'
    $fixtureMatches = $true
    $fixtureAddress = '::1'
    Refuses { Connect-Runtime -ClientPath $fixtureClient } 'LOCAL_MCP_AUTH_SCOPE_INVALID'
    $fixtureAddress = '127.0.0.1'
    $checks += 6
    foreach ($invalidConfig in @(
        $validConfig.Replace('127.0.0.1','localhost'),
        $validConfig.Replace('127.0.0.1','example.invalid'),
        $validConfig.Replace('port = 43123','port = 43123x'),
        $validConfig.Replace('port = 43123','port = 043123'),
        $validConfig.Replace('port = 43123','port = 43124'),
        $validConfig.Replace('port = 43123',('port = 43123'+[Environment]::NewLine+'"port" = 43124')),
        $validConfig.Replace('port = 43123',('port = 43123'+[Environment]::NewLine+"'route_id' = 'other-route-that-looks-valid-01234'")),
        $validConfig.Replace('port = 43123',"port = 43123"+[Environment]::NewLine+"port = 43124"),
        $validConfig.Replace('[mcp]', '[unrelated]'),
        $validConfig.Replace('[other]', '[mcp]'),
        $validConfig.Replace('[other]', '[other'),
        $validConfig.Replace('abcdefghijklmnopqrstuvwxyz012345','redacted-fixture-route-1234567890')
    )) {
        [IO.File]::WriteAllText($ConfigPath, $invalidConfig)
        Refuses { Connect-Runtime -ClientPath $fixtureClient -McpConfig $config } 'LOCAL_MCP_AUTH_SCOPE_INVALID'
        $checks++
    }
    [IO.File]::Delete($ConfigPath)
    Refuses { Connect-Runtime -ClientPath $fixtureClient -McpConfig $config } 'LOCAL_MCP_AUTH_SCOPE_INVALID'
    Need ($script:connectCalls -eq 2) 'INVALID_TARGET_STARTED_RUNTIME'
    $checks += 2
    } finally {
        foreach ($path in $fixtureFiles) { [IO.File]::Delete($path) }
        for ($index = $fixtureDirectories.Count - 1; $index -ge 0; $index--) { [IO.Directory]::Delete($fixtureDirectories[$index]) }
    }
        $checks
    }
    # Enter the real reconnect controller with its original journal intact and no client or owner.
    $checks += & {
        $definition = $ast.Find({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and
            $node.Name -ceq 'Invoke-ExistingRuntimeReconnect'}, $true)
        . ([scriptblock]::Create($definition.Extent.Text))
        $ReconnectWorkspace = $root
        $UserProfile = $root
        $ConfigPath = Join-Path $UserProfile '.catdesk\config.toml'
        $TunnelClientPath = Join-Path $UserProfile '.catdesk\tools\tunnel-client\current\tunnel-client.exe'
        $RuntimeAlias = $ProfileName = 'fixture'
        $TunnelId = 'tunnel_' + ('0' * 32)
        $ExpectedRuntimeOwnerPid = 23
        $ExpectedRuntimeOwnerCreatedUtc = '2026-11-28T10:00:00.0000000Z'
        $ExpectedBackendSha256 = $ExpectedTunnelClientSha256 = 'a' * 64
        $LegacyDirectManaged = $false
        $TransportMode = 'openai_secure_tunnel'
        $ForwardLocalMcpBearer = $true
        $ExecuteReconnect = $true
        $dir = [IO.Directory]::CreateDirectory((Join-Path $root '.catdesk\recovery-evidence'))
        $id = "$RuntimeAlias\0$TunnelId\0$ExpectedRuntimeOwnerPid\0$ExpectedRuntimeOwnerCreatedUtc"
        $hash = [Security.Cryptography.SHA256]::Create()
        try { $key = [BitConverter]::ToString($hash.ComputeHash([Text.Encoding]::UTF8.GetBytes($id))).Replace('-','').ToLowerInvariant() }
        finally { $hash.Dispose() }
        foreach ($suffix in @('.json','.result.json')) {
            $path = Join-Path $dir.FullName ("secure-mcp-reconnect-" + $key + $suffix)
            [IO.File]::WriteAllText($path,'original fixture evidence')
            try {
                Need ((Invoke-ExistingRuntimeReconnect) -ceq 'SECURE_MCP_RECONNECT_ATTEMPT_EXISTS_NO_RETRY') 'PUBLIC_PATH_LOST_HISTORICAL_REFUSAL'
                Need ([IO.File]::ReadAllText($path) -ceq 'original fixture evidence') 'PUBLIC_PATH_CHANGED_ORIGINAL_JOURNAL'
            } finally { [IO.File]::Delete($path) }
        }
        4
    }
    $intent = Join-Path $root 'existing-intent.json'
    $receipt = Join-Path $root 'existing-result.json'
    try {
        $owner = [pscustomobject]@{Id=23;HasExited=$false}
        $status = [pscustomobject]@{process_running=$true;process=@{pid=23};healthy=$true;ready=$true
            local=@{effective_health=@{readyz=@{ok=$true;status=200;body='ready'}}} }
        foreach ($case in @(
            @('ready', 'SECURE_MCP_RECONNECT_EXISTING_RUNTIME_READY'),
            @('ready (mcp initialize requires auth: fixture)', 'SECURE_MCP_RECONNECT_MCP_AUTH_REQUIRED_NO_RETRY'),
            @('ready (mcp startup probe timed out: fixture)', 'SECURE_MCP_RECONNECT_MCP_PROBE_TIMED_OUT_NO_RETRY'),
            @('unknown', 'SECURE_MCP_RECONNECT_MCP_PROBE_UNVERIFIED_NO_RETRY'),
            @(('x' * 1025), 'SECURE_MCP_RECONNECT_MCP_PROBE_UNVERIFIED_NO_RETRY')
        )) {
            $status.local = [pscustomobject]@{health=[pscustomobject]@{readyz=[pscustomobject]@{ok=$true;status=200;body=$case[0]}}}
            $observed = Get-ReconnectDisposition $status $owner $intent $receipt $false
            Need ($observed -ceq $case[1]) 'READ_ONLY_CLASSIFICATION_WRONG'
            $expectedExecute = if ($case[0] -ceq 'ready') { $case[1] } else { 'SECURE_MCP_RECONNECT_CONDITIONAL_STOP_UNAVAILABLE' }
            Need ((Get-ReconnectDisposition $status $owner $intent $receipt $true) -ceq $expectedExecute) 'UNCONDITIONAL_STOP_ADMITTED'
            $checks += 2
        }
        $status.local = [pscustomobject]@{effective_health=[pscustomobject]@{readyz=[pscustomobject]@{ok=$true;status=200;body='ready'}}}
        Need ((Get-ReconnectDisposition $status $owner $intent $receipt $false) -ceq 'SECURE_MCP_RECONNECT_MCP_PROBE_UNVERIFIED_NO_RETRY') 'FALLBACK_ALIAS_ACCEPTED'
        $status.process.pid = 24
        Need ((Get-ReconnectDisposition $status $owner $intent $receipt $true) -ceq 'SECURE_MCP_RECONNECT_OWNER_CHANGED') 'CHANGED_OWNER_ACCEPTED'
        Need (-not (Test-Path -LiteralPath $intent) -and -not (Test-Path -LiteralPath $receipt)) 'OBSERVATION_WROTE_JOURNAL'
        $checks += 3
        foreach ($path in @($intent,$receipt)) {
            [IO.File]::WriteAllText($path,'original fixture record')
            Need ((Get-ReconnectDisposition $status $owner $intent $receipt $true) -ceq 'SECURE_MCP_RECONNECT_ATTEMPT_EXISTS_NO_RETRY') 'HISTORICAL_FENCE_IGNORED'
            Need ([IO.File]::ReadAllText($path) -ceq 'original fixture record') 'HISTORICAL_FENCE_CHANGED'
            [IO.File]::Delete($path)
            $checks += 2
        }
    } finally {
        foreach ($path in @($intent,$receipt)) { if (Test-Path -LiteralPath $path) { [IO.File]::Delete($path) } }
    }
    @{state='PASS';checks=$checks;actualOwnedChild=$true;officialRuntimeStarts=0;operatorProfileReads=0;productionAuthChanges=0;parentEnvironmentPreserved=$true}|ConvertTo-Json -Compress
}finally{
    foreach($name in $names){[Environment]::SetEnvironmentVariable($name,$before[$name],'Process')}
    if(Test-Path -LiteralPath $ConfigPath){[IO.File]::Delete($ConfigPath)}
    if(Test-Path -LiteralPath $child){Remove-Item -LiteralPath $child -Force}
    foreach ($relative in @('.catdesk\recovery-evidence', '.catdesk')) {
        $directory = Join-Path $root $relative
        if ([IO.Directory]::Exists($directory)) { [IO.Directory]::Delete($directory) }
    }
    Remove-Item -LiteralPath $root -Force
}
