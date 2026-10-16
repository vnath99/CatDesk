# T-0478 D7 — Parent health failure handoff to the owned candidate

Date: 2026-10-16. Scope: reviewed-candidate source and local tests only. Base: `6ecc5392a2ef17ab87b60f6af9ae647bb642ac3f`.

After READY, the parent checks the exact child PID, Windows loopback listener ownership, and the authenticated isolated MCP tool catalog. Previously a failed parent health check immediately dropped its kill-on-close Job. That stopped the exact child tree but left the child-owned `serving.json` without a failure reason. The historical owner diagnosis could classify the resulting orphan, but the reason was unavailable.

The parent now sends the fixed five-byte `fail\n` command on its already private stdin pipe when its initial or periodic service probe or loopback PID check fails. The child distinguishes that command from `stop\n`, aborts its listener, and attempts the existing create-only, bound `runtime-attention.json` with the fixed `owner_health_failed` code before exiting in error. The parent waits at most ten seconds for the child to exit, then closes its exact kill-on-close Job and reaps the retained process handle. If the command fails, the child stalls, or the journal write fails, the parent still stops the exact child tree and reports that the journal requires read-only diagnosis. Child exit is never treated as proof that attention persisted. An existing `parent_channel_failed` path is now a valid fixed attention code rather than always failing the code validator.

No runtime journal or root is removed, no PID is looked up for cleanup, and no orphan is adopted or restarted. The control command is available only to the parent that spawned and retained the child pipe; no MCP endpoint or public CLI capability is added. An owner crash may still kill the child before any attention write, so the durable serving record and read-only orphan classifier remain necessary.

## Verification

- `cargo test --locked --offline --bin catdesk reviewed_candidate_ -- --test-threads=1`: 17 passed, zero failed. The Windows subprocess regression proves a responsive child receives `fail\n` and exits; a stalled child is terminated through the retained Job within the test deadline.
- `cargo test --locked --offline --bin catdesk owner_journal_survives_pre_root_and_bound_root_crash_windows -- --test-threads=1`: one passed. The owner-only journal wrote and reread `owner_health_failed` after reopening pinned handles, while rejecting a second writer and generation drift. The exact protocol parser test rejects alternate commands.
- `cargo clippy --locked --offline --workspace --all-targets --all-features -- -D warnings`, `cargo fmt --all -- --check`, and `git diff --check`: passed locally.
- Exact three-job GitHub Windows CI and a successful witnessed candidate launch: pending.

This increment is `IMPLEMENTED_SOURCE_ONLY` and `TESTED_LOCAL`. It grants no protected install, supervisor, WakeHost, or tunnel authority. T-0478-D still needs an actual CI-witnessed lifecycle, restart/orphan recovery transaction, and installed acceptance.
