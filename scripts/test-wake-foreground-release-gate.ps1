#requires -Version 7.0
param([Parameter(Mandatory)][string]$Executable)
$ErrorActionPreference='Stop'
Set-StrictMode -Version Latest
$image=(Get-Item -LiteralPath $Executable).FullName
$marker="CATDESK_DEV_FOREGROUND_OWNER_V1`n"
if(-not [Text.Encoding]::ASCII.GetString([IO.File]::ReadAllBytes($image)).Contains($marker)){
    throw 'DEV_RUNTIME_FOREGROUND_PACKAGE_REQUIRED'
}
$parent=Join-Path (Split-Path $PSScriptRoot -Parent) 'target'
$null=New-Item -ItemType Directory -Path $parent -Force
$parent=(Get-Item -LiteralPath $parent).FullName
$workspace=Join-Path $parent ('wake-foreground-gate-'+[Guid]::NewGuid().ToString('N'))
$null=New-Item -ItemType Directory -Path $workspace
$process=$null
try{
    $info=[Diagnostics.ProcessStartInfo]::new()
    $info.FileName=$image
    $info.WorkingDirectory=$workspace
    $info.UseShellExecute=$false
    $info.CreateNoWindow=$true
    $info.RedirectStandardInput=$true
    $info.RedirectStandardOutput=$true
    $info.RedirectStandardError=$true
    $info.ArgumentList.Add('--dev-foreground-host')
    $info.ArgumentList.Add($workspace)
    $process=[Diagnostics.Process]::new()
    $process.StartInfo=$info
    if(-not $process.Start()){throw 'FOREGROUND_GATE_PROBE_START_FAILED'}
    $null=$process.Handle
    $stdout=$process.StandardOutput.ReadToEndAsync()
    $stderr=$process.StandardError.ReadToEndAsync()
    # EOF supplies no owner gate; runtime startup must never be reached.
    $process.StandardInput.Close()
    if(-not $process.WaitForExit(5000)){
        $process.Kill()
        if(-not $process.WaitForExit(5000)){throw 'FOREGROUND_GATE_PROBE_EXIT_UNPROVEN'}
        throw 'FOREGROUND_GATE_PROBE_TIMEOUT'
    }
    if(-not $stdout.Wait(5000) -or -not $stderr.Wait(5000)){throw 'FOREGROUND_GATE_OUTPUT_TIMEOUT'}
    $result=$stdout.Result|ConvertFrom-Json
    if($process.ExitCode -ne 1 -or $result.error -cne 'DEV_RUNTIME_FOREGROUND_OWNER_UNAVAILABLE' -or $stderr.Result){
        throw 'FOREGROUND_GATE_EOF_NOT_REFUSED'
    }
    if(@(Get-ChildItem -LiteralPath $workspace -Force).Count){throw 'FOREGROUND_GATE_WROTE_RUNTIME_STATE'}
    [ordered]@{schemaVersion=1;classification='NORMAL_RELEASE_FOREGROUND_GATE_PROBE';imageSha256=(Get-FileHash -LiteralPath $image -Algorithm SHA256).Hash.ToLowerInvariant();markerPresent=$true;ownerEofRefused=$true;exitCode=$process.ExitCode;runtimeStateCreated=$false;ownedProcessExited=$true;deliveryProved=$false}|ConvertTo-Json
}finally{
    if($null -ne $process){
        if(-not $process.HasExited){$process.Kill();$null=$process.WaitForExit(5000)}
        $process.Dispose()
    }
    $resolved=(Get-Item -LiteralPath $workspace).FullName
    if((Split-Path $resolved -Parent) -cne $parent -or (Get-Item -LiteralPath $workspace).Attributes -band [IO.FileAttributes]::ReparsePoint){
        throw 'FOREGROUND_GATE_CLEANUP_SCOPE_INVALID'
    }
    if(@(Get-ChildItem -LiteralPath $resolved -Force).Count -eq 0){Remove-Item -LiteralPath $resolved}
}
