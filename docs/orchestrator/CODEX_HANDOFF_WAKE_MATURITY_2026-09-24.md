# CODEX HANDOFF — CATDESK WAKE MATURITY / RECOVERY / CLI PROGRAM
_Date: 2026-09-24_
_Canonical control chat: https://chatgpt.com/c/6ab465a8-63a8-83ea-adfc-758e368769e2_
_Debug-only Test chat: https://chatgpt.com/c/6aa98932-3a84-83e9-afa7-ad10a5c495a5_
_Workspace: <USER_PROFILE>\OneDrive\Desktop\Projects\CatDesk-codex-loop_

## 0. Role and operating model

You are taking over as the primary implementation/debugging agent for the next CatDesk phase. Treat ChatGPT Web as senior architecture/review authority and CatDesk as the durable local control plane. The operator does not want to relay prompts between agents or repeatedly run diagnostics that CatDesk can execute itself.

Do not ask for GitHub access. Git/CLI access is expected to already exist locally. Preserve the dirty worktree unless a reviewed task explicitly changes it. Do not publish or clean Git history merely to make the workspace look tidy.

Operator PowerShell commands, if genuinely unavoidable, must always be a single copy-pasteable one-liner. Prefer CatDesk-native controls instead of asking the operator to run commands.

Keep CATDESK_MILESTONES.md, CATDESK_NEW_CHAT_NOTES.txt, .catdesk/current_plan.md, .catdesk/todo.md, .catdesk/session.md, and relevant ticket/review bundles synchronized whenever material behavior changes.

The project should remain audit-oriented rather than authorization-heavy: leave truthful evidence, but do not add new trust domains, recurring UAC/signing ceremony, or feature-creep trust mechanisms.

## 1. Current CatDesk state — start here

Transport:
- CatDesk-Local-Tunnel-v3 is CONNECTED_VERIFIED.
- Local MCP is READY and self-check reports 89 tools.
- Official Secure MCP/tunnel runtime is externally owned and monitored by CatDesk; do not replace or casually restart it.
- Auto-connect/auto-recover are enabled.
- Current canonical project + Wake authority is Chat33:
  https://chatgpt.com/c/6ab465a8-63a8-83ea-adfc-758e368769e2
- Wake target generation: 16.
- Wake target digest:
  d32961ec350c42d039f5514e9171a59574674ecc291b82c0c28f4d9c14336212

Installed Wake:
- Current installed Wake: 1.0.0-dev.59.
- Installed package recorded in milestones:
  1.0.0-dev.59-03bfccd4ae6e-57791d5851a3
- Current host is RUNNING.
- dev.59 includes the latest pre-submit browser-readiness stabilization:
  1. exact target/editor must remain continuously ready for 10 seconds before typing;
  2. Stop/generation controls must remain continuously absent for 10 seconds before typing;
  3. exact loaded blank shell with no editor/send/Stop for 10 seconds gets one bounded normal reload;
  4. no reload/relaunch is introduced after first browser write.

Current Wake status is not clean enough to call mature:
- current status reports RESPONSE_COMPLETION_UNPROVEN / browser ATTENTION;
- queueDepth is 3;
- historical staleCount is 21;
- two old timers are ATTENTION past deadline:
  manual-wake-dev57-queued-sequential-003
  review-adc-t0422-dev58-wake-sequencing-receipt-review-20260924-6-independent_final_review
- these ambiguous historical records must remain preserved/quarantined and must never be replayed, downgraded, or fabricated as SENT.

Important proven milestones:
- dev.56 proved live current-event turnTimers during a real natural Wake and terminal SENT/COMPLETE.
- dev.57 fixed live queue/status convergence and prior-generation timer filtering; first installed-path diagnostic passed.
- dev.58 fixed two serious sequencing issues:
  * response completion could previously false-complete during an intermediate tool-call pause;
  * PAUSE/STOP could race after browser submit dispatch but before exact receipt drain, losing durable delivery evidence.
- dev.59 implements the operator-observed 10-second UI stabilization / blank-shell recovery behavior.
- T-0422 dev.58 natural review event did obtain an exact generation-16 USER receipt, but its response observer later failed closed as RESPONSE_COMPLETION_UNPROVEN. Do not replay it.
- The supported manual dev.59 readiness diagnostic has not yet been cleanly queued from the current ChatGPT connector because the source-registered Binagotchy MCP command is not exposed in this chat’s connector catalog.

Hourly continuation:
- Exactly one hourly deadman should remain enabled for canonical Chat33.
- It is fallback only. It never counts as natural Wake acceptance.
- Its instruction should stay intentionally generic: rehydrate durable docs and continue where work left off. Do not embed rapidly stale ticket/version state in the hourly prompt.

