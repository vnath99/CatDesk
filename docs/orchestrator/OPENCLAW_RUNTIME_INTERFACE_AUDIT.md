# OpenClaw Runtime Interface Audit

Date: 2026-07-25

Branch: `orchestrator/v1-coding-sprint`

OpenClaw: `OpenClaw 2026.7.1-2 (0790d9f)`

Scope: bounded runtime-interface investigation after Milestone A. No model worker was run, no provider credentials were entered, no persistent OpenClaw onboarding was performed, no Gateway service was installed, no global configuration was modified, and no adapter was implemented.

## Executive Summary

OpenClaw does expose a stable programmatic runtime surface, but it is the authenticated Gateway WebSocket protocol, not the standalone CLI and not direct imports from the plugin SDK.

The Gateway protocol documentation says external applications should connect over WebSocket and use documented RPC methods for agent runs, sessions, events, models, tools, artifacts, and approvals. It specifically lists `agent` plus `agent.wait`, `sessions.*`, `tasks.*`, `chat.abort`, `models.list`, `tools.catalog`, `tools.effective`, event subscriptions, and audit/task ledgers as Gateway surfaces.

That is enough to justify retaining OpenClaw only through a thin CatDesk-owned adapter if a disposable authenticated Gateway can be started and validated in the next approved runtime test. It is not enough to retain OpenClaw directly as the orchestrator runtime. The adapter must own auth setup, version pinning, method probing, `tools.effective` validation, event cursoring, cancellation semantics, and fail-closed behavior.

Recommendation: **B. retain OpenClaw through a thin adapter**, with a hard fallback to **D. implement a minimal CatDesk-controlled worker loop** if Gateway startup/auth/tool-policy validation cannot be made disposable and repeatable.

## Capability Matrix

| Required capability | Classification | Stable public surface | Evidence | Caveats |
| --- | --- | --- | --- | --- |
| Final effective model-visible tool definitions | SUPPORTED_STABLE, with warm-catalog caveat | Gateway RPC `tools.effective`; related `tools.catalog` | Local docs: `docs/gateway/protocol.md` documents `tools.effective` and says it fetches runtime-effective tool descriptors after final policy projection. Official docs confirm Gateway RPC is the external-app path. | `tools.effective` is read-only for MCP and does not create MCP runtimes or issue `tools/list`. If no warm session MCP catalog exists, it only returns core/plugin tools. CatDesk must prove when/how the CatDesk MCP catalog becomes warm before the first worker turn. |
| Structured worker events or reliable cursor-based event retrieval | SUPPORTED_STABLE for Gateway/session/audit/task events; POSSIBLE_WITH_THIN_ADAPTER for full worker UI stream | Gateway protocol events; `sessions.messages.subscribe`; `sessions.subscribe`; `audit.list` with cursor; `gateway stability --since-seq`; `tasks.*` | Local docs: `docs/gateway/protocol.md` documents event frames with `seq`, streamed agent/tool-result events, session event subscriptions, `audit.list` with `nextCursor`, and task ledger RPCs. `openclaw audit --help` exposes `--cursor`. | Audit is metadata-only and bounded; absence of a row proves nothing. Full model delta semantics must be verified against a real Gateway run later. |
| Worker-session creation and identification | SUPPORTED_STABLE | Gateway `agent`, `chat.send`, `sessions.create`, `sessions.send`; CLI `openclaw agent --session-id/--session-key` | `openclaw agent --help` exposes `--session-id`, `--session-key`, `--json`; docs recommend `agent` RPC plus `agent.wait`; protocol docs list `sessions.create`, `sessions.send`, `chat.send`, `agent.wait`. | Requires authenticated Gateway or a deliberate `--local` run. `--local` is one-shot and was not exercised. |
| Restart and resume of the same session | SUPPORTED_STABLE for same logical session via session key/id; UNKNOWN for exact worker process resume | CLI `--session-id` / `--session-key`; Gateway `sessions.*`; ACP replay/session tables | CLI docs explain explicit session keys and ids. SQLite schema includes `acp_sessions`, `acp_replay_sessions`, and `acp_replay_events`. Gateway docs list `sessions.resolve`, `sessions.describe`, `sessions.get`, `sessions.patch`, `sessions.reset`, and `sessions.compact`. | Exact provider/worker process resume was not proven without running a worker. CLI backend docs discuss backend-specific resumable sessions, but CatDesk must test the selected backend/runtime. |
| Pause, cancel, and status control | SUPPORTED_STABLE for abort/cancel/status; NOT_AVAILABLE for general pause | Gateway `chat.abort`, `sessions.abort`, `tasks.cancel`, `tasks.list`, `tasks.get`, `status`, `gateway.suspend.*` | Protocol docs list `chat.abort`, `sessions.abort`, and `tasks.cancel`; `openclaw tasks cancel --help` exists. CLI agent docs say SIGINT/SIGTERM sends `chat.abort` after Gateway acceptance. | Pause is not a general active-run control in the documented agent/session API. Gateway suspension is host admission control, not per-worker pause. |
| Model/provider transition visibility | POSSIBLE_WITH_THIN_ADAPTER | `agents.list`, `sessions.patch`, `models.list`, `models status --json`, streamed events and audit metadata | Protocol docs list configured/effective model metadata in `agents.list`, canonical model plus effective runtime in `sessions.patch`, and runtime-allowed catalog via `models.list`. Local `models status --json` returned default/fallback/provider-auth metadata, but secret-like output was not recorded in this report. | I did not find a stable explicit "model transition" event contract. `model-selected` appears in private/bundled UI diagnostics; transition-level evidence must be validated by observing Gateway events during an approved worker run. |

