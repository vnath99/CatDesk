# CatDesk — Important Command Reference

**Audience:** CatDesk operator, ChatGPT/CatDesk automation, and future handoffs.  
**Maintained:** 2026-10-09. **Workspace:** `C:\Users\Volap\OneDrive\Desktop\Projects\CatDesk-codex-loop`.  
This is the quick reference for **existing** controls; consult each control's live help/schema before assuming an example remains supported. Keep this document in GitHub.

## Rule zero: try CatDesk tools first

When the **CatDesk Local Tunnel v4** connector is `CONNECTED_VERIFIED` / `READY`, the assistant should execute supported project-local operations itself. Do not reflexively hand an operator a PowerShell command. First check transport, current status, available tools, and protected-state preconditions. If a narrow tool rejects an operation, check the **existing guarded operator path**; never bypass the policy by writing protected Wake/registry files directly.

**Source-of-truth order:** project registry + installed independent WakeHost readback, current session/review and Git evidence, then historical logs. A historical failure from an unrelated earlier chat is *forensic only*; use the latest applicable event and its exact receipt when evaluating whether current Wake works.

## 1. Rebind canonical CatDesk chat and independent Wake **together**

This is the preferred **assistant-side, protected/transactional rollover** (not a PowerShell command). It is backed by `operator_update_designated_chat_target` in `src/mcp.rs`; it compares the old digest, quarantines an existing `SUBMITTING` event on its old immutable target if needed, updates the Wake generation and the project registry in one guarded operation, and verifies the readback:

```text
CatDesk MCP: autonomy_project_registry_bind
  projectId:    catdesk
  decision:     DESIGNATED_CHAT_TARGET_URL=https://chatgpt.com/c/<NEW_CONVERSATION_UUID>
  expectedSha256: <CURRENT_64_HEX_DESIGNATED_TARGET_DIGEST>
```

**Obtain the current digest, do not reuse an old one:** call `autonomy_project_registry_read` and `catdesk_transport_status`. Require that project `catdesk`'s `chatgptTargetSha256` matches independent Wake `targets.catdesk.digest`. After the rollover require both to show exactly the new URL and digest and expect the Wake generation to increase by one. Do not retry blindly if compare-and-swap rejects.

**Why the ordinary target setter is not equivalent:** `catdesk_wake_target_set` updates only the project-local Wake target and is fail-closed when a prior Wake is `SUBMITTING`. The project-registry-only `autonomy_project_registry_chat_target_bind` does not perform the paired rollover. If a prior event is in flight, **use the designated chat rollover** above, not unpaired setters.

**Chat55 caution (2026-10-08):** Do not use `autonomy_project_registry_chat_target_bind` even with `action=PREFLIGHT` as a presumed read-only probe. A live call immediately updated the registry-only URL. Its return-to-old-URL operation restored the old URL but produced a registry SHA-256 that differs from the independent WakeHost-reported digest; the subsequent approved `DESIGNATED_CHAT_TARGET_URL` transaction was rejected with `INVALID_ARGUMENT`. Until the existing guarded Binagotchy CLI `target set` or supported paired tool reports successful coherent readback, do not issue any manual Wake to the new chat or claim canonical migration. Preserve the old incomplete event, do not attempt unpaired repairs or write protected target files.

**Existing PowerShell CLI fallback** (use only if the connector cannot perform the guarded rollover):

```powershell
Set-Location 'C:\Users\Volap\OneDrive\Desktop\Projects\CatDesk-codex-loop'; @('target set https://chatgpt.com/c/<NEW_CONVERSATION_UUID>','target','wake status','exit') | & '.\target\debug\catdesk.exe' --catdesk-binagotchy-cli
```

This invokes the *existing* Binagotchy `target set` implementation; the CLI calls the same guarded `operator_update_designated_chat_target` transaction. It must not be replaced with manual modification of `.catdesk` or independent Wake store files. `target` without `set` shows the current designated target. **Important:** An already-running Binagotchy console owns a singleton mutex. In that case the one-shot CLI can exit without applying the change. Close the existing Binagotchy console first, then run the line, and require an explicit `Designated Chat URL updated` plus independent project+Wake readback; absence of output does **not** establish success.

**Verified example (2026-10-08):** previously retained target generation 30 at `https://chatgpt.com/c/6ac63f72-a2ac-83e9-bf61-db8ab9d97224` was changed successfully through the MCP designated-chat route to generation 31 at `https://chatgpt.com/c/6ac6cbe8-6f0c-83ea-9f7d-13489d4d87f5`. Both WakeHost and registry independently reported digest `3a1d4cc69dfa3d05cb2bc33ef1197ac5a7433a120cfb4214a6996edb5a9d29e9`. That digest is **historical**, not a permanent default.

