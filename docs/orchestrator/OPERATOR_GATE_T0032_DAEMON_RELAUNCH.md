# T-0032 operator gate — safe CatDesk daemon relaunch

Status: `WAITING_FOR_CHATGPT` / operator action required.

The active MCP daemon was connected before the T-0031/T-0034 source changes.
This worker built the updated executable but did not terminate, restart, or
replace the active daemon from inside its own MCP control connection. No
continuity-preserving CatDesk reexec/handoff mechanism is present in this
worktree, so an in-session restart would risk breaking the only control path.

In an operator-owned PowerShell session, from the CatDesk workspace:

```powershell
.\scripts\operator_relaunch_updated_daemon.ps1
```

The preflight deliberately checks only that the updated build, workspace, and
operator-local `CATDESK_CODEX_CLI_EXECUTABLE` are available; it never displays
or changes the executable path, credentials, MCP bearer token, or tunnel
configuration. After a successful preflight:

1. Close the old CatDesk daemon using the operator's desktop/session controls.
2. Start the updated instance with:

   ```powershell
   .\scripts\operator_relaunch_updated_daemon.ps1 -Launch
   ```

3. Re-establish the existing Secure MCP tunnel/connection using the normal
   operator workflow. Do not create a replacement cloud/provider path.
4. Confirm transport health and that the new read-only
   `autonomy_execution_accounting` tool is listed.
5. Resume this approved task. Only then may T-0033/T-0037 live operations use
   the supported app-server transport to resolve the exact existing Codex
   thread.

Until the operator completes these steps, T-0033, T-0035, T-0036, and T-0037
remain blocked. No thread binding, external-workspace access, provider usage,
or source changes to BYOVD/Bug-Bounty were attempted.