## Stable Surfaces Found

### Gateway WebSocket Protocol

Official docs state that external applications should use the Gateway protocol: WebSocket transport plus RPC methods. The documented recommended path is to run or discover a Gateway, connect over the Gateway protocol, call documented RPC methods, pin the OpenClaw version, and recheck the RPC reference when upgrading.

Evidence:

- Official: https://docs.openclaw.ai/gateway/external-apps
- Official: https://docs.openclaw.ai/gateway/protocol
- Local package docs:
  - `C:\Users\Volap\AppData\Roaming\npm\node_modules\openclaw\docs\gateway\external-apps.md`
  - `C:\Users\Volap\AppData\Roaming\npm\node_modules\openclaw\docs\gateway\protocol.md`

Relevant documented Gateway methods:

- Runs: `agent`, `agent.wait`, `chat.send`, `chat.abort`
- Sessions: `sessions.list`, `sessions.subscribe`, `sessions.messages.subscribe`, `sessions.describe`, `sessions.resolve`, `sessions.create`, `sessions.send`, `sessions.steer`, `sessions.abort`, `sessions.patch`, `sessions.reset`, `sessions.delete`, `sessions.compact`, `sessions.get`
- Tools: `tools.catalog`, `tools.effective`, `tools.invoke`
- Tasks: `tasks.list`, `tasks.get`, `tasks.cancel`
- Models: `models.list`, `agents.list`
- Audit: `audit.list`
- Diagnostics: `diagnostics.stability`

### CLI Surface

The CLI is useful for scripting and inspection, but by itself is not the full orchestration API. It wraps Gateway calls for many operations and needs Gateway credentials for Gateway-backed methods.

Useful commands inspected:

- `openclaw agent --help`
- `openclaw gateway --help`
- `openclaw gateway call --help`
- `openclaw sessions --help`
- `openclaw audit --help`
- `openclaw tasks --help`
- `openclaw mcp --help`
- `openclaw models --help`
- `openclaw logs --help`
- `openclaw status --help`
- `openclaw acp --help`
- `openclaw attach --help`
- `openclaw infer --help`
- `openclaw node --help`
- `openclaw agents --help`

CLI findings:

