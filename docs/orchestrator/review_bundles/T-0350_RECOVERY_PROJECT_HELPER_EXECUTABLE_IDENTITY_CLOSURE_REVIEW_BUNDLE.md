# T-0350 — Recovery project-helper executable identity closure

## Scope

T-0350 closes the remaining policy-drift seam for project-owned PowerShell helpers reachable from public one-command recovery. The change does not expand CatDesk ownership of the externally owned Secure MCP runtime and does not require a clean worktree.

## Finding

Recovery already had an exact helper-path validator, `Resolve-TrustedBootstrapHelperPath`, which requires a real `FileInfo` leaf, rejects `ReparsePoint`, normalizes the expected and observed full paths, and requires exact `OrdinalIgnoreCase` identity. Release recovery is validated before its initial dot-source, while wake repair, ordered Secure MCP migration, and interrupted-promotion rollback validate their project helper before launching the fixed OS Windows PowerShell child.

The bounded Windows inventory helper had equivalent leaf/reparse/full-path checks duplicated locally instead of using the shared validator. Although the duplicated checks were presently equivalent, that left executable-authority policy split across two implementations and created a regression surface where one path could drift independently.

## Implementation

`Invoke-BoundedWindowsInventoryProbe` now resolves `query-catdesk-windows-inventory.ps1` through `Resolve-TrustedBootstrapHelperPath` before invoking it with the fixed OS Windows PowerShell identity. Its existing 5-second parent deadline, output cap, output-drain handling, JSON validation, and read-only provider role are unchanged.

The shared resolver continues to protect:

- `catdesk-release-recovery.ps1` before in-process dot-sourcing;
- `repair_wake_bridge_environment.ps1` before bounded wake-runtime repair;
- `setup-secure-mcp.ps1` before bounded ordered migration;
- `promote-reviewed-catdesk-build.ps1` before bounded rollback-only interrupted-promotion recovery; and
- `query-catdesk-windows-inventory.ps1` before bounded Windows provider observation.

This is intentionally leaf identity protection, not a clean-worktree/content-signing requirement. Existing dirty-worktree preservation remains unchanged.

## Regression coverage

`scripts/test-start-catdesk-stack.ps1` requires the shared resolver to retain `Get-Item`, `ReparsePoint`, `GetFullPath`, and exact `OrdinalIgnoreCase` comparison, verifies a normal synthetic helper resolves to its exact expected path, verifies release recovery cannot be dot-sourced before validation, and requires the wake repair, migration, promotion rollback, and Windows inventory paths to call the shared resolver.

Centralizing the inventory helper exposed a fixture dependency in `scripts/test-stale-canonical-daemon-recovery.ps1`: that fixture extracts production functions individually and did not import the newly required shared resolver. The fixture now imports `Resolve-TrustedBootstrapHelperPath` as part of the same production-function set. This was a test-harness dependency, not a production recovery failure.

## Verification performed

- `cargo test --test recovery_powershell -- --nocapture`: 2 passed / 0 failed.
- CatDesk sanctioned verifier: `cargo fmt --check` passed.
- CatDesk sanctioned verifier: default full `cargo test` passed.
- CatDesk sanctioned verifier: `cargo build` passed.
- `cargo test --all-targets --all-features` passed.
- `cargo clippy --all-targets --all-features -- -D warnings` passed.
- `git diff --check` exited 0; only the repository's existing LF-to-CRLF warnings were emitted.

The first broad verifier run correctly failed in the stale-daemon fixture because the fixture had not imported the shared resolver. After fixing that fixture dependency, the focused and broad verification gates above passed.

## Runtime / operator safety

No live `recover`, reviewed promotion, CatDesk daemon/tunnel restart or migration, manual browser wake, protected wake-target mutation, Scheduler mutation, Git publication, provider re-probe, Qwen implementation dispatch, or dirty-worktree cleanup was performed. The serving Secure MCP runtime remains externally owned.

## Independent review boundary

T-0350 is implementation/repository-verification complete but is not self-accepted. T-0319 remains open for genuinely separate independent final review of cumulative T-0325 through T-0350. The reviewer should confirm that every project-owned PowerShell helper executed by public recovery crosses the shared exact non-reparse leaf-identity boundary before execution, that Windows inventory retains its independent bounded-child semantics, and that this closure neither adds runtime ownership nor converts dirty-worktree state into an acceptance requirement.

T-0324 remains separately gated on reviewed promotion before its bounded live Qwen continuation canary.
