# Define-only ordinary developer preparation. No native owner is qualified.
# In particular, do not substitute synchronous anonymous-pipe polling for a
# proved bounded drain in a multithreaded PowerShell/.NET host.

function ConvertTo-DevLocalOwnerArgument([AllowNull()][AllowEmptyString()][object]$Value) {
    if ($Value -isnot [string] -or $Value.Length -gt 32766 -or $Value.IndexOf([char]0) -ge 0) { throw 'OWNER_ARGUMENT_INVALID' }
    if ($Value.Length -and $Value -notmatch '[\s"]') { return $Value }
    $escaped = [regex]::Replace($Value, '(\\*)"', '$1$1\"')
    $escaped = [regex]::Replace($escaped, '(\\+)$', '$1$1')
    return '"' + $escaped + '"'
}

function New-DevLocalOwnerEnvironmentBlock([object[]]$Entries) {
    $sorted = [Collections.Generic.SortedDictionary[string,string]]::new([StringComparer]::Ordinal)
    [long]$characters = 1 # Reserve the final NUL before collecting any entry.
    foreach ($entry in $Entries) {
        $name = $entry.Name; $value = $entry.Value
        if ($name -isnot [string] -or $value -isnot [string] -or
            $name -cnotmatch '\A[A-Z][A-Z0-9_]{0,127}\z' -or $sorted.ContainsKey($name)) {
            throw 'OWNER_ENVIRONMENT_INVALID'
        }
        # Include '=' and this entry's NUL. Refuse before insertion or scanning
        # an oversized value; never visit later entries after budget overflow.
        [long]$next = $characters + $name.Length + $value.Length + 2
        if ($next -gt 32767) { throw 'OWNER_ENVIRONMENT_TOO_LARGE' }
        if ($value.IndexOf([char]0) -ge 0) { throw 'OWNER_ENVIRONMENT_INVALID' }
        $sorted.Add($name, $value)
        $characters = $next
    }
    $block = [Text.StringBuilder]::new()
    foreach ($entry in $sorted.GetEnumerator()) {
        $null = $block.Append($entry.Key).Append('=').Append($entry.Value).Append([char]0)
        if ($block.Length -ge 32767) { throw 'OWNER_ENVIRONMENT_TOO_LARGE' }
    }
    $null = $block.Append([char]0)
    if (!$sorted.Count) { $null = $block.Append([char]0) }
    if ($block.Length -gt 32767) { throw 'OWNER_ENVIRONMENT_TOO_LARGE' }
    return $block.ToString()
}

function New-DevLocalOwnerPreparation {
    param(
        [Parameter(Mandatory=$true)][ValidateSet('build','clippy','test','fmt')][string]$Operation,
        [Parameter(Mandatory=$true)][string]$TargetPath,
        [ValidateRange(1,300000)][int]$DeadlineMilliseconds = 120000,
        [ValidateRange(1,65536)][int]$MaximumRetainedOutputBytes = 16384
    )
    # This checks only input shape. It does not observe/authorize any path.
    if ($TargetPath.Length -lt 4 -or $TargetPath -cnotmatch '\A[A-Za-z]:\\' -or
        $TargetPath.Length -gt 30000 -or $TargetPath.Substring(2).Contains(':') -or
        $TargetPath -match '[\x00-\x1f*?]' -or $TargetPath.Contains('/') -or
        $TargetPath.Substring(3).Contains('\\') -or
        $TargetPath -match '(?:^|\\)(?:\.{1,2}|[^\\]*[. ])(?:\\|$)' -or
        $TargetPath -match '(?:^|\\)(?:CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])(?:\.[^\\]*)?(?:\\|$)') {
        throw 'OWNER_TARGET_INPUT_INVALID'
    }
    $arguments = switch ($Operation) {
        'build' { @('build','--locked','--offline','--bin','catdesk','--target-dir',$TargetPath,'-j1') }
        'clippy' { @('clippy','--locked','--offline','--workspace','--all-targets','--all-features','--target-dir',$TargetPath,'-j1','--','-D','warnings') }
        'test' { @('test','--locked','--offline','--bin','catdesk','--target-dir',$TargetPath,'-j1') }
        'fmt' { @('fmt','--all','--','--check') }
    }
    $line = (@($arguments | ForEach-Object { ConvertTo-DevLocalOwnerArgument $_ }) -join ' ')
    if ($line.Length -gt 32000) { throw 'OWNER_COMMAND_LINE_TOO_LARGE' }
    [pscustomobject][ordered]@{
        schemaVersion = 1
        state = 'PREPARED_INPUT_ONLY'
        operation = $Operation
        programRole = 'FUTURE_QUALIFIED_CARGO_ONLY'
        arguments = @($arguments)
        commandLineArguments = $line
        environmentOverrides = @{ CARGO_INCREMENTAL = '0' }
        deadlineMilliseconds = $DeadlineMilliseconds
        maximumRetainedOutputBytes = $MaximumRetainedOutputBytes
        executionQualified = $false
        targetReuseAuthorized = $false
    }
}

function Get-DevLocalOwnerContract {
    # Preparation is data, never a capability or a native observation witness.
    [pscustomobject][ordered]@{
        schemaVersion = 1
        state = 'PURE_PREPARATION_ONLY'
        nativeEntryPointAvailable = $false
        executionQualified = $false
        targetReuseAuthorized = $false
        blocker = 'OWNER_OVERLAPPED_DRAIN_NOT_IMPLEMENTED'
    }
}
