# CatDesk — Competitor research: browser wake, new chats and resilient continuation
**2026-10-14 | Source-researched design input for Codex Desktop**  
**Disposition:** proposed architecture only; **not** permission to install a browser extension, modify the installed independent WakeHost, create ChatGPT chats, or change protected auth/tunnel state.  
**Primary reference:** [Chat On Steroids](https://github.com/totec448-spec/chat-on-steroids) (CoS), `main` repository inspected 2026-10-14; relevant specific source files linked below. Related projects came from prior ChatGPT conversation *CatDesk competitors* (2026-10-11).  
**Read alongside:** `docs/orchestrator/CATDESK_CODEX_DESKTOP_HANDOFF_2026-10-14.md` and `docs/orchestrator/tickets/T-0483_CODEX_SOL_MEDIUM_WAKE_CYCLE.md`.

## Findings: how Chat On Steroids works (verified from source)

**CoS is an Electron desktop app + browser extension + local MCP server.** Its browser is a real authenticated ChatGPT tab. It is **not** a ChatGPT supported new-conversation API, and it does **not** use Codex to drive the ChatGPT conversations. Its own README says the extension observes/automates the ChatGPT UI and locally records chat data, and warns to comply with provider restrictions.

### 1. Waking an existing chat
- `src/main/browser-wake.ts` maintains an authenticated local WebSocket `/wake` channel to the browser extension. It sends the **signal** `wake`, not the message payload, to connected authorized extension sockets. Reconnecting sends `wake` again so the extension re-reads pending work; a 20-second ping/pong heartbeat supports extension service-worker liveness. The WebSocket is only a notification transport, **not durable delivery authority**.
- `src/main/session/input.ts` maintains the durable local queued input and exclusive browser-send ownership. Its source explicitly states that a claimed browser send is **not automatically retried** if ACK is lost; an ambiguous send is not proof of failure.
- `extension/background.js` polls/picks up current app work after notifications and via a limited Chrome `alarms` retry mechanism. `deliverDesktopInputs(...)` first finds an open tab whose actual URL belongs to the intended `conversationId`; if none exists, it opens `https://chatgpt.com/c/<id>`. It persists an `inputOpenings[input.id]` *election* before creation (a lost tab-creation acknowledgment does **not** authorize opening a second tab), then instructs the tab via `chrome.tabs.sendMessage`.
- `extension/content.js` is the in-page observer/actor. It checks the document identity/route, composer and user-turn details and performs the send only under the app's authenticated durable claim + one-time send authorization. The app's browser event journal keeps provisional events keyed to a tab while a new conversation has no server ID, then rekeys to the exact accepted ID.
- Distinguish three claims: **browser tab opened**, **USER message accepted/observed**, and **assistant response completed**. None alone authorizes marking the others complete. Normal Goal/Loop can queue a next message only after the previous turn's completion/ownership checks.

Source links:
- [WebSocket wake broadcaster](https://github.com/totec448-spec/chat-on-steroids/blob/main/src/main/browser-wake.ts#L1-L90)
- [Durable input and send custody](https://github.com/totec448-spec/chat-on-steroids/blob/main/src/main/session/input.ts#L1-L120)
- [Extension wake/polling](https://github.com/totec448-spec/chat-on-steroids/blob/main/extension/background.js#L1025-L1065)
- [Existing-chat versus new-chat routing and elected tab ownership](https://github.com/totec448-spec/chat-on-steroids/blob/main/extension/background.js#L2200-L2410)

### 2. Creating a brand-new ChatGPT conversation
- `extension/background.js::createChatTab` uses browser-extension `chrome.tabs.create({url,...})` or a dedicated background window/tab group. For *new* chats it loads `https://chatgpt.com/?cos-input=<event-id>` (or a specific ChatGPT Project route `https://chatgpt.com/g/<project-id>/project?... `); it does **not** pre-invent a ChatGPT conversation UUID.
- Before an initial send, the new chat is id-less. A content script prepares the native New Chat/Work→Chat composer and sends the **first normal USER message** using browser UI. ChatGPT then assigns its own `/c/<id>`; the extension observes that route and binds provisional tab/event ownership to that ID. `extension/content.js` can also launch a New Chat directly from a user-authored Goal/Loop objective.
- For reuse of an idle tab, CoS requires an exact document identity/navigation epoch, empty-draft proof, non-pinned status, and a prepared transition; it does **not** assume any tab with ChatGPT open can be repurposed. Failed pre-send New Chat transition can spend exactly one persistently recorded replacement-tab attempt. Browser sends, browser tab creations, and continuation handoffs each have distinct idempotency fences.

Source links:
- [Actual native browser tab creation](https://github.com/totec448-spec/chat-on-steroids/blob/main/extension/background.js#L2133-L2198)
- [Id-less / project-aware new chat URL and persistence](https://github.com/totec448-spec/chat-on-steroids/blob/main/extension/background.js#L2200-L2410)
- [First USER message promotes id-less chat to real conversation](https://github.com/totec448-spec/chat-on-steroids/blob/main/extension/content.js#L8330-L8430)
- [Provisional journal rebinding](https://github.com/totec448-spec/chat-on-steroids/blob/main/extension/background.js#L725-L750)

### 3. Carrying context when a chat is full: Compact & Resume
- `src/main/session/handoff-prompt.ts` asks **Chat A itself** to write a self-contained handoff as its *final answer*. A continuation marker ties the brief to the exact requested handoff; bounded quality/completeness checks refuse empty or suspiciously short briefs.
- `src/main/session/handoff.ts` saves the brief, provenance, original conversation/turn and an application-minted continuation identity. The same `resumeBootstrapText` formatter is reused when presenting the brief as an ordinary USER message to replacement Chat B.
- `src/main/session/continuation.ts` is the important durable feature: a **transaction with states open → summary → claim → commit / abort**. It pins the local session/workspace/goal/workers to Chat A, creates and claims at most one Chat B, then atomically rebinds the local session only after B proves it received the exact bootstrap. Aborted migration leaves A authoritative; a competing claimant B' is refused. A write-ahead log plus idempotent recovery guards against crashes and orphan transfers.
- The **local session identity** (task ledger, workspace association and agent fleet) is distinct from the changing **ChatGPT conversation ID**. The handoff brief supplies *model context*; the durable local store remains the task/workflow source of truth. This is the key improvement CatDesk should adopt.

Source links:
- [Continuation transaction invariants and preflight/commit](https://github.com/totec448-spec/chat-on-steroids/blob/main/src/main/session/continuation.ts#L1-L47)
- [Handoff prompt from original chat](https://github.com/totec448-spec/chat-on-steroids/blob/main/src/main/session/handoff-prompt.ts#L13-L55)
- [Shared destination bootstrap and provenance](https://github.com/totec448-spec/chat-on-steroids/blob/main/src/main/session/handoff.ts#L69-L130)

**Limits:** This is a source review, not a live reproduction of CoS. The UI automation is unofficial/brittle and subject to account policy and login/CAPTCHA; new chats never grant extra model entitlement or bypass usage/context restrictions.

## Compare with CatDesk's verified state

| Capability | CoS source approach | CatDesk state at 2026-10-14 | Worth adopting? |
| --- | --- | --- | --- |
| Existing chat wake | Authenticated extension WebSocket, durable HTTP outbox, tab selection, in-page send/receipt | Independent Rust WakeHost + Python SeleniumBase dedicated browser, event receipts and timer; installed dev84 Chrome error current gen34; dev85 source CI green but not installed | **Yes**: borrow durable command ownership/reconciliation while **keeping Selenium**; preserve exact-receipt gate |
| Brand-new ChatGPT conversation | Native New Chat UI, then observe first user turn's `/c/<id>` | Fixed canonical `/c/<id>` target setter; no accepted autonomous creation/migration of new conversation | **Yes**: scoped new-chat owner and server-issued ID capture |
| Moving full long-lived work A→B | Durable continuation WAL, handoff provenance, atomic session rebind and worker/goal continuity | Manual chat rollover/paired target set and static handoff documents, not one automatic transaction | **High priority**: source-to-destination attested continuation |
| Automatic ongoing task | Goal/Loop finish checkpoints and durable input queues | CatDesk task orchestration, Codex/Qwen workers, review bundles; Sol/medium worker route unfinished | Adopt checkpoints and visible progress, keep CatDesk's stricter source security gates |
| Multiple worker chats | Worker sessions with local identity, background tabs and shared local app | Multi-project bindings and historical DAG tests exist; fair queues and GUI still planned | Later: project queues, owner leases and explicit per-worker UI |
| Permissions | Shared-folder/capability UI but shell remains a risky local power | CatDesk fixed allowlisted MCP operations, signed protected state and explicit review paths | **Retain CatDesk boundaries**, add friendly capability UI but do not widen shell |

## Other projects from the competitor discussion — specific ideas, not proof of wake features

- [Desktop Commander MCP](https://github.com/wonderwhy-er/DesktopCommanderMCP): mature MCP file/terminal/process streaming/management ergonomics; borrow tool naming, long-running process receipts, approval UX. Its MCP README is **not evidence of automatic new ChatGPT chat creation**.
- [ChatGPT Local Coder](https://github.com/hoangcoderr/chatgpt-local-coder): small self-hosted local MCP tools + stable tunnel; borrow lightweight setup/smoke tests and project-scoped discoverability, not its broadly advertised machine access.
- [ChatCMD](https://github.com/int04/ChatCmd): Rust local runtime, SQLite permission profiles, management web console and optional Chromium extension; study controls/config UX and observability.
- [Codex with ChatGPT](https://github.com/XiaoDuoYa/codex-with-chatgpt): ChatGPT as read-only planning/review, Codex as execution owner. Reinforces CatDesk's safer reviewer/implementer separation.
- [1Code](https://github.com/21st-dev/1code): git-worktree isolation, diff review, multi-agent/kanban, background jobs and tool activity visualization. Borrow strong task-diff-review UX only after runtime sound.
- [OpenMausBot](https://github.com/milind-soni/OpenMausBot): durable agent identities/sidebars, multi-agent chat management; inspiration for eventual Binagotchy/multi-project UX, not proof that its bots automatically create ChatGPT Web chats.

## Updated priority: T-0484 token accounting/compaction AFTER multi-project

Operator decision (2026-10-14): automatic context pressure and SeleniumBase verified Chat A→B rollover are **core** functionality but are scheduled **after the multi-project queue/fairness milestone**, not the next immediate release. Codex-ready gate/ledger/uncertainty/acceptance design: `docs/orchestrator/tickets/T-0484_POST_MULTI_PROJECT_CONTEXT_METER_AUTO_COMPACTION.md`.

CatDesk already counts tool arguments/results with `o200k_base` but without authoritative per-conversation context association. CoS's `eventTokens` counts USER/ASSISTANT, MCP args, MCP result (capped at 10,000 estimated tokens per result), and agent messages via rough characters/4 and excludes transient progress. In both systems, tool input/output traffic is not equivalent to provider context occupancy. Native document/image expansion, hidden system/reasoning and provider truncation are unknown. CatDesk must use a per-conversation deduplicated context ledger + uncertainty rather than add all history input/output.

CoS multi-project source [projects.ts](https://github.com/totec448-spec/chat-on-steroids/blob/main/src/main/projects.ts) stores a local registry (up to 200) with stable IDs, approved roots, exact session→project bindings and optional guarded ChatGPT Project links (`requested → claimed → creating → linked/uncertain`). Workers may inherit a prime chat's project; [workspace-terminal.ts](https://github.com/totec448-spec/chat-on-steroids/blob/main/src/main/workspace-terminal.ts) resolves the actual command cwd from the project; [control-workers.ts](https://github.com/totec448-spec/chat-on-steroids/blob/main/src/main/control-workers.ts) associates spawned workers with their project workspace. [1Code](https://github.com/21st-dev/1code) instead emphasizes per-agent Git worktrees and queues. CoS project isolation is not evidence of scheduler fairness. CatDesk should finish queue fairness/worker ownership first and adopt comparable project binding/durable-workspace guarantees without broadening the filesystem allowlist.

## Proposed roadmap for Codex Desktop — optional enhancement, do NOT displace current release work

**R0 — Finish dev85 current Wake release (existing highest operational priority).** Independently review hash-verified WakeHost installation and test one new generation34 diagnostic after approval; do not replay event `manual-wake-mcp-1791908511726`. Avoid installing a browser extension as a workaround to unfinished release acceptance.

**R1 — Context-pressure estimation and automatic-compaction policy (keep SeleniumBase).** CoS already solves the missing trigger, separately from its browser extension: `src/main/session/store.ts::autoCompactionReady` compares the current **estimated** `contextTokens` with the configurable `compaction.autoTokens` threshold, and `src/main/bridge.ts::considerAutomaticCompaction` additionally requires active same-chat work (or an eligible failed turn) and no competing continuation/stop. Its `src/shared/session.ts::eventTokens` estimates text at about one token per four characters (with wire estimates for USER messages and bounded MCP results); fresh CoS installs currently default to a 400,000-token compaction line and ~533,333-token advisory limit, **not authoritative provider limits**. Implement equivalent estimated per-chat pressure in CatDesk's durable Rust session/turn recorder; use a configurable conservative threshold per model/profile, headroom, low-pressure warnings, and explicit user override. Distinguish total lifetime history from *current ChatGPT conversation* usage and reset only the latter after committed handoff. The existing SeleniumBase adapter can observe same-chat turn activity and safely stop/settle active tools before sending a uniquely keyed handoff request. Test count drift, oversized result, soft threshold while idle, mid-tool-call safe settlement, aborted turn, missing provider telemetry and manual override. Do **not** introduce an extension to implement this.

**R2 — Durable browser command ledger + Selenium WakeHost receipt reconciliation (no new-chat yet).** Extend the existing Rust WakeHost event journal instead of adding an extension or WebSocket: immutable `BrowserCommand { commandId, projectId, targetConversationId, expectedGeneration, wakeEventId, sourceSessionId, createdAt, expiresAt, state }` with one owned Selenium browser/tab lease and state machine `QUEUED → CLAIMED → TARGET_OPENED → SENT_UNVERIFIED → RECEIPTED → COMPLETED` or `ATTENTION`. The Python SeleniumBase process executes only the exact authorized operation, and all retries consult durable Rust authority on restart. Require exact USER receipt and later response completion. Never re-click Send after ambiguous authorization, never treat Chrome error as permission to create another tab/retry original USER send, quarantine old-generation commands. A companion extension is optional future evaluation only if measured Selenium reliability remains inadequate after reviewed fixes.

**R3 — New chat creation and session rebind.** Design an explicit, separately authorized `CreateConversation` operation for an existing known CatDesk project:
  1. Reserve a session rollover ID and source Chat A identity; check single writer and source owner.
  2. Ask A for a complete handoff (or use reviewed durable project/task state); verify model-facing brief and provenance.
  3. In the existing authenticated SeleniumBase browser/profile, navigate to the authorized New Chat home/project UI (same owned tab where feasible, bounded fresh owned tab otherwise), never synthesize a `/c/<id>`; target remains **id-less** until ChatGPT assigns its server-owned conversation ID after the first authorized message.
  4. Persist the one initial send authorization, prove the **new** conversation ID and exact bootstrap receipt without borrowing previous TAB/chat evidence.
  5. Atomically update CatDesk `project.chatgptTargetUrl` **and** installed Wake target/digest/generation under an existing guarded compare-and-swap/rollover lock; rebind current task/session and worker queues. Keep A active until B is fully proven. Crash recovery resumes one transaction, not an entire duplicate create.
  6. Decline mixed/chat collisions, wrong account/project, CAPTCHA, changed route, lost authorizing document, exceeded context or provider access warning.
  7. If any step fails, leave A as the designated authority and escalate with safe bounded diagnostics; do not mutate signed image or live secure tunnel.

**R4 — Optional self-continuation UX and worker dashboard.** One-click `Continue in new chat`, preview handoff and provenance, show old/new URL, review/event receipt, operator approval, per-project agent/worker status, explicit no-spam limits. Only after the lower layers pass; no silent new chat creation when provider refuses or restricts automation.

### Acceptance matrix (must be deterministic and real-browser-tested before calling it complete)

- Existing chat: exact target, no duplicates after crash/Selenium process restart, transient Chrome error reopens without re-sending, wrong tab/account fail closed, exact current-generation USER receipt + terminal timer and assistant completion.
- New chat: id-less state until first authorized USER, correct project destination, server-issued `/c/<id>` observed, exact new-chat bootstrap receipt, one creator per transaction; tab creation ACK lost ≠ permission to duplicate; old A session remains authoritative if B creation/claim fails.
- Cross-chat continuity: durable task/session/worker ledger preserves history without relying on handoff text alone; only atomic verified A→B registry/Wake generation pair; destination collision/race, stale reply and interrupted WAL resume safely.
- Security: keep signed T0366 and official Secure MCP external owner, no profile/cookie/token export, operator-controlled account challenges, no automatic creation for usage-limit circumvention, provider policy conformance, no unrestricted new browser automation permission by default.
- Review gates: Windows CI Rust + Python Wake + Independent WakeHost 3/3 for exact source SHA, independently reviewed feature, immutable package manifest/install receipt, manually witnessed latest event SENT/COMPLETE, and *separate* genuine task-produced `review_ready` natural Wake acceptance.

**Decision:** Keep CatDesk's Rust WakeHost and SeleniumBase browser as the primary implementation. The highest-value lessons are CoS's `estimated active-conversation pressure + durable local session + one-shot handoff receipt + atomic verified Chat A→B rebind`, not its extension or `chrome.tabs.create` call. Implement as later isolated milestones after dev85 and the real Codex task→review cycle. An extension is only an *optional alternative* if Selenium cannot meet measured reliability needs; do not switch browser architecture by default. Independently review browser UI/provider restrictions before enabling automated chat creation.
