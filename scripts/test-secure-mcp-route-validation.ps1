$ErrorActionPreference = "Stop"

# Extract only the validation helpers from the setup script.  This regression
# test never runs setup modes, discovers a client, reads an operator config, or
# invokes a tunnel command.
$setup = Join-Path $PSScriptRoot "setup-secure-mcp.ps1"
$source = Get-Content -LiteralPath $setup -Raw
$tokens = $null
$parseErrors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseInput($source, [ref]$tokens, [ref]$parseErrors)
if ($parseErrors.Count -gt 0) { throw "setup-secure-mcp.ps1 did not parse" }

foreach ($name in @("Test-SafeRuntimeRoute", "Get-LocalMcpRuntimeUrl", "Get-TunnelClientOutput", "Migrate-LegacyOfficialRuntimeConfig")) {
    $functionAst = $ast.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name }, $true)
    if ($null -eq $functionAst) { throw "missing $name" }
    . ([scriptblock]::Create($functionAst.Extent.Text))
}

$missingClient = Get-TunnelClientOutput -Path "" -CommandArgs @("runtimes", "status")
if ($missingClient.State -ne "CLIENT_UNAVAILABLE" -or $null -ne $missingClient.Output) {
    throw "missing tunnel client was not classified without output"
}

$validRoute = "abcdefghijklmnopqrstuvwxyz012345"
if (-not (Test-SafeRuntimeRoute -Route $validRoute)) { throw "valid route rejected" }
foreach ($route in @(
    "<redacted>",
    "%3Credacted%3E",
    "%253Credacted%253E",
    "<persistent-route>",
    "placeholder-route-placeholder",
    "",
    "short"
)) {
    if (Test-SafeRuntimeRoute -Route $route) { throw "unsafe route accepted" }
}

$config = [pscustomobject]@{ Found = $true; BindHost = "127.0.0.1"; Port = 3200; Route = $validRoute }
$target = Get-LocalMcpRuntimeUrl -McpConfig $config
if ($target -ne "http://127.0.0.1:3200/$validRoute/mcp") { throw "valid route target was not constructed" }
$config.BindHost = "::1"
$ipv6Target = Get-LocalMcpRuntimeUrl -McpConfig $config
if ($ipv6Target -ne "http://[::1]:3200/$validRoute/mcp") { throw "IPv6 loopback target was not bracketed" }
$config.BindHost = "127.0.0.1"
$config.Route = "%253Credacted%253E"
try {
    $null = Get-LocalMcpRuntimeUrl -McpConfig $config
    throw "encoded redaction became a runtime target"
} catch {
    if ($_.Exception.Message -match "encoded redaction became") { throw }
}

if ($source -notmatch '\$localMcpUrl\s*=\s+Get-LocalMcpRuntimeUrl' -or
    $source -notmatch '"--mcp-server-url",\s*\$localMcpUrl' -or
    $source -notmatch 'Invoke-BoundedTunnelClient' -or
    $source -match '&\s+\$ClientPath\s+runtimes\s+(?:connect|stop)' -or
    $source -notmatch '"COMMAND_FAILED"' -or
    $source -notmatch '"<redacted-status-command-failed>"') {
    throw "connect command does not use validated internal route construction"
}

# Recovery can invoke this setup helper while migrating a legacy external_foreground
# configuration to the official runtime. A tunnel-client command that times out must
# therefore never regain an unbounded wait after Kill(). This test is deliberately
# source/AST-only so it cannot discover, stop, replace, or take ownership of a live
# Secure MCP runtime.
if ($source -notmatch 'public bool TerminationFailed;' -or
    $source -notmatch 'public bool OutputDrainTimedOut;' -or
    $source -notmatch 'process\.WaitForExit\(2000\)' -or
    $source -notmatch 'Task\.WaitAll\(new Task\[\] \{ stdout, stderr \}, 2000\)' -or
    $source -notmatch 'OutputDrainTimedOut = true' -or
    $source -notmatch 'output drain exceeded bounded timeout' -or
    $source -notmatch 'TerminationFailed = true' -or
    $source -notmatch 'bounded timeout and did not terminate' -or
    $source -match 'Task\.WaitAll\(stdout, stderr\);' -or
    $source -match 'process\.WaitForExit\(\);') {
    throw "setup tunnel-client process or output-drain timeout is not bounded"
}

