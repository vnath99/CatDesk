[CmdletBinding()]
param(
    [ValidateSet('ManagedFake','NativeSmoke')][string]$Mode='ManagedFake',
    [ValidateSet('connect-read-close','cancel','capture')][string]$Case='connect-read-close'
)
$ErrorActionPreference='Stop';Set-StrictMode -Version Latest
function Test-OutputFixtureAcceptance([object]$Observation,[string]$FixtureCase){
    try{
        if($null -eq $Observation -or $null -ne $Observation.Failure -or
           $Observation.NativeOrigin -isnot [bool] -or !$Observation.NativeOrigin -or
           $Observation.IoLifetimeSettled -isnot [bool] -or !$Observation.IoLifetimeSettled -or
           $Observation.State -isnot [string] -or $Observation.Eof.Count -ne 2 -or
           $Observation.Eof[0] -isnot [bool] -or $Observation.Eof[1] -isnot [bool]){return $false}
        $both=$Observation.Eof[0] -and $Observation.Eof[1]
        switch($FixtureCase.ToLowerInvariant()){
            'connect-read-close' {return $Observation.State -ceq 'DRAINED' -and $both}
            'capture' {return $Observation.State -ceq 'DRAINED' -and $both}
            'cancel' {return ($Observation.State -ceq 'CANCELLED_AFTER_EOF' -and $both) -or ($Observation.State -ceq 'CANCELLED_WITHOUT_EOF' -and !$both)}
            default {return $false}
        }
    }catch{return $false}
}
if($PSVersionTable.PSEdition -cne 'Core'){throw 'OUTPUT_QUALIFICATION_REQUIRES_CORE_NO_CSC_CHILD'}
$helper=Join-Path $PSScriptRoot 'catdesk-dev-local-target-output.ps1'
$t=$null;$e=$null
$ast=[Management.Automation.Language.Parser]::ParseFile($helper,[ref]$t,[ref]$e)
if($e.Count -or $ast.EndBlock.Statements.Count -ne 1 -or $ast.EndBlock.Statements[0] -isnot [Management.Automation.Language.FunctionDefinitionAst]){throw 'OUTPUT_HELPER_NOT_DEFINE_ONLY'}
. $helper
Initialize-DevLocalTargetOutput # Explicit Core in-process managed compile; no static native setup.
$type=[CatDesk.DevLocalTarget.OutputV1.OutputLibrary]
if($Mode -ieq 'ManagedFake'){
    $method=$type.GetMethod('ManagedChecks',[Reflection.BindingFlags]'NonPublic,Static')
    $results=@($method.Invoke($null,@()))
    $policy=[Collections.Generic.List[string]]::new()
    foreach($row in @(
        @{name='connect_success';case='connect-read-close';state='DRAINED';failure=$null;native=$true;settled=$true;eof=@($true,$true);expected=$true},
        @{name='capture_success';case='capture';state='DRAINED';failure=$null;native=$true;settled=$true;eof=@($true,$true);expected=$true},
        @{name='cancel_without_eof';case='cancel';state='CANCELLED_WITHOUT_EOF';failure=$null;native=$true;settled=$true;eof=@($false,$false);expected=$true},
        @{name='cancel_after_eof';case='cancel';state='CANCELLED_AFTER_EOF';failure=$null;native=$true;settled=$true;eof=@($true,$true);expected=$true},
        @{name='cancel_error_settled_refused';case='cancel';state='ERROR_SETTLED';failure='OUTPUT_OPERATION_UNVERIFIED';native=$true;settled=$true;eof=@($false,$false);expected=$false},
        @{name='connect_error_settled_refused';case='connect-read-close';state='ERROR_SETTLED';failure='OUTPUT_OPERATION_UNVERIFIED';native=$true;settled=$true;eof=@($true,$true);expected=$false},
        @{name='nonnull_empty_failure_refused';case='capture';state='DRAINED';failure='';native=$true;settled=$true;eof=@($true,$true);expected=$false},
        @{name='non_native_refused';case='capture';state='DRAINED';failure=$null;native=$false;settled=$true;eof=@($true,$true);expected=$false},
        @{name='string_origin_refused';case='capture';state='DRAINED';failure=$null;native='true';settled=$true;eof=@($true,$true);expected=$false},
        @{name='unsettled_refused';case='capture';state='DRAINED';failure=$null;native=$true;settled=$false;eof=@($true,$true);expected=$false},
        @{name='missing_closed_writer_refused';case='connect-read-close';state='DRAINED';failure=$null;native=$true;settled=$true;eof=@($true,$false);expected=$false},
        @{name='cancel_wrong_state_refused';case='cancel';state='DRAINED';failure=$null;native=$true;settled=$true;eof=@($true,$true);expected=$false},
        @{name='unknown_refused';case='cancel';state='UNKNOWN_QUARANTINED';failure=$null;native=$true;settled=$true;eof=@($false,$false);expected=$false},
        @{name='inconsistent_cancel_state_refused';case='cancel';state='CANCELLED_AFTER_EOF';failure=$null;native=$true;settled=$true;eof=@($false,$false);expected=$false}
    )){
        $synthetic=[pscustomobject]@{State=$row.state;Failure=$row.failure;NativeOrigin=$row.native;IoLifetimeSettled=$row.settled;Eof=$row.eof}
        if((Test-OutputFixtureAcceptance $synthetic $row.case) -ne $row.expected){throw ('OUTPUT_ACCEPTANCE_POLICY_FAILED:'+ $row.name)}
        $policy.Add($row.name)
    }
    [pscustomobject][ordered]@{
        schemaVersion=1;classification='MANAGED_FAKE_AND_FIXTURE_POLICY_ONLY';hostVersion=$PSVersionTable.PSVersion.ToString()
        passed=($results.Count+$policy.Count);managedStateChecksPassed=$results.Count;fixturePolicyPassed=$policy.Count
        cases=$results;fixturePolicyCases=@($policy);policyInputsSynthetic=$true;managedCompilations=1
        nativeSetupEntries=$type.GetField('NativeSetupEntries',[Reflection.BindingFlags]'NonPublic,Static').GetValue($null)
        nativeCreatePairEntries=$type.GetField('NativeCreatePairEntries',[Reflection.BindingFlags]'NonPublic,Static').GetValue($null)
        nativeSmokeExecuted=$false;executionQualified=$false;targetReuseAuthorized=$false
    }|ConvertTo-Json -Depth 4 -Compress
    return
}
if($Mode -ine 'NativeSmoke'){throw 'OUTPUT_TEST_MODE_UNVERIFIED'}
$Case=$Case.ToLowerInvariant()
# NativeSmoke is ONLY a future explicitly admitted, frozen one-shot fixture.
# The default above never enters this branch or creates a kernel object.
$pair=$null;$last=$null;$deadline=[Diagnostics.Stopwatch]::StartNew()
$flags=[Reflection.BindingFlags]'NonPublic,Instance'
function Bounded-Poll([scriptblock]$Done){
    while($deadline.ElapsedMilliseconds -lt 10000){
        $script:last=$pair.Poll()
        if(& $Done $script:last){return $script:last}
        if($script:last.State -ceq 'UNKNOWN_QUARANTINED'){throw 'OUTPUT_FIXTURE_QUARANTINED'}
        [Threading.Thread]::Sleep(1)
    }
    throw 'OUTPUT_FIXTURE_CALLER_DEADLINE'
}
try{
    if($Case -ceq 'capture'){
        $factory=$type.GetMethod('CreateFixturePair',[Reflection.BindingFlags]'NonPublic,Static')
        $pair=$factory.Invoke($null,@([int]64))
    }else{$pair=[CatDesk.DevLocalTarget.OutputV1.OutputLibrary]::CreatePair()}
    $null=Bounded-Poll {param($s) ($s.Channels[0] -ceq 'READ_PENDING' -and $s.Channels[1] -ceq 'READ_PENDING')}
    if($Case -ceq 'cancel'){
        $null=$pair.RequestCancel()
    }else{
        $payloads=if($Case -ceq 'capture'){
            @([Text.Encoding]::ASCII.GetBytes('out.'*32),[Text.Encoding]::ASCII.GetBytes('err.'*32))
        }else{@([Text.Encoding]::ASCII.GetBytes('stdout-fixture-A9'),[Text.Encoding]::ASCII.GetBytes('stderr-fixture-Z7'))}
        $write=$pair.GetType().GetMethod('FixtureWrite',$flags)
        for($i=0;$i -lt 2;$i++){$null=$write.Invoke($pair,@([int]$i,[byte[]]$payloads[$i]))}
        $s=Bounded-Poll {
            param($v)
            if($v.ObservedBytes[0] -gt $payloads[0].Length -or $v.ObservedBytes[1] -gt $payloads[1].Length){throw 'OUTPUT_FIXTURE_READBACK_MISMATCH'}
            $v.ObservedBytes[0] -eq $payloads[0].Length -and $v.ObservedBytes[1] -eq $payloads[1].Length
        }
        if($s.ObservedBytes[0] -ne $payloads[0].Length -or $s.ObservedBytes[1] -ne $payloads[1].Length){throw 'OUTPUT_FIXTURE_READBACK_MISMATCH'}
        if($Case -ceq 'capture' -and (!$s.Overflow[0] -or $s.RetainedBytes[0] -ne 64)){throw 'OUTPUT_FIXTURE_CAPTURE_BOUND'}
        $close=$pair.GetType().GetMethod('FixtureCloseWriter',$flags)
        for($i=0;$i -lt 2;$i++){$null=$close.Invoke($pair,@([int]$i))}
    }
    $s=Bounded-Poll {param($v) $script:last=$pair.TryFinish();$script:last.IoLifetimeSettled}
    $s=$pair.Current()
    if(!(Test-OutputFixtureAcceptance $s $Case)){throw 'OUTPUT_FIXTURE_COMPLETION_UNVERIFIED'}
    if($Case -cne 'cancel'){
        for($i=0;$i -lt 2;$i++){
            $actual=[byte[]]$pair.GetCaptured($i)
            $length=if($Case -ceq 'capture'){64}else{$payloads[$i].Length}
            $expected=[byte[]]$payloads[$i][0..($length-1)]
            if($actual.Length -ne $length -or [Convert]::ToBase64String($actual) -cne [Convert]::ToBase64String($expected)){throw 'OUTPUT_FIXTURE_EXACT_CAPTURE_MISMATCH'}
        }
    }
    [pscustomobject][ordered]@{schemaVersion=1;classification='NATIVE_SAME_PROCESS_FIXTURE_ONLY';case=$Case;state=$s.State;ioLifetimeSettled=$s.IoLifetimeSettled;eof=@($s.Eof);observedBytes=@($s.ObservedBytes);retainedBytes=@($s.RetainedBytes);exactPayloadsVerified=($Case -cne 'cancel');elapsedMilliseconds=$deadline.ElapsedMilliseconds;childProcessesCreated=0;executionQualified=$false;targetReuseAuthorized=$false}|ConvertTo-Json -Depth 4 -Compress
}finally{
    if($null -ne $pair -and !$pair.Current().IoLifetimeSettled){
        $null=$pair.RequestCancel()
        $cleanup=[Diagnostics.Stopwatch]::StartNew()
        do{$null=$pair.Poll();$status=$pair.TryFinish();if($status.IoLifetimeSettled -or $status.State -ceq 'UNKNOWN_QUARANTINED'){break};[Threading.Thread]::Sleep(1)}while($cleanup.ElapsedMilliseconds -lt 1500)
        # No Dispose/free/forced process control or replacement on UNKNOWN.
        if(!$pair.Current().IoLifetimeSettled){[Console]::Error.WriteLine('OUTPUT_FIXTURE_UNKNOWN_HOLD_NO_REPLACEMENT')}
    }
}
