param(
    [Parameter(Mandatory)][string]$PythonInstallation,
    [Parameter(Mandatory)][string]$ReviewedSitePackages,
    [switch]$ResumeIncompleteInstall
)
$ErrorActionPreference = 'Stop'
$wakeRoot = Join-Path $env:LOCALAPPDATA 'CatDeskWake'
$runtime = Join-Path $wakeRoot 'runtime'
$manifestPath = Join-Path $wakeRoot 'browser-runtime-manifest.json'
$base = (Resolve-Path -LiteralPath $PythonInstallation).Path
$packages = (Resolve-Path -LiteralPath $ReviewedSitePackages).Path
$resumedIncomplete = $false

if (Test-Path -LiteralPath $runtime) {
    if (Test-Path -LiteralPath $manifestPath) {
        throw 'Existing completed runtime must be inspected; this installer does not overwrite it'
    }
    if (-not $ResumeIncompleteInstall) {
        throw 'Existing incomplete runtime detected; rerun with -ResumeIncompleteInstall only after confirming it was created by the interrupted installer attempt'
    }
    $runtimePython = Join-Path $runtime 'python.exe'
    $runtimePackages = Join-Path $runtime 'Lib\site-packages'
    if (-not (Test-Path -LiteralPath $runtimePython -PathType Leaf) -or -not (Test-Path -LiteralPath $runtimePackages -PathType Container)) {
        throw 'Existing incomplete runtime is not safely resumable; required copied runtime components are missing'
    }
    $resumedIncomplete = $true
} else {
    New-Item -ItemType Directory -Path $runtime | Out-Null
    foreach ($name in @('python.exe','pythonw.exe','python3.dll','python312.dll','vcruntime140.dll','vcruntime140_1.dll','LICENSE.txt','DLLs','tcl')) {
        Copy-Item -LiteralPath (Join-Path $base $name) -Destination $runtime -Recurse
    }
    $lib = New-Item -ItemType Directory -Path (Join-Path $runtime 'Lib')
    Get-ChildItem -LiteralPath (Join-Path $base 'Lib') | Where-Object { $_.Name -notin @('site-packages','__pycache__') } | ForEach-Object {
        Copy-Item -LiteralPath $_.FullName -Destination $lib.FullName -Recurse
    }
    Copy-Item -LiteralPath $packages -Destination (Join-Path $lib.FullName 'site-packages') -Recurse
}

New-Item -ItemType Directory -Path (Join-Path $wakeRoot 'browser-profile') -Force | Out-Null
$runtimePython = Join-Path $runtime 'python.exe'
$verificationOutput = @(& $runtimePython -I -c 'import sys,seleniumbase; print(sys.executable); print(seleniumbase.__version__)')
$verificationExit = $LASTEXITCODE
if ($verificationExit -ne 0) { throw 'Independent browser runtime import verification failed' }
$verificationLines = @($verificationOutput | ForEach-Object { [string]$_ } | Where-Object { -not [string]::IsNullOrWhiteSpace($_) })
if ($verificationLines.Count -lt 2) { throw 'Independent browser runtime verification returned incomplete output' }
$reportedPython = [IO.Path]::GetFullPath($verificationLines[0].Trim())
$expectedPython = [IO.Path]::GetFullPath($runtimePython)
if (-not [string]::Equals($reportedPython, $expectedPython, [StringComparison]::OrdinalIgnoreCase)) {
    throw 'Independent browser runtime verification executed an unexpected Python interpreter'
}
if ($verificationLines[$verificationLines.Count - 1].Trim() -ne '4.51.5') {
    throw 'Independent browser runtime SeleniumBase version mismatch'
}

$runtimePrefix = $runtime.TrimEnd([IO.Path]::DirectorySeparatorChar, [IO.Path]::AltDirectorySeparatorChar) + [IO.Path]::DirectorySeparatorChar
$manifest = Get-ChildItem -LiteralPath $runtime -File -Recurse | Where-Object { $_.Extension -ne '.pyc' } | ForEach-Object {
    $fullName = [IO.Path]::GetFullPath($_.FullName)
    if (-not $fullName.StartsWith($runtimePrefix, [StringComparison]::OrdinalIgnoreCase)) {
        throw 'Browser runtime manifest encountered a file outside the runtime root'
    }
    [pscustomobject]@{ Path = $fullName.Substring($runtimePrefix.Length); Sha256 = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant(); Length = $_.Length }
}
$manifestJson = $manifest | ConvertTo-Json -Depth 3
[IO.File]::WriteAllText($manifestPath, $manifestJson, (New-Object Text.UTF8Encoding($false)))
[pscustomobject]@{Runtime=$runtime;Files=$manifest.Count;Python='3.12';SeleniumBase='4.51.5';SourceDependencyAtRuntime=$false;ResumedIncompleteInstall=$resumedIncomplete} | ConvertTo-Json