$root = Join-Path ([IO.Path]::GetTempPath()) ("catdesk-t0051-migration-" + [Guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $root | Out-Null
try {
    $configPath = Join-Path $root "config.toml"
    $original = @'
# preserved comment
[tunnel]
mode = "openai_secure_tunnel"
keep_this = true

[openai_tunnel]
process_mode = "external_foreground" # legacy operator setting
auto_recover = true

[unrelated]
value = "preserve"
'@
    [IO.File]::WriteAllText($configPath, $original, [Text.UTF8Encoding]::new($false))
    $migration = Migrate-LegacyOfficialRuntimeConfig -Path $configPath
    if ($migration -notmatch "migrated") { throw "minimal legacy config was not migrated" }
    $migrated = [IO.File]::ReadAllText($configPath)
    if ($migrated -notmatch 'process_mode\s*=\s*"official_runtime"' -or
        $migrated -notmatch '# preserved comment' -or
        $migrated -notmatch 'value\s*=\s*"preserve"') {
        throw "migration did not preserve unrelated content"
    }
    if (-not (Test-Path -LiteralPath "$configPath.pre-t0051-official-runtime.bak" -PathType Leaf)) {
        throw "migration did not create recoverable backup"
    }
    $afterFirst = $migrated
    $idempotent = Migrate-LegacyOfficialRuntimeConfig -Path $configPath
    if ($idempotent -notmatch "already official_runtime" -or [IO.File]::ReadAllText($configPath) -ne $afterFirst) {
        throw "migration was not idempotent"
    }
    $staleWriter = Join-Path $root "stale-writer.toml"
    [IO.File]::WriteAllText($staleWriter, $original, [Text.UTF8Encoding]::new($false))
    $preservedBackup = "$staleWriter.pre-t0051-official-runtime.bak"
    [IO.File]::WriteAllText($preservedBackup, "original-recoverable-evidence", [Text.UTF8Encoding]::new($false))
    $recovered = Migrate-LegacyOfficialRuntimeConfig -Path $staleWriter -PreserveExistingBackup
    if ($recovered -notmatch "backup preserved" -or [IO.File]::ReadAllText($preservedBackup) -ne "original-recoverable-evidence") {
        throw "stale-writer recovery overwrote the existing backup"
    }
    foreach ($case in @(
        "[tunnel]`nmode = ""openai_secure_tunnel""`n[openai_tunnel]`nprocess_mode = ""external_foreground""`n[openai_tunnel]`nprocess_mode = ""external_foreground""",
        "[tunnel`nmode = ""openai_secure_tunnel""`n[openai_tunnel]`nprocess_mode = ""external_foreground"""
    )) {
        $invalid = Join-Path $root ("invalid-" + [Guid]::NewGuid().ToString("N") + ".toml")
        [IO.File]::WriteAllText($invalid, $case, [Text.UTF8Encoding]::new($false))
        try {
            $null = Migrate-LegacyOfficialRuntimeConfig -Path $invalid
            throw "ambiguous or malformed config was migrated"
        } catch {
            if ($_.Exception.Message -match "ambiguous or malformed config was migrated") { throw }
        }
        if (Test-Path -LiteralPath "$invalid.pre-t0051-official-runtime.bak" -PathType Leaf) {
            throw "failed migration created a backup"
        }
    }
    $migrationAst = $ast.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq "Migrate-LegacyOfficialRuntimeConfig" }, $true)
    if ($migrationAst.Extent.Text -match 'route_id|CONTROL_PLANE_API_KEY') {
        throw "migration implementation inspects restricted config values"
    }
} finally {
    Remove-Item -LiteralPath $root -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Output "secure MCP route and migration validation tests passed"
