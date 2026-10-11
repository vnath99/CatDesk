# T-0481 R1 — Recovery health must follow the installed independent Wake owner

Date 2026-10-11 UTC. Source-only repair, not host activation or production installer approval.

## Live problem

A fully accepted gen32 Wake manual delivery now exists: `manual-wake-mcp-1791691944140` was independently read as `SENT`, `EXACT_USER_MESSAGE_APPENDED`, timer `COMPLETE`, queue0; host dev84 RUNNING. CatDesk's nine-layer `catdesk.ps1 diagnose` and lifecycle recovery still invoke `Test-WakeBridgeRuntime`, whose default code was unconditionally testing the **retired Python venv** even after `owner.json` selected `independent_v1`. This can falsely report `WAKE_RUNTIME_NOT_READY` and, in execute mode, call `Invoke-WakeBridgeRuntimeRepair` to modify obsolete Python rather than validate the installed native host.

The old serving MCP command gateway rejects the already-developed typed `catdesk.ps1 diagnose` with `INVALID_ARGUMENT`; this source repair does not claim that gateway is upgraded or that the supervisor startup/release gates are resolved. Those remain separate production requirements.

## Code changes

- `scripts/start-catdesk-stack.ps1` factors existing hash/selector checks into `Resolve-VerifiedInstalledWakeHost`. It reads only the fixed project owner selector and the fixed per-user `current.json`; rejects reparse/nonregular file, wrong schema/version or path, oversized data, and an installed EXE whose SHA256 differs from the installed pointer. It does not start a process. The existing `Start-CatDeskWakeHost` then reuses the same verifier and launches ONLY under its previous explicit lifecycle stage.
- `Test-WakeBridgeRuntime` now checks for a verified **independent owner** first. It runs only the exact installed `CatDeskWakeHost.exe status` through the existing bounded, captured, contained helper and validates a minimal fixed response (protocol version1, host RUNNING/STOPPED, one valid CatDesk generation/digest). A STOPPED but correctly installed host counts as an available runtime, not a running/delivering host; explicit later Wake start is unchanged. If the owner selector is absent or `legacy_python`, the original bounded Python venv identity/import probe remains.
- `Invoke-WakeBridgeRuntimeRepair` refuses fallback into Python repair when the independent owner is selected and the installed native host is verified; an invalid/missing installed native host also fails during identity validation rather than initiating legacy Python mutation. No new process launch on read-only `diagnose`, no extra Wake event, no source CLI argument from the user, no signed production image rotation, and no Secure MCP tunnel ownership change.
- Existing Windows fixture `scripts/test-start-catdesk-stack.ps1` imports the new verifier, retains current custom WakeRoot fixture/sha mismatch tests, and adds static AST source guards requiring a bounded native `status` probe and no independent-owner fallback to legacy repair. Windows CI parses both the development facade and fixed recovery script; the existing Rust/PowerShell harness exercises recovery fixtures.
- Operator PowerShell commands remain one-line; don't ask them to run anything until CI validates these changes. If a future host diagnostic still reports policy-unproven supervisor root, do not bypass.

## Caveats and acceptance

This proof is **source-only** until Windows CI + independently reviewed production deployment. The installed CatDesk transport still reports older code and missing supervisor policy/root. The repaired lifecycle probe improves operator diagnosis on source current, but does not automatically install or authorize the supervisor. Use the already-working independent MCP Wake commands to control the current host in the meantime. Preserved signed T0215 installed epoch1, T0366 pending epoch2 and seven unrelated historical untracked files.

Next: verify Windows CI and full PowerShell fixture; if failures, correct precisely rather than disabling tests. Afterwards continue T0478 isolated dev state/port/tool gating, and T0479 versioned trusted production worker updates.
