## Current post-login checkpoint - 2026-09-25
Operator login is complete. Prior serving-CatDesk probe: EXACT_READY_BUSY while Chat34 generated. New idle proof: EXACT_NO_EDITOR -> EXACT_READY_IDLE -> FINAL:EXACT_READY_IDLE, exit 0 (.catdesk/logs/1790297545-cab73644-a818-4903-919f-51e10f264f5a.log). Authentication/router blocker is cleared. Do not request login or increase navigation waits.
Live: CONNECTED_VERIFIED / MCP READY / 89 tools. Installed dev.66 PID 19292 reported RUNNING, not the earlier handoff's PAUSED state; queueDepth was 0. Project registry and Wake match generation 17, URL https://chatgpt.com/c/6ab5be17-e554-83e9-a8cb-c7753a063aff, digest 8766b100ea0ba654bc4c8531d99c84e34346a231be1ffa0b4deae30df552427e.
Exactly one new diagnostic manual-wake-dev66-chat34-postlogin-007 was published at Unix 1790297785 with expected generation/digest checks through serving CatDesk. It is MANUAL WAKE DEBUG / NOT natural acceptance. Observe this event before further queue/restart actions. Never replay manual-wake-dev66-home-settle-006 or historical ambiguous events. Readiness history remains a separate compatibility-safe surface.
Required: exact-once USER, durable matching receipt, live timer, SENT/COMPLETE, queue zero, owned-browser cleanup; then three consecutive passes plus network/timeout/tool-pause/restart/recovery gates. Preserve dirty worktree and external Secure MCP ownership.
OPERATOR ACTION: NONE. Expected continuation: Codex continues directly through CatDesk; installed Wake owns delivery. Hourly Chat34 deadman remains fallback only.

---
Historical checkpoints below are superseded by the current facts above.

# CatDesk Next-Chat Handoff — Wake Maturity Continuation
_Date: 2026-09-24_
_Canonical conversation at handoff time: https://chatgpt.com/c/6ab465a8-63a8-83ea-adfc-758e368769e2_

## 1. Current live state

CatDesk transport:
- CONNECTED_VERIFIED
- local MCP READY
- local MCP self-check: 89 tools
- external official Secure MCP runtime remains externally owned and untouched
- branch: orchestrator/chatgpt-codex-autonomous-loop
- worktree intentionally dirty; do not clean/publish Git yet

Canonical project/Wake authority:
- project: catdesk
- generation: 16
- target URL: https://chatgpt.com/c/6ab465a8-63a8-83ea-adfc-758e368769e2
- target digest: d32961ec350c42d039f5514e9171a59574674ecc291b82c0c28f4d9c14336212

Installed Wake:
- version: 1.0.0-dev.66
- immutable package: 1.0.0-dev.66-908aee4a476e-57791d5851a3
- WakeHost SHA-256: 908aee4a476e25aaff82037cb1ea08810bdec4e7bf9b3caddf7c355445007416
- wake_bridge.py SHA-256: 117e0462ae86be113f472ad4829a5641ae8dd408ec1451eece712804c4ad204e
- host: RUNNING
- latest submission: ATTENTION
- latest attention: TARGET_DRIFT
- queueDepth: 0
- staleCount: 21
- last successful durable receipt still belongs to T-0422/dev.58
- no duplicate USER wake from the latest dev.66 test

## 2. Exact latest Wake result

Exact test event:
manual-wake-dev66-home-settle-006

This event DID NOT reach Chat33 as a USER message.

Current result:
- terminal ATTENTION / TARGET_DRIFT
- receipt: none
- turn timer: none
- queueDepth: 0
- no USER write
- no duplicate/replay

Therefore the current blocker is entirely PRE-WRITE browser navigation/readiness.

This was an explicit MANUAL WAKE DEBUG event and never counts as natural Wake acceptance.

## 3. Why dev.66 exists

dev.65 had already proven that:
- replacing CDP open(url) with in-place cdp.get(exact_target) was not enough;
- both active-turn and idle-window diagnostics remained on ChatGPT HOME;
- readiness journal repeatedly showed:
  HOME / TARGET_DRIFT
  -> RECOVERY_BEGIN
  -> RECOVERY_RETURN still HOME
  -> second recovery
  -> still HOME
- failures remained safely pre-write.

The dev.65 idle-window event was:
manual-wake-dev65-idle-window-005
It also ended ATTENTION / TARGET_DRIFT, receipt null, timer null.

This ruled out “ChatGPT was simply busy generating” as the main cause.

