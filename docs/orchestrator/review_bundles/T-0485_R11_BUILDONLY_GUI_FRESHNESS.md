# T-0485 R11 — Inert dev.85 GUI build freshness

**Scope:** `wake/install.ps1` only. `-BuildOnly` now invokes the offline GUI release build even when `wake/target/catdesk-gui/release/catdesk.exe` already exists. It continues to build WakeHost first and returns before any install-store probe or staging. A normal invocation still fails at `WAKE_INSTALL_BROKER_UNPROVISIONED` before either build.

**Verification:** PowerShell syntax parsing and `git diff --check` passed. A no-write PowerShell probe replaced `cargo` with a function, confirmed an existing GUI target, and observed both release build calls for `-BuildOnly`; the same probe confirmed normal invocation made zero build calls and hit the broker hold. No release build, package stage, installation, browser event or protected-state operation ran for this change.

**Limit:** The build uses the current working tree and does not emit a canonical broker approval manifest or bind a Git source commit. Before package approval, rebuild from the final reviewed source, record all four artifact lengths and SHA-256 digests, and obtain exact-source three-job Windows CI. The signed broker, protected namespace, independent approval and dev.84 migration gates remain open.