## 2. PRIORITY 1 — Make the Wake system genuinely mature

This is the operator’s first priority. Do not move on to cosmetic or unrelated work while major Wake correctness/debuggability issues remain.

### 2.1 Create a separate persistent Codex browser-debug profile

Do NOT use or copy-lock the production WakeHost delivery profile as Codex’s working browser profile.

Create a separate persistent authenticated debug profile dedicated to Codex visual debugging. Suggested conceptual identity:
- Wake production profile: remains exclusively owned by installed WakeHost.
- Codex debug profile: a second durable profile, e.g. a dedicated local Chrome/Codex profile under a clearly separate root such as %LOCALAPPDATA%\CatDeskCodexDebug\browser-profile, or the equivalent browser-profile mechanism supported by Codex desktop/Chrome integration.

Expected operator interaction:
- The operator may need to log into ChatGPT exactly once in the Codex-debug profile.
- After login, persist and reuse that profile across debugging runs.
- Do not ask the operator for their ChatGPT password.
- Do not read/export/copy cookies, tokens, passwords, or credential database contents.
- Do not point both WakeHost and Codex at the same live profile concurrently.

Goal:
Codex should be able to visually observe ChatGPT Web while Wake is being tested: actual rendered controls, page hydration, Stop/Pause visibility, blank shell states, timeout cards, Retry controls, page reloads, navigation drift, tool-call pauses, final completion controls, and browser lifetime.

Use visual/CDP observations to correlate:
- what the user actually sees,
- what Selenium/CDP reports,
- what WakeHost status.json reports,
- what the durable Store reports,
- and what CatDesk thinks the delivery state is.

This debug profile is observation/debug authority only. It must never become an alternate Wake sender or bypass exact Wake authority.

### 2.2 Restore first-class bounded Wake test control

Source already implements/registers a bounded CatDesk/Binagotchy MCP command surface for:
- status
- start
- pause
- resume
- stop
- queue
- test
- retire

The source-side command is commonly referred to as catdesk_binagotchy_command.

Problem:
- local MCP reports 89 tools;
- source is current;
- this already-open ChatGPT connector catalog still does not expose the bounded Binagotchy tool.

Do not substitute raw Selenium, direct Store edits, arbitrary shell, or manual browser replay.

Determine the actual cause:
- connector/chat schema snapshot;
- plugin catalog caching;
- tool-list registration mismatch;
- serving catalog omission;
- or a refresh/reconnect boundary.

Ideal behavior:
- ChatGPT/Codex can call a bounded CatDesk Wake test control directly;
- test events are explicitly labeled MANUAL WAKE DEBUG / NOT natural acceptance;
- stale/ambiguous events can be safely retired only where the protocol permits;
- no direct protected-state edits;
- test events can be generated without operator PowerShell.

Immediate use once restored:
Queue exactly one fresh dev.59 diagnostic, e.g. manual-wake-dev59-readiness-001, and observe it end-to-end with the separate Codex debug browser.

### 2.3 Soak dev.59 and fix every real failure mode found

Do not call Wake “perfected” after one pass.

Minimum maturity test matrix:
A. Repeated clean delivery
- at least 3 consecutive fixed manual debug events;
- exact generation-16 target;
- one USER append each;
- exact durable receipt;
- current live timer while response active;
- final SENT / COMPLETE / queueDepth 0;
- no duplicates.

B. Page stabilization
- verify Wake does not type merely because editor appears briefly;
- require actual 10-second stable-ready hold;
- if Stop/Pause appears/disappears, require 10-second quiet window after disappearance;
- confirm no rapid reopen loop.

C. Blank-shell recovery
- when exact conversation loads but no textbox/buttons are usable, wait 10 seconds;
- reload exactly once at the bounded recovery boundary;
- re-evaluate exact target/login/network/editor state;
- never keep opening/reloading aggressively;
- never perform this recovery after first browser write.

D. Browser/network error
- preserve proven smoke-test resilience;
- before first browser write, allow bounded session/reload retry for real network/chrome-error conditions;
- after first write, never launch a fresh browser/session that could duplicate USER content;
- record precise stage/reason.

E. Tool-using long assistant turn
- do not interpret intermediate tool-call pauses as response completion;
- completion must require the real terminal assistant control/state;
- current dev.58 logic uses a completed-turn action signal; visually validate it against current ChatGPT UI.