## 4. dev.66 change

T-0428/dev.66 changes one narrow pre-write timing boundary.

Before dev.66:
- cdp.get(exact_target)
- immediately call get_current_url()
- record RECOVERY_RETURN from whatever URL is seen

Problem:
SeleniumBase/CDP navigation may return before ChatGPT's router settles. The older smoke-test path deliberately waits/polls after navigation.

dev.66:
- adds HOME_RECOVERY_SETTLE_SECONDS = 10.0
- after cdp.get(exact_target), poll the SAME active tab for up to 10 seconds
- HOME is allowed transiently during that bounded settle period
- RECOVERY_RETURN is emitted from the post-navigation verified route
- no second browser context
- no alternate URL
- no new USER write path
- no USER replay behavior
- no post-write reload behavior

Regression coverage added for:
1. delayed HOME -> exact conversation transition after navigation
2. persistent HOME consuming the full 10-second settle period
3. persistent-HOME readiness-window timing

Verification:
PASS:
- fixed-runtime Rust->Python response/readiness regression harness
- cargo check --features test-support
- strict Wake library Clippy -D warnings
- scoped git diff --check

Important limitation:
CatDesk's generic command gate repeatedly rejected the broad cargo test --features test-support invocation before execution. Do NOT claim the full dev.66 suite ran. dev.65 had the full suite green before this narrow Python timing-only production delta.

Review bundle:
docs/orchestrator/review_bundles/T-0428_WAKE_DEV66_HOME_NAVIGATION_SETTLE.md

## 5. Important earlier accepted Wake milestones

dev.56:
- real natural Chat33 wake reached USER
- exact generation-16 receipt
- live current-event turnTimers during GENERATION_ACTIVE
- terminal SENT / COMPLETE
- no duplicate
This proved the core event-driven path has worked end-to-end.

dev.57:
- status/queue convergence improvements
- first installed-path manual diagnostic reached USER and terminal SENT/COMPLETE
- restart-repeatability test also reached USER
- later sequential testing exposed response-completion/pause sequencing defects

dev.58:
- fixed response observer false-completing during intermediate tool-call pause
- fixed PAUSE/STOP race after submit dispatch but before exact receipt drain
- preserved no-replay semantics

dev.59:
- operator-requested UI readiness stabilization:
  * exact target/editor stable for 10 seconds before typing
  * 10 seconds continuously without Stop/Pause before typing
  * loaded exact blank shell waits 10 seconds then one bounded reload
  * all strictly before first browser write

dev.60:
- Codex maturity repair set
- one blank-shell reload limit
- missing-response-editor obeys 35m generation deadline
- receipt polling no longer busy-spins
- BLANK_SHELL_RECOVERY accepted across Python/Rust vocabulary
- latest-event status no longer overwritten by older quarantined records
- expected generation + target digest guard for manual test-event publication
- diagnosed Codex MSIX AppData virtualization/shadow Store issue

dev.61:
- never installed as originally written
- attempted to add readinessHistory directly to Status
- rejected because runtime::Status is deny_unknown_fields and serving CatDesk parser could break, same compatibility class previously seen with turnTimers

dev.65:
- resolved dev.61 compatibility issue
- readiness history moved to separate bounded/versioned readiness-history.json
- status.json kept backward-compatible
- live queueDepth redefined as USER-deliverable work only
- separate read-only queue-health classifies preserved backlog
- installed package:
  1.0.0-dev.65-666f61309523-57791d5851a3
- production backlog at that time:
  actionable 0
  reconciliation 3
  attention 3
  stale 21
  forensic 27
- preserved forensic records must never be replayed merely to make counts clean

## 6. Current browser/navigation blocker

The primary unresolved Wake bug is now:
**production Wake profile/browser opens or remains on ChatGPT HOME instead of converging to the exact canonical /c/<conversation-id> route.**

Important evidence:
- login = READY
- target authority is correct generation 16
- failures are not LOGIN_REQUIRED or CAPTCHA
- failures remain pre-write TARGET_DRIFT
- idle-window test still failed
- in-place get(url) did not solve it
- immediate vs delayed route read did not solve the actual delivery yet

Next chat should FIRST inspect dev.66 readiness-history for exact event:
manual-wake-dev66-home-settle-006

Use:
CatDeskWakeHost readiness-history
through the serving CatDesk environment / supported control path.

Determine whether dev.66 now shows:
A. HOME -> SAME_CONVERSATION at RECOVERY_RETURN, followed by some later readiness failure
or
B. HOME remains HOME for the full settle window