## 2. Wake operations (existing tools; no new setter required)

| Intent | Preferred CatDesk MCP operation | Binagotchy CLI command |
|---|---|---|
| Current Wake and receipt/status | `catdesk_transport_status`; `catdesk_binagotchy_command(status)` | `wake status` |
| Inspect current queue | `catdesk_binagotchy_command(queue)` | `wake queue` |
| Send **one** diagnostic manual Wake | `catdesk_binagotchy_command(test)` | `wake send` or `wake test` |
| Resume installed Wake | `catdesk_binagotchy_command(resume)` | `wake resume` |
| Start, pause or stop | `catdesk_binagotchy_command(start/pause/stop)` | `wake start`, `wake pause`, `wake stop` |
| Retire an *explicitly identified* stale event | `catdesk_binagotchy_command(retire, eventId=...)` | `wake retire <event-id>` |

Manual `test` events carry a **NOT natural acceptance** marker; a successful enqueue is not proof of actual delivery. Require exact current-event `EXACT_USER_MESSAGE_APPENDED` receipt and terminal state before declaring delivery passed. Ignore unrelated historical `ATTENTION`, `staleCount`, and old chats when assessing the latest Wake, but preserve their records for root-cause analysis. Do not blind-replay old events, nor retire records solely because they are old.

When source changes to Wake need installation, use the reviewed immutable installed-Wake procedure; do not arbitrarily restart, swap binaries, or mutate the external Secure MCP tunnel.

## 3. Turn timer on **every** substantial ChatGPT/CatDesk session

```text
CatDesk MCP: catdesk_turn_timer({action:"START", allow_without_plan:true})
CatDesk MCP: catdesk_turn_timer({action:"STATUS", timerId:"<returned-id>", allow_without_plan:true})
CatDesk MCP: catdesk_turn_timer({action:"STOP", timerId:"<returned-id>", allow_without_plan:true})
```

The independent timer runs to 20 minutes, with an 18-minute checkpoint; **aim to checkpoint/stop by 19 minutes**. Capture and use only the returned `timerId`. Stop or hand off before the lease expires, record durable state, and allow the hourly deadman to defer to any active writer. An expired historical timer is not authority to overwrite another session.

## 4. Routine lifecycle, diagnostics and recovery (PowerShell from workspace)

```powershell
.\catdesk.ps1 status
.\catdesk.ps1 diagnose
.\catdesk.ps1 recover
```

- `status`: nonmutating summary of the fixed canonical lifecycle.
- `diagnose`: nonmutating nine-layer diagnostic (`LIFECYCLE_ENGINE`, `CANONICAL_RELEASE`, `RECOVERY_AUTHORITY`, `LOCAL_MCP_CONFIG`, `LOCAL_DAEMON`, `LOCAL_MCP_PROTOCOL`, `WAKE_RUNTIME`, `OFFICIAL_RUNTIME`, `CODEX_CLI`), with primary failing layer and next action.
- `recover`: existing guarded recovery path. Use only after inspecting `diagnose` and relevant authority; do not treat recovery as a way around reviewed promotion or the Secure MCP tunnel ownership boundary.

Other facade commands currently accepted by `catdesk.ps1`: `install`, `start`, `stop`, `autostart`, `wake`. Check arguments and release authority before invoking mutating actions. Never interpret a healthy dev/debug `cargo build` as reviewed serving-release attestation.

## 5. Git / GitHub and Codex

```powershell
git status --short
git rev-parse HEAD
git rev-parse origin/orchestrator/chatgpt-codex-autonomous-loop
git diff --check
git pull --ff-only origin orchestrator/chatgpt-codex-autonomous-loop
```

Compare local/remote HEAD and do not force-push or modify `main`. Stage **only reviewed files**, e.g. `git add docs/orchestrator/CATDESK_IMPORTANT_COMMANDS.md`; never `git add -A` while unrelated untracked artifacts are present. Use the connected GitHub connector when direct Git operations are unavailable, then fast-forward local.

For **interactive Codex CLI session continuation**:

```powershell
codex resume --all
```

Select the intended existing session (use session directory when specifically prompted); then enter `/goal resume` **inside that session**. Usage availability and the user's cloud/local provider routing policy must be checked first; do not assume that invoking `codex` means capacity is available.

## 6. Current automation continuity

The requested Chat55 continuation prompt is:

