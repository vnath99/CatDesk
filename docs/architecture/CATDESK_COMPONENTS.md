# CatDesk — Components and Interface Contracts

**Decision 2026-11-22T02:52:27Z.** The development lifecycle `catdesk.ps1 promote <artifact-sha256>` asks the canonical native resolver for the active attested backend, closes that process, and pins the candidate before invoking the existing backend-only promotion transaction. [Facade qualification](../verification/BACKEND_PROMOTE_FACADE_2026_11_21.json). It waits for the exact matching terminal receipt without creating a second transaction. The Wake broker independently exposes local-user service start and unsigned local selection under enrolled policy. The broker still needs a protected intake. There is one broker and one connector. [Qualification and evidence](../verification/ROUTINE_PROMOTION_AND_V3_OWNER_RECONCILIATION_2026_11_21.json).

**Decision November 21, 2026, 7:31 p.m. EST.** The installed backend serves the canonical v5 connector. The provisioned, stopped Wake broker manages protected Wake selection. For ordinary task delivery and task-issued manual Wake, the backend also validates the selected adapter and bridge pair. Frozen d22 rejects the reviewed V3 pair. Integrated source `56aa9912` admits that pair and retains `git.checkpoint`. Verify selected Wake artifacts and qualify an exact headless release independently before the live cycle. [Wake handoff](../verification/WAKE_V3_OPERATOR_HANDOFF_2026_11_21.json). [Backend pair evidence](../verification/BACKEND_WAKE_PAIR_COMPATIBILITY_2026_11_21.json).

**Decision 2026-11-16T01:38:56.316414+00:00:** The local inspect-review-request command binds existing workflow/contract/consent/message hashes, actual approved WAITING_FOR_CHATGPT task, lease and state version; it holds the original native session lock and existing browser owner and rechecks after observation. Preflight starts no browser. Existing registered-project inspection still refuses unregistered targets; the review-only path carries explicit original destination approval and no Wake authority. Existing filtered submission metadata now retains semantic/anchor agreement counters. [Evidence](../verification/APPROVED_REVIEW_CONTEXT_OBSERVATION_2026_11_15.json).

**Decision 2026-11-16T00:34:15.175115+00:00:** inspect-readiness optionally accepts a frozen request file and exact raw SHA-256. Native pre/post hash checks, hash-pinned embedded probe and existing primitives return only a strict metadata frame. Normal readiness remains compatible. This mode creates no delivery receipt and cannot authorize a resend; no new MCP tool or plugin is added. [Evidence](../verification/FROZEN_REQUEST_OBSERVATION_2026_11_15.json).

**Decision 2026-11-16T00:09:22.940282+00:00:** Existing task metadata/resume now exposes journal-derived toolBudget. Terminal failed calls count; unreadable journals return null recorded/remaining counts. Original immutable admission and contract limits are retained across rollover. Elapsed limits are per serving connection; hidden ChatGPT turn usage is unknown. No new tool or plugin is required; this source change is not installed. [Evidence](../verification/NATIVE_RUNTIME_REPAIR_APPLIED_2026_11_15.json).

**Decision 2026-11-15T22:34:51Z:** Native logical filesystem permissions and OS resource ACLs are separate layers. Controlled private fixtures prove read/execute provisioning repairs this failure class and still denies writes; the application command implementation is unchanged. [Evidence](../verification/NATIVE_RUNTIME_READ_ACCESS_2026_11_15.json).

**Decision 2026-11-15T21:34:58Z:** The Unix app-server control socket requires WebSocket HTTP Upgrade; JSONL applies to its stdio transport. Existing bounded proxy diagnosis now verifies readiness without a new daemon. Command probes reuse the real runner, not a parallel implementation. [Evidence](../verification/NATIVE_SANDBOX_READINESS_AND_COMMAND_QUALIFICATION_2026_11_15.json).

**Decision 2026-11-15T21:01:12Z:** Future verify.command receipts optionally carry failureDiagnostic before the existing receipt hash/persistence. Successful receipts and existing tool contracts stay compatible; original private receipts are not rewritten. [Evidence](../verification/NATIVE_COMMAND_FAILURE_DIAGNOSTIC_2026_11_15.json).

**Source validation 2026-11-15T20:32:08Z:** retained proposal operations are now checked against their original full diff hash within the existing non-writing recovery validation. This adds no tool call, replay, authority, install or service. Source `9d96f8e32f3fdf8b528d15eda453e9398f74a392` passes eight native recovery tests and strict checks; selected live test app remains source `79a449b`.