- `openclaw agent` can address runs by `--session-id` or `--session-key`, emit `--json`, and abort accepted Gateway runs on SIGINT/SIGTERM.
- `openclaw audit` exposes bounded metadata queries with `--cursor`, `--kind`, `--run`, `--session`, and `--json`.
- `openclaw tasks` exposes `list`, `show`, and `cancel`.
- `openclaw mcp probe` lists MCP-discovered tools, but this is MCP discovery output, not final worker-visible policy.
- `openclaw gateway call <method>` can dispatch arbitrary Gateway methods, but it requires credentials/service access before method execution.

### Plugin SDK

The installed `package.json` exports these relevant SDK subpaths:

- `openclaw/plugin-sdk/agent-runtime`
- `openclaw/plugin-sdk/gateway-method-runtime`
- `openclaw/plugin-sdk/session-store-runtime`
- `openclaw/plugin-sdk/diagnostic-runtime`
- `openclaw/plugin-sdk/model-session-runtime`
- `openclaw/plugin-sdk/runtime-config-snapshot`
- `openclaw/plugin-sdk/runtime-group-policy`

The plugin SDK is a typed contract for plugins running inside OpenClaw. Official docs explicitly say external apps should use Gateway integrations instead of importing `openclaw/plugin-sdk/*`.

Evidence:

- Official: https://docs.openclaw.ai/plugins/sdk-overview
- Local declarations:
  - `C:\Users\Volap\AppData\Roaming\npm\node_modules\openclaw\dist\plugin-sdk\agent-runtime.d.ts`
  - `C:\Users\Volap\AppData\Roaming\npm\node_modules\openclaw\dist\plugin-sdk\gateway-method-runtime.d.ts`
  - `C:\Users\Volap\AppData\Roaming\npm\node_modules\openclaw\dist\plugin-sdk\session-store-runtime.d.ts`

The SDK exports generic planner helpers including `buildToolPlan` and `toToolProtocolDescriptors`, but I did not find a stable external SDK call that resolves a configured agent plus MCP runtime into final worker-visible tool definitions without Gateway participation.

## Private/Internal Surfaces Found

These exist but should not be treated as stable CatDesk dependencies:

- `C:\Users\Volap\AppData\Roaming\npm\node_modules\openclaw\dist\agent-runtime-BatklvVX.d.ts`
- `C:\Users\Volap\AppData\Roaming\npm\node_modules\openclaw\dist\agent-runtime-JUjSgUZE.js`
- `C:\Users\Volap\AppData\Roaming\npm\node_modules\openclaw\dist\agent-bundle-mcp-types-dLCU1Xhs.d.ts`
- `C:\Users\Volap\AppData\Roaming\npm\node_modules\openclaw\dist\agent-bundle-mcp-materialize-D9l-gQ5S.js`
- `C:\Users\Volap\AppData\Roaming\npm\node_modules\openclaw\dist\acp-cli-BXc5GttU.js`

Private findings:

- The internal planner has `ToolDescriptor`, `ToolPlan`, `buildToolPlan`, and `toToolProtocolDescriptors`.
- The MCP materializer can project an already-listed MCP catalog into agent tools.
- ACP code stores replay sessions/events and has resume checks.
- Internal abort code references `abortEmbeddedAgentRun` and queue clearing.

These are useful for understanding behavior, but their hashed filenames and placement under `dist` internals make them unsuitable as CatDesk integration contracts.

## Disposable T-0012 State Inspection

Inspected paths:

