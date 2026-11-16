# Exercises the real bounded helper, not the BackendOnlySupport fixture seam.
# The closed capability invocation creates no application state or services.
[CmdletBinding()]
param(
    [Parameter(Mandatory=$true)][string]$Executable,
    [Parameter(Mandatory=$true)][ValidatePattern('^[a-f0-9]{64}$')][string]$ExpectedSha256
)
$ErrorActionPreference='Stop'
Set-StrictMode -Version Latest
$resolved=(Resolve-Path -LiteralPath $Executable -ErrorAction Stop).Path
if ((Get-FileHash -LiteralPath $resolved -Algorithm SHA256).Hash.ToLowerInvariant() -cne $ExpectedSha256) {
    throw 'capability test executable identity differs'
}
. (Join-Path $PSScriptRoot 'start-catdesk-stack.ps1')
$canonical=[pscustomobject]@{Path=$resolved;Sha256=$ExpectedSha256}
if (-not (Test-BackendOnlySupport -Canonical $canonical -TimeoutMilliseconds 5000)) {
    throw 'native capability output was not accepted through the real bounded helper'
}
if ($script:BootstrapSeams.Count -ne 0) { throw 'native capability test unexpectedly installed fixture seams' }
[pscustomobject]@{
    state='NATIVE_CAPABILITY_VERIFIED';powerShellVersion=$PSVersionTable.PSVersion.ToString()
    executableSha256=$ExpectedSha256;nativeCapabilityQueries=1;daemonStarts=0;runtimeWrites=0
} | ConvertTo-Json -Compress