F. “Message delivery timed out. Please try again.”
- identify exactly one timeout card and exactly one associated Retry;
- click Retry in-place before any reload;
- verify retry actually starts;
- never resend the USER wake;
- max 3 response retries;
- retain 35-minute per-generation and 90-minute total bounds;
- fail closed on ambiguity.

G. Pause/stop race
- PAUSE before browser submit must abort before write;
- once submit is dispatched, PAUSE/STOP must not abandon receipt drain;
- after exact receipt is durable, host may honor pause;
- prove no lost receipt and no replay.

H. Restart continuity
- restart installed WakeHost through supported lifecycle;
- exact target, generation, profile, receipts, current pointer, immutable hashes, and safe queue state survive;
- no replay of SUBMITTING/SENT events.

I. Reboot/recovery continuity
- after Wake itself is stable, test a machine/app recovery boundary;
- expected behavior is one-command CatDesk recovery + automatic Binagotchy/Wake restoration using prior target/profile where appropriate;
- no manual multi-command reconstruction.

J. Browser cleanup
- after success, owned Selenium browser/session should close;
- no repeated orphan browser windows;
- no accidental termination of unrelated user Chrome.

Maturity target:
Wake should be treated as mature only after repeated installed-path passes, at least one restart/recovery cycle, one network/pre-write retry case, and one timeout/retry case without manual intervention, duplicate submission, or ambiguous durable state.

### 2.4 Improve Wake observability while preserving forensic evidence

Do not delete history merely to make status look clean.

Ideal live status should clearly separate:
- actionable current queue;
- active current delivery;
- current target/generation;
- current timer;
- historical completed timers;
- historical ATTENTION/quarantined evidence;
- stale forensic count.

staleCount should be labeled as historical/forensic if it is not current queue health.

Current event should be visually/top-prioritized over old completed timers.

Any status field shown in Binagotchy/CatDesk must be recomputed from durable authority rather than stale in-memory snapshots.

## 3. PRIORITY 2 — Finish Binagotchy as the operator-facing CLI

The operator rejected the old white GUI and wants a Codex-like/command-prompt experience.

Current source already has src/binagotchy_cli.rs and a console-first direction.

Required ideal behavior:
- ASCII Binagotchy cat / older terminal aesthetic;
- launches automatically with CatDesk start/recovery when appropriate;
- remains a separate Wake companion, not a second CatDesk daemon;
- does not own Secure MCP;
- can run headless when no interactive GUI/console is desired;
- remembers the prior designated wake target;
- if no target exists, does nothing dangerous.

First-class commands should include:
- status
- target
- target set <ChatGPT URL>
- wake status
- wake start
- wake pause
- wake resume
- wake stop
- wake queue
- wake test / test wake
- wake retire <safe event>
- safe clear/cleanup for stale debug items where protocol permits
- help
- clear screen
- exit

Target changes must keep project authority + Wake authority coherent; never expose a registry-only target edit that can split them.

Manual test wakes must always be visually obvious as tests and must never count as natural acceptance.

The same bounded control surface should be accessible via MCP so ChatGPT/Codex can test Wake without asking the operator to type CLI commands.

## 4. PRIORITY 3 — Make CatDesk recovery boring, perfect, and one-command

Operator goal:
One supported command should recover the entire CatDesk stack under ordinary failure conditions. No five-command repair recipes.

Historical problems to eliminate:
- LKG_AUTHORITY_MISSING;
- LKG_AUTHORITY_AMBIGUOUS_OR_DAMAGED;
- CANONICAL_BINARY_MISSING;
- signing/length mismatch;
- stale release/main-image authority;
- Join-Path empty-string failures;
- protected build output collisions;
- transient external tunnel outages being misdiagnosed as CatDesk daemon failure;
- needing manual Wake/Binagotchy reconstruction after CatDesk recovery.

Ideal one-command recovery:
1. Determine whether local CatDesk listener is already healthy.
2. If only the external tunnel/session is down, recover/reattach the tunnel without restarting CatDesk.
3. Validate canonical reviewed-promotion/LKG authority.
4. Recover exactly the reviewed executable/package; do not pick arbitrary “latest” bytes.
5. Start/reattach CatDesk.
6. Verify MCP READY and serving/canonical parity.
7. Restore/verify WakeHost independently.
8. Preserve prior designated ChatGPT target.
9. Launch/reconnect Binagotchy companion.
10. Verify browser runtime/profile availability without exposing credentials.
11. Return one truthful status summary.
12. Require operator action only for an actually inaccessible credential/login/elevation boundary.

The supported recovery command given to the operator, if ever needed, must be one PowerShell line.

