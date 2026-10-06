# T-0340 — Wake Python launcher discovery boundedness review bundle

## Status
Implementation/test complete. Independent final acceptance remains under T-0319.

## Defect
`scripts/repair_wake_bridge_environment.ps1` participates in one-command CatDesk recovery, but `Resolve-Python3` still invoked `py.exe` directly to discover the selected Python interpreter. That native child bypassed the repair script's bounded process primitive. A wedged Python launcher could therefore hold wake-environment repair until the much larger parent recovery-helper deadline instead of failing on a local bounded discovery deadline.

## Change
Route `py.exe` interpreter discovery through the existing `Invoke-NativeQuiet` bounded native-process helper with an explicit 5-second timeout. The discovery child writes `sys.executable` to a temporary file rather than redirected stdout, avoiding a new inherited-pipe/output-drain boundary. The discovered interpreter is then subjected to the existing bounded `Test-Python3` validation before it can be selected.

This changes only wake-runtime repair subprocess boundedness. It introduces no new authority over the externally owned Secure MCP runtime, no release promotion/restart authority, and no protected wake-target mutation.

## Regression
The approved Windows recovery harness parses `repair_wake_bridge_environment.ps1`, locates `Resolve-Python3`, rejects any return to direct `& $py.Source` execution, requires discovery to use `Invoke-NativeQuiet -Executable $py.Source`, and requires the explicit 5000 ms timeout.

## Verification
- `cargo test --test recovery_powershell -- --nocapture` — 2/2 passed.
- `cargo fmt --all -- --check` — passed.
- `cargo test --all-targets --all-features` — passed; main suite 876 passed / 0 failed / 21 ignored. The reviewed-build isolation test emits its expected access-denied child-helper proof while the enclosing suite succeeds.
- `cargo build --all-targets --all-features` — passed.
- `cargo clippy --all-targets --all-features -- -D warnings` — passed.
- `git diff --check` — passed; only existing Windows LF→CRLF warnings were emitted.

## Acceptance boundary
T-0319 remains the independent acceptance gate for the cumulative T-0325 through T-0340 recovery-hardening chain. This implementation lineage does not self-accept that gate.
