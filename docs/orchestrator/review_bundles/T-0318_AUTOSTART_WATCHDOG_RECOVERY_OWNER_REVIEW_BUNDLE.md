# T-0318 — Autostart Watchdog Recovery Owner Review Bundle

## Purpose

T-0318 is the immediate recovery-first follow-on to T-0317. T-0317 closes the deterministic stale-canonical-daemon/no-listener recovery defect, but a deliberate live daemon-loss test would still be unsafe if no independent process were already monitoring CatDesk. The existing T-0059 autostart design supplied such a supervisor in source, but live `catdesk.ps1 autostart enable` returned `AUTOSTART_UNAVAILABLE` on this Windows host.

The goal is to make the recovery supervisor available without requiring administrator elevation or waiting for the next logon, while retaining exact ownership and fail-closed conflict semantics.

## Root cause / live blocker

The historical autostart implementation had only one persistence backend: a Task Scheduler registration in the root Task Scheduler path. The current host refused that registration path, causing the entire `autostart enable` operation to collapse to `AUTOSTART_UNAVAILABLE`. The source contract also registered a logon trigger but did not arm the supervisor immediately in the current session, so even a successful registration would not by itself make an immediate destructive recovery drill safe.

The exact underlying Windows Scheduler policy error remains intentionally redacted by the public facade. T-0318 does not weaken that redaction or require privilege escalation to discover it.

## Repair

`catdesk.ps1` now preserves the existing Scheduled Task backend as the preferred persistence mechanism and adds a fixed current-user fallback:

- deterministic HKCU Run path: `HKCU:\Software\Microsoft\Windows\CurrentVersion\Run`;
- deterministic value name: the same workspace-bound `CatDesk.Autostart.<workspace-sha-prefix>` identity;
- deterministic value data: only the exact PowerShell host plus project-local `scripts\catdesk-autostart-supervisor.ps1` and resolved workspace argument;
- no password, token, route, endpoint, tunnel id, model/provider input, caller-selected path, or arbitrary command is stored;
- a non-empty same-name value that does not exactly match the expected definition is an `AUTOSTART_CONFLICT` and is never replaced;
- disable removes only an exact owned task and/or exact owned Run value.

Enable behavior is also strengthened for the current session:

1. use an exact existing owned Task or Run value if present;
2. otherwise try the historical exact Scheduled Task registration and verify the registered definition;
3. if Scheduler registration is unavailable or does not verify, install and read back the exact user-scoped Run value;
4. start the project-local supervisor immediately with `InitialDelaySeconds 0` when its workspace-bound named mutex is not already present;
5. poll boundedly for the mutex and return `AUTOSTART_ENABLED` only after the monitor is actually running.

The mutex remains the existing T-0059 singleton guard, so duplicate persistence triggers or repeated enable calls cannot create competing supervisors.

## Deterministic verification

The lifecycle fixture now seams all Run-key and supervisor-start/running operations so tests never touch the real registry or spawn a real monitor. It verifies:

- historical exact Scheduled Task registration still works;
- enable starts the supervisor immediately;
- repeated enable is idempotent when the supervisor is already running;
- Task Scheduler registration failure falls back to the exact user-scoped Run value;
- fallback enable performs only `task-register-failed,runkey-register,supervisor-start`;
- fallback status is owned and non-mutating;
- fallback disable removes only the exact owned Run value;
- existing task/action/principal/trigger/settings conflicts remain fail closed.

Verification passed:

- `cargo test --test recovery_powershell -- --nocapture` — 2 integration tests passed, including the PowerShell lifecycle fixture;
- `cargo fmt --check` — passed;
- `cargo clippy --all-targets --all-features -- -D warnings` — passed;
- `cargo test --all-targets --all-features` — passed.

## Live acceptance evidence

After the source repair, the same supported public facade succeeded on the real host:

- `catdesk.ps1 autostart enable` -> `AUTOSTART_ENABLED`;
- repeated `catdesk.ps1 autostart enable` -> `AUTOSTART_ENABLED`;
- `catdesk.ps1 autostart status` -> `AUTOSTART_ENABLED`.

Because `enable` now verifies the named singleton mutex, this is evidence that an independent supervisor process was armed in the current user session before any daemon-loss test.

The official Secure MCP transport remained `CONNECTED_VERIFIED`, local MCP remained `READY`, and the official-runtime monitor continued to report `keepRuntimeOnCatdeskExit=true`, `autoConnect=true`, and `autoRecover=true` after watchdog activation.

## Remaining live drill blocker

The first supported attempt to schedule a local-daemon stop through `catdesk.ps1 stop` returned `LIFECYCLE_STOP_UNAVAILABLE`; no daemon was stopped and transport remained healthy. The compiled live daemon's asynchronous lifecycle-stop helper therefore cannot currently be used as the destructive test trigger.

This is now the next bounded recovery issue. Do not bypass it with an arbitrary process kill through MCP. Either repair/redeploy the fixed asynchronous stop helper under reviewed lifecycle authority, or use a separately isolated disposable CatDesk instance for kill/recover acceptance while preserving the primary transport. Only after a supported destructive trigger exists should the live test prove:

1. official tunnel survives the local daemon loss;
2. watchdog observes loss without operator action;
3. T-0317 recovery retires/replaces the exact canonical stale daemon if necessary;
4. local MCP returns;
5. transport returns to `CONNECTED_VERIFIED`;
6. repeated recovery remains idempotent.

## Review decision

**Repository/source-test boundary: ACCEPTED.**

**Watchdog live activation boundary: ACCEPTED.** The current-session independent supervisor is now armed and persistent autostart reports enabled.

**Destructive daemon-loss acceptance: OPEN.** The supported asynchronous stop-helper failure must be fixed or an isolated test instance must be used before claiming full one-command recovery acceptance.

No Git publication, browser/wake/target change, Secure MCP stop/reconfiguration, reviewed-image signing/provenance work, or external-project mutation occurred.
