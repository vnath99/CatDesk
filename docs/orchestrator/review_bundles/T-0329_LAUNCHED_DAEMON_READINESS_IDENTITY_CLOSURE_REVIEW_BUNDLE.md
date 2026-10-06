# T-0329 — Launched-daemon readiness identity closure

## Scope

Close the remaining one-command recovery launch/readiness identity race in `scripts/start-catdesk-stack.ps1` without taking ownership of the externally managed Secure MCP runtime.

## Finding

When recovery observed no usable daemon, it launched the canonical binary with `Start-Process ... | Out-Null` and discarded the spawned process object. `Wait-FullStackReadiness` then accepted any listener whose path/hash matched the canonical binary. A concurrent canonical daemon could therefore win the listener race and satisfy readiness even if the process started by this recovery invocation exited or never acquired the port.

This was distinct from T-0328: T-0328 bound the detached restart worker's replacement handoff, whereas this path is the direct missing-daemon launch inside the one-command bootstrap.

## Implementation

- `Start-Process` now uses `-PassThru` for the direct daemon launch.
- The returned process is pinned immediately by acquiring its OS handle.
- `Wait-FullStackReadiness` accepts an optional expected daemon process and forwards it to local MCP readiness.
- When an expected process is present, `Test-LocalMcpReadiness` requires the listener PID to equal the spawned process PID and reuses `Test-CatDeskPinnedProcessMatchesRecoveryIdentity` to prove creation-time/path identity on that exact pinned object.
- Existing-daemon readiness is unchanged because no expected process is supplied in that case.
- No raced-in process is killed merely for failing this identity check; the boundary is fail-closed readiness, not new destructive authority.

## Regression coverage

`scripts/test-start-catdesk-stack.ps1` now verifies:

1. a canonical listener owned by a different PID is rejected when an exact launched process is expected;
2. the exact launched process/listener pair is accepted;
3. the process returned by the launch path is actually forwarded into readiness.

`tests/recovery_powershell.rs` executes `scripts/test-start-catdesk-stack.ps1`, so the full Rust test suite exercised this PowerShell regression fixture.

## Verification

- `cargo fmt --check` — PASS
- `cargo test` — PASS, including `tests/recovery_powershell.rs` and the bootstrap PowerShell fixture
- `cargo build` — PASS
- `cargo clippy --all-targets --all-features -- -D warnings` — PASS
- `git diff --check` — PASS; only pre-existing LF→CRLF working-copy warnings were emitted

## Safety / ownership

- Existing official Secure MCP runtime remained externally owned and untouched.
- No tunnel restart/replacement occurred.
- No browser wake bridge was invoked.
- No protected wake-target state was edited.
- Dirty worktree was preserved; no reset/cleanup was performed.

## Review disposition

T-0329 implementation and bounded regression verification are complete. T-0319 remains open for a genuinely independent final review of the cumulative recovery chain, now including T-0325 through T-0329. This implementation lineage must not self-approve that acceptance gate.