- `C:\Users\Volap\OneDrive\Desktop\Projects\CatDesk\.tmp\openclaw-closure-t0012\openclaw.json`
- `C:\Users\Volap\OneDrive\Desktop\Projects\CatDesk\.tmp\openclaw-closure-t0012\minimal-readonly-policy.patch.json`
- `C:\Users\Volap\OneDrive\Desktop\Projects\CatDesk\.tmp\openclaw-closure-t0012\state\state\openclaw.sqlite`
- `C:\Users\Volap\OneDrive\Desktop\Projects\CatDesk\.tmp\openclaw-headless-t0012\state\state\openclaw.sqlite`
- `C:\Users\Volap\OneDrive\Desktop\Projects\CatDesk\.tmp\openclaw-t0012\state\state\openclaw.sqlite`
- `C:\Users\Volap\OneDrive\Desktop\Projects\CatDesk\.tmp\T-0012-closure-review-20260725-085428-payload\review-bundles\T-0012-closure-openclaw-mcp-probe.json`

The T-0012 policy configured:

- `tools.profile: "minimal"`
- allowed tools:
  - `catdesk-t0012__catdesk_instruction`
  - `catdesk-t0012__plan_read`
  - `catdesk-t0012__read`
  - `catdesk-t0012__search`
- denied native runtime/filesystem/web/browser/UI/automation/exec/applyPatch/elevated/code-mode capability names.

The MCP probe evidence listed exactly those four CatDesk MCP tools. This proves MCP server discovery output, not final worker-visible policy.

SQLite findings:

- Each inspected database had 74 tables.
- Relevant runtime tables existed: `audit_events`, `diagnostic_events`, `diagnostic_stability_bundles`, `task_runs`, `flow_runs`, `subagent_runs`, `acp_sessions`, `acp_replay_sessions`, `acp_replay_events`, `agent_model_catalogs`, and `model_capability_cache`.
- Those tables were empty in the disposable T-0012 state because no worker turn was run.
- The schemas show plausible ledgers for sessions/events/tasks, but empty private tables cannot prove operational behavior.

## Runtime Probes Performed

All OpenClaw environment changes were process-scoped PowerShell variables.

CLI help commands:

```powershell
openclaw --version
openclaw --help
openclaw agent --help
openclaw gateway --help
openclaw gateway call --help
openclaw sessions --help
openclaw audit --help
openclaw transcripts --help
openclaw tasks --help
openclaw mcp --help
openclaw models --help
openclaw acp --help
openclaw attach --help
openclaw logs --help
openclaw status --help
openclaw node --help
openclaw agents --help
openclaw mcp probe --help
openclaw mcp tools --help
openclaw models status --help
openclaw sessions tail --help
openclaw tasks list --help
openclaw tasks show --help
openclaw tasks cancel --help
```

Package and source inspection commands:

