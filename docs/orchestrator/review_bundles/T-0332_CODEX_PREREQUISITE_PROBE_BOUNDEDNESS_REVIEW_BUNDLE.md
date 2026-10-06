# T-0332 — Codex prerequisite probe boundedness review bundle

## Scope

Close a one-command recovery boundedness defect in `scripts/start-catdesk-stack.ps1`: both `plan` and `recover` called `codex --version` and `codex login status` synchronously before recovery orchestration. A wedged Codex CLI could therefore hang `catdesk.ps1 recover` indefinitely even though Codex is only an on-demand worker prerequisite and not daemon/tunnel recovery authority.

## Implementation

- Added explicit Codex probe bounds: 5,000 ms per probe and 64 KiB output capture.
- Added `Invoke-BoundedCodexProbe`, reusing the existing `CatDesk.BoundedNativeProcess` timeout/capture implementation.
- Preserved `.cmd`/`.bat` and `.ps1` command-wrapper compatibility while keeping the probe arguments fixed to internal tokens.
- `Test-CodexOnDemandPrerequisites` now runs both version and authentication checks through the bounded probe and preserves the prior fail-closed unavailable/non-runnable/authentication semantics.
- Tightened `BoundedNativeProcess.Run`: after a timeout, once the selected wrapper process has been killed and exits, the helper returns the timeout result without an unbounded `Task.WaitAll` on inherited stdout/stderr handles. This prevents a wrapper child retaining a pipe handle from turning a bounded timeout back into an unbounded lifecycle wait.
- No CatDesk daemon, Secure MCP runtime, tunnel, protected wake target, release bytes, or promotion authority is mutated by this change.

## Regression coverage

`scripts/test-start-catdesk-stack.ps1` now:

1. imports `Invoke-BoundedCodexProbe` from the production script;
2. creates a deterministic fake Codex `.cmd` wrapper that loops indefinitely;
3. lowers only the test probe bound to 250 ms and proves the probe fails with a bounded-timeout diagnostic in under 3 seconds;
4. statically rejects reintroduction of direct `& $codex.Source` prerequisite probes;
5. retains the existing bounded tunnel-client regression.

The approved Windows harness remains `tests/recovery_powershell.rs`.

## Verification

- `cargo test --test recovery_powershell -- --nocapture` — PASS, 2 passed / 0 failed.
- `cargo fmt --check` — PASS.
- `cargo test` — PASS.
- `cargo build` — PASS.
- `cargo clippy --all-targets --all-features -- -D warnings` — PASS.
- `git diff --check` — PASS; only pre-existing Windows LF→CRLF working-copy warnings were emitted.

## Review decision requested

Verify that T-0332 correctly treats Codex as a bounded prerequisite rather than recovery authority, that timeout/capture behavior cannot reintroduce an unbounded wait after timeout, and that no externally owned Secure MCP or protected wake authority has been acquired. T-0319 must remain independently reviewed; this implementation lineage does not self-accept the cumulative recovery chain.