This distinction is the immediate next diagnosis.

If B:
Investigate authenticated production profile/router behavior, not more blind navigation retries.

Likely areas:
- whether ChatGPT home/router redirects this saved production profile away from /c/<id>
- whether SeleniumBase UC/CDP get() on current ChatGPT needs a different wait/navigation primitive
- whether service-worker/app-router state differs between profile startup and ordinary browser navigation
- whether the old smoke-test behavior succeeds against the same exact production profile when WakeHost is paused
- whether a fresh manually authenticated Wake profile is required again
- whether the production Wake profile has stale/changed auth/session state despite login selectors reporting READY

Do NOT:
- add unlimited navigation retries
- multiply browser contexts
- submit from HOME
- replay old events
- treat HOME as acceptable target
- weaken generation/digest binding

## 7. Codex browser debugging context

The user configured the Codex Chrome extension against an authenticated Chrome session.

Use Codex Chrome only as debug/observation browser.
Do NOT attach it to the production Wake profile and do not make it an alternate Wake sender.

Codex Medium burned usage very quickly:
- roughly 26 minutes consumed the immediate allowance from 100 -> 0
- weekly allowance dropped roughly 53 -> 38

Recommended future Codex mode:
- Astra Light for continuous Goal-mode work
- Medium only for a difficult bounded architectural/race review
- then return to Light

Codex discovered a Windows/MSIX issue:
local AppData inside Codex can be redirected into a package LocalCache shadow.
Therefore Codex-local Wake CLI can see a different Store from production.
Production Wake controls must execute through the serving CatDesk environment.

## 8. Wake maturity target

Do not call Wake mature yet.

Required remaining acceptance matrix:
1. fix exact-target HOME convergence
2. at least 3 consecutive clean installed-path diagnostics
3. exact USER append once
4. durable exact receipt
5. live current-event timer
6. final SENT / COMPLETE / queueDepth 0
7. no duplicate USER messages
8. bounded pre-write network recovery
9. timeout card Retry-in-place without USER replay
10. intermediate tool-call pause does not false-complete
11. WakeHost restart/profile continuity
12. safe reboot / recovery cycle
13. browser closes after terminal success
14. no orphan Selenium windows
15. bounded Binagotchy MCP test control exposed directly

Only then should Wake be described as mature/perfected.

## 9. Binagotchy / MCP control gap

Source already has bounded Binagotchy-style command/control support for:
- status
- start
- pause
- resume
- stop
- queue
- test
- retire

But this ChatGPT connector catalog still does not expose catdesk_binagotchy_command.

Local MCP reports 89 tools and source registration is current.
The remaining issue appears to be connector/chat catalog caching or integration-layer exposure.

Temporary diagnostic bridge currently used:
workspace CatDeskWakeHost test-event executed THROUGH SERVING CATDESK with:
- expected generation
- expected target digest

This is safer than Codex-local CLI, but it is still temporary.
Mature path should be bounded MCP/Binagotchy test control, not mutable workspace CLI against production Store.

## 10. Queue/status semantics

Current live queueDepth is intended to mean only work Wake may actually submit.

Preserve separately:
- reconciliation/SUBMITTING
- ATTENTION
- stale generation
- forensic historical records

Never delete evidence just to make status look healthy.

Old important ATTENTION timers include:
- manual-wake-dev57-queued-sequential-003
- review-adc-t0422-dev58-wake-sequencing-receipt-review-20260924-6-independent_final_review

These are historical forensic state, not acceptance failures for the newest event.

## 11. Turn timer / response completion design

Keep these separate:

Wake response timer:
- begins at exact USER receipt
- passive/advisory
- elapsed/remaining
- ~18m soft checkpoint
- ~20m nominal deadline surface
- COMPLETE only after browser-proven final response
- ATTENTION on unresolved terminal state
- never emits a USER wake by itself

Work-turn budget:
- ChatGPT web turns have repeatedly died around ~23-26m
- stop launching new long operations around 18-20m
- checkpoint durable docs/session
- end turn deliberately and let Wake/deadman continue
- do not rely on assistant-authored completion flags

Browser observation should remain deterministic authority.

## 12. Recovery / CLI / broader backlog after Wake maturity

Priority after Wake:
1. Binagotchy CLI/MCP control surface
2. perfect one-command CatDesk recovery
3. richer CLI metadata
4. turn-budget/lease polish
5. connector tool-catalog parity
6. CatDesk/Binagotchy lifecycle integration
7. workspace/storage/Git cleanup
8. GitHub publication when safe