```powershell
where.exe openclaw
Get-Content -Raw "C:\Users\Volap\AppData\Roaming\npm\node_modules\openclaw\package.json"
Get-Content "C:\Users\Volap\AppData\Roaming\npm\node_modules\openclaw\openclaw.mjs" -TotalCount 120
Get-ChildItem "C:\Users\Volap\AppData\Roaming\npm\node_modules\openclaw\docs" -Recurse -File
Get-Content "C:\Users\Volap\AppData\Roaming\npm\node_modules\openclaw\dist\plugin-sdk\agent-runtime.d.ts" -TotalCount 260
Get-Content "C:\Users\Volap\AppData\Roaming\npm\node_modules\openclaw\dist\plugin-sdk\gateway-method-runtime.d.ts" -TotalCount 260
Get-Content "C:\Users\Volap\AppData\Roaming\npm\node_modules\openclaw\dist\plugin-sdk\session-store-runtime.d.ts" -TotalCount 260
Get-Content "C:\Users\Volap\AppData\Roaming\npm\node_modules\openclaw\dist\agent-runtime-BatklvVX.d.ts" -TotalCount 430
Get-Content "C:\Users\Volap\AppData\Roaming\npm\node_modules\openclaw\dist\agent-bundle-mcp-types-dLCU1Xhs.d.ts" -TotalCount 160
Get-Content "C:\Users\Volap\AppData\Roaming\npm\node_modules\openclaw\dist\agent-bundle-mcp-materialize-D9l-gQ5S.js" -TotalCount 230
rg -n "buildToolPlan|toToolProtocolDescriptors|ToolPlan|ToolDescriptor|tool.*descriptor|mcp.*catalog|tool.*catalog" "C:\Users\Volap\AppData\Roaming\npm\node_modules\openclaw\dist" -g "*.d.ts" -g "*.js"
rg -n "agent.wait|agent\.|chat\.send|chat\.abort|sessions\.|tasks\.|models\.|tools\.|events|tool-events|effective|visible|tool" "C:\Users\Volap\AppData\Roaming\npm\node_modules\openclaw\docs\gateway\protocol.md" "C:\Users\Volap\AppData\Roaming\npm\node_modules\openclaw\docs\gateway\external-apps.md" "C:\Users\Volap\AppData\Roaming\npm\node_modules\openclaw\docs\gateway\cli-backends.md" "C:\Users\Volap\AppData\Roaming\npm\node_modules\openclaw\docs\cli\agent.md" "C:\Users\Volap\AppData\Roaming\npm\node_modules\openclaw\docs\cli\mcp.md"
rg -n "audit|cursor|sequence|metadata_only|tool.action|agent.run|sessionKey|run_id|tool_name" "C:\Users\Volap\AppData\Roaming\npm\node_modules\openclaw\docs\cli\audit.md"
rg -n "resume|session-id|session-key|restart|cancel|pause|status|SIGTERM|SIGINT|chat.abort|in_flight" "C:\Users\Volap\AppData\Roaming\npm\node_modules\openclaw\docs\cli\agent.md" "C:\Users\Volap\AppData\Roaming\npm\node_modules\openclaw\docs\cli\tasks.md" "C:\Users\Volap\AppData\Roaming\npm\node_modules\openclaw\docs\gateway\protocol.md" "C:\Users\Volap\AppData\Roaming\npm\node_modules\openclaw\docs\gateway\external-apps.md"
```

SQLite inspection command:

```powershell
@'
import sqlite3, pathlib, json
paths = [
  pathlib.Path('.tmp/openclaw-closure-t0012/state/state/openclaw.sqlite'),
  pathlib.Path('.tmp/openclaw-headless-t0012/state/state/openclaw.sqlite'),
  pathlib.Path('.tmp/openclaw-t0012/state/state/openclaw.sqlite'),
]
interesting = ('session','transcript','audit','task','tool','model','provider','run','event','diagnostic','usage')
for p in paths:
    conn = sqlite3.connect(f'file:{p.as_posix()}?mode=ro', uri=True)
    cur = conn.cursor()
    tables = [r[0] for r in cur.execute("select name from sqlite_master where type='table' order by name")]
    print(p, len(tables), tables)
    for t in tables:
        count = cur.execute(f'select count(*) from "{t}"').fetchone()[0]
        if any(k in t.lower() for k in interesting):
            sql = cur.execute("select sql from sqlite_master where type='table' and name=?", (t,)).fetchone()[0]
            print(t, count, sql)
    conn.close()
'@ | python -
```

Process-scoped disposable state probes:

```powershell
$env:OPENCLAW_CONFIG_PATH=(Resolve-Path .tmp\openclaw-closure-t0012\openclaw.json).Path
$env:OPENCLAW_STATE_DIR=(Resolve-Path .tmp\openclaw-closure-t0012\state).Path
openclaw tasks list --json
openclaw sessions --json
openclaw gateway status --json --no-probe
openclaw gateway call tools.effective --json --params '{}'
openclaw gateway call agent.wait --json --params '{}'
openclaw gateway call sessions.list --json --params '{}'
openclaw gateway call chat.abort --json --params '{}'
```

Results:

- `openclaw tasks list --json` returned zero tasks.
- `openclaw sessions --json` returned zero sessions and the disposable agent session store path.
- `openclaw gateway status --json --no-probe` reported a valid disposable config, loopback bind host `127.0.0.1`, port `18789`, service not installed/running, and port free.
- Gateway RPC calls failed before dispatch with `GatewayCredentialsRequiredError`, confirming the CLI call surface does not bypass Gateway authentication.

