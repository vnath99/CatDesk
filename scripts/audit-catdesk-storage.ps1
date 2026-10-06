[CmdletBinding()]
param(
    [string]$Workspace = (Join-Path $PSScriptRoot ".."),
    [string]$JsonPath = "",
    [int]$TargetDepth = 2,
    [int]$RuntimeDepth = 2
)

$ErrorActionPreference = "Stop"

function Test-ReparsePoint { param([IO.FileSystemInfo]$Item) return (($Item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) }

function Get-Classification {
    param([string]$Relative)
    if($Relative -match '^target[\\/]release(?:[\\/]|$)'){return @{Category='KEEP'; Eligible=$false; Rationale='Canonical release identity is retained until separately reprovisioned.'}}
    $first=($Relative -split '[\\/]')[0]
    switch -Regex ($first) {
        '^(target|target-verify|target-debug)$' { return @{Category='REBUILDABLE'; Eligible=$true; Rationale='Cargo build, verification, and test output can be recreated after live-owner checks.'} }
        '^\.catdesk$' { return @{Category='PROTECTED_RUNTIME'; Eligible=$false; Rationale='Runtime, review, recovery, and browser-control state require separate migration.'} }
        '^\.git$' { return @{Category='KEEP'; Eligible=$false; Rationale='Repository history and worktree metadata.'} }
        '^(src|tests|scripts|docs|assets)$' { return @{Category='KEEP'; Eligible=$false; Rationale='Source-controlled project material.'} }
        '^(node_modules|\.venv|venv|__pycache__|\.pytest_cache)$' { return @{Category='REBUILDABLE'; Eligible=$true; Rationale='Dependency or interpreter cache can be recreated after review.'} }
        '^(logs|diagnostics|review_bundles)$' { return @{Category='ARCHIVE'; Eligible=$false; Rationale='Evidence should be retained or separately archived.'} }
        default { return @{Category='UNKNOWN'; Eligible=$false; Rationale='Unclassified paths are never deletion candidates.'} }
    }
}

function Get-ContainedRelativePath {
    param([string]$Root,[string]$Path)
    $prefix=$Root.TrimEnd('\')+'\'
    if(-not $Path.StartsWith($prefix,[StringComparison]::OrdinalIgnoreCase)){throw 'audit node escaped workspace'}
    return $Path.Substring($prefix.Length)
}

function Measure-StorageNode {
    param([string]$Root,[string]$Path)
    $bytes=[Int64]0;$files=[Int64]0;$dirs=[Int64]0;$errors=[Int64]0
    $stack=[Collections.Generic.Stack[string]]::new();$stack.Push($Path)
    while($stack.Count -gt 0){
        $current=$stack.Pop()
        try {$children=[IO.Directory]::EnumerateFileSystemEntries($current)} catch {$errors++;continue}
        foreach($childPath in $children){
            try {
                $item=Get-Item -LiteralPath $childPath -Force -ErrorAction Stop
                if(Test-ReparsePoint $item){continue}
                if($item.PSIsContainer){$dirs++;$stack.Push($item.FullName)}else{$files++;$bytes=[Int64]($bytes+[Int64]$item.Length)}
            } catch {$errors++}
        }
    }
    $relative=Get-ContainedRelativePath -Root $Root -Path $Path;$policy=Get-Classification $relative
    [pscustomobject][ordered]@{path=$relative;bytes=$bytes;gib=[Math]::Round($bytes/1GB,3);fileCount=$files;directoryCount=$dirs;accessErrors=$errors;classification=$policy.Category;deletionEverEligible=$policy.Eligible;rationale=$policy.Rationale}
}

function Get-AuditNodes {
    param([string]$Root,[int]$TargetDepth,[int]$RuntimeDepth)
    $nodes=[Collections.Generic.List[string]]::new()
    foreach($item in (Get-ChildItem -LiteralPath $Root -Force -ErrorAction Stop)){
        if(Test-ReparsePoint $item){continue}
        if(-not $item.PSIsContainer){continue}
        $nodes.Add($item.FullName)
        $limit=if($item.Name -in @('target','target-verify','target-debug')){$TargetDepth}elseif($item.Name -eq '.catdesk'){$RuntimeDepth}else{0}
        if($limit -gt 0){
            $queue=[Collections.Generic.Queue[object]]::new();$queue.Enqueue(@($item.FullName,1))
            while($queue.Count -gt 0){$pair=$queue.Dequeue();$dir=$pair[0];$depth=[int]$pair[1];if($depth -gt $limit){continue};try{$children=Get-ChildItem -LiteralPath $dir -Directory -Force -ErrorAction Stop}catch{continue};foreach($child in $children){if(Test-ReparsePoint $child){continue};$nodes.Add($child.FullName);$queue.Enqueue(@($child.FullName,$depth+1))}}
        }
    }
    return $nodes | Select-Object -Unique
}

function Measure-WorkspaceAudit {
    param([string]$Root)
    $workspace=[pscustomobject]@{bytes=[Int64]0;files=[Int64]0;dirs=[Int64]0;errors=[Int64]0}
    $buckets=@{}
    $ensure={param($key)if(-not $buckets.ContainsKey($key)){$buckets[$key]=[pscustomobject]@{bytes=[Int64]0;files=[Int64]0;dirs=[Int64]0;errors=[Int64]0}};return $buckets[$key]}
    $stack=[Collections.Generic.Stack[string]]::new();$stack.Push($Root)
    while($stack.Count -gt 0){$current=$stack.Pop();try{$children=[IO.Directory]::EnumerateFileSystemEntries($current)}catch{$workspace.errors++;continue};foreach($childPath in $children){try{$item=Get-Item -LiteralPath $childPath -Force -ErrorAction Stop;if(Test-ReparsePoint $item){continue};$relative=Get-ContainedRelativePath -Root $Root -Path $item.FullName;$parts=$relative -split '[\\/]';$primary=$parts[0];$keys=@($primary);if(($primary -in @('target','target-verify','target-debug','.catdesk')) -and $parts.Count -gt 1){$keys+=($primary+'\'+$parts[1])};if($item.PSIsContainer){$workspace.dirs++;foreach($key in $keys){(& $ensure $key).dirs++};$stack.Push($item.FullName)}else{$workspace.files++;$workspace.bytes=[Int64]($workspace.bytes+[Int64]$item.Length);foreach($key in $keys){$bucket=& $ensure $key;$bucket.files++;$bucket.bytes=[Int64]($bucket.bytes+[Int64]$item.Length)}}}catch{$workspace.errors++}}
    }
    $entries=@($buckets.Keys|ForEach-Object{$key=$_;$value=$buckets[$key];$policy=Get-Classification $key;[pscustomobject][ordered]@{path=$key;bytes=$value.bytes;gib=[Math]::Round($value.bytes/1GB,3);fileCount=$value.files;directoryCount=$value.dirs;accessErrors=$value.errors;classification=$policy.Category;deletionEverEligible=$policy.Eligible;rationale=$policy.Rationale}}|Sort-Object bytes -Descending)
    return [pscustomobject]@{workspace=$workspace;entries=$entries}
}

$root=(Resolve-Path -LiteralPath $Workspace -ErrorAction Stop).Path
$measured=Measure-WorkspaceAudit -Root $root;$entries=$measured.entries
$report=[pscustomobject][ordered]@{schemaVersion=1;generatedAtUtc=[DateTime]::UtcNow.ToString('o');workspace='<workspace>';workspaceBytes=$measured.workspace.bytes;workspaceGiB=[Math]::Round($measured.workspace.bytes/1GB,3);reparsePointsFollowed=$false;entries=$entries}
if($JsonPath){$out=[IO.Path]::GetFullPath($JsonPath);$parent=Split-Path -Parent $out;if(-not(Test-Path -LiteralPath $parent)){throw 'JSON output parent must already exist'};$report|ConvertTo-Json -Depth 5|Set-Content -LiteralPath $out -Encoding utf8}
$entries | Select-Object path,bytes,gib,fileCount,directoryCount,classification,deletionEverEligible,rationale | Format-Table -AutoSize
$report | ConvertTo-Json -Depth 5
