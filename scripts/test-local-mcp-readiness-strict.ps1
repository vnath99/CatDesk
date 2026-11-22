# Regression for callers that enable strict mode before using public startup.
# No process, network, credential, workspace or installed-state operations.
$ErrorActionPreference='Stop'
Set-StrictMode -Version Latest
$tokens=$null;$errors=$null
$ast=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot 'start-catdesk-stack.ps1'),[ref]$tokens,[ref]$errors)
if($errors.Count){throw 'startup script parse failed'}
foreach($name in @('Invoke-BootstrapSeam','New-LocalMcpReadinessResult','Get-LocalMcpReadiness')){
    $nodes=@($ast.FindAll({param($n)$n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -ceq $name},$true))
    if($nodes.Count -ne 1){throw 'readiness function is missing or ambiguous'}
    . ([scriptblock]::Create($nodes[0].Extent.Text))
}
$script:BootstrapSeams=@{}
$script:MaxMcpReplyBytes=1048576
$script:listenerReads=0
$script:listenerBefore=[pscustomobject]@{MatchesCanonical=$true;Pid=601;CreationTimeUtc='2026-11-22T12:00:00Z'}
$script:listenerAfter=$script:listenerBefore
function Get-LocalMcpProbeHeaders { param($LocalMcp) @{Authorization='fixture-auth-header'} }
function Get-LoopbackCatDeskListener {
    param($Port,$Canonical)
    if ($script:listenerReads++ -eq 0) { return $script:listenerBefore }
    if ($script:listenerAfter -is [scriptblock]) { return (& $script:listenerAfter) }
    return $script:listenerAfter
}
$endpoint=[pscustomobject]@{Uri='http://127.0.0.1:1/fixture/mcp';Port=1}
$script:initializeBody='{"result":{"serverInfo":{}}}'
$script:toolsBody='{"result":{"tools":[{"name":"catdesk_instruction"},{"name":"delegated_run_list"}]}}'
$script:BootstrapSeams['LocalMcpHttp']={param($uri,$body,$headers)
    if (!$headers.ContainsKey('Authorization')) { return [pscustomobject]@{StatusCode=401} }
    [pscustomobject]@{StatusCode=200;Content=$(if($body -match 'initialize'){$script:initializeBody}else{$script:toolsBody})}
}
$goodInitialize=$script:initializeBody;$goodTools=$script:toolsBody;$cases=0
function Check-Readiness([bool]$Expected,[string]$Gate,[switch]$RequireBearer){
    $script:listenerReads=0
    $r=Get-LocalMcpReadiness -LocalMcp $endpoint -Canonical ([pscustomobject]@{}) -RequireBearer:$RequireBearer
    if($r.Ready -ne $Expected -or $r.Gate -cne $Gate){throw "strict readiness case $script:cases expected $Expected/$Gate, observed $($r.Ready)/$($r.Gate)"}
    $script:cases++
}
Check-Readiness $true 'READY'
foreach($bad in @('{"error":{"code":-32000}}','{}','{"result":null}','{"result":{}}','{"result":{"serverInfo":null}}','{"result":{"serverInfo":"invalid"}}')){
    $script:initializeBody=$bad;Check-Readiness $false 'LOCAL_MCP_PROTOCOL_UNREADY'
}
foreach($bad in @('null','[]','true','broken json')){
    $script:initializeBody=$bad;Check-Readiness $false 'LOCAL_MCP_RESPONSE_INVALID'
}
$script:initializeBody=$goodInitialize
foreach($bad in @('{}','{"result":{}}','{"result":{"tools":[{}]}}','{"result":{"tools":[null]}}','{"result":{"tools":[{"name":false}]}}','{"result":{"tools":[{"name":"catdesk_instruction"}]}}')){
    $script:toolsBody=$bad;Check-Readiness $false 'LOCAL_MCP_PROTOCOL_UNREADY'
}
$script:toolsBody=$goodTools;Check-Readiness $true 'READY'
Check-Readiness $true 'READY' -RequireBearer
$script:listenerAfter=$null
Check-Readiness $false 'LOCAL_MCP_LISTENER_MISSING' -RequireBearer
$script:listenerAfter=[pscustomobject]@{MatchesCanonical=$true;Pid=602;CreationTimeUtc=$script:listenerBefore.CreationTimeUtc}
Check-Readiness $false 'LOCAL_MCP_LISTENER_IDENTITY_MISMATCH' -RequireBearer
$script:listenerAfter=[pscustomobject]@{MatchesCanonical=$true;Pid=$script:listenerBefore.Pid;CreationTimeUtc='2026-11-22T12:00:01Z'}
Check-Readiness $false 'LOCAL_MCP_LISTENER_IDENTITY_MISMATCH' -RequireBearer
$script:listenerAfter=[pscustomobject]@{MatchesCanonical=$false;Pid=$script:listenerBefore.Pid;CreationTimeUtc=$script:listenerBefore.CreationTimeUtc}
Check-Readiness $false 'LOCAL_MCP_LISTENER_IDENTITY_MISMATCH' -RequireBearer
$script:listenerAfter={ throw 'fixture listener census is ambiguous' }
Check-Readiness $false 'LOCAL_MCP_LISTENER_IDENTITY_MISMATCH' -RequireBearer
[pscustomobject]@{state='STRICT_READINESS_REGRESSIONS_PASS';cases=$cases;powerShellVersion=$PSVersionTable.PSVersion.ToString();processStarts=0;networkCalls=0;runtimeWrites=0}|ConvertTo-Json -Compress
