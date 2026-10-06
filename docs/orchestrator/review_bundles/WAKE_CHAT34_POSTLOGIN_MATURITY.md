# Chat34 post-login maturity evidence

## Verified authority and login
Serving CatDesk transport was CONNECTED_VERIFIED, MCP READY with 89 tools. Project registry and independent Wake both matched generation 17 / digest 8766b100ea0ba654bc4c8531d99c84e34346a231be1ffa0b4deae30df552427e / canonical Chat34.

The production-profile idle probe completed with EXACT_NO_EDITOR -> EXACT_READY_IDLE -> FINAL:EXACT_READY_IDLE, exit 0. Log: .catdesk/logs/1790297545-cab73644-a818-4903-919f-51e10f264f5a.log. Prior post-login EXACT_READY_BUSY was valid, not LOGIN_VISIBLE. No further login requested. Live host was already RUNNING, unlike the prior PAUSED handoff; queueDepth was zero.

## One fresh installed-path diagnostic
manual-wake-dev66-chat34-postlogin-007 was published once at Unix 1790297785 through serving CatDesk with exact expected generation and digest. It is MANUAL WAKE DEBUG / NOT natural acceptance. No historical event was replayed. Event creation log: .catdesk/logs/1790297785-47379f76-406b-48ab-b3c8-13e3ab418fa3.log.

Subsequent connector status/event requests timed out with HTTP 504. This is missing observation, not a terminal delivery result. Host PID 19292 and its Python PID 35536 / Chrome PID 56224 were live at 2026-09-25 01:06 UTC. Do not resend or restart on this evidence. Existing probe processes 9792 and 36280 also remained live with Python children; no Chrome descendants were observed for those probe roots. Their unbounded shutdown needs later investigation without disturbing the active host.

Observation-only Chrome tab 1383640935 initially opened Chat34 but redirected to HOME after reload; no diagnostic USER was proven in that separate browser. Do not conflate that with the production profile's verified exact idle state. No production browser attachment or credential inspection was used.

## Source verification and additional repairs
The current dev.66 Wake suite passed 37 library, 26 protocol/store, 3 process-tree, 3 embedded Python harness, and 1 CLI test (70 total), with one intentional manual harness ignore. A stale persisted queue assertion was corrected from one to zero: receipted SUBMITTING is reconciliation-only and not actionable work. A separate build directory avoided a live locked ProfileProbe executable.

A new source-only fix clamps Retry-start observation to the existing total response deadline and returns RESPONSE_TOTAL_TIMEOUT at that boundary. Deterministic tests prove this bound and that a 30-second tool-call pause without completion controls survives resumed generation. Both and the existing max-three-retries test pass. These changes are not installed and do not modify dev.66's active immutable package.

## Open gates
Exact current-event receipt, live timer, SENT/COMPLETE, queue zero, exact-once USER and owned-browser cleanup remain unproven pending restored observation. Three consecutive passes, installed restart/recovery, and first-class connector control remain open. Preserve forensic records and compatibility-safe separate readiness history.

OPERATOR ACTION: NONE. Expected continuation: Codex continues directly through CatDesk, with installed dev.66 owning the single diagnostic.

Full Python baseline completed: 97 tests passed in 306.804 seconds, exit 0. The two newly added deadline/tool-pause regressions passed separately and in the embedded harness; they were added after the full run loaded its test module.

Latest process check: diagnostic adapter PID 35536 and Chrome root PID 56224 are no longer present; WakeHost PID 19292 remains live. This supports owned-browser exit only, not SENT/COMPLETE or receipt proof. Durable result is still required before another diagnostic.

## Bounded profile-probe repair - 2026-09-25
Two pre-existing ProfileProbe processes (9792 and 36280) remained live far beyond their intended 20-second observation. The wrapper used unbounded Command.output(), allowing startup/shutdown hangs to retain a command indefinitely. Source now requires PAUSED control plus host PAUSED acknowledgement, contains the child in the existing kill-on-close Windows Job, gates Selenium startup on a post-containment stdin handshake, enforces a 60-second whole-process deadline, and bounds/filters returned output to fixed PROBE states. It terminates only its own newly launched process tree. Existing probe processes and installed Wake were not killed/restarted.
Verification: three Rust probe tests pass (fixture, fixed-state output, timeout termination), strict probe Clippy passed, Python probe syntax passed. No additional production probe was launched while Wake is RUNNING. This utility change is source-only and does not activate a new Wake package. Current diagnostic 007 still requires durable readback; transport observation remains unavailable. Preserve separate readiness journal and all receipts/forensics.
OPERATOR ACTION: NONE. Expected continuation: Codex retrieves diagnostic 007 through serving CatDesk before further installed-path tests.