**Decision 2026-11-15T20:23:03Z:** Dedicated-owner task recovery now admits exact settled read/search/preview histories using the existing retirement transaction. `coldRecoveryProfile` describes this as DEDICATED_NAMED_JOB_NONMUTATING; the legacy zeroCallRecoveryOwnerClaim field remains for interface compatibility. The existing `review read` command is the event-bound body reader; `work response` reobserves installed timer state. No tool publication, connection or second reader service is added. See [dated proof](../verification/SETTLED_READ_ONLY_TASK_RECOVERY_2026_11_15.json).

**Release-model clarification 2026-11-15T19:33:36Z:** one system and one unchanged canonical v5 interface. Frozen independently verified releases are production; authorized source/worktrees/builds and isolated local fixture tests are development. Reuse current selection, immutable artifacts, existing fallback/promotion/rollback and bounded owners. No new installation, release manager, tunnel, plugin, external connection or backend infrastructure. Local source work proceeds independently of protected-release acceptance.

## 2026-11-15T19:18:45Z — Development/production separation decision

The existing runtime is implemented in `wake/src/development_runtime.rs` and `src/operator_development_backend.rs` / `src/operator_development_project.rs`. Reuse `web-dev-runtime` package/status/run commands and the existing suspended-child owner. A normal root application build is the packaged Binagotchy app; unchanged Host/adapter/bridge may be retained with explicit component provenance. Production supervisor activation is omitted and refused in the development realm; read-only status/preflight remain available. See [dated qualification](../verification/DEVELOPMENT_PRODUCTION_SEPARATION_2026_11_15.json) and [current handoff](../../CATDESK_PROJECT_HANDOFF.md) for exact maturity and remaining gates.

> **Revision timestamp (UTC):** `2026-11-14T18:47:56Z` · **Status:** Reviewed consolidation reference; source paths verified against the November 14 development inventory, not a claim of deployed qualification.
> **Authority:** [System architecture](CATDESK_ARCHITECTURE.md) determines product flow; [security](CATDESK_SECURITY_MODEL.md) governs authorization; [release/recovery](CATDESK_RELEASE_AND_RECOVERY.md) governs operational lifecycle. Live capability status comes from the [project handoff](../../CATDESK_PROJECT_HANDOFF.md) and original journals.

## 1. Component map and evidence vocabulary

| Layer / component | Responsibility | Concrete source/record starting points | Current boundary |
|---|---|---|---|
| ChatGPT Web agent | Interprets project objective, plans edits, invokes MCP tools, checkpoints and requests continuation | Registered project conversation, observed USER/assistant receipts | A generated response is not proof of an actual local change |
| Secure MCP transport | Forwards authenticated connector traffic to the local CatDesk server | `src/mcp.rs`; official external tunnel/client state | Tunnel process is externally/operator owned; CatDesk cannot silently recreate it |
| Rust local backend | MCP catalog, tool dispatch, project-scoped enforcement, task/result status | `src/mcp.rs`, `src/project_mcp.rs`, `src/project_mcp_task.rs` | Selected binary/catalog must be checked; published source is not installed proof |
| Registry and project control | Binds project ID → approved workspace, task owner, current Web URL, target digest/generation and history | `src/delegated/autonomy_projects.rs`, `src/operator_web_onboarding.rs`, `src/operator_web_initialization.rs` | Exact target binding and original grants gate effects |
| File/Git/verification | Read/search/patch/diff/verify within registered worktree; attributable edit and test evidence | `src/project_mcp.rs`, `src/project_mcp_task.rs`, `src/delegated/patch_engine.rs` | Generic global-workspace tools are *not* evidence of project-isolated execution |
| Durable work/receipt journals | Track attempt IDs, original task identity, result/unknown status and audit | `src/operator_web_receipts.rs`, `src/delegated/journal.rs` | Must reconcile unknown effects without replay |
| Work lane / task coordinator | One active project local execution lane; others safely queued, paused or yielded | Existing queue/lease state in CatDesk; new serial policy requires verification | Serial MVP is an operator requirement; full installed enforcement not established |
| TurnTimer | One turn's start/status/stop and 18-min soft checkpoint/20-min deadline | v5 `catdesk_turn_timer`; Wake TurnTimer store | ~19-minute graceful finish is agent policy; timer isn't arbitrary/configurable |
| WakeHost / browser | Bounded project-specific SeleniumBase submission and readback | `wake/src/runtime.rs`, `wake/src/protocol.rs`, `scripts/wake_bridge.py`, `wake/adapter.py` | Browser lease, exact registered target and no-replay state govern sends |
| Context meter / rollover | Estimated context pressure, compact summary, successor URL and paired target transition | `src/operator_web_rollover.rs`, `src/operator_web_auto.rs`, `src/operator_web_observe.rs`; ticket T-0484 | Partial source/controlled evidence; incomplete provider usage measurement |
| Supervisor / lifecycle | Passive diagnostics, bounded restart, installed/selected release authority and ownership | `src/supervisor_lifecycle.rs`, `src/release_pair_stopped.rs`, `catdesk.ps1`, `wake/src/install_broker_*` | Protected operations demand separate reviewed authority; not all sources runnable |
| Optional native model workers | Codex/Qwen provider policies, bounded sessions, tool-call processing | `src/delegated/*`, `src/delegated/codex_cli.rs`, `src/delegated/provider_router.rs` | Optional; do not require Codex model usage for direct Web MVP |
| GUI | Binagotchy-style operator visibility and controls | Existing original GUI and WakeHost/Binagotchy lifecycle | Visible installed GUI acceptance remains separate from backend/tool behavior |

