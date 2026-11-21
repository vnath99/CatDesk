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
function Get-LocalMcpProbeHeaders { param($LocalMcp) @{} }
function Get-LoopbackCatDeskListener { param($Port,$Canonical) [pscustomobject]@{MatchesCanonical=$true} }
$endpoint=[pscustomobject]@{Uri='http://127.0.0.1:1/fixture/mcp';Port=1}
$script:initializeBody='{"result":{"serverInfo":{}}}'
$script:toolsBody='{"result":{"tools":[{"name":"catdesk_instruction"},{"name":"delegated_run_list"}]}}'
$script:BootstrapSeams['LocalMcpHttp']={param($uri,$body)
    [pscustomobject]@{StatusCode=200;Content=$(if($body -match 'initialize'){$script:initializeBody}else{$script:toolsBody})}
}
$goodInitialize=$script:initializeBody;$goodTools=$script:toolsBody;$cases=0
function Check-Readiness([bool]$Expected,[string]$Gate){
    $r=Get-LocalMcpReadiness -LocalMcp $endpoint -Canonical ([pscustomobject]@{})
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
[pscustomobject]@{state='STRICT_READINESS_REGRESSIONS_PASS';cases=$cases;powerShellVersion=$PSVersionTable.PSVersion.ToString();processStarts=0;networkCalls=0;runtimeWrites=0}|ConvertTo-Json -Compress