## Verified stale-probe cleanup - 2026-09-25 01:17 UTC
After revalidating exact wrapper executable paths, original child creation times, parent PIDs, fixed probe-script command identity, age over 15 minutes, and absence of all grandchildren, stopped only diagnostic Python children 31712 and 52052. Their wrappers 9792 and 36280 then exited naturally. WakeHost 19292 remains live. No Chrome process or production delivery process was stopped. This clears the old unbounded probe processes; the new source wrapper prevents recurrence with owned job cleanup and a 60-second process deadline.
Current transport status request remains pending; diagnostic 007 durable receipt/completion is still unproven. Do not resend or infer success from process exit.

Additional probe verification: integration test profile_probe_cli passed, proving RUNNING and unacknowledged PAUSED states are refused before process launch or event publication. The Python probe now derives its existing profile path from the same serving LOCALAPPDATA root as its Rust wrapper instead of a hard-coded user path. Continue running production probes only through serving CatDesk; this does not eliminate Codex/MSIX virtualization.

## Dev.67 installed-path maturity and recovery follow-up - 2026-09-25

The dev.66 response-completion failure exposed by diagnostic 008 was repaired by treating the completed-turn action as structural DOM evidence rather than requiring the action control to be visibly rendered. The repair retained the existing exact latest-USER binding, no-active-Stop requirement, timeout-card handling, sequence checks, and target constraints. The source was versioned as dev.67, independently regression-tested, and published through the immutable reviewed Wake transaction as `1.0.0-dev.67-21592c86cff7-57791d5851a3`.

Manual installed-path repeatability is now closed with three clean passes: 007, 009, and 010. Diagnostics 009 and 010 under installed dev.67 each reached generation-17 `EXACT_USER_MESSAGE_APPENDED`, terminal `SENT`, timer `COMPLETE`, actionable queue zero, and `CLOSED_AFTER_SUCCESS` without USER replay. Diagnostic 008 remains immutable historical dev.66 ATTENTION evidence and is not a pass. These manual diagnostics remain distinct from natural event-driven acceptance.

Installed-package restart continuity also passed. `catdesk_wake_restart_installed(confirm=true)` moved WakeHost from PID 22224 to PID 32368 while preserving the exact generation-17 target, 010 receipt, completed timers, and actionable queue zero. The immediate restart snapshot briefly exposed startup queue depth one, but settled `queue-health` was actionable=0 / reconciliation=1; the sole reconciliation record is historical 008 and was never rearmed for USER delivery.

The deterministic Python bridge harness now includes the outer network-retry invariants in addition to timeout-Retry and tool-pause coverage: one pre-write network failure may reopen a fresh browser session, three pre-write failures stop at the bound, non-network attention never relaunches, and no fresh-session retry is permitted after the first browser write. A live synthetic browser-network fault probe was designed but the command safety layer refused execution before browser launch; that policy was not bypassed, so no live synthetic network-fault acceptance is claimed.

One-command recovery was also hardened in `scripts/start-catdesk-stack.ps1`. Local MCP readiness now reports fixed redacted gates including `LOCAL_MCP_RESPONSE_TIMEOUT` instead of collapsing every local failure to PENDING. Recovery may recycle only the exact pinned canonical CatDesk daemon when the local MCP request path specifically times out and the externally owned official runtime independently verifies READY. The branch has no tunnel mutation authority; if runtime health is unverified, the same local timeout causes no selective mutation. The PowerShell lifecycle/recovery integration fixture passes with positive and negative cases plus a static no-tunnel-mutation guard.

The legacy MCP `catdesk_release_recovery` path is intentionally closed. Read-only stable-supervisor status/preflight currently remain `SUPERVISOR_ROOT_UNAVAILABLE` / `SUPERVISOR_STARTUP_POLICY_UNPROVEN`; therefore the fixed 127.0.0.1:3201 front door is intentionally not probed or activated. External Secure MCP ownership remains unchanged.

OPERATOR ACTION: NONE.