**Maturity terms:** `DESIGNED` (this document), `SOURCE_TESTED`, `CONTROLLED_LIVE`, `INSTALLED_VERIFIED`, `UNATTENDED_VERIFIED`, `OPEN/UNVERIFIED`. A component may have parts at different levels. Status is always tied to exact artifact and scope; never paste an old whole-project PASS into the current architecture.

## 2. Project registry contract

Each registered project should have a stable logical `projectId`, approved normalized Windows workspace root and physical identity, Git repository/branch expectations, verification profile, optional native worker/thread ownership, current ChatGPT Web URL, target digest and monotonically changing generation, historical predecessor URLs and task associations.

**Admission invariant:** the tool request's authenticated assignment, permitted project/worktree, exact task and current target generation/digest must agree at the transaction boundary. A change to URL, root, task, release generation or owner cannot inherit authority from a stale request.

**Multiple projects:** BYOVD, Wake2, ComicLens and other registered projects may coexist; registration is not the same as permission to execute a task. The core one-active-project lane is global for local mutations and browser-work sequencing, but project-specific queues/journals stay independent. No project may consume another's task or registered URL. Operator status reads can enumerate all projects without waking them.

**Proposed minimal project status projection (design, not guaranteed current API):** `projectId`, `workspaceDisplayName`, `currentChatUrl` (operator UI only), `targetGeneration`, `projectState`, `activeTaskId`, `queueDepth`, `lastVerifiedWake`, `lastErrorCode`, `verificationMaturity`. Machine-readable status must be privacy-safe in external surfaces; avoid full local paths and private transcript bodies.

## 3. Direct ChatGPT Web tool cycle

```text
registered Web task + exact assignment
    -> authenticated project-scoped v5 call
    -> per-task/workspace permission and original-journal check
    -> bounded file/diff/verification operation
    -> durable result + actual test/exit evidence
    -> user-visible checkpoint and optional manual Wake
```

The current development design includes `read`, `search`, `patch.preview`, `patch.apply`, `patch.compare`, `diff.actual`, `verify.command` and explicitly admitted `wake.manual_end_turn`. The v5 cached compatibility path uses an exact `CATDESK_WEB_TASK=` JSON envelope through `run_command`, **not** arbitrary shell text; it requires the original project, request, event, assignment SHA, generation/digest, execution key, task/call IDs, tool and arguments. When this route is used, omit `cwd` and timeout overrides. This closed envelope neither bypasses task grants nor establishes ChatGPT conversation-origin cryptographic identity by itself.

**Acceptance must measure effects:** exact target-scoped file diff, real verification exit/code, correct Git state where requested, no unintended sibling changes, authorized journaled task outcome and one attributable completed Web response. An error or stale task is a recorded refusal, not completion. A newly selected binary with source support still needs an installed Web-client proof.

## 4. TurnTimer and continuation state machine

**Turn protocol**:

1. `START` once as a new actual turn begins; retain server-generated timer ID.
2. Check `STATUS` periodically (especially before slow tools). No busy polling that burns the turn.
3. At the fixed **18-minute soft checkpoint**, stop initiating risky, long-running or irreversible work. Store completion evidence, remaining work, original task ID, open effects and next safe action.
4. Aim to exit near/before **19 elapsed minutes** (operator policy), leaving buffer before the **20-minute fixed deadline**. A submitted wake intent must be journaled before the conversational turn ends; a human-readable promise to resume is not enough.
5. `STOP` only the manual timer appropriate to that turn when allowed. Wake-event timers and manual timers are not interchangeable IDs. A model cannot assume a timer that was not started is running.

**Wake phases (design-neutral, no-replay ordering):**