Do not add new trust domains. Finish the existing reviewed-source -> BUILD_ATTESTED -> reviewed promotion -> reviewed_promotion LKG -> recovery model.

Also preserve the known distinction:
- external tunnel failure != local CatDesk failure;
- if 127.0.0.1:3200/local MCP is healthy, recover only tunnel connectivity.

## 5. PRIORITY 4 — Improve CatDesk CLI/status metadata and truthfulness

The operator wants richer useful metadata, not decorative noise.

Ideal CatDesk/Binagotchy status should distinguish:
- current active task;
- active executor/provider;
- task/session ID;
- current phase/state;
- last completed task;
- last final review;
- current Wake event;
- last successful Wake event;
- current canonical chat target;
- Wake target generation/digest;
- WakeHost installed version/hash/PID;
- queue depth;
- current browser state;
- current receipt state;
- current timer state;
- current operator action requirement;
- current continuation mechanism expected;
- tunnel state;
- local MCP tool count;
- recovery/LKG state;
- dirty Git state;
- provider usage/resets ONLY when authoritative.

Do not invent token/context usage. Show token/context only when the system has an authoritative source. Otherwise show UNKNOWN/UNAVAILABLE instead of estimates presented as facts.

Make states concise enough to scan in a terminal but allow a detailed mode.

## 6. PRIORITY 5 — Turn timer / long-turn lease behavior

The operator explicitly liked the timer concept.

Two separate timer concepts must remain clear:

A. Wake response timer
- begins from the exact durable USER-message receipt;
- passive/advisory only;
- reports elapsed/remaining;
- soft checkpoint around 18 minutes;
- nominal deadline around 20 minutes / existing configured bound;
- freezes COMPLETE only after final successful delivery;
- ATTENTION on unresolved terminal problems;
- must not itself generate/replay USER work.

B. ChatGPT/CatDesk work-turn budget
- empirical ChatGPT web tool turns have historically failed around ~23–26 minutes;
- stop launching new long operations around 18–20 minutes;
- persist durable project/session state;
- return a checkpoint so Wake/deadman can continue;
- long builds/tests should run in CatDesk-managed sessions rather than holding one ChatGPT turn open.

Ideal behavior:
CatDesk should provide enough timer/lease state that ChatGPT/Codex can deliberately stop before UI timeout instead of being surprised by it.

Do not rely on ChatGPT manually “remembering to flag” completion; deterministic durable states should drive conclusions.

## 7. PRIORITY 6 — Executor-agnostic review/wake behavior

Regardless of executor:
- Codex;
- Qwen/Ollama;
- ChatGPT direct CatDesk work;

successful reviewed work must converge on the same durable finalization/review path and emit a review event eligible for Wake discovery.

Existing direct-work support includes claim/finalize paths and has previously been repaired so direct ChatGPT completion goes through the same actionable Wake seam.

Do not create executor-specific side channels that skip verification/review.

Provider preference from operator:
- use Codex 6 Astra med/low when authoritative evidence says >2 resets remain;
- otherwise use GPT-5.6 Terra high;
- if paid usage is truly exhausted, try Qwen;
- if Qwen is inadequate, ChatGPT/Codex may directly perform bounded work through CatDesk.

Never infer provider resets/usage from stale evidence.

## 8. PRIORITY 7 — Connector/tool catalog parity

Recurring product-integration issue:
source/local MCP may expose a tool while the already-open ChatGPT connector schema does not.

Examples historically included:
- autonomy_session_finalize_direct_work;
- catdesk_binagotchy_command.

Ideal behavior:
- serving CatDesk local MCP tools/list;
- secure tunnel/plugin catalog;
- ChatGPT connector-visible schema;
must converge predictably after a supported reload/reconnect boundary.

Do not repeatedly restart CatDesk or Secure MCP if the issue is merely a chat/session schema snapshot.

Add a truthful diagnostic that can say:
LOCAL_MCP_TOOL_PRESENT / CONNECTOR_CATALOG_MISSING
and recommend the narrowest supported refresh.

## 9. PRIORITY 8 — CLI launch/recovery integration

Binagotchy should be associated with CatDesk lifecycle:
- CatDesk launch/recover should make the companion available automatically;
- prior wake target persists;
- if no target, remain inert;
- Wake remains a separately owned subsystem so CatDesk upgrades do not silently break it;
- one subsystem restart should not cause duplicate daemon/tunnel/browser ownership.

Avoid the old standalone white control panel as the default operator experience.

## 10. PRIORITY 9 — Workspace/storage/Git hygiene