One `openclaw models status --json` probe was also run and showed static model/auth metadata. Its output included masked provider-auth detail, so it is intentionally not reproduced here. No secret value was written into this report.

## Findings By Objective

### 1. Final Effective Model-Visible Tool Definitions

`tools.effective` is the key stable Gateway RPC. It is the correct surface for a CatDesk adapter to ask "what tools would this agent/session effectively see?" before allowing a model turn.

However, the MCP caveat matters. The docs say `tools.effective` may project a warm session MCP catalog through final policy but does not create MCP runtimes, connect transports, or issue `tools/list`. Therefore an adapter must validate one of these before running a worker:

- the CatDesk MCP server is already warm and appears in `tools.effective`; or
- the adapter has an approved, read-only pre-warm step that creates the MCP catalog without a model turn; or
- OpenClaw cannot be retained for first-turn CatDesk MCP safety.

### 2. Structured Events / Cursor Retrieval

OpenClaw has several event surfaces:

- Gateway event frames include optional `seq`.
- Gateway broadcasts `session.message`, `session.operation`, `session.tool`, and `sessions.changed`.
- `sessions.messages.subscribe` and `sessions.subscribe` expose session updates.
- `audit.list` returns metadata-only events with `nextCursor`.
- `diagnostics.stability` accepts `sinceSeq`.
- `tasks.*` exposes task state.
- `openclaw mcp serve` exposes `events_poll` and `events_wait`, but that is for OpenClaw-as-MCP-channel bridging, not for CatDesk controlling an OpenClaw worker.

The adapter can likely build a reliable event loop from Gateway subscriptions plus `agent.wait` and audit/task fallbacks. Full worker streaming still needs a real disposable worker run to verify event names, ordering, terminal states, and reconnect behavior.

### 3. Worker-Session Creation / Identification

Stable enough through Gateway and CLI. `openclaw agent --help` exposes explicit `--session-id` and `--session-key`, and Gateway docs list session creation, resolution, send, describe, get, and patch methods.

### 4. Restart / Resume Same Session

Stable enough for logical session continuity through `sessionKey`/`sessionId` and `sessions.*`. Exact worker process resume remains unproven. ACP/internal tables show replay machinery, and CLI backend docs discuss resumable backends, but CatDesk must test the selected OpenClaw runtime rather than assuming all backends resume equivalently.

### 5. Pause / Cancel / Status Control

Cancel and status are supported through documented Gateway/CLI surfaces:

- `chat.abort`
- `sessions.abort`
- `tasks.cancel`
- `tasks.list`
- `tasks.get`
- `status`
- `gateway status`

General per-worker pause is not supported as a stable documented control. Gateway suspension exists, but it is host admission control and quiescence fencing, not an active model-run pause mechanism.

### 6. Model / Provider Transition Visibility

Static model/provider visibility exists:

- `models.list`
- `models status`
- `agents.list`
- `sessions.patch` reporting resolved canonical model and effective runtime

Runtime transition visibility is not fully proven. The docs and internal strings show model/provider data can appear in diagnostics and run metadata, but I did not find a stable explicit "model changed from A to B during this run" event contract. Treat this as adapter-observable only after a worker-run validation.

## Risks

- Direct OpenClaw retention would couple CatDesk to OpenClaw operational state, auth, and policy semantics without CatDesk owning the fail-closed checks.
- `tools.effective` may not include CatDesk MCP tools until the session MCP catalog is warm.
- CLI `mcp probe` output is not equivalent to final worker-visible policy.
- Private SQLite tables and hashed `dist` chunks are tempting but not stable contracts.
- Gateway calls require credentials; a disposable auth setup must be designed before any real runtime test.
- Model/provider transition telemetry is not yet proven at the granularity CatDesk likely wants for delegated orchestration.

## Recommendation

Choose **B. retain OpenClaw through a thin adapter** for the next architecture review step.