> continue progressing where you left off, use the timer workflow for bounded turns. If this deadman is in a new chat, read 'https://chatgpt.com/c/6ac823ac-91b8-83e9-8996-f639c461de50', for context on where it left off

**Current schedule: ACTIVE** as of 2026-10-09, re-enabled under the original operator request after unblocking Windows CI. The only active CatDesk hourly deadman is `CatDesk Hourly Deadman — 6ac823ac`, scheduled every hour in America/New_York and bound to the exact Chat55 continuation prompt quoted above. All earlier CatDesk hourly deadmen remain disabled. This is **fallback** transport, not natural Wake acceptance; defer source mutations when a direct manual-work timer holds the writer. The requested Chat55 URL is not yet established as independent Wake/project canonical authority; those still report Chat54 until the guarded paired-target rollover finishes.

**T-0460 update (2026-10-09):** The previous CLI/registry pre-read failure is now diagnosed and addressed **in source**, not yet live-bound. Before repair, the CatDesk project registry stored an invalid digest `8eb1e045...` for the old Chat54 URL while independent WakeHost generation 31 stored verified `3a1d4cc...` for that same URL. CI for source HEAD `b3585ad` passed all three Windows jobs (Actions run `37992094331`). The narrow `operator_update_designated_chat_target` recovery (commit `b71d113`) corrects only the digest mismatch witnessed by Wake, then performs the existing paired generation-changing CAS. A follow-up `src/binagotchy_cli.rs` fix teaches the CLI `target set` to obtain the old digest from independent Wake when normal readback is corrupted; it still routes through the **same** guarded paired transaction, rejecting invalid URLs/digests. Verify the follow-up source HEAD against fresh CI before using it.

CatDesk MCP `run_command` refuses interactive/piped Binagotchy CLI execution (`INVALID_ARGUMENT`), even for read-only `target`; building a source-current debug binary through `cargo build --bin catdesk` worked, but a debug binary is **not** a reviewed serving-release installation. If no approved CatDesk invocation can run this existing CLI, the **operator** may need to execute the PowerShell fallback in section 1 against the source-current compiled executable after CI verification. Do not use an unpaired target setter, direct registry edit, or browser replay. The existing CLI singleton mutex may require closing an already-running Binagotchy console before a one-shot invocation. Post-action verify project registry and independent Wake show exactly Chat55 URL, SHA `fc92062981985bb64c392ff9bdf1663f5ae3e975b2b7d0d0286092b97484cde7`, and Wake generation at least 32. A manual Wake is a separate acceptance step.

## 7. Current recovery and build-gate reminders (T-0419)

- Current protected build `7e1fe502dd5a4191895956a15a2e1b81` from R3b completed `BUILD_FAILED_OR_AMBIGUOUS`, Cargo exit 101, no attestation. Do not retry or promote it.
- Re-diagnose the **exact** failure under bounded/no-raw-stderr procedures; targeted MSVC D8037 classification was newly added in subsequent committed code. Do not infer D8037 from an exit 101 alone.
- Source code, review authority, build attestation, canonical SHA, and serving reload are separate gates. External Secure MCP tunnel is not owned by these commands.

**Operating rule:** before asking the operator to paste any command, check whether an existing connected CatDesk operation (including the paired designated-chat route) can execute it safely, and actually try it when authorized.

## 8. Python verification and Wake source/deployment separation

The supported project-local CatDesk `run_command` operation accepts narrow, bounded `pytest` commands with `allow_without_plan=true`; use it when `verify_project` has a broad-suite timeout. Useful commands:

```powershell
pytest --collect-only -q
pytest tests/test_stable_wake_browser_adapter.py -q
pytest tests/advisors/test_deepseek_web_advisor.py -q
pytest tests/test_wake_bridge.py -k receipt -q
pytest tests/test_wake_profile_login.py -q
pytest tests/test_wake_smoke_state.py -q
```

Root pytest configuration must add the workspace root to `pythonpath` so `experimental.advisors` resolves under the console-script `pytest` entrypoint. `tests/test_stable_wake_browser_adapter.py` must unit-test version-controlled `scripts/stable_wake_browser_adapter.py`, **not** assume that the separately installed `.catdesk/wake-bridge/stable-runtime-v1` exists. The installed adapter's provenance, SHA, presence, and browser acceptance remain independent release gates: passing source tests does **not** attest or install that runtime. A 30-second `verify_project` timeout is not equivalent to a test failure; the receipt test subset alone can take over 20 seconds. Preserve full-suite verification status separately from focused results.
