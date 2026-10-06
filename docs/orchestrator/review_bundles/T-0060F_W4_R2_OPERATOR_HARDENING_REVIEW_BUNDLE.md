# T-0060F-W4-R2 Operator Hardening Review Bundle

## Release-blocking repairs addressed

The W4-R1 operator facade no longer resolves `powershell.exe` through PATH or
the current workspace. The repair action derives its only executable from
`%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe`, then requires
an absolute canonical regular, non-reparse target at exactly that expected
location and outside the workspace. No command-line, environment override,
PATH lookup, working-directory candidate, or user-supplied executable is
accepted. `%SystemRoot%` is used only as the OS-owned Windows system-root
locator.

The repair action also no longer uses the generic command timeout. Its dedicated
runner starts the fixed, noninteractive PowerShell invocation with redirected,
bounded output, assigns it to a Windows Job Object configured with
`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, and retains that job while the process
runs. On timeout or wait failure it closes the job (terminating the owned
PowerShell descendant tree) and waits for the root process before returning
attention. The wait is attempted even if tree termination itself reports an
error. Assignment failure kills and waits for the root immediately. Normal
completion also closes the job only after root exit. Output overflow or read
failure returns attention after the child has already exited; no captured output
is exposed to the operator facade.

## Preserved behavior

- CLI syntax remains exactly:

  ```text
  catdesk operator wake-target set --conversation-url https://chatgpt.com/c/<id> [--expected-current-target-sha256 <64-hex>]
  catdesk operator repair-wake-bridge-environment
  ```

- `operator wake-target set` continues to delegate directly to the reviewed W4
  canonicalization, config safety, CAS, and atomic-write implementation.
- Both actions remain terminal early exits before daemon, MCP listener, or TUI
  startup. No browser/profile/state/inbox/tunnel/Scheduler/Git surface is used.

## Changed paths

- `src/operator_facade.rs` — trusted PowerShell resolver, Job Object process
  tree runner, bounded output failure handling, and deterministic tests.
- This review bundle.

## Deterministic coverage

Focused operator-facade tests cover strict parsing, W4 target delegation,
trusted absolute system PowerShell selection, workspace-hijack resistance,
missing/unsafe resolver conditions, fixed script invocation shape, clean/nonzero
and missing-marker outcomes, output overflow/read failure, and timeout cleanup
ordering (`kill` then `wait`, including a reported kill failure). These tests use seams and do not run the actual
repair script.

Local static and regression checks completed for this change: `cargo fmt --
--check`, `cargo clippy --all-targets --all-features -- -D warnings`, and
`cargo test` (476 passed, 18 ignored). Independent CatDesk verification remains
the acceptance authority.

## Residual live sequence

After independent review: build an isolated candidate; run its fixed repair
action; run its guarded current-chat target update; verify no stale autonomy
session; create one fresh read-only W2 canary; immediately end the ChatGPT turn
so the composer is idle. No live repair, target change, browser launch, daemon
reload/promotion, or external-runtime action was performed for this task.