Recovery ideal:
one supported command should:
- distinguish local CatDesk failure from tunnel-only failure
- distinguish tunnel-down from tunnel-healthy/local-MCP response-deadline failure; on repeated deadline failures selectively probe/recover the local MCP worker/front-door path before reconnecting the tunnel
- recover only the failed layer
- verify reviewed/LKG authority
- restore exact reviewed binary
- reconnect MCP
- restore independent Wake
- preserve designated target/profile
- bring Binagotchy back
- report one truthful summary

Never return to multi-command operator recovery as normal behavior.

## 13. Operator preferences / hard rules

- Senior assistant/Codex may make engineering decisions without waiting for ordinary approval.
- Leave an audit trail; avoid feature-creep trust mechanisms.
- No GitHub access requests unless a concrete auth failure proves unavailable.
- PowerShell shown to operator must always be ONE LINE.
- Do not ask operator to relay prompts between agents.
- Test events must clearly say MANUAL WAKE DEBUG / NOT natural acceptance.
- Natural Wake acceptance must come through normal independent WakeHost/Python path.
- Hourly deadman is fallback only and never counts as natural acceptance.
- Keep durable docs synchronized.
- Preserve external Secure MCP ownership.
- Preserve dirty worktree.
- Preserve quarantined historical evidence.
- No Git publication/cleanup yet.

## 14. Hourly deadman

Exactly one recurring CatDesk hourly fallback should remain enabled and pinned to the canonical conversation.

Its prompt should remain generic:
- continue where canonical CatDesk work left off
- rehydrate durable docs
- verify current transport/authority
- continue highest-priority unfinished work
- do not embed rapidly stale version/ticket specifics
- never count hourly automation as natural Wake acceptance

## 15. Immediate next actions for next chat

Do these first:

1. Verify CatDesk_Local_Tunnel_v3:
   - CONNECTED_VERIFIED
   - local MCP READY
   - canonical generation 16 + exact digest
   - installed Wake dev.66

2. Inspect exact event:
   manual-wake-dev66-home-settle-006
   Confirm:
   - ATTENTION / TARGET_DRIFT
   - receipt null
   - timer null
   - queueDepth 0
   - no USER message

3. Read readiness-history and isolate ONLY the dev.66 event.
   Determine whether the new 10-second settle saw:
   - SAME_CONVERSATION at any point
   - persistent HOME
   - network error
   - auth route
   - other route

4. If persistent HOME:
   - pause WakeHost
   - compare production-profile navigation behavior against the proven smoke-test pattern
   - use only read-only/no-submit visual probes
   - determine whether production Wake profile itself needs re-auth/login refresh
   - if operator login is genuinely needed, provide exactly one one-line PowerShell command to open/save the Wake login profile
   - otherwise keep debugging autonomously

5. Do not queue another diagnostic until the exact dev.66 failure mechanism is understood and the next bounded repair is tested.

6. After any repair:
   - deterministic regression
   - focused Python harness
   - compile/check
   - strict Clippy
   - diff-check
   - full Wake suite when CatDesk command policy permits
   - immutable install
   - exactly one fresh generation/digest-bound diagnostic
   - end turn to give it idle window

## 16. Key files

Read immediately:
- CATDESK_MILESTONES.md
- CATDESK_NEW_CHAT_NOTES.txt
- .catdesk/current_plan.md
- .catdesk/session.md
- docs/orchestrator/review_bundles/T-0428_WAKE_DEV66_HOME_NAVIGATION_SETTLE.md
- docs/orchestrator/review_bundles/T-0427_WAKE_DEV65_COMPAT_QUEUE_OBSERVABILITY.md
- docs/orchestrator/review_bundles/WAKE_DEV60_MATURITY_REPAIRS.md
- docs/orchestrator/CODEX_HANDOFF_WAKE_MATURITY_2026-09-24.md
- scripts/wake_bridge.py
- scripts/wake_browser_smoke_test_cdp.py
- scripts/chat33_readiness_visual_probe.py
- wake/src/runtime.rs
- wake/src/store.rs
- wake/src/bin/CatDeskWakeHost.rs
- src/binagotchy_cli.rs
- src/mcp.rs

## 17. Current operator action

NONE at handoff time.

The next chat should continue diagnosis autonomously. Only request operator action if the production Wake browser profile truly requires a one-time login refresh or another genuinely operator-only boundary.
