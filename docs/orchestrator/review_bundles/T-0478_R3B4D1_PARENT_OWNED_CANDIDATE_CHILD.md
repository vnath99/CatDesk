# T-0478 R3B4-D1 — Parent-owned candidate child source increment

Date: 2026-10-14. Scope: source-only reviewed-candidate lifecycle.

## Change

`--reviewed-candidate-owner` accepts the candidate command's exact fixed
loopback, port-zero and `/dev/mcp` options. It starts the same executable as a
suspended child, assigns that process to a private Windows kill-on-close Job,
then resumes it. The parent retains the child process handle and Job handle.
The child Job closes if the parent crashes, terminating the exact child and
its descendants without finding a process by PID or deleting a pathname.

Before relaying ready, the parent bounds and validates the child JSON record,
requires the expected source commit and child PID, verifies the loopback
listener's OS socket owner, and repeats the authenticated read/search MCP
probe. It watches child exit and listener ownership every second. Ctrl+C to
the parent sends the process-scoped `stop\n` control signal to the child and
waits at most 20 seconds for graceful shutdown. The child uses that control
channel only when the parent launcher enables it. Its existing exact owned
snapshot retirement remains the only normal cleanup path.

On child exit, listener drift, readiness failure or shutdown timeout, the
parent exits with an error and drops the Job. An abrupt child death leaves its
durable journal for attention; the parent does not adopt or remove an orphan
snapshot from its generated name, PID, port or ready record.

## Verification

- `cargo test --locked --offline --bin catdesk reviewed_candidate_ --quiet`:
  11 passed, 0 failed.
- `cargo test --locked --offline --bin catdesk
  hard_owner_crash_closes_job_and_kills_exact_child -- --nocapture`: passed.
  It proved the exact child process handle was unsignaled before a hard parent
  kill and signaled afterward.
- `cargo clippy --locked --offline --bin catdesk -- -D warnings`: passed.
- `git diff --check`: passed.

## Remaining acceptance work

The current exact branch has no successful three-job GitHub CI witness, so no
positive reviewed-candidate launch was attempted. This increment does not
prove a versioned immutable child image, durable crash reconciliation of an
orphaned snapshot, bounded restart, or T-0478-D acceptance. It did not change
installed WakeHost, protected binaries, the official tunnel, auth/browser
state, or the disabled hourly deadman.
