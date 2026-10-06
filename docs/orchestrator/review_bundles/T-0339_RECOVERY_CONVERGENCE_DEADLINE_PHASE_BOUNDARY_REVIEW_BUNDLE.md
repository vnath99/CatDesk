# T-0339 — Recovery convergence deadline phase boundary review bundle

## Status
Implementation/test complete. Independent final acceptance remains under T-0319.

## Defect
The final `readinessDeadline` was created at bootstrap entry, before bounded wake-runtime repair and ordered legacy-to-official Secure MCP migration. Wake repair can legitimately consume up to 25 minutes while `ReadyTimeoutSeconds` is constrained to 5–180 seconds. A long but successful prerequisite phase could therefore expire the final readiness budget and make one-command `recover` require a second invocation.

## Change
Move creation of the final convergence deadline until after wake-runtime repair and any ordered runtime migration have successfully completed. Those prerequisite phases retain their existing independent bounded timeouts. The final daemon/runtime convergence phase now receives a fresh full `ReadyTimeoutSeconds` budget.

No new authority over the externally owned Secure MCP runtime is introduced, and protected wake state is unchanged.

## Regression
The existing fake recovery flow now delays wake repair and migration by 1.2 seconds each with `ReadyTimeoutSeconds=5`, invokes `Invoke-CanonicalStackBootstrap` once, requires at least 4.5 seconds of final readiness budget after prerequisites complete, and verifies the action order remains `wake-repair,stop:130,migrate,start:--catdesk-daemon`.

## Verification
- `cargo test --test recovery_powershell -- --nocapture` — 2/2 passed.
- `cargo fmt --all -- --check` — passed.
- `cargo test --all-targets --all-features` — passed.
- `cargo build --all-targets --all-features` — passed.
- `cargo clippy --all-targets --all-features -- -D warnings` — passed.
- `git diff --check` — passed.

## Acceptance boundary
T-0319 remains the independent acceptance gate for the cumulative T-0325 through T-0339 recovery-hardening chain. This implementation lineage does not self-accept that gate.
