# T-0362 — stale-daemon production enumeration repair

## Scope and demonstrated cause

T-0361 reproduced the disposable stale-daemon fixture failure after T-0360:
the process remained alive but production candidate enumeration did not expose
it. T-0362 added temporary fixture-local observation through the already
sanctioned integration path and demonstrated that both `Get-CimInstance` and
`Get-WmiObject` `Win32_Process` reads for that exact fixture PID returned
`Access denied`. The old inventory helper used `SilentlyContinue`, converting
that denied observation into an empty result. The temporary diagnostics were
removed before final verification.

## Repair

`scripts/query-catdesk-windows-inventory.ps1` now uses a fixed local
limited-information process handle plus `NtQueryInformationProcess` class 60
to read command lines, `System.Diagnostics.Process` for path/creation, and
fixed native IPv4/IPv6 TCP owner-PID listener tables. It accepts no caller path,
command, process selector, or authority input beyond the existing fixed
inventory mode/validated PID or port.

Daemon mode scans a bounded 257-row same-image snapshot and fails closed on
overflow. It emits only exact `--catdesk-daemon` observations and stops at the
existing one-extra 65-row relevant bound; `Get-CatDeskDaemonProcessCandidates`
still rejects more than 64 rows. The parent keeps exact daemon-token,
canonical path/SHA-256, creation time, pinned `System.Diagnostics.Process`
handle, listener recheck, foreign-row, and multiple-canonical ambiguity rules.
No PID-only mutation was introduced.

## Regression coverage

`scripts/test-start-catdesk-stack.ps1` requires the fixed local reader, native
listener reader, command-line narrowing before the relevant-row cap, raw
snapshot ceiling, and the >64 relevant-daemon refusal. The existing disposable
fixture continues to prove discovery, exact stale retirement, replacement
listener identity, and changed-instance refusal through the production path.

## Verification

- `cargo fmt --all -- --check` — PASS.
- `cargo test --test recovery_powershell -- --nocapture` — PASS, 3/3.
- `cargo clippy --all-targets --all-features -- -D warnings` — PASS.
- `cargo test --all-targets --all-features` — PASS; main suite 876 passed,
  21 ignored.
- `git diff --check` — PASS; only pre-existing LF-to-CRLF warnings.

## Non-actions and next step

No live recovery, daemon/tunnel restart, promotion, browser wake, protected
target mutation, Scheduler/service action, Git publication, or worktree cleanup
occurred. T-0319 is not accepted by this implementation ticket. The exact next
step is a fresh separate Codex/Terra-high T-0319 cumulative review; only an
independent accepting verdict permits transition to T-0324.
