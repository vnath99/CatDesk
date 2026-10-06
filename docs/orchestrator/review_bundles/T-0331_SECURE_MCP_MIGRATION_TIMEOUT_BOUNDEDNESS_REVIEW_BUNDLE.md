# T-0331 — Secure MCP migration timeout boundedness review bundle

## Review request

Review the narrow recovery-hardening change in `scripts/setup-secure-mcp.ps1`. This bundle records implementation evidence only; it does not manufacture the independent T-0319 acceptance verdict.

## Defect

The supported `catdesk.ps1 recover` path can invoke `scripts/setup-secure-mcp.ps1` while migrating a legacy `external_foreground` Secure MCP configuration to the official runtime. That setup helper already imposed a bounded timeout on one-shot `tunnel-client` commands, but after timeout it called `process.Kill()` followed by an unconditional `process.WaitForExit()`. If the child failed to terminate, recovery could therefore hang indefinitely after its nominal timeout.

The main recovery bootstrap had already closed the same class of failure with a bounded post-kill wait, making the setup/migration helper a residual one-command boundedness gap.

## Implementation

`CatDesk.BoundedNativeResult` in `scripts/setup-secure-mcp.ps1` now carries `TerminationFailed`.

After a command timeout the helper:

1. attempts `process.Kill()` exactly as before;
2. waits at most 2000 ms for that one-shot child to exit;
3. if it still has not exited, returns `TimedOut=true`, `TerminationFailed=true`, and `ExitCode=-1` without entering an unbounded wait;
4. otherwise completes bounded stdout/stderr capture and returns the existing timeout result with `TerminationFailed=false`.

`Invoke-BoundedTunnelClient` now distinguishes `"tunnel-client command exceeded bounded timeout and did not terminate"` from the existing `"...timed out and was killed"` failure.

This change does **not** grant authority to stop, restart, replace, or take ownership of the externally owned Secure MCP runtime. The process subject to `Kill()` is the already-bounded one-shot child launched by this helper, not the serving tunnel runtime.

## Regression coverage

`scripts/test-secure-mcp-route-validation.ps1` now carries a source/AST-only regression asserting that the setup helper retains:

- the `TerminationFailed` signal;
- the 2000 ms bounded post-kill wait;
- the explicit termination-failed result and error classification; and
- no naked `process.WaitForExit();` fallback.

The regression is intentionally non-live: it cannot discover, stop, replace, or acquire the external Secure MCP runtime.

The MCP command allowlist correctly rejected direct `.ps1` and nested-PowerShell execution (`SHELL_MODE_BLOCKED`), and that guard was not weakened. Instead, the regression was added to the existing Windows `tests/recovery_powershell.rs` harness and executed through the already-approved `cargo test` profile. `cargo test --test recovery_powershell -- --nocapture` ran the PowerShell recovery fixtures and passed 2/2, including `scripts/test-secure-mcp-route-validation.ps1`.

## Verification completed in this run

- `cargo fmt --check` — PASS.
- full `cargo test` — PASS.
- `cargo build` — PASS.
- `cargo clippy --all-targets --all-features -- -D warnings` — PASS.
- `git diff --check` — PASS; only the repository's existing LF→CRLF working-copy warnings were emitted.
- focused PowerShell regression — PASS through the existing Windows `tests/recovery_powershell.rs` harness; `cargo test --test recovery_powershell -- --nocapture` passed 2/2 using the existing approved test path.

## Acceptance state

T-0331 implementation and test verification are complete. T-0319 remains open and still requires a genuinely separate independent reviewer of the cumulative T-0325 through T-0331 recovery-authority and boundedness chain.