| Phase | Meaning | Allowed next step |
|---|---|---|
| `REQUESTED` | Task-bound intent stored, no browser effect proven | Resolve eligibility and lease |
| `QUEUED/CLAIMED` | One project-specific event admitted/owned; no submission observed | Continue only under the original journal and target |
| `PRE_SUBMIT` | Draft/target/editor checks; no acknowledged send | Revalidate under exclusive browser ownership |
| `SUBMITTING/UNKNOWN` | External effect may have happened | **Read-only reconciliation only**; no blind retry |
| `SENT` | Exact USER receipt observed for intended event | Never send again; observe reply |
| `RESPONSE_PENDING/COMPLETE` | Assistant response not yet / now observed | Check actual task effect separately |
| `ATTENTION` | Login, CAPTCHA, drift, browser/error or unsupported state | Report bounded cause; preserve originals |

Names above are a conceptual cross-system projection, not a claim that every installed store uses these exact strings. Store-native status names and meanings take precedence over prose.

### Historical timer/Wake interoperability evidence

The [system architecture](CATDESK_ARCHITECTURE.md#historical-acceptance-preserved--single-chat-wake-and-timer) preserves the October 10 **18m56s timer STOP** and October 11 **exact USER-appended Wake receipt** for the historical CatDesk conversation. They demonstrate real pieces of this component contract. They do not establish that an expired timer automatically requested every Wake or that the presently installed WakeHost can still deliver. Regression fixtures must distinguish (1) manual timer state, (2) Wake event timer state, (3) durable queue intent, (4) actual browser USER receipt, and (5) completed ChatGPT response. In the historical/current comparison, a regression is not a new feature and a stale state snapshot is not a new test failure.

## 5. One-active-project scheduler

**First release contract:** One admitted project task at a time may hold the local execution lane and drive successive wake continuations. A separate project task is durably queued with its own URL and identity. The active task retains the lane between turns until done, explicitly yielded/paused, or safely deactivated after reconciled state.

**Required liveness controls:** operator `pause`, `yield` and resume at safe boundaries; an attention-blocked task must not monopolize the lane indefinitely; clear `active` versus `queued` in UI; no takeover merely because a process is gone; no host-wide work execution while an unknown prior operation is unresolved. Do not steal active native sessions from another AI project.

**Serialization scope:** project mutation/admission and shared browser use are serialized as needed. Passive status, health checks and forensic read-only recovery may run independently. Further parallelism can be layered on isolated worktrees later, but must not silently reduce isolation or browser no-replay guarantees.

## 6. Context meter and rollover components

Ticket [T-0484](../orchestrator/tickets/T-0484_POST_MULTI_PROJECT_CONTEXT_METER_AUTO_COMPACTION.md) requires:

- Attribution by project ID + exact conversation URL/target digest and generation, turn/upstream call ID, content digest and revision.
- Three separately displayed metrics: **observed MCP traffic**, **estimated unique active-conversation context**, **uncertainty/headroom**.
- Deduplication of retries, repeated readback and streamed revisions; never count a historical predecessor generation as the replacement chat's current context.
- Bounded result visibility and attachment/hidden-context uncertainty. The `o200k_base` estimator is **not** the selected provider's verified usage counter. Do not present local 96k/24k/8k policy examples as provider facts.
- Calibrated sample classes: prose, code, JSON, Unicode, literal special markers, huge/truncated results, nested calls, replay, missing metadata and attachments. Record method version and observed undercounts.
- Separate source handoff, new-chat URL creation, successor initialization, exact paired-target CAS and transaction recovery. Unknown create/send is held without resubmission; predecessor history remains queryable.

**Rollover transition (conceptual):** `MONITORING → PRESSURE_REACHED → HANDOFF_REQUESTED → HANDOFF_RECEIVED → SUCCESSOR_CREATED → SUCCESSOR_VERIFIED → PAIR_COMMITTED → CONTINUATION_VERIFIED`. Each step has durable journal identity and a refusal/reconciliation path. Shared browser creation takes its own exclusive lease, distinct from ordinary file work.

## 7. Recovery, GUI and optional provider interfaces

- **Recover/diagnose:** the same status schema should identify layer, selected runtime identity, observed reason, last safe result, owner and next action; passive diagnose does not mutate. See [release/recovery](CATDESK_RELEASE_AND_RECOVERY.md).
- **Minimal Binagotchy-style GUI:** `Project`, `Current chat URL`, `Target generation`, `Active/queued/paused`, `Task`, `WakeHost/last receipt`, `Layer health`, `Diagnose` and `Recover` controls with correct confirmation gates. Merely opening GUI must not silently start or replace WakeHost. Current GUI availability is not equal to accepted visible project/dashboard behavior.
- **Optional native AI:** provider registry and Capabilities distinguish Codex CLI's local sandbox/verification utility from an actual paid/quota-limited Codex model request. Local Ollama/Qwen and future compatible DeepSeek clients reuse the same project-scoped tools and audit boundaries. Their auth and rate limits are separate. No fallback to a remote model without authorized disclosure policy.

## 8. Component acceptance tests (minimal operator-visible proofs)

| Component | Smallest compelling proof |
|---|---|
| Project isolation | Two projects with different roots and exact chat targets; blocked cross-root read/write; no foreign Wake |
| Web file executor | Actual Web caller edits one permitted file, verifies it, obtains exact diff/result record; zero required Codex model calls |
| Timer | Start/check/checkpoint/stop and turn rollover before the real deadline; idempotent / separate event timers |
| Manual Wake | Exact current-target USER receipt + later completed response; no double send under timeout/restart |
| Serial scheduler | Active project continuation holds lane; queued other not lost; explicit yield releases safely; attention recovery proceeds |
| Rollover | Source-authored handoff → one new URL → verified initialized chat → paired CAS target; crash at each boundary does not replay |
| Recovery | Fault individual eligible layers, return correct `RECOVERED`, `REFUSED`, or `UNVERIFIED` with supported evidence |
| GUI | Interactive Win11 display of two project rows, actual URL/status, controls and stale-state qualification |
| Other providers | Only after core path; separate authenticated tools/scopes, disclosure and completion proof |

### Original GUI design reference

The operator favors the simple UI from the upstream [Xeift/CatDesk preview](https://github.com/Xeift/CatDesk/blob/main/docs/images/catdesk_preview.gif). This is a **visual product reference**, not authority to copy upstream source or assets without checking provenance and license. Initial GUI scope is a project/Wake status list, current project and URL association, queue, active/paused state, and safe diagnose/recover affordances. It does not justify redesigning the interface ahead of P0 execution and recovery acceptance.

**QA addendum 2026-11-14T19:07:55Z:** original GUI reference and single-chat historical timer/Wake evidence made explicit; no API/installed maturity promoted.

## 9. Decision history and source references

**Architecture adoption timestamp:** `2026-11-14T18:47:56Z` — component boundaries clarified and single-active-project policy specified for MVP. Material future component contract revisions must be appended with timestamp, rationale and related milestone; routine test results belong in the root [activity log](../../CATDESK_ACTIVITY_LOG.md).

**Source pointers:** `src/project_mcp.rs`, `src/project_mcp_task.rs`, `src/operator_web_receipts.rs`, `src/operator_web_rollover.rs`, `src/operator_web_auto.rs`, `src/delegated/autonomy_projects.rs`, `src/delegated/provider_router.rs`, `wake/src/runtime.rs`, `wake/src/protocol.rs`, `scripts/wake_bridge.py`, `wake/adapter.py`; [historical technical index](../orchestrator/ARCHITECTURE_DOCUMENT_INDEX.md). These identify source responsibilities and do **not** assert that the installed release contains every feature.

## 2026-11-16T02:38:42.136005+00:00 — Existing startup owner qualification

The existing immediate workspace-monitor launcher now initializes a Windows PowerShell child with its host-default module path. Its executable, arguments, hidden window, singleton and unrelated environment remain unchanged; the parent environment is untouched. This corrects a reproduced Core-only module shadowing failure without another launcher service. The source fix is not installed. [Installed and source evidence](../verification/SUPERVISOR_STARTUP_HEALTH_2026_11_15.json).

## 2026-11-16T02:51:59.272044+00:00 — Explicit scope on the existing startup owner

The facade and passive doctor share exact task/Run scope interpretation. Both logon and immediate arguments carry BackendOnly when explicitly selected; ordinary status reads the actual known registration. Mixed scopes and a surviving unregistered monitor fail closed. Registered scope and a mutex signal do not prove live process scope. [Source verification](../verification/BACKEND_ONLY_AUTOSTART_SCOPE_2026_11_15.json).

## 2026-11-16T03:27:34.014882+00:00 — Local UI host bridge qualification

C1-UI-002 retains the existing widget/resource identity and appearance. The source bridge adds bounded initialization, validated parent messages, result hydration, size notifications, health ping and teardown. It automatically invokes no server tool, follow-up or retry. The source remains on the existing independent UI feature branch under PR 27. [Exact source and local evidence](../verification/C1_UI_002_HOST_BRIDGE_2026_11_15.json).