The adapter should be CatDesk-controlled and fail closed. It should:

1. Start or discover only a disposable loopback Gateway under process-scoped config/state.
2. Authenticate through a short-lived token or equivalent disposable credential.
3. Probe Gateway protocol version and required methods before any model turn.
4. Call `tools.effective` and require the approved CatDesk read-only tools to appear before a worker run.
5. Refuse to proceed if CatDesk MCP tools are absent because the catalog is not warm.
6. Create or resolve a session through `sessions.*`/`agent`.
7. Subscribe to session/tool events and use `agent.wait` for terminal status.
8. Use `chat.abort`/`sessions.abort`/`tasks.cancel` for cancellation.
9. Record model/provider metadata from `agents.list`, `sessions.patch`, event payloads, and audit/task ledgers.
10. Pin `OpenClaw 2026.7.1-2` during validation and repeat the surface audit on upgrades.

Do not choose **A. retain OpenClaw directly**. The direct CLI path does not expose enough preflight control by itself.

Keep **D. implement a minimal CatDesk-controlled worker loop** as the fallback if the next approved Gateway probe cannot make `tools.effective` work before the first model turn, or if OpenClaw cannot run with native file/shell/browser capabilities disabled under a disposable authenticated Gateway.

## Implementation Cost And Roadmap Impact

| Option | Viability | Estimated cost | Roadmap impact |
| --- | --- | --- | --- |
| A. Retain OpenClaw directly | Not recommended | 40-80 credits | Lowest build effort, but unsafe. Would rely on CLI behavior and operator discipline instead of CatDesk-enforced boundaries. |
| B. Retain OpenClaw through a thin adapter | Recommended next path | 120-220 credits | Add an adapter ticket before T-0014/T-0016 worker execution: disposable Gateway auth, method probing, `tools.effective` gate, event subscription, abort/status bridge, and runtime verification bundle. |
| C. Replace OpenClaw with another runtime | Viable but delayed | 180-320 credits | Requires a new runtime survey and proof against the same six capabilities. Keep as backup if OpenClaw Gateway validation fails. |
| D. Minimal CatDesk-controlled worker loop | Viable fallback | 220-420 credits | More code, but best authority control. Replace OpenClaw-specific worker tickets with a CatDesk provider/session/event loop and reuse existing CatDesk MCP hardening. |

## Open Questions For Architectural Review

- Is a disposable authenticated loopback Gateway acceptable for the next spike if it does not install a service or persist global config?
- Can OpenClaw warm CatDesk MCP catalogs before the first model turn without granting native file/shell/web/browser tools?
- Does `tools.effective` return exactly the four approved CatDesk tools under the minimal policy after warm-up?
- Do Gateway events expose enough structured progress to satisfy CatDesk's delegated-orchestrator UX?
- Is missing general pause acceptable if cancel/status/steer are available?
- What level of model/provider transition visibility is required: selected model per run, fallback chain attempted, or every provider handoff?

## Sources

- Official OpenClaw CLI reference: https://docs.openclaw.ai/cli
- Official OpenClaw Gateway external-app guidance: https://docs.openclaw.ai/gateway/external-apps
- Official OpenClaw Gateway protocol: https://docs.openclaw.ai/gateway/protocol
- Official OpenClaw Plugin SDK overview: https://docs.openclaw.ai/plugins/sdk-overview
- Official OpenClaw MCP CLI docs: https://docs.openclaw.ai/cli/mcp
- Official OpenClaw agent CLI docs: https://docs.openclaw.ai/cli/agent
- Official OpenClaw audit history: https://docs.openclaw.ai/gateway/audit
- Installed package metadata: `C:\Users\Volap\AppData\Roaming\npm\node_modules\openclaw\package.json`
- Installed package docs: `C:\Users\Volap\AppData\Roaming\npm\node_modules\openclaw\docs`
- Installed package declarations/runtime chunks under `C:\Users\Volap\AppData\Roaming\npm\node_modules\openclaw\dist`
