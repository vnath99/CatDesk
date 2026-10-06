# T-0137 lifecycle parameter-scope hardening review bundle

## Diagnosis

`catdesk.ps1` imports `scripts/start-catdesk-stack.ps1` to reuse its internal
lifecycle functions.  PowerShell dot-sourcing executes the imported script's
parameter block in the importing scope.  The helper has defaults for the same
five public facade variables:

| Public facade value | Helper duplicate parameter |
| --- | --- |
| `Workspace` | `Workspace` |
| `ConfigPath` | `ConfigPath` |
| `ExpectedBuildSha256` | `ExpectedBuildSha256` |
| `BuildFingerprintPath` | `BuildFingerprintPath` |
| `ReadyTimeoutSeconds` | `ReadyTimeoutSeconds` |

An isolated unbound dot-source reproduced helper-default evaluation in the
caller scope: its `Workspace` default attempted `Join-Path $PSScriptRoot ..`
while that value was empty during parameter-default evaluation.  Therefore an
unbound import can either replace the caller-bound values with helper defaults
or fail before lifecycle routing.  Neither outcome is a valid public-facade
identity boundary.

## Change

The facade snapshots the five already-bound public values, dot-sources the
helper with all five passed as explicit named arguments, then retains the
existing snapshot restoration as a second barrier against a future helper-body
assignment.  The helper remains internal and is not executed at import time.

This is deliberately limited to `catdesk.ps1`; no root/source Rust module,
tunnel owner, promotion path, browser/wake path, Scheduler surface, or host
lifecycle operation was changed.

## Canonical-identity regression

`scripts/test-catdesk-lifecycle.ps1` now creates an isolated real workspace
fixture containing `target\\release\\catdesk.exe` and its matching SHA-256
fingerprint.  It removes the `CanonicalIdentity` seam, so status and start use
the facade's real canonical identity implementation.  The fixture proves:

- `status` resolves the caller-supplied workspace and matching fingerprint;
- status forwards the exact caller `ConfigPath` through both local-MCP and
  runtime-status checks;
- `start` forwards the exact workspace, config path, expected SHA-256,
  fingerprint path, and `ReadyTimeoutSeconds` to the bounded recovery seam;
- public output remains the existing fixed redacted state vocabulary:
  `LOCAL_DAEMON_PENDING` and `CONNECTED_VERIFIED` in these fixtures.

No `CanonicalIdentity` mock is used in this regression.  Other seams only
prevent fixture tests from querying a live listener or external runtime.

## Verification

| Gate | Result |
| --- | --- |
| `powershell -NoProfile -ExecutionPolicy Bypass -File scripts/test-catdesk-lifecycle.ps1` | PASS |
| `powershell -NoProfile -ExecutionPolicy Bypass -File scripts/test-start-catdesk-stack.ps1` | PASS |
| `cargo fmt --all -- --check` | PASS |
| `cargo clippy --all-targets --all-features -- -D warnings` | PASS |
| `cargo test --all-targets --all-features --no-fail-fast` | PASS (836 unit tests plus integration suites) |
| `cargo build --all-targets --all-features` | PASS |
| `git diff --check` | PASS |
| Separate local `rust_full` runner | No standalone runner was configured/discoverable; the prescribed Rust-wide commands above were run directly. |

The Rust commands emitted the pre-existing non-fatal `could not canonicalize
path C:\\Users\\Volap` warning. `git diff --check` emitted only existing CRLF
working-copy warnings and returned success.

## Attribution and boundaries

The attributable T-0137 changes are the explicit named helper import in
`catdesk.ps1`, the five-parameter real-identity regression additions in
`scripts/test-catdesk-lifecycle.ps1`, and this bundle.  These files are already
untracked/dirty in the broad workspace; no unrelated cleanup, staging, commit,
reset, or publication was performed.

No daemon, tunnel, canonical release, promotion, wake/browser, Scheduler,
host activation, external project, signing/provenance, or Git state was
mutated.  Request ChatGPT independent final review; passing local checks are
not independent acceptance.