Workspace previously grew above 200 GB and root accumulated ~200 files.

Do not perform blind deletion.

Reuse T-0056/T-0057 audit/cleanup safety machinery:
- inventory first;
- classify Git-worthy vs local runtime/forensic/disposable;
- produce a deletion manifest before deletion;
- preserve reproducibility and forensic evidence.

Likely Git-worthy:
- source;
- scripts;
- tests;
- architecture docs;
- tickets;
- review bundles;
- reproducibility metadata that is safe to publish.

Likely local/ignored:
- .catdesk runtime state;
- browser profiles;
- credentials/tokens;
- local provider state;
- target verification output;
- disposable build/test roots;
- caches;
- transient package staging.

GitHub access should be used through existing CLI when ready. Do not ask the operator to grant GitHub access unless a concrete authentication failure proves it is unavailable.

## 11. PRIORITY 10 — Recovery/release telemetry and one-shot diagnostic observability

A prior T-0412/T-0416 V5-equivalent host linker diagnostic was executed successfully once, but the ignored test did not persist the exact classifier outcome/digest/length/truncation.

For future one-shot diagnostics:
- persist bounded non-authority result telemetry;
- fixed vocabulary only;
- include digest/length/truncation where designed;
- never require rerunning a protected one-shot merely to recover observability.

Preserve the rule:
never inherit ambient Visual Studio variables and never blind-retry protected build generations.

## 12. Ideal Wake end-state

The operator should be able to think of Wake as boring infrastructure:

- CatDesk review completes.
- WakeHost discovers exactly one eligible event.
- Exact canonical target is already persisted.
- Browser opens using dedicated authenticated Wake profile.
- Page stabilizes before typing.
- Blank shell/network transients recover boundedly.
- USER message is appended exactly once.
- Durable exact receipt appears.
- Timer starts from receipt.
- Assistant response is observed through tool-use pauses.
- Timeout card, if any, is retried in place without USER replay.
- Browser closes on terminal success.
- Delivery becomes SENT.
- Timer becomes COMPLETE.
- Queue converges to zero.
- Binagotchy/CatDesk status shows current truth.
- Fresh review events can repeat this reliably.
- Restart/reboot/recovery preserve authority.
- Hourly deadman is rarely needed and remains fallback only.

Do not declare this mature/perfected until repeated live soak proves it.

## 13. Immediate next steps for Codex

Do these in order:

1. Re-read:
   - CATDESK_MILESTONES.md
   - CATDESK_NEW_CHAT_NOTES.txt
   - .catdesk/current_plan.md
   - .catdesk/session.md
   - docs/orchestrator/review_bundles/T-0422_WAKE_DEV58_POST_SUBMIT_PAUSE_RECEIPT_DRAIN.md
   - docs/orchestrator/review_bundles/T-0423_WAKE_DEV59_READINESS_STABILIZATION.md

2. Verify current live CatDesk/Wake state. Expect installed dev.59 RUNNING, canonical Chat33 generation 16.

3. Build the separate persistent Codex visual-debug browser profile. Ask the operator only for the one-time interactive ChatGPT login if required. Preserve the profile afterward. Do not inspect/export credentials.

4. Restore/expose the bounded Binagotchy MCP/CLI test control without weakening security or using raw Store/browser mutation.

5. Queue exactly one fresh dev.59 manual diagnostic and visually observe it through the Codex debug browser while also reading WakeHost/CatDesk durable state.

6. Fix any remaining browser-readiness, response-completion, receipt, queue, timer, profile, cleanup, or retry bugs found. Each repair must have deterministic regression coverage.

7. Repeat installed-path tests until at least 3 consecutive clean passes.

8. Exercise bounded network-error recovery and timeout Retry-in-place.

9. Restart WakeHost and repeat. Then perform a safe reboot/recovery cycle when appropriate.

10. Only when Wake is convincingly mature move down the priority list to recovery polish, CLI metadata, workspace cleanup, and other quality-of-life work.

## 14. Operator boundaries

Current operator preference:
- Do not stop merely because a normal engineering decision is needed; make the decision and leave an audit trail.
- Involve operator only for genuine operator-only boundaries.
- If a login is required for the new Codex debug profile, ask clearly and only once.
- If PowerShell is required, provide one line.
- After each major checkpoint, state:
  OPERATOR ACTION: NONE
  or exactly what is required.
- Also state the expected continuation mechanism:
  * event-driven WakeHost/Python browser wake;
  * hourly Chat33 deadman fallback;
  * or explicit operator action.

Do not move work to another ChatGPT conversation unless the operator explicitly changes canonical authority.
