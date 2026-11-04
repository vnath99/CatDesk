$ErrorActionPreference='Stop'
Set-StrictMode -Version Latest
$tokens=$null;$errors=$null
$ast=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot 'start-catdesk-stack.ps1'),[ref]$tokens,[ref]$errors)
if($errors.Count){throw 'BOOTSTRAP_PARSE_FAILED'}
function Import-Helper([string]$Name){
 $nodes=@($ast.FindAll({param($n)$n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -ceq $Name},$true))
 if($nodes.Count -ne 1){throw 'HELPER_IDENTITY_INVALID'}
 . ([scriptblock]::Create($nodes[0].Extent.Text))
 # Definitions imported inside this helper need script scope for the fixture.
 Set-Item -LiteralPath ('Function:script:'+$Name) -Value (Get-Item ('Function:'+$Name)).ScriptBlock
}
foreach($name in @('Invoke-BootstrapSeam','Get-ConfiguredLocalMcpAuthToken','Get-LocalMcpProbeHeaders')){Import-Helper $name}
$hadProcess=[Environment]::GetEnvironmentVariables('Process').Contains('CATDESK_MCP_AUTH_TOKEN')
$savedProcess=[Environment]::GetEnvironmentVariable('CATDESK_MCP_AUTH_TOKEN','Process')
$script:BootstrapSeams=@{LocalMcpUserAuthToken={'synthetic-lifecycle-user-reference-0123456789'}}
$endpoint=[pscustomobject]@{Port=3200;Uri=('http://127.0.0.1:3200/'+('a'*24)+'/mcp')}
$checks=0;$process=$null;$root=$null
function Need([bool]$Value,[string]$Reason){if(!$Value){throw $Reason};$script:checks++}
function Refuses([scriptblock]$Action,[string]$Reason){try{$null=& $Action;throw 'MISSING_REFUSAL'}catch{Need ($_.Exception.Message -ceq $Reason) 'WRONG_OR_PRIVATE_FAILURE'}}
try{
 Remove-Item Env:CATDESK_MCP_AUTH_TOKEN -ErrorAction SilentlyContinue
 $headers=Get-LocalMcpProbeHeaders -LocalMcp $endpoint
 Need ($headers.ContainsKey('Authorization') -and $headers.Authorization -ceq 'Bearer synthetic-lifecycle-user-reference-0123456789') 'USER_REFERENCE_NOT_USED_BY_OLD_LAUNCHER'
 foreach($name in @('Get-ConfiguredLocalMcpAuthToken','New-CanonicalDaemonStartInfo')){Import-Helper $name}
 Need ([string]::IsNullOrEmpty([Environment]::GetEnvironmentVariable('CATDESK_MCP_AUTH_TOKEN','Process'))) 'PARENT_AUTH_MUTATED'
 $script:BootstrapSeams.LocalMcpUserAuthToken={throw 'PRIVATE_USER_STORE_ERROR'}
 Refuses {Get-LocalMcpProbeHeaders -LocalMcp $endpoint} 'LOCAL_MCP_AUTH_TOKEN_INVALID'
 foreach($bad in @('short',(('a'*24)+' '),(([string][char]233)*24))){
  $current=$bad;$script:BootstrapSeams.LocalMcpUserAuthToken={ $current }.GetNewClosure()
  Refuses {Get-LocalMcpProbeHeaders -LocalMcp $endpoint} 'LOCAL_MCP_AUTH_TOKEN_INVALID'
 }
 $script:BootstrapSeams.LocalMcpUserAuthToken={'synthetic-lifecycle-user-reference-0123456789'}
 [Environment]::SetEnvironmentVariable('CATDESK_MCP_AUTH_TOKEN','synthetic-explicit-project-reference-0123456789','Process')
 Need ((Get-LocalMcpProbeHeaders $endpoint).Authorization -ceq 'Bearer synthetic-explicit-project-reference-0123456789') 'EXPLICIT_PROJECT_AUTH_OVERRIDDEN'
 [Environment]::SetEnvironmentVariable('CATDESK_MCP_AUTH_TOKEN','invalid','Process')
 Refuses {Get-LocalMcpProbeHeaders $endpoint} 'LOCAL_MCP_AUTH_TOKEN_INVALID'
 Remove-Item Env:CATDESK_MCP_AUTH_TOKEN -ErrorAction SilentlyContinue
 Refuses {Get-LocalMcpProbeHeaders ([pscustomobject]@{Port=3200;Uri=('http://example.invalid:3200/'+('a'*24)+'/mcp')})} 'LOCAL_MCP_AUTH_SCOPE_INVALID'
 $script:BootstrapSeams.LocalMcpUserAuthToken={$null}
 Need ((Get-LocalMcpProbeHeaders $endpoint).Count -eq 0) 'LEGACY_NO_REFERENCE_CHANGED'
 $script:BootstrapSeams.LocalMcpUserAuthToken={'synthetic-lifecycle-user-reference-0123456789'}
 $parent=[IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\')
 $root=Join-Path $parent ('catdesk-user-bearer-'+[Guid]::NewGuid().ToString('N'))
 [void][IO.Directory]::CreateDirectory($root)
 $image=(Get-Process -Id $PID).Path;$identity=[pscustomobject]@{Path=$image}
 $info=New-CanonicalDaemonStartInfo -Canonical $identity -Root $root -Config (Join-Path $root 'fixture.toml') -BackendOnly
 Need ($info.Arguments -ceq '--catdesk-daemon --backend-only' -and !$info.UseShellExecute -and $info.CreateNoWindow) 'BACKEND_LAUNCH_POLICY_CHANGED'
 $full=New-CanonicalDaemonStartInfo -Canonical $identity -Root $root -Config (Join-Path $root 'fixture.toml')
 Need ($full.Arguments -ceq '--catdesk-daemon' -and $full.EnvironmentVariables['CATDESK_MCP_AUTH_TOKEN'] -ceq 'synthetic-lifecycle-user-reference-0123456789') 'FULL_DAEMON_AUTH_NOT_FORWARDED'
 $child=Join-Path $root 'child.ps1'
 [IO.File]::WriteAllText($child,'$valid=$env:CATDESK_MCP_AUTH_TOKEN -ceq "synthetic-lifecycle-user-reference-0123456789";@{childAuthMatches=$valid}|ConvertTo-Json -Compress;if(!$valid){exit 1}')
 $info.Arguments='-NoProfile -NonInteractive -File "'+$child+'"';$info.RedirectStandardInput=$true;$info.RedirectStandardOutput=$true;$info.RedirectStandardError=$true
 $process=[Diagnostics.Process]::Start($info);$process.StandardInput.Close()
 Need ($process.WaitForExit(10000)) 'OWNED_CHILD_TIMEOUT'
 $out=$process.StandardOutput.ReadToEnd();$err=$process.StandardError.ReadToEnd()
 Need ($process.ExitCode -eq 0 -and ($out|ConvertFrom-Json).childAuthMatches -and $err.Length -eq 0 -and !$out.Contains('synthetic-lifecycle-user-reference-0123456789')) 'CHILD_AUTH_OR_PRIVACY_FAILED'
 Need ([string]::IsNullOrEmpty([Environment]::GetEnvironmentVariable('CATDESK_MCP_AUTH_TOKEN','Process'))) 'CHILD_FORWARDING_CHANGED_PARENT'
 @{state='PASS';checks=$checks;actualChild=$true;userEnvironmentWrites=0;parentEnvironmentPreserved=$true;catdeskStarts=0;networkRequests=0}|ConvertTo-Json -Compress
}finally{
 if($hadProcess){[Environment]::SetEnvironmentVariable('CATDESK_MCP_AUTH_TOKEN',$savedProcess,'Process')}else{Remove-Item Env:CATDESK_MCP_AUTH_TOKEN -ErrorAction SilentlyContinue}
 if($process){try{if(!$process.HasExited){$process.Kill();[void]$process.WaitForExit(2000)}}finally{$process.Dispose()}}
 if($root){$resolved=[IO.Path]::GetFullPath($root);if(!$resolved.StartsWith($parent+'\',[StringComparison]::OrdinalIgnoreCase) -or [IO.Path]::GetFileName($resolved) -cnotmatch '^catdesk-user-bearer-[a-f0-9]{32}$'){throw 'FIXTURE_CLEANUP_REFUSED'};Remove-Item -LiteralPath $resolved -Recurse -Force}
}
