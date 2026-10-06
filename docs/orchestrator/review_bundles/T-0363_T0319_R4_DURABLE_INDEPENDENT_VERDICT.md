# T-0319 R4 durable independent verdict

ACCEPT

`scripts/restart_catdesk_daemon.ps1::Resolve-TrustedRestartWorkerScriptPath` derives the fixed worker path, requires a regular `FileInfo`, rejects `ReparsePoint`, normalizes both paths, and requires exact ordinal-ignore-case identity before the trusted PowerShell `-File` handoff. `scripts/test-restart-catdesk-daemon-process-identity.ps1` rejects regression to leaf-existence-only worker trust.

`scripts/query-catdesk-windows-inventory.ps1` uses fixed local limited-information process/TCP readers, caps the raw same-image snapshot at 257, narrows observations to exact `--catdesk-daemon` rows, and returns the one-extra 65-row relevant set. `scripts/start-catdesk-stack.ps1::Get-CatDeskDaemonProcessCandidates` still rejects more than 64 rows and rechecks the daemon token, canonical path/SHA-256, and creation identity. `Get-CatDeskDaemonProcessInstanceForRecovery` repins and rechecks the exact process; `Stop-StaleCanonicalCatDeskDaemonForRecovery` refuses foreign rows, multiple canonical candidates, and listener races before any mutation.

Fresh read-only verification passed: `cargo fmt --all -- --check`; `cargo test --test recovery_powershell -- --nocapture` (3 passed, including the disposable stale-canonical-daemon fixture); and `git diff --check` (exit 0; pre-existing CRLF warnings only). No live recovery, daemon/tunnel, wake/target, Scheduler/service, Git, or external-project action occurred.
