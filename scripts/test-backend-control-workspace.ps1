$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$source = Join-Path $PSScriptRoot 'start-catdesk-stack.ps1'
$tokens=$null;$errors=$null
$ast=[Management.Automation.Language.Parser]::ParseFile($source,[ref]$tokens,[ref]$errors)
if($errors.Count){throw 'bootstrap parse failed'}
foreach($name in @('Resolve-BackendControlWorkspace','Start-BackendOnlyDaemon','Invoke-BackendOnlyBootstrap')){
    $nodes=@($ast.FindAll({param($n)$n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -ceq $name},$true))
    if($nodes.Count -ne 1){throw "missing unique function $name"}
    . ([scriptblock]::Create($nodes[0].Extent.Text))
}
function Require([bool]$Condition,[string]$Message){if(!$Condition){throw $Message}}
function Refuses([scriptblock]$Action){try{& $Action|Out-Null}catch{return};throw 'invalid control workspace was accepted'}
$tempParent=[IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd([IO.Path]::DirectorySeparatorChar)
$testRoot=Join-Path $tempParent ('catdesk-control-workspace-test-'+[Guid]::NewGuid().ToString('N'))
$release=Join-Path $testRoot 'release'
$control=Join-Path $testRoot 'control'
$link=Join-Path $testRoot 'linked-control'
$registry=Join-Path $control '.catdesk/projects/projects.json'
try{
    [void][IO.Directory]::CreateDirectory($release)
    [void][IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($registry))
    $bytes=[Text.Encoding]::UTF8.GetBytes('{"schemaVersion":1,"projects":[]}')
    [IO.File]::WriteAllBytes($registry,$bytes)
    Require ((Resolve-BackendControlWorkspace -Root $release) -ceq $release) 'default release scope changed'
    Require ((Resolve-BackendControlWorkspace -Root $release -Requested $release) -ceq $release) 'explicit default scope changed'
    Require ((Resolve-BackendControlWorkspace -Root $release -Requested $control) -ceq $control) 'existing control root refused'
    Refuses {Resolve-BackendControlWorkspace -Root $release -Requested 'relative'}
    Refuses {Resolve-BackendControlWorkspace -Root $release -Requested 'C:relative'}
    Refuses {Resolve-BackendControlWorkspace -Root $release -Requested '\\server\share'}
    Refuses {Resolve-BackendControlWorkspace -Root $release -Requested ([IO.Path]::GetPathRoot($control))}
    Refuses {Resolve-BackendControlWorkspace -Root $release -Requested $registry}
    $unregistered=Join-Path $testRoot 'unregistered';[void][IO.Directory]::CreateDirectory($unregistered)
    Refuses {Resolve-BackendControlWorkspace -Root $release -Requested $unregistered}
    [void](New-Item -ItemType Junction -Path $link -Target $control)
    Refuses {Resolve-BackendControlWorkspace -Root $release -Requested $link}
    [IO.Directory]::Delete($link)
    # The control root is distinct from the release owner in every launch input.
    $script:launches=@();$script:listener=$null;$script:daemonCandidates=@()
    $ConfigPath=Join-Path $testRoot 'fixture-config.toml';$Mode='recover';$Execute=$true;$ReadyTimeoutSeconds=5
    $ExpectedBuildSha256='fixture';$BuildFingerprintPath=Join-Path $release 'fixture.sha256'
    $canonical=[pscustomobject]@{Path=(Join-Path $release 'catdesk.exe');Sha256='fixture'}
    function Invoke-BootstrapSeam {param($Name,$Arguments,$Default)
        Require ($Name -ceq 'BackendOnlyStart') 'unexpected process seam'
        $script:launches+=,@($Arguments)
        return [pscustomobject]@{Id=77}
    }
    function Get-BackendOnlyUtcNow {[DateTime]::UtcNow}
    function Get-ConfiguredLocalMcp {param($Path)[pscustomobject]@{Port=3200}}
    function Get-LocalMcpProbeHeaders {param($LocalMcp)@{}}
    function Get-LoopbackCatDeskListener {param($Port,$Canonical)$script:listener}
    function Get-CatDeskDaemonProcessCandidates {param($Canonical)$script:daemonCandidates}
    function Test-BackendOnlySupport {param($Canonical,$TimeoutMilliseconds)$true}
    function Stop-StaleCanonicalCatDeskDaemonForRecovery {throw 'explicit control switch recycled an unknown daemon'}
    function Get-CanonicalCatDeskIdentity {param($Root,$ExpectedHash,$FingerprintPath)
        Require ($Root -ceq $release -and $FingerprintPath -ceq $BuildFingerprintPath) 'release authority moved to control root'
        return $canonical
    }
    function Get-LocalMcpReadiness {param($LocalMcp,$Canonical,$ExpectedDaemonProcess)
        Require ($ExpectedDaemonProcess.Id -eq 77) 'readiness lost exact launched owner'
        [pscustomobject]@{Ready=$true;Gate='READY'}
    }
    function Write-RedactedStatus {param($State,$Detail,$RecoverySource,$Gate)
        [pscustomobject]@{State=$State;Gate=$Gate}
    }
    $ready=Invoke-BackendOnlyBootstrap -Root $release -Canonical $canonical -RuntimeWorkspace $control
    Require ($ready.State -ceq 'LOCAL_BACKEND_READY' -and $script:launches.Count -eq 1) 'fresh scoped backend failed'
    Require ($script:launches[0][1] -ceq $release -and $script:launches[0][2] -ceq $ConfigPath -and $script:launches[0][3] -ceq $control) 'release/config/control launch inputs were confused'
    $script:launches=@();$script:listener=[pscustomobject]@{MatchesCanonical=$true;Pid=91}
    $blocked=Invoke-BackendOnlyBootstrap -Root $release -Canonical $canonical -RuntimeWorkspace $control
    Require ($blocked.Gate -ceq 'LOCAL_MCP_CONTROL_WORKSPACE_OWNER_UNPROVEN' -and $script:launches.Count -eq 0) 'existing listener was adopted under a different scope'
    $script:listener=$null;$script:daemonCandidates=@([pscustomobject]@{Pid=92})
    $blocked=Invoke-BackendOnlyBootstrap -Root $release -Canonical $canonical -RuntimeWorkspace $control
    Require ($blocked.Gate -ceq 'LOCAL_MCP_CONTROL_WORKSPACE_OWNER_UNPROVEN' -and $script:launches.Count -eq 0) 'unproven stale daemon was recycled'
    Require ([Convert]::ToBase64String([IO.File]::ReadAllBytes($registry)) -ceq [Convert]::ToBase64String($bytes)) 'registry bytes changed'
    # Public facade must reject unsupported scope combinations before dispatch.
    $facade=Join-Path $PSScriptRoot '../catdesk.ps1'
    $invalidDispatches=[Collections.Generic.List[string]]::new()
    $refuseDispatch={ $invalidDispatches.Add('unexpected');throw 'invalid request reached lifecycle work' }.GetNewClosure()
    foreach($verb in @('start','recover','install','status','stop','diagnose','autostart','wake')){
        $observed=@(& $facade $verb -Workspace $release -ControlWorkspace $control -TestSeams @{CanonicalIdentity=$refuseDispatch;Recovery=$refuseDispatch;StopProcess=$refuseDispatch})|ConvertFrom-Json
        Require ($observed.state -ceq 'ACTION_REQUIRED') 'unsupported public control option was accepted'
        Require ($invalidDispatches.Count -eq 0) 'invalid public scope reached lifecycle work'
    }
    $seen=[Collections.Generic.List[object]]::new()
    $seams=@{Recovery={param($engine,$root,$config,$expected,$manifest,$timeout,$requested,$backend,$selectedControl)
        $seen.Add(@($root,$selectedControl,$backend))
        [pscustomobject]@{State='LOCAL_BACKEND_READY';Gate='';RecoverySource=''}|ConvertTo-Json -Compress
    }.GetNewClosure()}
    $observed=@(& $facade start -Workspace $release -ControlWorkspace $control -BackendOnly -TestSeams $seams)|ConvertFrom-Json
    Require ($observed.state -ceq 'LOCAL_BACKEND_READY' -and $seen.Count -eq 1) ("public scoped startup did not dispatch once: state="+$observed.state+" calls="+$seen.Count)
    Require ($seen[0][0] -ceq $release -and $seen[0][1] -ceq $control -and $seen[0][2] -eq $true) 'public facade clobbered the explicit scope'
    'backend control workspace tests passed: defaults, paths, junction, registry preservation, owner refusal, exact startup inputs, public forwarding'
}finally{
    $resolved=[IO.Path]::GetFullPath($testRoot)
    if(-not $resolved.StartsWith($tempParent+[IO.Path]::DirectorySeparatorChar,[StringComparison]::OrdinalIgnoreCase) -or [IO.Path]::GetFileName($resolved) -notmatch '^catdesk-control-workspace-test-[a-f0-9]{32}$'){throw 'fixture cleanup path refused'}
    if(Test-Path -LiteralPath $link){[IO.Directory]::Delete($link)}
    if(Test-Path -LiteralPath $resolved){Remove-Item -LiteralPath $resolved -Recurse -Force}
}
