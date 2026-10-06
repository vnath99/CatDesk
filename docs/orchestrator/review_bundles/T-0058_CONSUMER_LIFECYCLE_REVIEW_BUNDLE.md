# T-0058 Consumer Lifecycle Review Bundle

## Delivered

- Added root `catdesk.ps1`, the only PUBLIC/SUPPORTED consumer lifecycle
  facade, with `install`, `start`, `status`, `recover`, and `stop` commands.
- Added deterministic `scripts/test-catdesk-lifecycle.ps1` coverage.
- Updated the README and canonical architecture to direct routine users to the
  facade and identify lower-level scripts as INTERNAL/ADVANCED.

## Command semantics and ownership

| Command | Behavior |
| --- | --- |
| `install` | Validates a fingerprinted canonical release, current-user Codex availability/authentication without reading auth contents, tunnel-client availability, and bounded wake runtime; it may use the existing installer/repair helpers but does not compile, provision a release, log in, or create connector resources. |
| `start` / `recover` | First require the canonical release path and SHA-256 identity, then delegate to the existing `start-catdesk-stack.ps1` recovery engine. Normal execution never compiles. |
| `status` | Performs bounded read-only local/runtime checks and returns only fixed redacted state vocabulary. Its local side is READY only after the existing JSON-RPC `initialize` and `tools/list` readiness path verifies the required tools; canonical listener ownership alone is insufficient. It never repair/migrates/reconnects/launches/stops or changes configuration. |
| `stop` | Resolves the configured loopback listener and stops it only after the existing helper positively verifies the canonical CatDesk executable path and SHA-256. Missing, ambiguous, or mismatched listeners fail closed. |

The official OpenAI Secure MCP tunnel runtime remains external/operator owned.
The facade contains no tunnel-runtime stop/remove route. Its CatDesk-only stop
action never targets a tunnel process. Codex remains on demand rather than a
permanent lifecycle process.

## Deterministic verification evidence

`scripts/test-catdesk-lifecycle.ps1` passed. It covers arbitrary-CWD invocation,
the exact command vocabulary and routing, no compile/provision/tunnel-stop
surface, non-mutating redacted status, idempotent healthy status, start/recover
canonical routing, bounded install preparation, verified-only stop, mismatch
refusal, missing release fail-closed behavior, and reduction of unexpected
route-/credential-shaped internal recovery output to a fixed public state.
T-0058-R1 additionally proves that a canonical listener with failed/unavailable
JSON-RPC readiness is never reported READY, while local JSON-RPC readiness plus
an unavailable external runtime reports `LOCAL_READY_EXTERNAL_RUNTIME_PENDING`.

The affected `scripts/test-start-catdesk-stack.ps1` behavioral suite also
passed. It continues to cover canonical release manifest mismatch rejection,
release-only recovery, native daemon orchestration, redacted readiness, and
external official-runtime ownership boundaries.

## Required project verification

Run:

- `powershell -ExecutionPolicy Bypass -File scripts/test-catdesk-lifecycle.ps1`
- `powershell -ExecutionPolicy Bypass -File scripts/test-start-catdesk-stack.ps1`
- `cargo fmt --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test`
- `git diff --check`

No T-0058 verification invokes the new public facade against the live machine.
No live CatDesk daemon, official tunnel runtime, browser, credential, account,
connector, or Git state was modified by this ticket.
