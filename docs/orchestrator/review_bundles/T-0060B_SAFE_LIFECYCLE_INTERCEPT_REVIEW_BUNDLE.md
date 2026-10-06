# T-0060B Safe Lifecycle Intercept Review Bundle

## Implementation

`run_command` now has a narrow typed intercept for exactly seven workspace-root
public lifecycle requests:

- `.\catdesk.ps1 status`, `start`, `recover`, or `stop`
- `.\catdesk.ps1 autostart status`, `enable`, or `disable`

The only equivalent accepted script spelling is `./catdesk.ps1`. Matching is
case-insensitive for Windows path and PowerShell parameter semantics. The
intercept resolves the canonical root `catdesk.ps1`, uses a fixed direct
`powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File`
argument vector, and appends only hard-coded lifecycle tokens. It never sends
the supplied command string, `cwd`, user paths, flags, environment values, or
credentials to a child process.

## Security invariants

- `shell_mode = allowlist` is unchanged; unrelated shell commands remain
  blocked.
- Extra arguments, `-Workspace`, arbitrary/parent/absolute scripts, quotes,
  environment assignments, expressions, chaining, pipes, redirection, and
  near-miss verbs do not match the intercept.
- Synchronous calls have fixed time limits and require a small exact redacted
  facade JSON contract. Nonzero, malformed, stderr-producing, or oversized
  output fails closed without returning raw child output.
- MCP `stop` returns `STOP_ACKNOWLEDGED` first, then a detached native CatDesk
  helper waits briefly and invokes only `catdesk.ps1 stop`. The facade retains
  the existing canonical-listener/PID verification before it can terminate the
  local daemon. The helper has no arbitrary shell text, PID argument, tunnel
  action, credential, or external runtime operation.

## Regression evidence

Focused unit tests cover all accepted operations, rejected near-misses, direct
argument construction, bounded/redacted facade output parsing, dry-run
intercept routing, unchanged allowlist rejection for unrelated commands, and
exact stop-helper argument parsing. No test schedules tasks, stops a daemon,
launches a browser, or changes a tunnel.

Observed local verification: focused lifecycle-intercept tests, lifecycle
fixtures, `cargo build`, `cargo fmt --check`, `cargo clippy --all-targets
--all-features -- -D warnings`, and `git diff --check` passed. `cargo test`
ran 467 tests: 449 passed, 11 ignored, and 7 host-only failures remained from
an unavailable advisor program and Windows process-tree access denial; these
are outside this intercept change.

No live lifecycle, Scheduled Task, daemon/reload, browser, official runtime,
credential, or Git publication action was performed. Independent review and
review-inbox wake remain CatDesk-host actions pending verification.
