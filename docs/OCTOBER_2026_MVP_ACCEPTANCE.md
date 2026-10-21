# CatDesk October 2026 MVP acceptance

## October 20 Web tool serving parity (SOURCE + DEVELOPMENT LOOPBACK; MVP PARTIAL)

The five existing `catdesk_web_work_request`, `catdesk_web_work_resolve` and
`catdesk_web_context_{capture,configure,observe}` tools now use the existing
supervisor-mode predicate for both catalog and dispatch. They are available
in Computer/Both with Multi Tools or Supervisor Only. Read Only and Browser
Only still refuse them. Exact project generation/digest witnesses, closed
schemas, target CAS, per-project work ownership and disclosure/install guards
are unchanged; queue package read/publication tools remain Supervisor Only.
No caller identity or task completion is inferred from tool access.

The three existing Web mode regressions fail before the fix and pass afterward,
covering catalog/dispatch agreement, invalid scope refusal, no state creation
and private-error redaction. The broader initial run passes 1,538 with one
outdated exact-catalog expectation, which is updated for these five tools.
The final full main suite passes **1,539 / 28 ignored**; strict all-target,
all-feature Clippy, formatting and diff checks pass. Source `784c729` is
built normally and selected into stopped development v27, preserving 77
non-selection state files, the authentication profile and production.

The first internally bounded 20-second loopback probe finishes its functional
checks and exits normally with zero owned browser/Wake processes. Its final
preservation gate is **PARTIAL**: only the isolated `catdesk.toml` changes.
Usage accounting persists the process's temporary mode/listener settings.
No exact pre-probe configuration backup is found, so defaults are not guessed
or reconstructed. The post-probe file and first controller/exit evidence are
preserved; all five bindings and project/canary history remain intact.

The existing configuration regression now reproduces that headless side
effect. Source keeps headless mode/tool mode/bind address/port overrides
process-local while still persisting usage counters and explicit ordinary
configuration updates. This correction requires its own matched package
readback. Legitimate accounting changes must be distinguished from launch
setting drift rather than silently exempting the whole configuration file.

Source `742ede6` passes the full main suite **1,539 / 28 ignored**, strict
all-target/all-feature Clippy, format and diff checks. Its normal optimized
image is selected through the existing stopped development **v28** update;
Wake/Python inputs, browser profile and all 77 non-selection files are preserved.
The corrected **20-second** loopback probe passes: unauthorized access returns
401, all five Web tools appear exactly once and reject private scope overrides,
the separately gated queue tools remain absent, and a successful MCP registry
read returns seven paired projects and the preserved continuation proof.

The listener exits normally at its internal deadline with zero owned browser/
Wake processes, no events, browser delivery or WakeHost start. Of 81 checked
files only four numeric usage counters in the isolated configuration can change;
all other configuration bytes/settings and project/history files are preserved.
Independent original-five paired readback passes, both controls remain STOPPED,
and protected production pointer/configuration are unchanged. Evidence:
`approved-catalog-run-20261021T033800303Z` under the private checkpoint above.
This verifies the selected development serving path, not provider connector
catalog refresh, valid work delivery, protected installation or a natural cycle.
Private dated evidence: parent `checkpoints/web-serving-parity-oct20`.
Connector HTTP 429 and protected installation remain independent live gates.

## October 20 optimized status verification (READ-ONLY; MVP PARTIAL)

The existing release profile resolves the debug aggregate-reporting delay
without changing source, authority checks or status contents. Built at source
`e2cf222`, the first optimized image returns the same complete seven-project
status in **3.49 seconds**, versus about 50 seconds for the debug candidate.
After the fixture build, the restored normal image passes its own deadline
fixture and reads the same status in 3.64 seconds. Its exact SHA-256 is
`2db278a875cba1a924b442a3559241c195bfb5a18eb4c01d62385112b57b14e9`.
The guarded update selects that image into stopped development **v26**;
the other three package inputs are unchanged, and v25 remains preserved.
Status through the selected path takes **3.00 seconds** (1.00 second CPU),
the exact-target read takes 0.24 seconds, and invalid scope refuses with
`WEB_USAGE`/exit 2 in 0.03 seconds. Each read preserves all 79
checked state files and leaves zero owned browser/Wake processes. No runtime
or browser starts. Both optimized native headless fixtures also pass; these
use disposable local fixtures, not installed Wake or ChatGPT.

The package selection itself preserves all 77 non-selection state files, the
existing authenticated browser profile and the protected production pointer.
An independent comparison of the five original paired registrations/Wake
targets passes. Both controls remain STOPPED. This is verified development
package selection/readback, not protected installed production acceptance.

Private dated evidence: parent `checkpoints/web-status-performance-oct20`.
The single subsequent connector registry read still returns HTTP 429 from the
MCP SSE dependency. No retry, transport restart or account/settings change
follows. The authenticated GitHub repository is active and not disabled;
its latest visible run remains the historical October 14 push. A billing read
returns HTTP 404, which does not establish an account restriction. No workflow
was dispatched. Protected installation, exact-current CI, central connector
acceptance and a genuine development/review/next-task cycle remain open.

## October 20 automatic monitoring while rollover is quarantined (CONTROLLED LIVE; MVP PARTIAL)

The corrected **v25** candidate at source `148debabdbe68b92d0de4a120bb66cad6e23fbd3`
passes the fresh scoped periodic test: CatDesk, Wake2 and PortfolioDesk each
recorded a new capture for their exact current URL/generation within the
480-second scheduler window. There were no manual capture calls during that
run. CatDesk remained quarantined, and no project advanced rollover or sent a
message. Both canaries were excluded. The scheduler exited normally at its
internal deadline; host tree exit is proved and owned process count is zero.
All policies restored exactly, all five bindings and canary histories preserved,
all existing deliveries/timers unchanged, both controls STOPPED, production
pointer/configuration unchanged. Private evidence:
`checkpoints/web-quarantine-monitor-oct20/approved-monitor-run-20261021T021001323Z`.

This closes controlled periodic monitoring for the three projects, not protected
installation or the central ChatGPT development loop. The earlier partial run
and diagnostic probes below remain preserved. Intermittent target-read refusals
also occurred but did not prevent this acceptance. A separate aggregate
`operator web status` diagnostic hit its 30-second limit; its bounded follow-up
completed successfully in about 50 seconds, with steadily increasing CPU time.
It returned seven paired projects, the three fresh advisory estimates and the
preserved fast-canary continuation proof. This is a CPU-heavy reporting path,
not a deadlock or lost state. The optimized read-only result above resolves
that reporting delay for the release image. No browser
or runtime was started for the status profile.

Inspection found that a preserved QUARANTINED attempt suppressed context reads
even when independent observation was enabled. Its scheduler step now permits
only that separately opted-in capture of the current paired conversation,
retaining QUARANTINED and `rolloverAdvanced:false`. It never evaluates the
rollover trigger afterward. The existing quarantine regression now traverses
this step: opt-out performs no read; opt-in observes above the configured
threshold without changing the journal, creating a handoff or lifting the
project work lease. Unknown submission and missing source-witness paths remain
blocked. This is continued monitoring, not recovery authorization.

The existing headless MCP command gains optional `--max-runtime-seconds 1..900`
and repeatable `--web-auto-project ID` scope. The deadline uses the existing
scheduler cancellation/drain path; explicit scope filters scheduled projects
before task creation. Default behavior remains unchanged. A disposable native
listener exits on its internal one-second deadline without an external stop.
Main CatDesk passes **1,539 / 28 ignored**, both native headless fixtures pass,
strict all-target/all-feature Clippy, build, format and diff checks pass. Wake
and Python source are unchanged and their preceding accepted source gates were
not repeated. Private evidence: parent `checkpoints/web-quarantine-monitor-oct20`.
The source-matched stopped development **v24** update preserved the authenticated
profile and production pointer. Its actual scoped periodic test is **PARTIAL**:
an internally bounded 480-second headless run captured Wake2, while CatDesk and
PortfolioDesk encountered browser contention/attention and did not record fresh
captures within the test. Native listener exit was normal, the foreground host
proved its tree exited, owned process count was zero, both controls STOPPED,
all policies restored exactly, all five bindings and canary history preserved,
and no old delivery/timer changed. No messages were sent. Source success does
not close this concurrent monitoring gap.

Private periodic evidence:
`approved-monitor-run-20261021T014418207Z` under the monitor checkpoint. A
separate, single non-recurring CatDesk quarantine-step probe succeeds against
the authenticated browser, retains QUARANTINED/`rolloverAdvanced:false`, leaves
the journal byte-identical and cleans up. It narrows the problem to concurrent
operation; it is not periodic-monitor acceptance. The first parallel CLI probe
also erased browser errors because its code filter omitted `BROWSER_`. That
omission is now corrected; scheduler status retains recognized code prefixes
before private details. Existing privacy regressions verify paths stay excluded.
The corrected source diagnostic confirms `BROWSER_SESSION_BUSY`, with normal
exit and zero owned processes after cleanup.

A cold capture can exceed the profile's 30-second admission wait. The scheduler
now retains each project worker for at most two extra local admission retries,
separated by cancellable 30-second waits. It retries only `BROWSER_SESSION_BUSY`,
not metadata, authority or provider failures. Other projects retain independent
worker slots. Cancellation is checked before attempts and during waits. The
existing busy-browser regression covers success, exhaustion, other failures and
cancellation; the existing worker-drain regression exercises the actual retry
wait. The main suite passed 1,539 / 28 ignored after the error-code correction;
then all 61 Web tests, strict Clippy and build passed after the retry change.
Wake/Python and headless entry behavior are unchanged by that retry correction.
The subsequent v25 periodic acceptance above is separate from these source gates.

## October 20 approved three-project live test (CONTROLLED DEVELOPMENT)

The operator explicitly approved bounded testing and dated audit records,
superseding the preparation holds recorded below. The stopped v22 candidate
ran with its internal 900-second ceiling from **01:07:08 to 01:10:40 UTC on
October 21** (October 20 locally), completing early in 212 seconds. CatDesk,
Wake2 and PortfolioDesk each received one exact prepared review request at
their own generation-1 target. All three have `EXACT_USER_MESSAGE_APPENDED`
receipts, COMPLETE timers and separately read, exact-event verified assistant
bodies with the requested project markers. Recorded response windows overlap
for 18 seconds. There were no unexpected deliveries or duplicate submissions.

Foreground exit reports `ownedTreeExited:true`; process enumeration confirms
zero owned browser/Wake processes after it and after each sequential read.
Both controls remain STOPPED. All five original registry/Wake pairs, previous
deliveries, completed canary journals and context histories are preserved. Of
9,922 pre-existing non-profile files, only expected readiness-history and
status files changed; three new event/receipt/timer records and exact review
inputs are intentional additions. Production pointer SHA-256 remains
`25b8430dc25fdb7cf2be3465d5f6c5431f6ff274d0dfcf3e09f9c2aa3cb50185`.
No native task, hourly scheduler or recurring browser process was started.
The completed rollover was not repeated.

Private timestamped evidence is in parent
`checkpoints/web-three-project-review-oct20/approved-run-20261021T010650920Z`:
`audit.jsonl`, `acceptance-summary.json`, delivery/readback proofs and final
preservation comparison. An earlier comparison encountered locks during an
active one-shot read; the completed readable comparison supersedes it. The
native app inventory read timed out without session mutations. Existing native
sessions were not interrupted or replaced; this test does not independently
qualify their current development tasks.

One read-only connector request at 01:16:02 UTC still returned
`MCP SSE probe returned 429 from openai.org`; no retry followed. This is a
connector verification dependency. Local authenticated delivery/readback
passed. Protected installation, an exact-source CI result, a genuine
deliverable-to-review-to-next-task cycle and central connector operation remain
OPEN. Review replies are advice on supplied checkpoints, not independent
release approval. Overall October acceptance remains PARTIAL.

### Continued source development: capture freshness

Validated browser ingestion now records a monotonic, per-conversation
`browserCoverage.lastObservedAtUnix`, including unchanged/deduplicated captures.
New generations start without a timestamp; legacy ledgers retain unknown
freshness. The timestamp commits after snapshot validation and the final
target/cancellation check; rejected reads retain previous freshness.
The timestamp reports observation time, not full-history coverage or provider
context capacity. Regressions exercise replay, durable readback, project and
generation isolation, legacy state, cancellation and context-lock contention.
Full root regression passes **1,538 CatDesk tests / 28 ignored** and all other
enabled targets, including the nested Wake suite. Strict all-target/all-feature
Clippy, formatting and diff checks pass. The first run exposed a replay equality
assertion that needed to permit advancing capture time; its failure and corrected
rerun are retained in parent `checkpoints/web-context-freshness-oct20`.
Source checkpoint `41a8c49e19a01e2f6b72142bf7652248dde0271a` was built and
selected as stopped **development v23** through the supported package update.
WakeHost and both Python inputs are unchanged. One bounded standalone capture
then verified the exact CatDesk target and generation, current capture timestamp,
2,123 advisory visible tokens and retained incomplete-history warnings. It sent
no USER message, started no host/scheduler, and exited with zero owned processes
remaining. All five pairs and existing policies are preserved. Both controls
remain STOPPED; the protected production package/pointer is unchanged. Private
`live-freshness-result.json` and timestamped audit in the freshness checkpoint
record this controlled development evidence; it is not production acceptance.

## October 20 acceptance audit and central context-wait reporting (SOURCE)

Read-only central status confirms seven paired registrations (five original
projects and two canaries), separate advisory context estimates, and the
completed fast canary at generation 2 with `continuationProved:true`. No
completed step was repeated. The ten requested live requirements and original
P0–P4 scope are evaluated in parent
`checkpoints/web-acceptance-audit-oct20/requirements-audit.json`. Controlled
development proof remains separate from protected installed, fresh current
source delivery, central connector and natural review-cycle acceptance.
Native inventory reports Bug Bounty, Wake2 and PortfolioDesk active, BYOVD idle;
no native messages, ownership changes or interruptions were issued.

The audit found a real source reporting gap: the durable worker records
`AWAITING_CONTEXT` / `WEB_CONTEXT_BUSY`, but the central registry projection
omitted that stage and returned `UNKNOWN_STAGE`. The existing held-ledger
worker regression was extended through the actual central reader; it reproduced
the wrong stage, then passed after the projection correction, preserving the
reason and null permanent failure. No duplicate stage-only test was added.
Full CatDesk passes **1,537 tests / 28 ignored**, with strict all-target/
all-feature Clippy, formatting and diff checks passing. Wake/Python product
sources are unchanged; their preceding gates were not repeated.

This source-only correction is not selected into another development package.
The stopped v22 candidate, its prepared controller/payload hashes and all live
project state remain unchanged. Fresh live execution still requires the
existing exact three-review approval. Overall October MVP remains `PARTIAL`.

## October 20 verified implementation-review read and installation audit

One explicit `operator web review read` for
`dev-oct18-catdesk-draft-recovery-review-1` succeeded with `eventVerified:true`,
`readOnly:true` and `sendAttempted:false`, using the preserved authenticated
profile and stopped v22 development package. The exact delivered USER digest
and complete latest ASSISTANT pair matched the current CatDesk target. The
1,363-character response is preserved privately in parent
`checkpoints/web-install-audit-oct20`; it reviews the supplied contract only
and is not independent release approval. This resolves retrieval of that
implementation-review body without relying on the throttled connector.

The review requests exact-draft recovery, guarded legacy clearing, and canary
isolation evidence, specifically calling out a composer edit before deletion.
The existing six archive/resume regressions pass at source `87c8ebd`, including
that late-edit refusal and the adapter's post-archive-ACK drift refusal. No
duplicate test or replay was added. The completed rollover/continuation remains
preserved. Actual legacy-composer clearing is not established by this review
read or offline fixtures. All 91 state files, five original bindings and the
production pointer are unchanged, both controls stopped, owned process count
zero. No recurring scheduler or WakeHost was started.

The installation audit also corrects a material distinction:

| Gate | Current authoritative result |
| --- | --- |
| Installed prerequisite | Last fixed preflight: `WAKE_BROKER_SERVICE_UNPROVISIONED`; no provisioning/start attempted. |
| Service source wiring | `install_broker_service_runtime::run_one_service_submission` unconditionally returns `WAKE_BROKER_TRANSACTION_GATES_UNAVAILABLE` after an admitted frame. Provisioning alone cannot complete installation. |
| First-generation admission | Offline V2 verification returns diagnostic readback, with no ledger/install consumer. Production ledger admission still requires unproved migration-anchor and old-owner evidence before exact CI and INTENT. |
| Reusable components | Typed candidate staging/publication and protected pointer writer already exist. They require accepted typed leases; no replacement publisher is needed. |
| Required next release work | Independently accepted closed store/launcher inventory and anchor, V2 admission/ledger/service integration, protected reader/bootstrap cutover, approved provisioning, exact remote CI and installed verification remain open. Matching paths, no-process snapshots or diagnostic signatures cannot supply these proofs. |

The existing install guards remain intact. A large new security framework was
not started during the audit. The prepared bounded three-project development
test is separate from this installation work and still awaits explicit approval.

## October 20 bounded review controller and internal deadline (SOURCE; LIVE HELD)

The prepared external controller has 14 passing synthetic/native-helper checks
for exact project, target, receipt, timer, message digest, sibling completion,
literal argument handling and retained-process timeout cleanup. Default mode
only performs read-only preflight; it does not copy inputs, enqueue or start a
host. An approved run copies the exact payload bytes into the public control
workspace as required by the existing enqueue command, verifies the actual
wrapped queue message, then uses existing run/stop controls. The empty isolated
discovery inbox is pinned read-only during that test to exclude extra work.
Real project native sessions do not write this isolated inbox.

Review found that controller death could otherwise discard its external
deadline. The existing run command now accepts optional
`--max-runtime-seconds 1..900`; the foreground owner enforces this independently,
requests STOP once, permits 30 seconds of graceful exit and then drains its
private Job. Bounded failure retains STOPPED intent. The default unbounded
command is preserved. A disposable gated helper proves the internal stop
without an external controller; invalid CLI/runtime budgets refuse before
control changes. No actual WakeHost/browser was used for these checks.

Final gates pass **1,537 CatDesk tests / 28 ignored**, **294 Wake library
tests / 1 ignored** and enabled integrations, strict root/Wake Clippy, both
builds, formatting and diff checks. The source-matched v22 development package
was selected stopped. The controller's default read-only preflight passes;
all 91 tracked state files, original five pairs and production pointer remain
unchanged. Both controls are stopped and owned browser/Wake process count is
zero. Exact prepared events remain absent from queue and delivery history.
Evidence: parent `checkpoints/web-three-project-review-oct20`.

This remains preparation for the exact three review payloads, not delivery,
protected installation, connector or natural review acceptance. The explicit
operator start hold remains in force and the bounded-test approval request is
pending. Earlier completed canaries and all project histories remain preserved.

## October 20 foreground development ownership (SOURCE; LIVE HELD)

Inspection for the next independent-project test found that the foreground
development launcher waited on an ordinary child without owning its process
tree. It now assigns the existing private Windows kill-on-close Job before
releasing an exact stdin startup gate. EOF or a wrong gate exits the internal
child entry before runtime activity. The foreground launch refuses a package
without that protocol before changing control state. Existing detached start,
protected installation and ordinary host dispatch remain separate paths.

Two native regressions use disposable PowerShell helpers, never WakeHost or a
browser. Retained OS handles prove both helper and descendant were live before
the launcher was abruptly killed, then prove their exit. Normal parent exit
also drains a live descendant while preserving exit code 7. Exact gate and
old-package refusal checks pass. Five finalized focused tests pass; the full
Wake suite passed **293 library tests / 1 ignored** and all enabled integration
targets, including the unchanged Python regression target. Full CatDesk passed
**1,537 / 28 ignored**. Strict root/Wake Clippy, both builds, format and diff
checks pass. The final fixture-only cleanup/live-handle refinements were
rechecked with all five focused tests and strict Wake Clippy.

The source-matched v21 development package was selected through the supported
stopped update. Both Python inputs are unchanged; the new built host contains
the owner protocol. All 91 project-state files, the five original paired
bindings and production pointer remain unchanged. Both controls are stopped
and the final owned browser/Wake process count is zero.

Three exact review-only requests are prepared privately in parent
`checkpoints/web-three-project-review-oct20`, with fresh event IDs, payload
hashes and the current paired targets for CatDesk, Wake2 and PortfolioDesk.
They preserve existing native ownership and distinguish focused/collected
evidence from completed full qualification. Nothing is enqueued or sent.
The execution plan limits a future approved development run to three events
and 900 seconds, then supported stop and exact owned-tree exit checks. It
excludes both canaries and never enables the MCP automatic scheduler.

These source tests close a concrete launcher-loss orphan gap. They do not
authorize or establish live WakeHost, recurring automation, protected installed,
connector or natural review acceptance. The operator's start hold remains in
force. The completed rollover remains preserved without repetition.

## October 20 automatic worker context contention (SOURCE; LIVE HELD)

A disposable held-ledger test reproduced another stop/drain stall: automatic
policy and post-observation readiness checks opened the context store with a
blocking lock. They now use its existing nonblocking read-only mode. Shared
rollover mutation steps use the existing nonblocking writable mode, retaining
policy rechecks, paired-target guards, submission fences and durable event
deduplication. Metadata contention reports `AWAITING_CONTEXT` /
`WEB_CONTEXT_BUSY`; it does not create a permanent failure or request a handoff.

The regression holds the OS ledger lock while both workers exit and drain,
verifies a sibling remains startable, checks unchanged context history and zero
handoff callbacks, then verifies eligibility after release. A prepared-rollover
test also checks confirmation refuses promptly without advancing its phase or
changing history, then succeeds after release and still honors later opt-out.
These are source fixtures, not browser or installed acceptance. The full root
suite passed **1,537 tests / 28 ignored**, with strict all-target/all-feature
Clippy, application build, formatting and diff checks passing. Evidence is in
parent `checkpoints/web-auto-context-oct20`.

The four-file v20 development package was selected through the supported
`PACKAGE_UPDATED_STOPPED` path. Its unchanged Wake/Python inputs match v19.
All 91 project-state files, the five original paired bindings and production
pointer are unchanged. Both controls remain `STOPPED`; no owned browser or
Wake process was present after verification. This is a stopped development
candidate, not protected installed acceptance.

No live capture, message, canary replay, binding change, WakeHost or recurring
start was needed. The completed canary and three prior independent live context
reads remain accepted within their documented development scope. Fresh
multi-project delivery, connector verification and protected installation remain
open. Inspection of `development_runtime::run_foreground` confirms it starts the
shared host scheduler; it is not a scoped one-event alternative to the current
operator start hold. No automatic restart is authorized by this checkpoint.

## October 20 bounded standalone capture (SOURCE + DEVELOPMENT LIVE READS)

The existing `operator web context capture` now selects the current paired
target and uses the same bounded, cancellable worker as MCP capture. It retains
the current-generation/digest checks, per-project capture slot, private adapter
owner and nonblocking count/hash ingestion. It returns before TUI/MCP startup,
and identifies its result as `STANDALONE_OPERATOR`; this is not proof that the
central connector exposes the new tool.

Initial explicit PortfolioDesk and BYOVD reads hit the 60-second outer deadline
and exited after cleanup at roughly 65 seconds. The observed first reader tree
contained 14 reader descendants; all had exited at post-check. Source inspection
showed that the outer limit preempted the adapter's existing 90-second startup
allowance. The fixed overall budget is now 180 seconds, covering the existing
30-second profile-lock wait, startup and a bounded snapshot read. Internal limits,
owner checks and cleanup are unchanged; callers cannot override the budget.

BYOVD then succeeded in 70.1 seconds: five complete visible messages were
recorded, producing a 1,446-token visible estimate. Wake2 succeeded in 64.3
seconds: six complete visible messages and an independent 895-token estimate.
Both matched their existing generation-1 paired targets and returned no raw text,
no send and no host start. Unknown history/coverage gaps remain explicit; these
are advisory visible estimates, not exact provider context usage. The preserved
authenticated profile was reused without sign-in or cloning. No owned browser
process remained after either successful read.

PortfolioDesk then succeeded in 69.4 seconds, recording six visible messages and
its separate 916-token estimate. All three outputs matched the exact preflight
project/generation/URL digest. Only two expected context-ledger files changed;
PortfolioDesk's observation deduplicated without a ledger rewrite. The other 89
tracked state files, all five original bindings, every rollover journal and the
production pointer remained unchanged. Both controls stayed `STOPPED`, and the
final owned browser/Wake process count was zero. No recurring browser activity
was started. These source-matched live reads used the stopped v19 development
package; they do not constitute protected installed or connector acceptance.

Final source gates pass 1,536 root tests / 28 ignored, strict all-target/all-feature
Clippy, application build, formatting and diff checks. Tests cover current-target
selection after rebind, preserved predecessor history, invalid/unregistered scope,
cancellation, contention and duplicate exclusion. Low disk space caused an early
test link failure; only inactive worktree incremental caches and the failed test
PDB were removed. Source, executables, immutable packages, profiles and state were
preserved, and the full gate passed after rebuilding. Failure evidence is retained
in parent `checkpoints/web-operator-capture-oct20`.

The completed canary is excluded from these reads. No message was sent, no
binding changed, and no recurring scheduler or WakeHost was started. Source/live
read verification does not establish fresh work/review delivery, central connector
parity, protected installation or remote CI acceptance. **October MVP: `PARTIAL`.**

## October 20 context ledger contention and install preflight (SOURCE; LIVE HELD)

The existing zero-argument broker preflight returned exactly
`WAKE_BROKER_SERVICE_UNPROVISIONED` (exit 1). Source inspection confirms the
fixed service still refuses transaction admission, while the V2 offline path
has no independently reviewed closed-store/launcher inventory or provisioned
broker evidence. These remain protected-install gates; diagnostic signatures,
the completed Web canary and a stopped development package do not replace them.
No service, policy, trust key or protected installation was changed.

The inspection also found a concrete cancellation/independence gap in context
operations: the online meter used a blocking OS ledger lock. An authenticated
disposable HTTP regression held that lock and released it from a separate
thread after 1.5 seconds. The old request waited and then succeeded, reproducing
a stalled reactor rather than a bounded refusal. Its failure log is retained.

Online context policy/observation and browser excerpt ingestion now open the
existing meter in nonblocking mode. Contention returns `WEB_CONTEXT_BUSY`; other
I/O failures retain their existing error classification. Store layout, object
checks, file locks, atomic writes, target guards and stable event/revision
deduplication remain unchanged. A completed snapshot cannot wait indefinitely
behind a metadata writer and strand its capture lease. Other maintenance uses
retain their existing locking mode.

The expanded HTTP fixture holds the lock through configuration, observation and
registry status reads: both mutations refuse, while all three Wake pairs remain
readable. A separate fake-snapshot fixture verifies busy ingestion, lease release
and an exact deduplicated retry. The store regression verifies unchanged ledger
and policy bytes plus successful use after release. Fixtures start no browser,
WakeHost or recurring scheduler and do not modify real project state.

A separate Codex app inventory reported all four existing native project chats
active. This proves observed session availability, not project task completion.
No messages, goal changes, native ownership changes or interruptions were issued.
The app's CatDesk Web review read returned an opaque content reference for the
assistant body, so that alternate transport did not establish new review text.
The completed rollover and prior assistant-body proof remain preserved.

Final verification: **1,536 root tests passed / 28 ignored**, strict root
all-target/all-feature Clippy, all-feature application build, formatting and diff
checks passed. The held-lock failure and corrected HTTP tests are retained.
Wake/Python product sources are unchanged from the preceding full checkpoint.
The supported v17 development update returned `PACKAGE_UPDATED_STOPPED` with
profile/state preserved, `browserAction:NONE` and `hostStarted:false`.

Evidence is in parent `checkpoints/web-context-locks-oct20`. Installed serving,
connector catalog parity, exact-head remote CI and fresh multi-project live
delivery remain separate open gates. **October MVP: `PARTIAL`.**

## October 20 explicit bounded browser context capture (SOURCE; LIVE HELD)

`catdesk_web_context_capture` now reuses the existing private snapshot owner and
excerpt ingestion through Computer/Both + SupervisorOnly MCP. Its closed scope
requires the current paired project generation and URL digest. The reader
rechecks that witness before browser access and before ingesting each message.
Results expose hashes/counts with `BOUND_BROWSER_SNAPSHOT` provenance, verified
browser binding and unauthenticated caller identity. Unknown history remains
explicit. No message, binding update or recurring worker start is performed.

Blocking capture runs outside the server reactor with a 60-second cooperative
deadline and existing adapter cleanup. A cancelled request retains its project
slot until the blocking worker exits. Four process-local slots cap active reads;
the existing shared browser owner still controls actual profile access. Target
and registration locks are released before browser work so sibling status and
metadata do not wait on the snapshot. Existing CLI/scheduler behavior is reused.

Disposable mock tests exercise cancellation with the worker deliberately held,
sibling capture, same-project refusal, release/unwind, deduplication and target
changes both after preflight and during the read. The authenticated local HTTP
fixture also refuses capture with a stale witness before reaching a reader.
No new real browser read or canary delivery was needed. The first full-suite
run exposed a two-second test deadline that included registration/Git and ledger
I/O under parallel load; the bounded fixture deadline was corrected without
changing production timing. The sandbox application build encountered LNK1318
writing its PDB; the same build passed as the Windows user.

A subsequent full run caught unrelated development controls contending on the
global target mutex: one control's target read returned attention while another
registered/bound projects. Target locks now follow the canonical Wake authority
(or workspace for legacy local target files). Separate development stores no
longer block one another; two controls selecting the same installed authority
still share one mutex. The 250 ms status deadline and paired CAS/compensation
checks are unchanged. Dead weak entries are discarded, and no lock is held
while waiting for a browser. Regression tests cover aliases, separate roots and
shared installed authority without opening the production store.

Final root gate: **1,535 passed / 28 ignored**, including all three development
control fixtures running together. Strict all-target/all-feature root Clippy,
the all-feature application build, formatting and diff checks pass. Wake/Python
product sources and the built WakeHost are unchanged from the preceding
288-passed Wake library plus integration / 229-passed Python checkpoint.
Evidence: parent `checkpoints/web-context-capture-oct20`, including both original
failure logs and the final corrected gate. These are source/mock results.

One read-only connector probe again returned exactly
`McpServerError: MCP SSE probe returned 429 from openai.org`. This remains a
connector verification dependency. The completed fast canary stays preserved
at generation 2 with continuation and assistant response already verified.
All five original bindings and independent native sessions remain untouched.
Recurring browser automation and WakeHost remain stopped.

The supported v16 development-package update returned
`PACKAGE_UPDATED_STOPPED`, `browserAction:NONE`, `hostStarted:false`, with profile
and runtime state preserved. One source-built local status read then reported
all seven registered projects and the fast canary's paired generation-2
`CONTINUATION_QUEUED` / verified continuation. Post-read comparison found all 91
tracked project-state files and all five original bindings unchanged; production
pointer bytes were unchanged, both runtimes were `STOPPED`, and no owned browser
or Wake processes were observed. An overlapping hash attempt refused the status
reader's held journal lock; the verification was completed after that reader
exited. This confirms stopped candidate/readback behavior, not installed serving
parity or a new live Web delivery.

Protected first-generation install admission/provisioning, exact-head remote CI,
connector catalog parity and fresh independent multi-project live delivery are
still separate acceptance gates. No installation authority follows from these
source tests or a stopped development package. Next executable local work is to
inspect the existing protected-install admission evidence and remaining reader
cutover gates, then verify the actual connector catalog and fresh independent
project routing when their dependencies permit. Safe source/evidence work needs
no operator action; existing independent release approval/bootstrap requirements
remain prerequisites for protected installation. **October MVP: `PARTIAL`.**

## October 20 central context controls (SOURCE + LOCAL HTTP VERIFIED)

Source now exposes `catdesk_web_context_configure` and
`catdesk_web_context_observe` in Computer/Both + SupervisorOnly mode. They reuse
the existing per-project meter, policies, event IDs, revisions, deduplication
and token estimator. Configuration requires all automatic flags explicitly;
observation accepts one event up to 65,536 UTF-8 bytes and assigns server time.
Only hashes/counts persist. Caller-reported text remains advisory with
`originAuthenticated:false`; project scope does not identify the ChatGPT caller
or automatically attribute shared HTTP traffic to a conversation.

Both operations require the exact current paired generation/digest and retain
the existing registration and Wake configuration guards during mutation. The
full root regression initially exposed excessive process-wide mutex retention:
a sibling status could return attention during context I/O. A deterministic
same-control sibling-read regression reproduced it. Context I/O now runs after
that mutex is released while registration and a shared Wake transaction read
keep target writers out. Sibling readers remain available, including on errors.

Authenticated ephemeral-loopback HTTP tests cover three independent budgets,
same-ID replay, revision conflicts and replacement, tool-argument/result counts,
unknown-attachment reserve, raw-text privacy, stale/cross-project witnesses and
separate old/new ledgers after a rebind. Unauthorized calls and unsupported
modes refuse. No context operation queues a Wake event or native work request.
The current command reference includes the new schemas and corrects an older
statement that the already-completed fast canary was still prepared.

Final source gates: **1,532 root tests passed / 28 ignored**, **288 Wake library
tests passed / one ignored**, all Wake integration targets, strict root/Wake
Clippy and formatting. The main/Wake build logs and before/after contention
evidence are in parent `checkpoints/web-context-mcp-oct20`. Python product sources
are unchanged from their full 229-passed/one-skipped checkpoint; the bounded
Python integration harness also passed in the full Wake run.

These results establish source and disposable local HTTP behavior. The actual
connector catalog, protected installed runtime, fresh independent project
delivery and scheduler lifecycle still require live acceptance. No real project
policy, target, browser profile, native session or completed canary was changed
to run the fixtures. **Overall October MVP: `PARTIAL`.**

## October 20 scheduler rollover eligibility (SOURCE VERIFIED)

The context ledger already limited uncertainty-only rollover to one use in a
project's history. The scheduler's separate coarse pressure check did not use
that durable decision: after observing a replacement it could prepare another
handoff, take another source snapshot, and then receive
`WEB_ROLLOVER_NOT_DUE_OR_DISABLED` from the ledger. This produced unnecessary
browser work and an attention state despite the guard behaving correctly.

`rollover_readiness` now reads pressure and eligibility under the same meter
lock. The scheduler uses that decision before entering handoff preparation;
intent creation rechecks the same eligibility logic and retains its existing
target, task, timestamp and quiescence checks. An ineligible observation remains
`OBSERVED`. The ordinary opted-in observation cadence is unchanged.

Local fixture tests verify no handoff callback for a spent guard, opted-out
policy or stale generation; beta/gamma can still progress independently, and
new observed pressure makes alpha's replacement eligible again. Readiness on a
read-only meter preserves ledger bytes and returns busy without waiting for a
writer. These are source tests with fake handoff callbacks, not live dispatch.

Final verification: **1,530 root tests passed / 28 ignored**, strict root Clippy,
main build, formatting and diff checks pass. Wake/Python sources are unchanged
from the preceding full regression checkpoint. Evidence lives in parent
`checkpoints/web-eligibility-oct20`. No browser, WakeHost, recurring scheduler,
production pointer, project binding or native session was changed by the tests.
Installed and remote CI acceptance remain open. **October MVP: `PARTIAL`.**

## October 20 complete local regression gates (SOURCE VERIFIED)

The full root suite passed **1,529 tests / 28 ignored** on `f9a4e83`.
The full Wake run exposed a stale Rust-to-Python test selector referring to
the old browser-write-boundary test name. The selector now invokes the existing
submission-boundary test, extended to check both a network error at the durable
checkpoint before typing and a network error after typing. Neither may relaunch
the browser context or repeat the checkpoint.

The required full Python CI command then exposed two stale cleanup fakes missing
the exact-URL and text-entry operations used by the current bridge. Updated
fixtures assert owned-tab closure before context exit after success and unknown
receipt failure, plus no typing or receipt after target drift at the checkpoint.
The shared browser's quit method remains forbidden. The draft/presentation
fixture now runs the production readiness logic with its virtual clock.

Final Wake verification passed **287 library tests / one ignored**, all binary
and integration targets, and its Python bridge regression harness. Full offline
pytest passed **229 tests / one skipped / 35 subtests**, using existing pytest
9.1.1 with third-party plugin autoload disabled. Strict Wake Clippy, formatting
and diff checks pass. These changes affect tests only; main runtime code and
the stopped v13 development package remain the preceding verified build.
Evidence: parent workspace `checkpoints/web-regression-oct20`.

Migration remains held on independently reviewed closed-store/launcher evidence
and protected bootstrap/admission. Task closure already preserves the original
target witness after a rebind; no replacement implementation was needed.
Next source task: make the scheduler consult the ledger's existing one-time
incomplete-history eligibility before attempting another handoff, preserving
ordinary observed-pressure rollover and independent projects. No recurring
browser/Wake process or completed canary was started or replayed for these tests.
Remote exact-commit CI and installed/live acceptance remain separate open gates.

## October 20 central status and read-only access (SOURCE + STOPPED DEVELOPMENT READBACK)

The central rollover report now qualifies saved automation ticks and failures
independently against its paired target generation. Earlier generations are
explicitly `HISTORICAL_GENERATION_SNAPSHOT`; matching generations remain
observations, never proof of a live worker. Missing/invalid generations and
unexpected future generations remain unverified or anomalous. Original stages,
failure reasons and completed continuation proof remain preserved.

The existing local `operator web status` now includes the same `webProjects`
projection as `autonomy_project_registry_read`, allowing central status checks
without the connector. Existing local fields remain available. Registry reads
bypass unrelated autonomy-store initialization. Wake/context diagnostic handles
open existing directories and lock files, refuse mutations, and use nonblocking
shared file locks. Status target-lock acquisition is bounded to 250 ms; execution
retains its existing control-lock behavior. Context status also requires its
reported current pressure to match the sampled active conversation key.

Final focused regression groups passed 42 Web/MCP/context tests, 59 Web runtime
tests (overlapping groups), one missing-registry read test and 23 Wake store
tests. Coverage includes absent roots/locks, busy writers, preserved file bytes,
refused mutations/host ownership, historical failures, and three-project central
status. Strict root/Wake Clippy, both builds, formatting and diff checks pass.

One source-built local status invocation against the stopped development control
completed in 26 seconds. All seven registered projects were reported; the fast
canary remained generation 2, `CONTINUATION_QUEUED`, paired and continuation
verified. Its saved generation-1 failure was explicitly historical. All 98
compared state files and 12 directory entries were unchanged after that read,
and all five original registrations/targets still matched their baseline.
No browser, connector request, send, WakeHost or recurring automation was used.

Evidence: `checkpoints/web-status-oct20` in the parent workspace. Production
still selects stopped dev.84; installed parity, migration admission/provisioning,
exact-source remote CI and fresh multi-project delivery remain open gates.
The connector's last HTTP 429 is separate from these local source/readback
results. **Overall October MVP: `PARTIAL`.**

## October 20 scheduler ownership and shutdown (SOURCE VERIFIED)

Inspection found that stopping the MCP server dropped blocking project task
handles, and a quick stop/restart could escape a boolean-only lifecycle check.
The scheduler now registers each run with an independent cancellation flag and
retains its driver handle in application state. Normal server stop cancels and
joins the drivers; each driver retains and drains its blocking project tasks.
Finished listener handles are checked every 250 ms without browser requests.
A retired run stays cancelled across restart and cannot overwrite new status.

Cancellation reaches the existing browser lock wait, private adapter startup,
snapshot/probe receive loops, handoff reads, bootstrap acknowledgements, paired
binding and continuation publication. Adapter unwinding retains the existing
private Windows Job cleanup. Already-authorized in-flight actions retain their
durable checkpoints; SEND_ATTEMPTED is never replayed. Cancellation before
binding preserves the receipt without falsely recording source corruption or
requiring a new reconciliation. Independent WakeHost and native sessions are
not stopped by the scheduler's cancellation flag.

Offline verification passed 59 Web and 12 Wake runtime tests, including quick
restart, listener exit, draining two blocking workers, pending channel/lock
cancellation, no adapter/lock creation after cancellation, and durable receipt
preservation. No browser, WakeHost or recurring automation ran in those tests.
Strict root/Wake Clippy, both builds, formatting and diff checks also pass.
These tests do not establish live browser shutdown or installed parity. The
source-matched package must be promoted and a bounded live test authorised
before recurring automation is restored; the user's current stop remains.

The first-generation install gate remains separate: inventory hashes alone do
not prove closed store/launcher coverage. No independent inventory or provisioned
broker exists, so admission was not enabled from caller-supplied claims. Safe
source work continues independently of that gate and the connector's last 429.
**Overall October MVP: `PARTIAL`.**

## October 20 first-generation migration target readback (SOURCE ONLY)

Current source already has typed package publication and a protected pointer
writer. The fixed service still refuses transaction admission; neither those
writers nor the completed Web canary establish an installed dev.85 migration.
The next source increment strengthens the existing V2 migration diagnostic:
it pins the selected old root's fixed `config.json`, reads the retained registry
file, verifies both exact raw-byte snapshot hashes, and proves the selected
project's URL/digest/conversation ID and Wake generation match the signed
observation. The registry has no separate stored generation; the matching
Wake target supplies that paired readback's generation. Existing pointer and
host-lock roles must also resolve as exact children of the pinned old root.

Snapshots are bounded to 1 MiB, duplicate project targets/registrations refuse,
and incomplete, split, changed or absent target evidence fails before signature
inspection can succeed. Retained Windows handles block new writers and object
replacement. Fifteen focused offline migration tests pass, including actual
Windows pin/lock fixtures, wrong hashes/generations/conversations, duplicate
fields/projects, missing targets, oversize files and replacement denial.
Strict root/Wake Clippy, both builds, formatting and diff checks also pass.
This is read-only source evidence, with no new ledger or installation authority.

Remaining installed work: independently established closed old-store/launcher
inventory and its reviewed offline approval, first-generation ledger admission,
protected service provisioning and reader cutover, exact-source CI, then live
installed acceptance. Source development can continue on those existing seams;
connector HTTP 429 affects connector checks separately. No browser, WakeHost,
recurring scheduler, production pointer or project binding was changed.
**Overall October MVP: `PARTIAL`.**

## October 20 assistant snapshot verified and three-project readback (DEVELOPMENT LIVE)

One bounded read of the already-completed canary continuation returned
`eventVerified:true`, with the exact inherited marker and task in the verified
assistant response. No bootstrap, binding update or continuation was repeated.
The earlier missing-body state was not reproduced, so a permanent selector
mismatch is not established. The existing private snapshot path now includes
bounded DOM-shape diagnostics when its final assistant body is incomplete;
diagnostics never promote incomplete text or appear in public status.

Checkpoint `43dd541` adds a ten-second, same-document readiness window requiring
two matching complete final message frames, with target checks on each sample.
A follow-up regression closes the deadline edge: a single complete-looking
frame at expiry remains explicitly unverified. There is no reload, Retry click,
typing or send. Source gates pass 23 adapter, 36 bootstrap and 10 executable DOM
tests; main/Wake builds and strict root Clippy pass. The deadline edge is source
verified; the live three-project reads used the preceding v10 package.

Under the stopped v10 isolated development package, separate bounded reads of
the latest saved completed events for Bug Bounty, BYOVD and Wake2 all returned
`eventVerified:true`, exact generation 1, `sendAttempted:false` and owner exit 0.
These are independent readback tests of existing delivery receipts and current
responses, not new dispatch, parallel worker or natural-Wake acceptance. No
WakeHost or recurring scheduler started. Native sessions and project targets
were untouched. The same existing authenticated profile was reused.

Installed parity is a separate open gate: the production pointer still selects
`1.0.0-dev.84-02431feba5ed-1c6bcdfcedf0`. Its adapter/bridge hashes match the
pointer, but it lacks source-snapshot and new-conversation helpers and remains
`STOPPED`. No production package or authority was edited. The existing supported
promotion/migration path must be completed and qualified before these source
and isolated-development results can count as installed acceptance.

The connector's last observed HTTP 429 still affects connector-based ownership
and catalog verification; no connector retry was made in this cycle. One exact
`43dd541` GitHub run lookup returned no runs, so remote CI is not accepted.
Source/package preparation, offline regressions and bounded local readback can
continue independently. Installed promotion, fresh multi-project dispatch and
central request/resolve tool acceptance remain open. No new sign-in or native
session change is needed. **Overall October MVP: `PARTIAL`.**

## October 20 controlled rollover and continuation (DEVELOPMENT LIVE PARTIAL)

Source checkpoint `673e853` adds bounded fresh-document response observation.
Its v8b isolated package advanced the saved canary receipt without replay:
`RESPONSE_PENDING` -> `CREATED` -> `CONTINUATION_QUEUED`. The source handoff
was revalidated, registry and independent Wake agreed on generation 2 and its
exact destination digest, both histories retained the generation-1 predecessor,
and the new conversation's context meter was recorded. All five original
project bindings still exactly matched their saved baseline after the change.
The deliberately low canary threshold was restored to conservative settings.

One foreground development WakeHost delivered the sole queued continuation
event with an exact appended-USER receipt and a completed response timer.
Its owner ran for 77 seconds, issued supported STOP in cleanup, exited normally
and confirmed `HOST_LEASE_RELEASED`. No recurring scheduler started. The later
bounded response read also exited normally; a final process check found no
owned Chrome, driver, WakeHost or scheduler. Native project sessions were not
modified, paused or replaced.

The private DOM excerpt visibly contains `FAST_CANARY_CONTINUED` and the exact
inherited marker/task. Its latest USER body matches the continuation event.
However the assistant-body snapshot selector did not establish `textComplete`,
so the local review reader correctly returned `UNVERIFIED_EVENT_SNAPSHOT`.
Do not label that excerpt a fully verified review response. Durable delivery
and response-completion proofs are separate and passed. The replacement chat
reported its independent ownership/artifact lookup blocked by connector HTTP
429. One transport-status call here independently returned the same 429; it
was not retried. No further task execution was requested from that chat.

Source status now computes `continuationProved` from the existing exact
receipt, completed turn, current owner and paired target checks instead of
returning a constant false. The source-local canary status reads `VERIFIED`.
Missing Wake directories are not initialized by this status read. The central
projection exposes only bounded proof codes. Regression gates: 32 rollover and
11 Web CLI tests, 20 adapter and 35 bootstrap tests, strict root Clippy and
both main/Wake builds passed. The earlier broad bridge suite remains partial.
The new status-reader change is source-local, not yet in the v8b live package
or protected installation. Historical automatic-driver failure fields remain
preserved; use current generation and durable proof when interpreting them.

Next executable work: diagnose the incomplete assistant-body snapshot against
the observed DOM, then qualify the installed/central tool catalog and a bounded
three-project Web work cycle when connector transport is available. No new
login, target edit or native-session pause is needed. Do not keep retrying 429,
restart hourly workers, or claim remote CI/installed/natural-Wake acceptance.
Those gates and full cross-project Web execution remain open. **Overall: `PARTIAL`.**

## October 20 replacement issued; completion observation (LIVE PARTIAL)

Checkpoint `65480c5` supports the observed New Chat Send control. Under the
updated isolated development package, the same saved canary draft was submitted
once, and the actual newly issued conversation URL passed fresh exact-USER
receipt verification. The journal is `RESPONSE_PENDING` / `SEND_ATTEMPTED`,
with a saved destination receipt and no bound target. It must never replay.
The server conversation record shows a completed assistant response, while two
separated browser probes still reported pending. No production target changed.

The next source change gives a fresh browser document a ten-second read-only
observation window in the same visit. Every sample retains exact target,
USER receipt, sequence and usable assistant-body checks; completion still needs
two separated proofs. Errors terminate immediately. Fake-clock tests cover
delayed hydration, a fixed pending budget, drift and transient completion;
20 adapter and 35 bootstrap tests pass. Both binaries rebuild successfully.
The next live action is one bounded probe of the saved receipt, followed by
guarded paired binding and continuation only if completion is proven. No
recurring scheduler or WakeHost is running. No operator action is required.
This supersedes the earlier missing-Send and ready-draft checkpoints below.
**Overall: `PARTIAL`.**

## October 20 ready-draft retry and exact blocker (LIVE PARTIAL)

The read-only composer probe subsequently confirmed the same exact owned draft
and found an enabled `BUTTON` with `type=submit`, `aria-label=Send`, and no ID or
test ID. The existing selectors did not cover it. Source adds only
`button[type='submit'][aria-label='Send']`, retaining uniqueness, disabled-control
and exact-draft checks. Executable offline DOM tests cover this observed shape,
reject duplicate or disabled Send controls and unrelated buttons, and reproduce
the Show more extraction regression. Eight current-DOM tests pass. A further
bounded retry under the updated test package remains pending.
Four focused bridge control/smoke tests and 35 bootstrap tests also pass.
The optional broad bridge suite is not a passing gate: its initial run was
stopped at a 174-second budget after 28 completed cases. The identified
network-error fixture now uses an injected clock and passes in 0.023 seconds;
broader real-time fixtures remain subject to the bounded test budget.

Checkpoint `b3d2a43` is pushed. The corrected USER-body extraction passed a
fresh one-shot read of the previously delivered review with
`eventVerified:true`; the expected review marker and all three acceptance
checks were read. No second review message was sent. The rebuilt v5b isolated
package applied through `PACKAGE_UPDATED_STOPPED`, preserving the existing
authenticated profile and runtime state. The first v5 candidate had correctly
refused with `DEV_PACKAGE_SCRIPT_SOURCE_MISMATCH` because the main image
predated the final script edit; no authority guard was bypassed.

One bounded canary retry revalidated the complete saved draft, retained the
same intent, skipped typing and cleared the overly broad ready-draft fence.
It then refused with the concrete reason `BOOTSTRAP_SEND_CONTROL_MISSING`.
The record remains `SUBMITTING` / `DRAFT_READY`, with null reconciliation
reason, receipt and bound target. No `SEND_ATTEMPTED` checkpoint was reached.
The owner exited 2 before its four-minute cap; neither WakeHost nor a recurring
scheduler was started. This is recovery progress, not completed rollover.

The next executable task is a read-only inspection of visible composer control
attributes on that exact owned home draft. Source inspection now includes a
bounded optional `controls` array, accepts legacy inspection frames, and
validates field types, lengths, scope and wire budget. It reads only rendered
control attributes, with no typing, clicking, clearing or send authorization.
The draft and all project histories remain preserved. No operator action is
currently required for the diagnostic. **Overall: `PARTIAL`.**

## October 20 bounded recovery resumed (SOURCE VERIFIED; LIVE PARTIAL)

The approved implementation-review request reached exact `SENT` on October 20.
Its temporary worker stopped normally with `HOST_LEASE_RELEASED`; subsequent
inspection found no owned automation process or development scheduler. The
review response requests exact-draft/idempotency checks, archive-before-clear
and composer-race checks, and canary continuation with project isolation. It
explicitly reviews the supplied contract only, without independent live proof.

The preserved canary remains `SUBMITTING` / `DRAFT_READY`, with no send-attempt
checkpoint, receipt or replacement binding. The v2 code had classified every
pre-send exception as ambiguous, including read-only checks after full draft
readiness. Source now permits that specific ready-draft case to recover only
through the full existing exact-draft checks. It clears the old broad fence only
after a new verified DRAFT_READY checkpoint. Ambiguous typing, unrelated
reconciliation reasons, legacy unknown sends and SEND_ATTEMPTED remain blocked.
Bounded helper reasons and exception classes are retained for diagnosis.

An explicit local `operator web review read --project-id P --event-id E` reads
one already delivered review using the existing browser owner and private Job.
It does not run WakeHost, send a message or complete a timer. Only that exact
delivery's passive response timer may be pending; another pending turn or
queued task still refuses. A digest-matching complete latest pair sets
`eventVerified:true`; otherwise a bounded private diagnostic snapshot is
labelled `UNVERIFIED_EVENT_SNAPSHOT`. Such snapshots are not public status.

The first live read exposed a DOM extraction defect: the complete USER body
had an appended ellipsis and Show more control. Source now selects the same
dedicated USER body as exact delivery receipts and treats missing bodies as
incomplete. The captured assistant response contains the expected review marker,
but that earlier snapshot did not establish an exact event pair. Qualification
of this extraction fix and the canary retry remains pending.

Focused checks pass: 35 Python bootstrap tests, 18 adapter tests, 6 current-DOM
tests, 11 Rust Web operator tests, 9 Wake bootstrap tests, the persisted rollover
restart regression, and the Windows private-Job termination/accounting test.
These filters overlap. Strict root and Wake Clippy and both builds passed.
All five original bindings passed another exact comparison. No native Codex
project was paused or edited. Recurring schedulers and WakeHost remain stopped;
further live checks must be one-shot and observed through cleanup. **Overall:
`PARTIAL`.**

## October 18 owned-draft recovery (TESTED_LOCAL_SOURCE; LIVE PENDING)

The user identified the preserved composer as a CatDesk rollover bootstrap.
The old journal records `SUBMITTING` without a send boundary or receipt. Its
predecessor, intent and full expected-message digest are retained. The former
slow typing path could exceed the 180-second progress watchdog after that
combined checkpoint, leaving a saved draft. An exact draft match establishes
ownership, but does not establish that an earlier send never happened.

Protocol 2 now journals text-entry authorization, full-draft readiness and
send-attempt authorization separately before each browser ACK. A pre-send
restart resumes only its exact complete saved draft and skips typing. Empty,
partial or foreign drafts refuse. Explicitly observed pre-send ambiguity is
durably fenced; legacy unknown and send-attempted records never replay.

The supported `inspect-draft` command reads a bounded full-draft comparison.
The separate `archive-draft` command applies only to a legacy unknown intent:
it saves actual DOM text and expected journal text privately before authorizing
an atomic exact-draft clear, then reloads HOME and verifies empty. Its old
intent, source pair and history remain preserved and non-replayable. Completed
archive receipts identify historical observations rather than claiming a new
browser read. Oversized archive protocol witnesses refuse before clear.

Verification: 34 Python new-conversation tests, 18 adapter tests, 6 current-DOM
tests, and 10 Rust Web CLI tests pass. The Web rollover suite passed 31 cases
in the sandbox; its sole existing Windows metadata fixture failed with
`CONFIG_ROOT_UNAVAILABLE` and passed on the normal-user rerun. The final
restart case and Wake protocol case each passed. Strict all-target/all-feature
Clippy passed for root and Wake. Independent static review found no remaining
blocking issue after corrections to ambiguity handling, frame limits, raw-text
preservation and the clear boundary. These checks are not live acceptance.

The manual login browser closed normally; no new sign-in was requested. A fresh
read-only comparison found the five original registry/Wake pairs exactly
unchanged. The next live steps are: compare the old canary draft; archive it
only on exact ownership; verify the separate PREPARED canary has a clean HOME;
then run normal scheduling and verify the actual replacement URL, paired
generation, history and continuation while another project receives a review
event. Production Wake targets and native project sessions are outside this
canary. Installed release, central connector acceptance and exact-source CI
remain separate gates. **Overall: `PARTIAL`.**

## October 18 completed continuation with advisory meter retry (TESTED_LOCAL_SOURCE; LIVE PARTIAL)

Source `9102b9e` allows new work after a proved completed continuation when
the context meter is still `RetryRequired`. It requires the exact paired
destination, bootstrap receipt, continuation delivery and completed timer,
unchanged native owner, verified saved source witness and no reconciliation
reason. A single locked meter read must show either the exact predecessor
intent or the destination without a new intent. Missing, corrupt, unrelated
or pending current-generation state still refuses work.

The real work-lease fixture uses a prepared development root, Git registrations
and inert package files. It verifies the retry path, incomplete continuation
refusal, corrupt-meter refusal and another project's independent lease. No
browser or service starts. The Web operator filter passed 47/47, context ledger
12/12 and Web work 13/13; these filters overlap. Final strict all-target/all-feature
Clippy passed after correcting one redundant borrow, and the integrated
continuation fixture passed again on that final source. Scoped formatting and
diff checks passed. Private logs are in `checkpoints/web-advisory-retry-oct18`.
The initial sandbox fixture attempt hit `CONFIG_ROOT_UNAVAILABLE`; its scoped
normal-user rerun passed.

The running manual-login executable and existing authenticated test profile
were preserved. This source is not yet built into the live runtime. Draft
inspection, actual replacement URL, paired binding and continuation remain
live acceptance gates; other native project chats remain usable. **Overall:
`PARTIAL`.**

## October 18 retained rollover diagnosis (TESTED_LOCAL_SOURCE; LIVE PARTIAL)

Pushed source `6c8c0b1` retains the last concrete automatic rollover failure
through subsequent in-progress, stopped and policy-disabled ticks. Central
status exposes bounded diagnostic metadata and requests draft inspection only
when the saved PREPARED rollover still matches the current paired target.
Legacy records remain readable; corrupt advisory status does not prevent a
new truthful status write or disclose raw private data. Four focused tests,
strict all-target/all-feature Clippy, scoped formatting and diff checks passed.
The checkpoint includes one mechanical allocation lint fix in an existing
operator parser test.

This is source verification. The isolated development runtime still uses the
earlier `bce01e9` package, with its existing authenticated profile open for the
pending user draft inspection. No draft was cleared, replacement conversation
accepted, target rebound or native project goal paused by this change.
Installed release, exact-source remote CI and central connector acceptance
remain separate and unproved. **Overall: `PARTIAL`.**

## October 18 seven-project live Web checkpoint (DEVELOPMENT_LIVE_PARTIAL; MVP PARTIAL)

**Final live checkpoint: `PARTIAL`.** Registry and Wake contain seven guarded pairs: five original projects and two controlled canaries. Pushed source `bce01e974054646006520a598ed83d449d30afaa` passed main/Wake builds, 49 focused Python tests, three focused Rust STOP/recovery regressions (two STOP, one recovery) and strict Clippy. Exact-commit CI lookup returned `[]`; remote acceptance is missing. The v3 isolated development package was applied through supported update with authenticated profile and state preserved. Independent raw-file comparison against the original five-project snapshot found all five registry records and Wake targets exactly unchanged with stable read hashes (private `current-binding-comparison.json`, SHA-256 `83EF59A1DDC68F37C0E5E145DA20ED879545826B7D95086CA4248C99EAE6546F`).

The normal scheduler observed 254 visible canary tokens against the deliberate 200-token threshold. Its 800-character handoff request entered `SUBMITTING` at Unix 1792326933, reached exact `SENT` at 1792326965 (32 seconds later) and response `COMPLETE` at 1792326982. A 2,227-character handoff was collected with its marker preserved; later visible context was estimated at 782 tokens. The fresh rollover intent reached `PREPARED` but ended `ATTENTION` / `EXISTING_DRAFT` at Unix 1792327111 with no bootstrap ACK, receipt or guarded binding change. The home-composer guard establishes only nonempty text; its owner is unknown. Automatic rollover was disabled only for this canary through context configuration. Supported STOP released the host with exit 0; scheduler session 20297 later terminated exit 1 after Ctrl+C drain. Guarded login opened the preserved test profile (`LOGIN_BROWSER_OPEN`) without new sign-in. The user was asked for manual inspection of the New chat draft and clearing only if recognizable old canary text; otherwise leave it. No inspection result or clearing is verified, and no automated safe inspect/clear route is accepted. Preserve both uncertain attempts without replay.

During this cycle an independent real BYOVD checkpoint review reached exact `SENT` at 1792326761 and `COMPLETE` at 1792326811; Wake2 native work remained active. BYOVD commits `f184038`/`536690d` passed 118 tests with three skips. Bug Bounty public-pilot gates remain open, and Portfolio's separate external-window block is unchanged. Protected production profile, installed release, remote CI and central connector catalog remain open. Earlier paragraphs below record prior checkpoint states. **Overall: `PARTIAL`.**

Earlier live checkpoint: the first separate scheduled canary rollover attempt (intent prefix `5a92897b`) ended `ATTENTION` / `BOOTSTRAP_OUTCOME_UNKNOWN`, with durable phase `SUBMITTING`, null receipt and unchanged generation-1 source pair. A supported `STOP` succeeded, but in-flight `CollectHandoff` continued into browser bootstrap after STOP; timeout and no-replay preserved the uncertain outcome. At that earlier checkpoint, the development host and scheduler were stopped. A distinct browser source-canary conversation was created but is not registered or accepted as a replacement. Uncommitted source changes replace delayed Python `press_keys` with `send_keys` (49 focused tests pass) and recheck Rust STOP after collection, before bootstrap, before ACK and before probe while retaining Wake-first recovery (three focused tests and strict Clippy pass). Source files are frozen for packaging; live retest is pending. The earlier checkpoint details below describe prior running-service and pre-attempt states. No guarded pair update, completed rollover, installed proof or remote CI acceptance follows from this attempt. **Overall: `PARTIAL`.**

Earlier source checkpoints `1c4295bab2210e539674c6600ac67a5476bcb2c2` and `304dcdfeb1a17285971ea9b73a563bed11b302ba` were pushed to `codex/web-rollover-oct17`. Under the first updated package, the canary's ordinary event and a Wake2 event reached `SENT`/`COMPLETE`; the scheduler then reported `WAKE_STORE_AUTHORITY_POINTER_INVALID` from a legacy development package-ID parser. Commit `304dcdf` adds development-only full-hash authority validation (137 lines). Its native fixture passed 1/1, strict Wake and root Clippy passed, and both builds passed. A second supported package update succeeded with the same authenticated profile, and the updated host and loopback scheduler restarted. Actual scheduled canary rollover remains pending; no manual auto tick was used. Authenticated repository readback found Actions permission enabled and the workflow active, yet no run for exact `1c4295b` after push. The workflow has push/PR triggers and no `workflow_dispatch`; this is not remote CI acceptance. Central `web_work` request/resolve remain hidden in the current Computer + Multi Tools surface, while source exposes Computer + Supervisor Only. Reviewed serving parity, supported UI mode selection and existing connector catalog refresh are needed; the earlier v4 HTTP 429 is a separate limit. No authentication or tunnel change was made. **Overall: `PARTIAL`.**

Exact development source `5cedbb78ea5e6af90fd8aa1f2b3282aebd551548` (tree `64ff6686c661d4e93ee0809a485d43fe35a234e3`) built the earlier all-features CatDesk binary, SHA-256 `DB0F5600C19D42721BBB147C7A0052861B25C463644661132074F5D646821C7D`. Original login session 94778 ended normally, exit 0, as `LOGIN_WINDOW_CLOSED_UNVERIFIED`; window closure alone did not prove authentication. Subsequent browser work reused that same isolated profile and observed authenticated access. It did not touch the protected production browser profile or installed Wake. Foreground development Wake PID 55012 and headless scheduler session ID 82583 were observed live at the earlier checkpoint, with the latter serving `http://127.0.0.1:53354/catdesk-web-dev/mcp`. Both stopped normally for a supported `PACKAGE_UPDATED_STOPPED` package update, preserving the authenticated profile, configuration, queue and old version; no native project goal was paused. The updated Wake host and loopback scheduler then ran. These transient process and loopback observations are not installed-service proof.

At that earlier checkpoint, six independent project conversations were guarded-paired between the development registry and Wake. The original CatDesk, Wake2 and Portfolio pairs were unchanged; BYOVD and Bug Bounty were added with matching generation-1 readbacks, followed by the controlled `catdesk-rollover-canary` at initial generation 1. Their native Codex sessions and project checkouts were not taken over (`nativeSessionAction:NONE`). The five original project events each reached durable `SENT`, `EXACT_USER_MESSAGE_APPENDED` and response timer `COMPLETE`. The CatDesk, Wake2 and Portfolio markers/answers plus Bug Bounty's answer were observed through the browser UI. BYOVD has the durable receipt/timer, but its UI content was not separately verified because that refresh failed to render. Portfolio's transient `ATTENTION` cleared through the existing read-only probe without resend or a source change. The canary's ordinary initial event separately reached verified exact `SENT`; its multiline input file was normalized by enqueue, so this is not raw multiline browser-entry proof. These are delivery and response observations, not independent verification that all requested project work was completed.

Read-only context estimates recorded CatDesk 247 initially and 1,007 after handoff, Wake2 238, BYOVD 565, Portfolio 526 and Bug Bounty 709 visible tokens across four messages, each with incomplete history coverage. CatDesk's prior explicit test policy (32,768-token budget, 32,568-token headroom, 8,000-token unknown-history reserve) deliberately made a 200-token test trigger `nearCapacity:true`; its automatic rollover trigger is now disabled. The low 200-token automatic policy is enabled only for the controlled canary. Neither signal proves natural capacity pressure or complete transcript capture.

The CatDesk autonomous handoff event reached durable `SENT`/`COMPLETE`, and the collected handoff reached rollover `PREPARED`. The first live attempt ended `ATTENTION` / `BOOTSTRAP_HOME_DRIFT` at Unix time 1792323858 before the parent's `SUBMITTING` checkpoint, with a null receipt. Read-only browser inspection then found an unpaired orphan chat with multiple USER turns containing fragments of the multiline handoff: `cdp.press_keys` treated newlines as Enter during text entry. `PREPARED` and a null receipt did not establish that no browser effect occurred. The supported exact-intent/observed-URL quarantine has now been applied to the old CatDesk intent: its current phase is `QUARANTINED`, preserving the orphan, source target and collected handoff and preventing replay without changing the original guarded pair. No complete bootstrap, guarded pair update or continuation is verified. CatDesk's automatic rollover trigger remains disabled, while the separate canary uses the low test threshold; no native project goal was paused. The update-package command succeeded with the same isolated profile and requires the same four source files for an interrupted retry. Source verification passed 42 focused root Web tests, four quarantine tests, five development CLI tests, strict root Clippy/build, 45 focused Python text-entry tests, and four Wake package update tests with strict Clippy. Earlier sandbox fixture failures passed when rerun outside the sandbox. A genuine replacement rollover remains pending. Portfolio later reached observer `OBSERVED`, so read-side observation still progressed. Do not retry the quarantined bootstrap or infer full rollover from the earlier completed handoff. Central provider tool routing remains unaccepted after the prior 429/catalog limit; installed release/Wake, exact-source remote CI and the full October MVP remain open. **Overall: `PARTIAL`.**

The following October 18 sections preserve earlier checkpoint wording. Their login-pending, three-paired, build-pending and no-delivery statements were superseded by this live checkpoint; their source-only and safety limits still apply.

## Earlier October 18 HTTP work routing and five-project companion setup (historical checkpoint; PARTIAL)

The existing three-project fixture now exercises an actual authenticated
loopback MCP listener. One project's request, exact retry, conflict refusal,
status and queued retirement use JSON-RPC HTTP while the other two retain
their independent queued work. An unauthenticated request returns 401 and
creates no event or work intent. Requests have a finite timeout. The focused
fixture passed 1/1; strict root Clippy and formatting passed. Final verification
retained unchanged Rust-source hashes in private
`web-companions-r3-verification.json`, including the diagnostic test below and
an exact project/event-ID check for the two remaining queue entries. An initial
compile error in that new assertion was corrected; its failed log is retained.
This verifies
local HTTP composition, not central ChatGPT connectivity or browser delivery.

Two additional browser-issued conversations were created for BYOVD and Bug
Bounty. Both returned their requested readiness markers and a bounded review
answer; their context preserves existing native development ownership. These
are manual UI setup results. Their URLs and responses are retained privately,
outside the repository. The previous three browser URLs match their stored
bindings. All five now have companion conversations, but only the original
three are registered and paired in the development runtime.

The attempted BYOVD registration returned generic `attention`; a separate
development-runtime status read identified `DEV_LOGIN_CLEANUP_UNVERIFIED`.
The existing login session remains live and awaits the operator's sign-in
confirmation. No lock or marker was cleared. Exact before/after comparison
confirmed that the registry and Wake target configuration did not change.
Bug Bounty registration was not attempted after that refusal. Private evidence
is `companion-registration-attention.json`; finish registration through the
supported commands after login cleanup and then verify paired readback.

The observed generic response prompted a small source fix: `operator web`
parse and execution failures now expose a bounded `errorCode`. A focused
diagnostic test passed 1/1, including the actual login-guard code and rejection
of raw/path-bearing detail. Legacy, Codex and development-runtime error output
keep their existing behavior. The standalone application remains the prior
build while its login process is live; the new CLI output is not yet a live
binary result.

The current v4 connector probe returned HTTP 429 from the provider gateway,
and its exposed tool catalog lacks the new Web work tools. A loopback test
does not establish provider routing or bearer-header compatibility. External
tunnel ownership is preserved. Three real deliveries, browser-derived context
estimates, genuine rollover/continuation and central initiation remain open;
native Codex projects need no pause for this development work.

## October 18 central Web work requests (TESTED_LOCAL_SOURCE; LIVE UNVERIFIED)

SupervisorOnly MCP now exposes `catdesk_web_work_request` and
`catdesk_web_work_resolve`. A request uses the registered project and exact
paired target witness, publishes through the existing Wake queue, and retains
one active work intent per project. Same-ID retries are idempotent; a different
request waits for explicit resolution. Status separates the active intent from
transport records. Existing native sessions keep their writers and checkouts.

Resolution uses the original request target, including after rollover. A
completed disposition requires its exact SENT receipt and COMPLETE response
timer, but remains a coordinator report, with `taskCompletionVerified:false`.
Retirement handles exact unsent events and intents saved before publication;
submission ambiguity and mismatched evidence refuse closure. History remains
preserved. A project rollover lease prevents overlapping handoff/continuation
and new work; automatic rollover selects the current work intent as its task
anchor rather than an older completed request.

Focused verification passed: Web work 13/13, development 5/5, continuation
7/7 and Wake retirement 6/6. The filters overlap; no full regression is claimed.
Strict root/Wake Clippy and formatting passed. The final unchanged-source
witness and logs are in private `central-work-verification.json`, verified
at `2026-10-18T05:54:20Z`. An earlier verification detected a source change
during the run and was not accepted; the final run retained matching hashes.
The standalone application was not rebuilt while its prior executable held
the login session. The live login remains at
`LOGIN_BROWSER_OPEN`, awaiting the operator's existing sign-in confirmation.
No new browser delivery, context capture, rollover, installation or remote CI
acceptance is claimed by this source change. The running login executable is
preserved while test executables are compiled separately. Other project chats
can continue; no pause is required for this development work.

## October 18 guarded development login (TESTED_LOCAL_SOURCE; MANUAL_SIGN_IN_PENDING)

`operator web-dev-runtime login --project-id PROJECT` now opens the selected
project's paired ChatGPT target in the existing isolated development profile.
It requires stopped control, exact development authority, current target
isolation, and retained host/browser locks. The helper starts its browser only
after assignment to the owned process Job. Closing it requires terminal Enter;
the locks remain held until the helper is reaped and the Job reports no active
processes. Uncertain cleanup retains an attention marker. No installed profile,
authentication material, target binding or event receipt is copied or changed.

Focused verification passed: Wake development runtime 7/7, native owned-Job
cleanup 1/1, Python helper 3/3, and application development tests 3/3 (including
the three-project registration/routing fixture). Strict root/Wake Clippy,
formatting and the all-features application build passed. The final source
hashes and logs are retained in private `20261018T052714Z-login-verification.json`.
The existing broad regression was not repeated for this narrow change.

The built operator (SHA-256
`AB93E1BBA08BAD928AD415A5EE39495AE198E24B6B41EA2B0D69693E90D10B4F`)
was launched against the existing prepared control workspace and reported
`LOGIN_BROWSER_OPEN`. It is waiting for the operator's manual sign-in and
terminal acknowledgement. This proves the guarded helper reached its open
step; it does **not** establish authentication or exact-conversation readiness.
After it closes, use read-only context capture before retrying delivery. The
successful close result itself remains `LOGIN_WINDOW_CLOSED_UNVERIFIED` with
`authenticationVerified:false`. Actual deliveries, context capture and genuine
rollover remain open. Other projects' native conversations need no pause.

## October 18 isolated Web development runtime (TESTED_LOCAL_SOURCE; DEVELOPMENT_LIVE_PARTIAL)

Source `7bfaab0c5d9980a48399998326f313fc03ad8269` adds an explicit
`DEVELOPMENT_UNPROTECTED` Wake runtime for
a **new, empty, separate control workspace**. `operator web-dev-runtime prepare`
copies the source-matched Wake package and Python runtime into that workspace's
`.catdesk/web-development/runtime`, records a fixed authority, and leaves the
host `STOPPED`. It does not install or migrate protected Wake. Normal Web
target, context, rollover and queue paths select this root when its authority
is valid. Partial or invalid development authority fails closed rather than
falling back to the installed store. Status labels the selected mode.

The fixed development package, per-project paired target and isolation checks
are required before controlled queue publication or browser work. Development
targets are checked against known production current and historical ChatGPT
conversation IDs, with another check by WakeHost immediately before submit.
The `web-dev-runtime enqueue` command queues a bounded workspace-supplied
`test` event for a paired project; its response explicitly leaves delivery and
natural review **unproved**. The development lane uses ordinary user-owned
files and does not provide protected install, global profile isolation or
production release authority.

The explicit `operator web project register` route now registers an exact Git
root and origin as Web-only, with no Codex thread, native writer claim or
checkout edit. It uses the existing exclusive registration lock and rejects
duplicate IDs/workspaces; `catdesk` must first identify the control Git root.
The local fixture uses this actual operator route for three projects.

The unchanged pre-registration source passed the broad local Cargo regression
(main binary 1,487 passed / 28 ignored; the full command also ran the nested
Wake verification fixture).
After the Web-only registration delta, focused local checks passed: Web
operator 36/36, real-Git registration 1/1, and the three-project native
integration fixture 1/1. Strict root Clippy, formatting and final CatDesk build
passed; Wake strict Clippy and host build passed after two mechanical lint
fixes. The fixture prepares a runtime without launching it, binds and reads
three Web targets, publishes controlled events through the operator route,
records independent context, and checks that selected production control
files were unchanged. It also checks replay/conflict and invalid-authority
refusal. These are fixture conversations and queued events, **not** three live
ChatGPT deliveries. The full root suite was not repeated after the narrow
registration delta. The operator sequence is in
`docs/orchestrator/CATDESK_IMPORTANT_COMMANDS.md`.

**Development live results:** three fresh ChatGPT URLs were issued and observed
through the authenticated browser UI for controlled CatDesk, Wake2 and
PortfolioDesk review tasks. CatDesk and Wake2 returned their requested markers;
PortfolioDesk displayed `This response couldn't load`, including after reload.
This manual setup is separate from CatDesk delivery. The actual development
runtime was prepared, then the three projects were registered through the
Web-only command and paired with distinct generation-1 targets. Three bounded
review events were queued. All native thread IDs remained null.

The retained foreground host ran against the fresh development browser profile.
Its first event remained on ChatGPT HOME and ended in pre-submit
`TARGET_DRIFT` attention without a receipt. The scheduler then claimed the
PortfolioDesk event, showing that the first attention state did not prevent
another project from being selected. The test was stopped before any submit;
the retained host exited successfully, PortfolioDesk recorded
`HOST_PAUSED_BEFORE_SUBMIT`, and Wake2's event remained queued. Selected installed
Wake control-file hashes matched before preparation and after the live run.

**Still open:** exact-conversation readiness in the fresh development browser
profile, three actual Wake deliveries, browser-derived context estimates,
genuine rollover and continuation, and central ChatGPT access to the local MCP
endpoint. HOME drift alone does not establish an authentication failure. At
this runtime checkpoint, a guarded manual login flow was not exposed; the
subsequent login section above records its implementation and pending live
sign-in. The legacy login helper must not bypass the shared browser lock.
No installed Wake release was activated, and exact remote CI
acceptance remains separate. Local evidence is retained in the private
`checkpoints/web-development-oct18` directory outside the source checkout.
The October milestone remains **PARTIAL**. Existing project conversations do
not need to pause for this development lane; only this test host was stopped.

## October 18 canonical release-pair reconciliation (TESTED_LOCAL_SOURCE; LIVE PARTIAL)

Source `2cf2751c72f491513760d21cd183a891f01d699d` adds the missing recovery path
for the observed canonical binary/sidecar mismatch. The operator's
`release-pair prepare/confirm/result` commands reuse existing independent
review, committed snapshot and producer evidence. A separate one-shot record
binds both original file hashes, preserved slot-a and fixed script/interpreter
bytes. The worker rejects a mapped canonical process, occupied listener,
canonical worker image or enclosing Windows Job before unsafe replacement.

The daemon starts in the existing private native Job. Durable pending evidence,
exact serving-owner checks, Job disarm/close and a fresh owner check precede
`reviewed_reconciliation` authority for slot-b. An abort before handoff proves
the whole owned tree exited before restoring the original binary and raw
sidecar bytes. An uncertain outcome after release requires attention; it does
not restore potentially mapped files or replay a claimed attempt. Slot-a and
the original escrow remain preserved.

Full root regression passed **1,801 tests / 29 ignored**. This includes the
root fixture that invokes Wake's test-support suite; nested test counts are
not printed on success. New reconciliation, existing promotion and bootstrap
PowerShell fixtures passed, as did strict Clippy, formatting and build. A
compiled CLI status check created no files in an empty temporary workspace.
Four focused reconciliation tests and nine native ownership tests also passed.
Independent source and ownership reviews found no remaining concrete blocker.

The first overall witness rejected a late change to two runtime scripts.
The passing full Rust result was retained only after unchanged Rust/compile
inputs were verified, and the final script fixtures were rerun. A later private
smoke assertion expected an obsolete status label; it was corrected to the
source's `RESULT_UNAVAILABLE` without changing production code. All witnesses,
log hashes and exact source bindings are recorded in
`docs/acceptance/canonical_reconciliation_tests_20261018.json`.

**Installed acceptance remains open.** The script fixtures simulate owner
proof; the separate native tests exercise real child/descendant ownership.
No composed installed reconciliation was run. The read-only snapshot still
shows canonical SHA `9A8F4E...65568C` versus sidecar/slot-a SHA
`A264C8...649939`; process inspection was unavailable in that snapshot.
No Wake binding, installed image, service or external tunnel changed.

Next: obtain the actual current reviewed candidate and release acceptance,
then use this guarded path. GitHub fork-workflow re-enable authorization,
Wake first-generation migration/owner/service wiring, aligned project targets,
three-project Web delivery and genuine autonomous rollover remain open. The
combined October milestone is **PARTIAL**. CatDesk's goal remains active;
other native project conversations need no pause or reservation.

## October 17 retained offline Wake locks and historical-store evidence (TESTED_LOCAL_SOURCE; LIVE PARTIAL)

Source `d5e64f6a0c05a9d555b547d0af919c1ad7e53822` extends the existing offline
migration module with retained exclusion for a bounded set of exact old-store
`host.lock` objects. Selection supports paths and already retained root handles,
including a root recovered by file ID. Physical aliases are checked and only
the same root/lock pair is deduplicated. The module also reads bounded SHA-256
from the retained predecessor pointer and host image. Borrowed root handles
keep their caller's share mode; these checks do not prove complete inventory,
current launch-path ownership, durable restart prevention or install authority.

Seven focused native tests and the full Wake suite passed: **301 passed / 3
ignored**. Strict root/Wake lint, formatting and builds passed on unchanged
source. An initial sandbox run failed in native object inspection in the existing
installer fixture. The unchanged test and full suite passed outside the sandbox;
parent-directory metadata restrictions are the diagnosed cause, inferred from
the paired runs and a separate read-only probe. Both failure
and successful verification evidence are retained in
`docs/acceptance/wake_offline_locks_tests_20261017.json`; no check was weakened.

Read-only file-ID probes located both recorded historical roots and their
different lock objects. In this process/token's handle-relative snapshot,
`config.json`, `status.json` and `control.json` opened the same respective
physical objects under both roots. Only the historical root exposed a
`current.json`, selecting dev84. The observed config has one CatDesk target at
generation 1 and desired state `STOPPED`. The dev59 status file is stale and is
not current process evidence. Each leaf was double-read and reopened with
matching ID/size/hash; this was not an atomic snapshot across files or roots.
The private evidence filename and hash are recorded in the manifest.

No installed locks, bindings, images or services changed. First-generation V2
admission, a no-replace protected genesis pointer, owner/launch proof and broker
service wiring remain open. Exact CI and reviewed main-release recovery also
remain required. Three-project Web delivery and autonomous rollover are still
unaccepted. CatDesk's goal is active; other native chats need no reservation or
pause. Their saved blocked goals are separate from CatDesk's earlier handoff.

## October 17 protected Wake pointer switch (TESTED_LOCAL_SOURCE; LIVE PARTIAL)

Source `fcabbe3cbb4aa4b0d59a878bcb47fb239517eb82` implements one protected `current.json` switch
after a durable `OldOwnerStopped` phase and retained package-publication receipt.
It records a single-attempt marker before creating the new pointer, replaces the
fixed relative name under the existing global ledger lock, and verifies both
the canonical bytes and exact renamed file identity before recording
`PointerSwitched`. Reopen requires the original marker and its matching pointer
binding. An uncertain marked attempt refuses replay; previous packages remain
preserved. Independent review findings about missing replay evidence and exact
renamed-object identity were fixed and rereviewed.

Nine native pointer tests exercise successful replacement, wrong predecessor,
missing owner-stop prerequisite, an independent reader preventing replacement,
failures before and after rename, missing/corrupt markers and an interrupted
phase-record flush. Full Wake regression passed **297 tests / 3 ignored** on the
final unchanged source. Four enabled `python_bridge_validation` tests passed;
its manual bridge regression harness remained ignored.
Strict root/Wake lint, formatting and builds passed on that same unchanged source.
Commands, source/binary/log hashes and test groups are recorded in
`docs/acceptance/wake_pointer_switch_tests_20261017.json`. Unchanged root Rust
tests and standalone Python suites were not rerun in this checkpoint.

The tests use isolated temporary files, unprivileged fixture ACLs and a test-only
owner-stop prerequisite. Their protected-schema predecessor does not demonstrate
migration of the actual installed dev84 pointer. The production policy/ACL path
is compiled but has not run as the installed service. No installed image,
service, Wake target or browser authority changed.

Remaining release work is concrete: first-generation migration and old-owner
proof, the typed owner-stopped transition, post-switch launch proof, and service
transaction wiring. Exact resume inspection for `OldOwnerStopped` and
`PointerSwitched` remains unsupported and refuses rather than inventing recovery
authority. The main canonical binary/manifest mismatch still requires the
separately reviewed recovery path described in R38. GitHub fork-workflow
re-enable authorization remains pending. Three-project live delivery and actual
autonomous rollover remain unaccepted.

CatDesk's goal is active. Native project chats need no reservation or pause for
this work; the repeated BYOVD checkpoint permission request was corrected using
the user's existing approval. PortfolioDesk's dated observation dependency is a
blocked goal, distinct from pausing a project for CatDesk.

## October 17 central rollover status and native regression checkpoint (TESTED_LOCAL_SOURCE; LIVE PARTIAL)

Source `ec1ec272f303be4847b397c3c0950ca3f031cca1` adds per-project durable
rollover and automation progress to `autonomy_project_registry_read.webProjects`.
The report includes handoff progress, pending responses, attention and stopped
states without exposing handoff text or receipts. Missing, corrupt or busy
state is reported for that project while siblings remain readable. Rollover
target matching checks both generation and digest; automation's saved generation
is labeled separately. The status reader opens existing state without creating
directories or lock files. Independent review findings were corrected.

The earlier full-suite hang was reproduced twice under 15-second watchdogs:
the malformed-image test entered `CreateProcessW` and did not return. Both
failure-cleanup tests now inject a failed create at that boundary in test builds
only, retaining real empty-Job accounting, uncertain ownership and retry checks.
The release launch path is unchanged. Successful native launch, descendant and
handle-isolation tests also passed in the full regression run.

Verification: the full CatDesk run passed **1,476 tests / 28 ignored** before
the final target-identity reporting refinement and status-helper cleanup. The
final unchanged source then passed **53 focused Web/MCP tests**, strict
all-target/all-feature root Clippy, formatting, diff checks and build. This is
full regression evidence plus focused verification of the final refinements,
not a second full-suite run on the final commit. Exact scope, commands, source,
binary and log hashes: `docs/acceptance/web_status_tests_20261017.json`. Unchanged
Wake/Python suites were not rerun. Debug image SHA-256:
`13353C6445FAB987BA6E7467AAF84C3941A6C456AB7F5E2FDF6896676ED89D27`.

No installed activation, production target change or live rollover occurred.
GitHub fork workflow re-enable authorization is still pending; no remote CI
acceptance is claimed. The installed release/recovery and broker transaction
blockers described below remain open. The next source slice is the existing
broker's protected pointer switch after package publication, with a retained
ledger lease, one-attempt marker and exact reader verification. Owner/migration
acceptance and post-switch launch verification remain required. CatDesk's goal
is active; existing native project chats do not need to pause for this work.

## October 17 recurring rollover and connected review publication (TESTED_LOCAL_SOURCE; LIVE PARTIAL)

Source `aa1492c82d47e0edaedc936f26fc6d072b699829`, tree
`c14240c41f3b1636ffd38bb1547c7f88dae0eb9e`, adds two usable source paths:

- A continuation-only conversation can carry its original task into a later
  rollover using the saved handoff, exact completed continuation and unchanged
  native owner. A saved current handoff request retries its original event ID
  after interruption before publication. Ownership drift refuses a new request;
  an uncertain browser submission still cannot be resent.
- Supervisor MCP can publish one exact prepared queue review invitation to its
  registered project's paired Wake target. Current package/disclosure/binding
  checks precede a durable target intent and idempotent Wake event. Changed
  targets or evidence require reconciliation; bounded refusal messages identify
  missing prerequisites. Preparation checks Wake readiness before its first
  immutable write. Queue acceptance is separate from delivery, review and any
  next-task authority.

Verification passed 54 distinct focused tests: 33 Web/operator, 16 context/MCP,
four invitation tests and one exact Wake publication test. The refusal-classifier
test was also repeated separately. Independent review findings were fixed and
rereviewed; strict all-target/all-feature root Clippy, format, diff check and build
passed on unchanged source. Exact commands, hashes and retained limitations:
`docs/acceptance/web_continuation_tests_20261017.json`. Debug SHA-256:
`FED1DA8D3A4ADC2785C158EF9BC2C6AC9DF09D01C28CDE9AF19F9FCDF5913053`.
The full CatDesk suite was not rerun; its earlier native lifecycle hang remains
unresolved. Unchanged Wake/Python suites were not rerun in this checkpoint.

No installed activation, production target mutation, live review publication or
live rollover acceptance occurred. CatDesk's goal remains active. Existing
native chats need no CatDesk pause: BYOVD continued with a public-case report
feature (88 passed/one skipped), and Bug Bounty reconciled its full offline
regression (2,723 passed/30 skipped). Their work remains in their own chats and
checkouts. PortfolioDesk's October 19 observation window remains independent.

## October 17 authenticated GitHub Actions diagnosis (CI NOT ACCEPTED)

After push `c3c5ffdb37b80c496f5202a75006ebe4787ad189`, the exact commit had
zero Actions runs and zero check runs. The authenticated repository Actions
page then displayed **Workflows on this fork have been disabled**, explaining
that GitHub disables inherited fork workflows to avoid unnecessary compute.
It offers a repository workflow re-enable button. No setting was changed;
the user was asked for authorization because the prior investigation was
limited to read-only checks. The screenshot and visible page text are retained
privately under `CatDesk_v2/checkpoints/web-auto-oct17/github-actions-disabled.*`.

This observed UI restriction is distinct from the API's enabled repository
permissions and active workflow state, the historical reported HTTP 422, and
the missing `workflow_dispatch` trigger. The old 422 response body was not
preserved and was not recreated by another dispatch. The reviewed workflow has
push/pull-request triggers and three Windows test jobs. Exact-source CI still
requires a successful run after normal workflow operation is restored. No
GitHub Support escalation is established as necessary while the supported
repository re-enable action remains available.

Fresh live-path inspection also distinguishes missing implementation from
missing configuration. Normal `wake/install.ps1` and native installer entry
remain unconditionally disabled outside build/test support. A broker binary
exists, but its production service ends with
`WAKE_BROKER_TRANSACTION_GATES_UNAVAILABLE`; the approved install transaction
is not wired. Provisioning the service alone would not finish installation.
The main daemon's canonical binary hash differs from its manifest, and its
only recovery snapshot is `operational_verified`, which the current recovery
path rejects as reviewed release authority. The supported diagnosis returns
`RECOVERY_AUTHORITY` / `LKG_AUTHORITY_MISSING`. No manifest, recovery authority,
installation, service, or project target was changed during this inspection.
These are live activation blockers; they do not require pausing native project
development or the CatDesk source goal.

Evidence retention correction: a later helper invocation replaced the private
`web-auto-oct17/root-web-tests.log` with a newer sandbox-denied fixture run.
That one historical log no longer matches its hash in the earlier manifest;
the manifest is retained unchanged as historical evidence. Other earlier logs
were not overwritten. New source checks use `web-continuation-oct17` filenames.

## October 17 automatic Web driver checkpoint (TESTED_LOCAL_SOURCE; LIVE PARTIAL)

Source `270bba669270f94c65b6305892769d0dafdd4508`, tree
`a8e79fa327db33e06710f0c6a1a722f5d4b28145`. Verification: 46 focused CatDesk Web
tests (30 operator, 16 context/MCP), full Wake suite 288 passed/three ignored,
and 56 focused Python checks (38 adapter/bootstrap, six current-DOM, 12 response)
passed. The Wake installer-authority fixture initially failed in the sandbox;
the full Wake rerun with normal Windows process-handle access passed. Strict
all-target/all-feature root and Wake Clippy, formatting, diff checks and the
all-feature CatDesk build passed. Root lint findings were corrected before the
final rerun. Full CatDesk and full legacy Python suites were not rerun; their
earlier incomplete runs remain incomplete.

Exact commands, source/log/evidence hashes and limitations are recorded in
`docs/acceptance/web_auto_tests_20261017.json`. Debug image SHA-256:
`D4C2157843D6D72AA310DE93B9C97F2A379DC88CBF3CA1EE4179F3D0EF5A939A`.
Private logs and before/after screenshots are under
`CatDesk_v2/checkpoints/web-auto-oct17`. No remote CI pass is claimed by this
source manifest. Authenticated pre-push GitHub reads showed Actions enabled
and the workflow active; the latest visible run was still the October 14
success `38646210548` at `4e9a2e2`. This is distinct from the historical HTTP 422
report and the absent `workflow_dispatch` trigger. No dispatch or settings
change was attempted.

The current source connects per-project context capture, handoff collection and
rollover to a periodic production-service driver. Existing project Codex chats
retain their work and do not need to be paused. BYOVD received a coordination
correction: stale Git preconditions in old queue job prompts do not freeze its
native checkout under the user's later development instruction. Those old
queue jobs remain inactive, avoiding duplicate execution. Its existing native
chat then verified the advanced checkout and passed 29 public static-report
regressions, preserving its untracked helper; no other project's checkout was
changed by this CatDesk source task.

Implemented behavior:

- A 120-second driver schedules up to eight independent project workers and
  reports durable per-project status. `automaticObservationEnabled` and
  `automaticRolloverEnabled` are configured per project; both default false.
  Stopped/paused Wake prevents browser actions. Busy browser sessions are
  retryable and do not change another project's target or task state.
- WakeHost and operator browser tasks share one bounded browser-session lease.
  The running host must prove current store ownership and the new cooperative
  protocol. The replacement's issued URL and fresh exact USER receipt are saved
  as `RESPONSE_PENDING`; response probes only read that conversation. A crash
  with an unknown `SUBMITTING` outcome never permits a second submission.
- Automatic handoffs retain a stable source event-ID fingerprint, checked
  around collection and atomically with target mutation. A fresh source read
  before binding must still show the exact handoff request and saved answer.
  Pending or intervening work leaves the receipt and queue intact for
  reconciliation. Rollback refuses to abandon a staged generation that has
  acquired work. Other projects remain schedulable.
- A saved, proved Wake-first update can finish the interrupted registry write
  automatically. It must retain the exact completed receipt, adjacent target,
  old registry binding, handoff request and native owner/workspace. These are
  rechecked before binding; unrelated divergence requires reconciliation.
- Current ChatGPT semantic message units are supported without depending on
  the old author-role attributes. USER Copy controls and code-block Copy
  controls cannot prove assistant completion. Load errors and empty bodies
  remain incomplete.

### Controlled browser result and actual live gates

The existing replacement canary received one distinct durability diagnostic
and returned `CATDESK-DURABILITY-20261017-1 ACK.`. After one reload, the USER turn
and URL persisted, but the assistant showed `This response couldn’t load`.
No resend or regeneration followed. Exact production USER, turn-anchor and
role-count probes ran through CUA: two USER messages, anchor `ready/1/1`, and
role counts `2/2/6`. The count includes structural containers and is used only
to detect a populated home page. The full snapshot/completion expressions
could not execute through the available CUA DOM surface (`cloneNode` is not
exposed; raw CDP evaluation was unavailable). Those expressions passed offline
DOM fixtures, and their source bytes match the preserved probe hashes. This
does not establish live completion or installed Wake acceptance.

Normal-host readback still selects `1.0.0-dev.84-02431feba5ed-1c6bcdfcedf0`,
desired `STOPPED`, with no observed WakeHost process. Its sole CatDesk target
differs from the central registry. BYOVD and Bug Bounty have registered Web
URLs but no independent Wake targets; Wake2 has no Web URL. Normal installation
still requires the existing package/broker approval and exact Windows CI;
there is no supported full live bypass in the source-only developer lane.

Manual-file handoffs remain an explicit operator workflow; automatic execution
requires the collected source witness. A human can still type in the old chat
after the final snapshot because ChatGPT UI activity cannot be locked atomically
with the Wake store. The controlled rollover procedure must keep that source
conversation quiescent. Automatic refresh of a changed handoff remains a
reconciliation step. Three independently progressing live Web projects, actual
paired authority, durable rollover/continuation and continued native sessions
are still required for the October 31 milestone. Overall acceptance is PARTIAL.

## October 17 cooperative Web progress and handoff checkpoint (TESTED_LOCAL_SOURCE; MVP PARTIAL)

Source `e97d5f0847104ea4a6fe7db9d0526bab84257782`, tree
`60182a01787211d14b29f0e34446726f722cf94b`, adds these working source paths:

- Wake schedules one oldest eligible event per project per round. A fresh-document
  exact USER receipt commits delivery before waiting for the assistant. Durable
  pending timers reserve only their own conversation; short, read-only response
  probes allow other projects to receive work. Probe deadline is 90 seconds
  including adapter startup. Restart resumes observation without resending.
  An uncertain submission reserves its project even before an acceptance timer
  exists. Its recovery window starts at the durable submission boundary, not
  queue creation. Final receipt commit rechecks current target authority.
- Browser completion requires usable assistant content and assistant-scoped
  completion controls. Controls-only placeholders, load errors and a USER Copy
  button cannot complete an assistant timer. A current UI with no provable
  assistant-scoped control will remain pending; this needs live qualification.
- Context status persists explicit history, identity, text and overflow gaps.
  Recent excerpts remain `coverageKnown:false` and
  `pressureAssessment:UNKNOWN_OR_INCOMPLETE`. Token estimates describe observed
  content plus configured reserves, not a proven whole-chat upper bound.
  Repeated incomplete polls do not add synthetic token reserves or erase earlier
  observations. Optional `incompleteHistoryRollover` defaults false and requires
  actual capture, automatic rollover enabled and a quiescent point. Its trigger
  is allowed once per project lineage, preventing immediate repeated rollovers;
  subsequent observed token pressure can still request rollover.
- `operator web rollover handoff-request --project-id P --source-task-id T`
  publishes one durable, idempotent handoff request on the exact paired source.
  `operator web rollover handoff-collect --project-id P` requires its exact SENT
  receipt, completed timer and fresh matching latest USER/ASSISTANT JSON before
  preparing the existing rollover. It rechecks project, task, native owner and
  source binding, and rejects unverified native path ownership claims. Unchecked
  status proof is null; a matching collected record reports historical proof.
  These commands replace manually authored handoff JSON for this path. They are
  not yet automatically driven by the running host scheduler.

Verification: 35 focused Web tests passed (19 operator, 16 context/MCP); Wake
library 244 passed, one ignored; Python adapter 36 and bridge response 12 passed.
The Node DOM regression checks controls-only, USER Copy and assistant Copy cases.
Three disposable registry tests initially hit sandbox access denial and passed
with normal Windows access. Strict root/Wake Clippy, formatting, diff check and
all-feature CatDesk build passed. Exact commands, log hashes and source file hashes
are in `docs/acceptance/web_cooperative_tests_20261017.json`; private log copies are
under `CatDesk_v2/checkpoints/web-cooperative-oct17`. Debug image SHA-256:
`26F23A5861A44FDA9D0FF077A2196175A0B002F97A719A9989AD641DFE3AE8D8`.
The full CatDesk and legacy Python suites were not rerun; their prior incomplete
runs remain incomplete. No exact-source remote CI pass is claimed.

**Installed/live remains separate:** readback still shows
`1.0.0-dev.84-02431feba5ed-1c6bcdfcedf0`, desired `STOPPED`. No package activation,
production target change or fresh live Web delivery occurred in this checkpoint.
The earlier controlled browser canary and its response-load limitation below
remain the live evidence. Source capture and bootstrap still require the free
global host lease; bootstrap still waits for its response. Running-host capture,
handoff and cooperative bootstrap integration are the next source work, followed
by reviewed installation/binding and the genuine three-project rollover demo.

**Other project chats are free to resume.** CatDesk does not require the original
Bug Bounty, BYOVD, Wake2 or PortfolioDesk conversations paused for this work. Their
existing checkout/task ownership and PortfolioDesk observation-window constraints
remain in effect. No duplicate native project work was dispatched.

## October 17 combined ChatGPT Web P3/P4 checkpoint (SOURCE CHECKPOINT; LIVE PARTIAL)

The user's revised October 31 milestone requires per-project ChatGPT Web work,
context pressure, compaction, actual new-chat creation and task continuation.
Native Codex scheduling is supporting functionality. Earlier P4 deferrals below
are historical and superseded.

- Existing primitives reused: native threads/workdirs, central project registry,
  Wake target generations/history, durable event IDs and exact USER receipts,
  owned SeleniumBase browser, token estimator and existing install guards.
- New source: paired project target binding/history and exact-generation routing;
  advisory per-conversation context ledgers with configurable thresholds;
  bootstrap into a verified empty new conversation; actual issued-URL receipt;
  durable local rollover intent/handoff, per-project lock, no resend after an
  uncertain submit, paired CAS, and idempotent continuation publication.
  Source `6e4ed8c9cf49ee1f4cdc3e5261142da6e0101e57` (tree
  `35a882376bde0d5a66adae948af41e34078b02b5`) also connects the local operator
  commands, installed-package private source capture, qualified DOM identity
  hashes, streaming revisions and explicit excerpt uncertainty. Raw observed
  text is not stored in the context ledger or returned in public diagnostics.
  Wake-first binding recovery retains the exact source witness. A compensated
  higher generation remains explicitly reconciliation-required, with no second
  bootstrap. Advisory meter failure cannot withhold the continuation event.
- Verification at that source: 26 focused all-feature Web tests passed; Wake
  library 234 passed/one ignored, profile-probe unit tests three passed, and
  process-tree integration three passed. The native installer authority test
  passed after rerunning with its required process-handle access. Python
  adapter/bootstrap 27 passed and focused bridge response tests 12 passed.
  Strict all-target/all-feature Clippy, format, diff check and all-feature
  CatDesk build passed. Debug image SHA-256:
  `CFED6A064AFE575A8C4FD9216ACABE9D8FF9D098E223CD28F2CD30BF188DC92D`.
  The full CatDesk suite was stopped at the unchanged Windows lifecycle test
  `failed_creation_time_spawn_proves_empty_job_before_clean_failure` after it
  stopped progressing; only that exact owned test process was terminated.
  The full legacy Python bridge suite was also stopped in slow presentation
  fixtures. Neither broad run is accepted as passing. See the source-test
  manifest beside the controlled browser evidence for exact log hashes.
- Controlled browser evidence: predecessor
  `https://chatgpt.com/c/6ad40c82-6d80-8332-aa8c-2eeac9c4ea4a` generated a usable
  handoff; replacement
  `https://chatgpt.com/c/6ad40de5-5370-8327-91d1-5f355963a214` preserved its link and
  task ID and implemented the remaining synthetic task. Five local behavior
  checks passed. The computer-use driver performed this safe partial workflow;
  it is not an installed CatDesk/Wake receipt. Both chat URLs remain accessible.
  Source reload retained the original USER message but showed "This response
  couldn't load" for the assistant handoff. Full predecessor-history readback
  therefore remains unproven. Replacement reload showed the same assistant-body
  failure while retaining the exact USER handoff and distinct URL. No regeneration
  or resend was attempted; initial-response output and local behavior checks
  remain evidence of a partial handoff, not durable live rollover acceptance.
- Existing project development continued: BYOVD completed its offline HTML
  triage report (full regression 5,180 tests, 24 skips, zero failures/errors at
  `b5708e4`, clean documentation HEAD `e52625bf`); Bug Bounty completed its
  pinned owned-qualification runner (24 focused checks, real qualifier/checker
  exit 0 at `ce40137`, clean documentation HEAD `9d1efa1`). These are their own
  thread-reported gates, not new CatDesk verifier receipts. The isolated Wake2
  audit ran 11 tests: nine passed and two symlink tests skipped; its original active checkout
  remains untouched. PortfolioDesk's next admissible observation window remains
  October 19, 09:30–13:30 UTC; existing timers were preserved.
  After another authorized development turn, BYOVD additionally completed a
  public four-case comparison with 98 affected tests and repeatable CLI formats,
  clean HEAD `833839fb149c07e27fe03dc3b828657b4c3f8915`. Its qualified benign-control
  gate remains open. Bug Bounty's existing chat is running its next integrated
  regression/review; that in-progress run is not yet accepted. No duplicate
  CatDesk task was dispatched. Wake2's three isolated audit files also have a
  hashed copy under `CatDesk_v2/checkpoints/wake2-corpus-oct17`; its scoped Git
  checkpoint remains pending after two automatic approval-review timeouts.
- Installed/live gates remain open: dev84 lacks bootstrap capability; current
  registry and independent Wake CatDesk targets differ, other registered Web
  targets are not paired; no three-project Web delivery/rollover acceptance is
  claimed. One-shot bootstrap currently requires the free global Wake host
  lease, so running-host scheduling is still needed for independent progress.
  The one-shot source capture now feeds the advisory meter, but a late first
  capture of a long conversation can miss older content beyond its last-16
  excerpt and fixed unknown reserve. Whole-history coverage and periodic host
  capture are not accepted; this estimate must not be treated as proof that
  such a conversation is safely below its context limit. Automatic handoff
  extraction still requires integration. Shared MCP bearer
  does not authenticate which ChatGPT conversation originated a request.
- No claim-reconciliation bypass, protected install, production Wake rebind,
  workflow-dispatch retry, or remote CI acceptance was performed. The historical
  35 claims/85 accounting rows remain preserved. Exact current remote CI is
  unproven; earlier HTTP 422 and missing workflow_dispatch are separate facts.

Detailed source command/data contracts are in T-0484; controlled evidence is in
`docs/acceptance/web_rollover_canary_20261017.json`. Overall acceptance is PARTIAL.

### Remaining October 31 sequence

1. Extend the existing Wake scheduler to yield between response probes so a
   long ChatGPT response or rollover cannot block other project deliveries.
2. Connect periodic observations with explicit initial-history coverage and
   an automatic, task-bound handoff request/collection path.
3. Use the existing reviewed installation flow, reconcile project/Wake targets
   from current authority, and exercise the controlled replacement through that
   installed path. Preserve the observed ChatGPT response-load failure as an
   open live gate until fresh readback succeeds.
4. Demonstrate three actual project Web targets routing and continuing while
   one rolls over. Keep source, installed, delivery and native-session evidence
   separate; existing native project work continues during all stages.

## October 16 native queue and recovery checkpoint (TESTED_LOCAL; PARTIAL)

- Source-built CatDesk `9199ee9b51df927e853dbc5bb24e2a8949780647` (tree `77d7c65eada7eb3c591adc621741d15460565edb`) passed locked serial all-feature workspace tests: main 1,405 passed, zero failed, 28 ignored, and all enabled integration targets passed. Strict all-target/all-feature Clippy, format and all-feature build exited zero. Debug executable SHA-256: `7998251BC5C6405F3DA22CE1D17262968E0D280B3542AE3139FEDB3AE9D8F62B`; test log SHA-256: `FB8748A70BFA4E947D9365FCD0B81A5A4A09711A54ED4C3DB78F7655EF195CAB`.
- Queue job `BB-OCT16-OWNED-LOCAL-QUALIFICATION-2` resumed preserved original Bug Bounty thread `019fdd98-0f08-7032-a1d1-ad5c95472469` from clean source `1f694165614f424b8b6215f6b1b71c12df945586`. Receipt `be71b048-4d8f-4c94-8174-9cfa08c37535` records native exit zero and actual `gpt-6.1-sol`/medium; its candidate changes only three status Markdown files. A fresh private 127.0.0.1 owned SQLite fixture, qualifier and checker exited zero, and independent read-only audit matched six artifact hashes and all three candidate file hashes. This is a local PARTIAL result, not a public finding. Fixed locked/offline `queue verify` is still running; formal CatDesk receipt review and CatDesk verification remain pending. Earlier receipt `93ad9826-86cf-46b3-bed2-29cc67a0b693` remains unverified and was not replayed.
- BYOVD original chat goal is paused for handoff. Its clean HEAD `48ac1daa50570e9facd94bd98ba1a7ea8c88a51b` (tree `ba3cd754bdd9c2fe1311fddf70ae03db0a0ad2a3`) retained its original thread while guarded binding changed only fixed profile metadata to `python_uv_group_dev_touched_v2`, binding SHA-256 `b0192802fd0eb215c57629a04d7b874c90e40ff5417703cd51db3bd8f17f10f1`. Read-only preflight is `CANDIDATE_ONLY` with zero recorded blockers and clean Git. No BYOVD job has launched; its fixed verifier needs source repair before that first run. Preflight does not inspect 263 ignored `drivers/` paths or prove writer ownership.
- Independently reviewed `6fe5f6a1164ffae5b3c8242dfaaa46282b9007d0` (tree `18af098d1bf2bd329664aee01d35302dde4c7d51`) adds independent Codex CLI readiness to public diagnosis when canonical release identity fails. Focused PowerShell lifecycle tests and the committed-tree Rust recovery integration (5/5) passed. Actual read-only `catdesk.ps1 diagnose` returned `ACTION_REQUIRED`, primary `RECOVERY_AUTHORITY`, `CANONICAL_HASH_MISMATCH`, `LKG_AUTHORITY_MISSING`, and `CODEX_CLI READY`. A guarded fresh Temp fixture under bundled PowerShell passed four exit-zero public commands `HEALTHY -> DEGRADED -> CONNECTED_VERIFIED -> HEALTHY` with one fixture-only restart; result SHA-256 `57625d7640c6cbc746cb8b3c48a59a1ce7bac05c60279e007c5e63191c481606`. A separate Windows PowerShell replay grew unexpectedly and was stopped without a final result; it is not accepted evidence. Installed recovery still lacks reviewed rollback authority.
- The canonical queue has one exercised Bug Bounty path and a second BYOVD preflight candidate; BYOVD's fixed verifier needs source repair before its first job. CatDesk's own root remains blocked by 35 historical claims and 85 accounting review rows; its clean scratch checkout belongs to a separate registry. A third canonical project job, installed Wake/natural receipt and exact-source remote GitHub CI remain open. Overall October MVP remains `PARTIAL`.

## October 16 authenticated GitHub Actions follow-up (REMOTE_CI_UNACCEPTED)

- At the read-only October 16, 19:20 UTC audit, GitHub reported `orchestrator/chatgpt-codex-autonomous-loop` at exact pushed tip `696b66a7f1a19afe4afdef236b6154bdaa135a65`. The committed `.github/workflows/ci.yml` has unfiltered `push` and `pull_request` triggers and no `workflow_dispatch`; the absent manual trigger is separate from the absent push runs. Authenticated API readback reported repository Actions `enabled:true`, `allowed_actions:"all"`, workflow `311012404` `active`, and the repository neither disabled nor archived.
- Exact `696b66a` has zero Actions runs, zero check runs, and zero classic commit statuses. Repository-wide and workflow-specific run listings both stop at successful push run `38646210548` on October 14, 22:14 UTC for older `4e9a2e2`. A bounded branch-commit readback listed at least 20 newer remote commits dated October 16, 16:26–19:19 UTC; it does not establish 20 separate push events. The earlier manual-dispatch HTTP 422 stderr said Actions was disabled, but its raw body and headers were not retained. Enabled/active readbacks and the 422 text do not prove the cause of missing push runs or a current administrative disablement.
- Both billing read endpoints returned HTTP 404 with `X-Accepted-Oauth-Scopes: user`; the authenticated CLI has `repo` and `workflow` but not `user`, so billing/account restrictions remain unknown. No dispatch, OAuth scope refresh, setting change, or push was made during this audit. The owner should inspect the authenticated Actions page and account/billing notices. If GitHub confirms an administrative disablement despite enabled settings, escalate to GitHub Support with the 422 stderr, workflow ID, older successful run, exact-SHA absence, and enabled/active readbacks. Exact-source Windows CI remains unaccepted. See `docs/orchestrator/review_bundles/T-0485_R32_GITHUB_ACTIONS_READONLY_AUDIT.md`.

## October 16 scoped Python verification and Bug Bounty owner recovery (TESTED_LOCAL; PARTIAL)

- Exact combined source `3bba1967133bc07b1771ff31eff996d7f31f000c` (tree `e133290a1b53a3416f1ae5665cb79f4d04e25d87`) includes the reviewed, read-only workspace-bind diagnostic and two explicit Python touched-source v2 verification profiles. The diagnostic checks exact old/new Git roots, HEAD ancestry, origin, tracked and nonignored untracked state, ordinary tracked index flags, and fixed profile inputs; when those checks pass, it reports `BLOCKED_NATIVE_WRITER_RELEASE_UNPROVED`, `registrationMutation:false`, and `launchAuthorized:false`. Ignored entries are counted but their contents are unchecked. The v2 profiles keep locked offline full pytest and run Ruff format/check against every changed Python source named by the immutable review witness; deleted or renamed-away Python source, changed toolchain inputs, unsafe paths, or altered verification argv fail closed. Existing v1 profiles remain unchanged. Neither feature was used to change a live registration or run a queue job.
- With `TEMP` and `TMP` set to the ordinary sibling `CatDeskSuiteTempOct16` directory, exact `cargo test --locked --workspace --all-features --quiet -- --test-threads=1` exited 0: main 1,402 passed, zero failed, 28 ignored, and every enabled integration target passed. Two deliberately denied nested child probes printed `FAILED` inside the log; their enclosing tests passed. Local log: `C:\Users\Volap\OneDrive\Documents\ChatGPT\CatDeskSuiteTempOct16\catdesk-integrated-oct16.log`, SHA-256 `D32B190C978D6ADBAB11AE94F63C7DEBA39473DBC6426E59FF5AA6998FC5F7B9`. Strict all-target/all-feature Clippy, `cargo fmt --all -- --check`, `git diff --check`, and all-feature debug CatDesk build exited 0. Debug executable SHA-256: `AF858CA475B7FCD6A9A35264D351544BAB791C2FAB354D063BAEE963522658A5`.
- The original Bug Bounty app chat completed its owner handoff. An earlier exact CLI resume from its clean h1 checkpoint failed before a turn with `thread-store conflict: thread ... already has an active writer`. A later single supported normal-daemon, read-only resume of that **same** thread `019fdd98-0f08-7032-a1d1-ad5c95472469` through desktop Codex CLI `0.163.0-alpha.10.2` exited 0; app turn `01a1461c-2b67-7313-a8f7-8581b49e057a` completed in four seconds with a final answer only, no tools or edits. The clean h1 worktree remained at `362a9a79` (tree `0b894ed4`), and the old registered root remained at `a5d7de35` with 237 preserved untracked paths. The previous conflict's cause is unproven. This one-shot turn does not reserve the native writer or authorize a registration rebind.
- A guarded same-thread rebind was not implemented because CatDesk cannot retain a supported native thread ownership lease against independent app/CLI resumes. A new-thread successor transaction was also stopped before live use: retirement recorded only in CatDesk's control registry would not fence delegated runtime, resident tick, app-server continuity, Goal resume, and Wake paths that read an execution workspace's separate local registry or a title. A fresh detached fallback worktree was prepared at the reviewed Bug Bounty commit, but no second native thread, registration, job, dispatch, or project task was created. The old chat and both earlier checkouts remain preserved.
- Read-only preflight from this exact debug image still returns `BLOCKED` for CatDesk (35 historical claims, 85 accounting rows, nine preserved untracked paths) and for the registered Bug Bounty root (`rust_full` profile lacks its Rust inputs; 237 untracked paths). The BYOVD chat is still active and its registered root has new untracked work, so its quiescent owner handoff, profile bind and queue launch remain blocked. Queue status has `jobs:[]`. The historical claims lack original-owner/descendant-exit proof and were not cleared. Live three-project queue dispatch, installed Wake/natural receipt, protected recovery, and exact-source remote CI remain unaccepted. The user can run the authenticated CLI scratch edit/test/continuation already recorded below and inspect these read-only diagnostics today. Overall October MVP remains `PARTIAL`.

## October 16 integrated full-ID pins and scoped queue preflight (TESTED_LOCAL; PARTIAL)

- Source-tested CatDesk revision 96c46eedaaba1095f7644dee436266d41fbade8f (tree 9518130d8d81e2c18825d0b37495a7544df88884) integrates independently reviewed Wake V2 retained full file-identity pins and a read-only project queue preflight. The Wake inspection binds eight roles to legacy plus 64-bit volume and 128-bit file IDs from retained handles, including fixed preflight and policy handles. It still returns NO_INSTALL_AUTHORITY and has no install consumer. Queue preflight checks exact registration/thread, managed accounting, fixed profile inputs and a clean Git witness using a pinned first Git probe; it reports BLOCKED or CANDIDATE_ONLY with launchAuthorized:false and creates no queue job, lease, marker or native turn. Writer lock, native CLI, operator queue, owner handoff and ignored files remain outside that read-only report.
- With TEMP/TMP set to a dedicated ordinary directory outside Git and the Windows packaged LocalCache alias, exact integrated cargo test --locked --workspace --all-features --quiet -- --test-threads=1 exited 0: main 1,397 passed, zero failed and 28 ignored, with every enabled integration target passing, including the nested Wake store probe. Strict workspace all-target/all-feature Clippy, cargo fmt --all -- --check, all-feature CatDesk debug build and diff check exited 0. Debug executable SHA-256: 0071E98575C67E3F8B9B2A9AACE34FB474389DCE6720F773D3F09E0C1FCEA202. The earlier LocalCache probe failure was a lexical Windows path alias in the fixture; the exact focused probe and complete suite passed with an ordinary path.
- Read-only preflight of the registered CatDesk root returned BLOCKED: 35 historical queue claims, 85 accounting review rows and nine preserved untracked paths; profile inputs were tracked and valid. The BYOVD root was locally checkpointed clean at 715e784, but its registration still says rust_full, and the fixed Python profile currently fails baseline Ruff format (443 files) and lint (554 errors). The Bug Bounty bound chat yielded and seven reviewed text edits were locally checkpointed in its clean h1 worktree at 362a9a7; its registered root remains at a5d7de3 with 237 preserved Git-visible untracked files and rust_full registration. Its read-only preflight is BLOCKED on profile inputs and Git baseline. The operator queue remains empty. No actual registered three-project job, owner claim clearance, profile binding or workspace rebind is claimed.
- Human authorized the BYOVD and Bug Bounty chat takeover and scoped checkpoints. No independent owner/process record exists for CatDesk's 35 historical claims, so they remain in recovery. A guarded Bug Bounty workspace rebind would be needed to use a clean worktree without staging local donor and raw capture evidence; no rebind was made. Installed Wake, fresh natural receipt, real queue dispatch and exact-source remote CI acceptance remain open. Overall October MVP remains PARTIAL.

## October 16 integrated P3 handoff and read-only Wake V2 (TESTED_LOCAL_SOURCE; PARTIAL)

- At source `44ac9e43146b5a10c4885a04b5b9ec8f97d3c78c` (tree `4723a3c377fc1be632f98a0212d424a773b836ad`), the read-only P3 owner-handoff packet and Wake V2 signed-pair inspection are integrated. The locked serial root Rust suite passed (main 1,387 passed, 28 ignored; all enabled targets passed); strict root Clippy, format, debug build and diff check passed. Exact debug executable SHA-256: `3ED0F57688B04ACC6C9775D28FAA201401ACAED7CEDA2D3839F1148096156B78`.
- The isolated Wake full locked/offline suite passed (224 library tests, one ignored; all enabled targets passed), including seven focused V2 tests; strict Wake Clippy and format passed. V2 remains read-only and grants `NO_INSTALL_AUTHORITY`. See `docs/orchestrator/review_bundles/T-0485_R40_P3_OWNER_HANDOFF_READ_ONLY_20261016.md` and `docs/orchestrator/review_bundles/T-0485_R41_OFFLINE_V2_SIGNED_PAIR_SOURCE.md`.
- Read-only status reported authenticated `codex-cli 0.160.0` and `NEEDS_OWNER_RECOVERY` with 35 queue blockers and 85 accounting rows. The v4 connector's SSE route returned HTTP 404; no local daemon or listeners were observed. Public status reported `LKG_AUTHORITY_MISSING`, and diagnose reported `CANONICAL_HASH_MISMATCH`. Exact-source remote CI remains unproven; no workflow dispatch retry occurred. No installed acceptance, live project dispatch or live-state mutation follows. Overall MVP remains `PARTIAL`.

## October 16 integrated owner and invitation checkpoint (TESTED_LOCAL; PARTIAL)

- Exact pushed source `31d72faaa07e842c36c028012ffa7d806900376e` (tree `77454023f66bdff34749da0cbd7da193e7f37b1e`, CatDesk `0.1.6`) integrates independently reviewed read-only legacy Wake owner inventory (`5ca9694`), exact-child parent health attention handoff (`af02fa7`), and review-invitation chronology/consent corrections (`803d967`, `31d72fa`). The Wake diagnostic reports no matching named WakeHost process and two caller-view slots aliasing one physical root/lock; it cannot establish the historical connected owner. The parent health path sends a fixed failure command over the retained exact child pipe, waits up to 10 seconds, then stops/reaps through the retained Job; an uncertain marker still requires journal diagnosis. Invitation preparation uses one observed time to require package creation and a live disclosure grant before persisting a canonical create-once request. The request remains `PENDING`, `NOT_PUBLISHED`, and `wakeReady:false`; it is not an independent decision, Wake event, ACK, or next-task authorization. See `docs/orchestrator/review_bundles/T-0485_R39_FIRST_GENERATION_WAKE_MIGRATION_AUDIT.md` and `docs/orchestrator/review_bundles/T-0478_D7_PARENT_HEALTH_ATTENTION_HANDOFF.md`.
- Exact integrated `cargo test --locked --workspace --all-features --quiet -- --test-threads=1` exited 0: main binary 1,387 passed, zero failed, 28 ignored; every enabled integration target exited 0. The deliberately denied AppContainer child printed a failure while its enclosing test passed. Strict workspace all-target/all-feature Clippy, root and standalone Wake formatting, `git diff --check`, and all-feature CatDesk debug build exited 0. Debug executable SHA-256: `20F53FEF1C90F5EF1F7CBDE652E45BC4B48D61032914CC8B79AFC4B8DEE46758`. The full Python 3.12.14/pytest 9.1.1 run with isolated dependencies, bytecode disabled and pytest cache disabled reached 100% and exited 0 (215 collected, one skip); all 22 tracked Python files parsed. The isolated Wake diagnostic source passed its full standalone locked all-feature suite (217 library passed, one ignored; enabled targets passed), strict Clippy, formatting and build before cherry-pick. The integrated read-only diagnostic also built offline and exited 0 with `NO_MATCHING_NAMED_PROCESS_IN_SNAPSHOT`; its SHA-256 is `063260AB124F7D84623C93B8F0A9E095BEAD6E6FA009D47D91588F3B8AAC7650`. These are local source gates, not installed or remote CI acceptance.
- Read-only status from this exact CatDesk image reports authenticated `codex-cli 0.160.0`, `NEEDS_OWNER_RECOVERY`, 35 queue blockers, 85 accounting review rows, no pending writer marker and `launchAuthorized:false`; the operator queue remains empty. CatDesk, BYOVD and Bug Bounty remain distinct registrations, but their registered roots retain 9, 6 and 237 untracked paths respectively. The Python projects still use `rust_full`, and bound thread/worktree owner handoff is unproven. No real three-project job, claim reconciliation, project-profile change, disclosure grant, protected install, target mutation or new Wake receipt occurred. The direct authenticated Codex CLI scratch edit/test/continuation remains the demonstrated local CLI lane; actual registered-root dispatch is blocked.
- A source-bound, read-only owner handoff packet at `b3427e9` inventories the exact three registered roots, preserved file hashes, bound thread state, separate Bug Bounty patch, publication hazards and the required owner sequence. It identifies no current registered-workspace replacement command and authorizes no file move or dispatch. See `docs/orchestrator/review_bundles/T-0485_R40_P3_OWNER_HANDOFF_READ_ONLY_20261016.md`.
- The source push reached the feature branch. An immediate read-only exact-SHA GitHub query returned no Actions run and zero check runs, although repository Actions permissions read `enabled:true`/`allowed_actions:all` and `ci.yml` read `active`. The workflow has push/PR triggers but lacks `workflow_dispatch`; the historical manual HTTP 422 text, “Actions has been disabled for this repository,” is separate evidence. The raw 422 body was not retained, and billing/account restriction is unknown to this token. No dispatch retry or setting change was made. Exact-source remote CI and protected release acceptance remain open.
- Three-minute safe operator demo today: from the exact registered CatDesk root, run `target\debug\catdesk.exe codex status --project-id catdesk --workspace <that exact root>` and inspect authenticated status plus the explicit recovery block; run `target\debug\catdesk.exe codex recovery prepare --project-id catdesk --workspace <that exact root>` for candidate-only evidence; run the locally built read-only `wake\target\debug\CatDeskWakeLegacyOwnerAudit.exe` and compare its caller-view JSON with R39. For an edit/test/resume demonstration, use the previously approved clean scratch registration and checkpoint `24803f9`, not the blocked live control. Human action still needed: owner-reviewed claim and bound-thread handoffs, clean preserved project checkpoints/profile binding, and an independently reviewed protected Wake migration/release transaction. Overall October MVP: `PARTIAL`.

## October 16 bound review invitation and real-project readiness (TESTED_LOCAL; PARTIAL)

- At source `64f02c7e5ac350b5e77334816ec403349916179c` (tree `4caebd0414b2a2dd441dc8493386d61974c570dd`), an independently reviewed `codex queue review-invite-prepare` command persists a create-once, exact-package review request under current registration and live local disclosure. Independent review found and repaired a canonical-byte replay defect before integration. The record explicitly remains `independentReview=PENDING`, `wakePublication=NOT_PUBLISHED`, `wakeReady=false`; no installed Wake store, ChatGPT decision/ACK, event, or next-task authorization follows. See `docs/orchestrator/review_bundles/T-0485_R35_QUEUE_INVITATION_PREPARATION_SOURCE.md`.
- The exact integrated locked serial workspace Rust suite exited 0: main binary 1,384 passed, zero failed, 28 ignored; every enabled integration target passed. Strict workspace Clippy, formatting, diff check and all-feature CatDesk build exited 0. The debug image SHA-256 is `201F306E4B795B2D466BFF14F6A7AAB0997873CC4DA3C604627F8F75982152EC`. Read-only actual-control status from this image still reports authenticated `codex-cli 0.160.0`, 35 queue blockers, 85 accounting review rows, `NEEDS_OWNER_RECOVERY` and `launchAuthorized:false`; queue jobs remain empty. No actual invitation or job was created.
- The read-only three-project audit at `aa307fc` was checkpointed as `64caccc` and found distinct CatDesk, BYOVD and Bug Bounty registrations, but all three registered roots have preserved untracked work that the clean-Git launch guard refuses. Both Python projects still use `rust_full`, and their bound threads require owner handoff; CatDesk's historical claims require reviewed reconciliation. See `docs/orchestrator/review_bundles/T-0485_R34_THREE_REAL_PROJECT_READINESS_20261016.md`. No registration, profile, claim, workspace or target was changed.
- Immediate read-only GitHub checks after pushing source `64f02c7` returned no exact-commit Actions run and zero check runs. Repository Actions permissions and workflow state were enabled/active at the prior readback; billing or administrative restrictions are unproven. The prior manual HTTP 422 disabled-Actions stderr and absent `workflow_dispatch` trigger are separate observations. No workflow dispatch or settings change was made. Exact-source remote CI, actual three-project jobs, installed recovery, protected Wake, independent ChatGPT review and natural Wake receipt remain open. Overall October MVP is `PARTIAL`.

## October 16 integrated queue and candidate source (TESTED_LOCAL; PARTIAL)

- At tested source `911dc61575540b2f83ce636d93aa61911ceaef76` (tree `714dac1f9df25f9e13319895feda2ccc38c72178`), independently reviewed commits `1121b2f`, `5203109`, and `911dc61` added a bounded authenticated candidate MCP health watch, a clean-Git prelaunch queue gate that preserves the exact attempt-marker guard, and interactive exact-package disclosure renewal. The renewal does not create independent review or Wake authority. See `docs/orchestrator/review_bundles/T-0485_R33_QUEUE_REVIEW_CANDIDATE_SOURCE_INTEGRATION.md` for source identities and review limits.
- The exact integrated locked serial full workspace Rust suite exited 0: main binary 1,383 passed, zero failed, 28 ignored; all enabled integration targets passed. Strict all-target/all-feature Clippy, format, diff check, and all-feature CatDesk build exited 0. The debug image SHA-256 is `4AB3F948B2AE2862D331A8DFCEB25545C489B673E2EF25609C36079F1FD4C664`. The nested AppContainer denied child printed an expected failure while its enclosing test passed.
- Normal-access, read-only status from that image still reports authenticated `codex-cli 0.160.0`, `NEEDS_OWNER_RECOVERY`, 35 historical queue blockers, 85 accounting review rows and `launchAuthorized:false`; operator queue jobs remain empty. The nine preexisting untracked files remain untouched. No actual three-project job, installed candidate, protected Wake release, independent ChatGPT review, fresh Wake receipt or exact-source remote CI acceptance follows. The historical HTTP 422 Actions response is distinct from the CI workflow's missing manual-dispatch trigger; no workflow dispatch or settings change was made. Overall October MVP remains `PARTIAL`.

## October 16 typed protected Wake publication source (SOURCE ONLY; PARTIAL)

- Isolated `8d8fa4c` and `73256ed` were integrated as `56536c2` and `002c792` with identical source tree `eb21e11d341a8dce515227e9e2227e0a3a883323`. The fixed-broker source now binds a STAGED candidate to a create-once attempt marker, reopens and checks exact objects and the candidate owner/DACL before a handle-relative no-replace package rename, flushes both namespace parents, pins and checks the moved package, and writes a typed PUBLISHED ledger receipt. A failed or uncertain publication cannot be retried. Independent review found the candidate ACL gap, and final rereview passed after its fix; the drift fixture proves no move or PUBLISHED transition and sticky attention.
- At exact isolated `73256ed`, Wake format check, locked offline strict all-target/all-feature Clippy, release build and diff check exited 0. The locked offline all-feature Wake suite passed with normal filesystem authority: 217 library tests passed, one ignored; every enabled binary, integration and Python bridge target passed. An initial sandboxed temporary-root authority fixture returned `WAKE_STORE_AUTHORITY_OBJECT_UNAVAILABLE`; the same test and full suite passed outside that sandbox. At exact integrated `002c792`, serial locked all-feature root-workspace Cargo test exited 0, including enabled integration targets; no exact aggregate count is claimed. Strict all-target/all-feature root Clippy, root format check and all-feature CatDesk debug build exited 0. Exact-source dev.85 Wake `BuildOnly` exited 0, producing WakeHost SHA-256 `F3D4BB4411138A185AABF0B03A2471AAA64228DC7F0AEA08AD06C537A694A69A` and GUI SHA-256 `FA2F110030AD3864C0DAF268479DEFBFBC9DA8E9B8F44674695D9D737C628403`. This is local source and fixture evidence. The service remains gated by `WAKE_BROKER_TRANSACTION_GATES_UNAVAILABLE`; `wake/install.ps1` still refuses `WAKE_INSTALL_BROKER_UNPROVISIONED`. Read-only installed pointer evidence still selects dev.84 WakeHost SHA-256 `02431FEBA5ED73A5D40208B535D80CDC85C9A1C239058FAF029F0311E51E89A2`, with no fixed broker service or WakeHost process observed. No installed dev.85, protected owner/pointer cutover, fresh natural Wake/Chat53 receipt, actual three-project job or exact-source GitHub CI acceptance follows. Overall MVP remains `PARTIAL`. See `docs/orchestrator/review_bundles/T-0485_R31_TYPED_PROTECTED_PACKAGE_PUBLICATION_SOURCE.md`.

- Authenticated October 16 Actions readback found the repository enabled and CI workflow active, but zero runs/checks for remote tip `711192a`. The older October 14 successful push run cannot accept current source. The recorded manual HTTP 422 disabled-Actions stderr is distinct from the absent `workflow_dispatch` trigger; billing/account restrictions remain unknown. No dispatch or setting change occurred. See `docs/orchestrator/review_bundles/T-0485_R32_GITHUB_ACTIONS_READONLY_AUDIT.md`.

## October 16 exact-head local gates and live authority (TESTED_LOCAL; PARTIAL)

- At pushed branch HEAD `994827abbddd55bab1e99fb24f6dfd11fba2c2a3` (tree `9cc71cb178daad59872abb751a2364847fc65392`), `wake/install.ps1 -BuildOnly` exited 0 and built dev.85 WakeHost SHA-256 `C770A7B90150120D7011D51C9FA1C69D11B1910843C707C1CDBB635531AE8726` and a freshly rebuilt GUI SHA-256 `7E9A9FD80F7169939125EF642F8D7BA0CACC4E36CAED606D9C6B88836937BECC`. Standalone `wake/Cargo.toml` format check, locked offline strict all-target/all-feature Clippy, and locked offline serial all-target/all-feature tests exited 0 (213 library tests passed, one ignored; enabled targets passed). The installed pointer and dev.84 WakeHost hashes were unchanged at `25B8430DC25FDB7CF2BE3465D5F6C5431F6FF274D0DFCF3E09F9C2AA3CB50185` and `02431FEBA5ED73A5D40208B535D80CDC85C9A1C239058FAF029F0311E51E89A2`. No WakeHost process or fixed broker service was present; the installed host reported stale dev.59 `STOPPED`, queue zero and no receipt. Ordinary installation still refuses with `WAKE_INSTALL_BROKER_UNPROVISIONED`. This is a current-source build and local test result, not an installed dev.85 or natural Wake result.
- The Windows local Python gate used bundled Python 3.12.14 with CI-pinned pytest 9.1.1 in an isolated task directory. With `PYTHONDONTWRITEBYTECODE=1`, `PYTHONPATH` pointing to that directory, and the repository as working directory, `python -m pytest -q --tb=short -p no:cacheprovider` exited 0 after reaching 100%. Collection found 215 tests across six files; the run showed one skip and no failures. An independent syntax parse of all 22 tracked Python files passed. The extra cache suppression only avoids test-runner writes; it does not expand CI coverage. The Python run caused no tracked source changes. This local result does not replace the missing exact-commit GitHub Actions run.
- The public `catdesk.ps1 status` and read-only recovery plan returned `LKG_AUTHORITY_MISSING`; `diagnose` returned `ACTION_REQUIRED` with `CANONICAL_HASH_MISMATCH` and `LKG_AUTHORITY_MISSING`. The canonical release executable hashes to `9A8F4EBF6EA57CBB61EFAA12E8AFA54D18B8DB7374B9884067325BC84565568C`, while its sidecar names `A264C8DC85FA55311E74114158F4F4CD598F6E9E9D0A6CA535E14D18AA649939`. The sole matching escrow slot is marked `operational_verified`, which the current public recovery reader rejects as reviewed rollback authority. There is no interrupted promotion transaction or reviewed-promotion record, and no listener on ports 3200/3201. Direct reviewed promotion requires a valid prior canonical pair and a live canonical listener. A public recovery execution cannot be accepted from this state; an owner-reviewed bootstrap/reconciliation path is needed. No image, sidecar, escrow, tunnel or installed process was changed.
- Read-only registration audit found zero queued jobs. The actual CatDesk control still has 35 historical queue blockers and 85 accounting review rows. BYOVD has six untracked operator documents in its registered root; Bug Bounty has 237 untracked paths there, while its bound chat's latest turn used a different worktree with seven unstaged tracked files. Both Python registrations still say `rust_full` despite tracked manifests/locks matching their fixed Python profiles. A profile bind alone would not supply owner handoff or a clean initial Git witness. No real project was bound, cleaned, dispatched or resumed. At this HEAD, GitHub reports Actions enabled and CI active, but no exact-commit run or check; the prior manual HTTP 422 and absent `workflow_dispatch` trigger are separate observations. No workflow dispatch or settings change was made.

## October 16 read-only native recovery candidates (TESTED_LOCAL_SOURCE; PARTIAL)

- Integrated source `1d163fb6e0230c4464be43f31b2ab9b371c7835a` (tree `d27a775e3c07d822944246c16bea4e3e5ec8ff8b`) cherry-picks independently reviewed isolated source `6d68cd93443187132da4662b9113c997f8f58bf9`. `codex recovery prepare` now returns bounded, candidate-only native diagnostic metadata for an in-flight claim when the recorded session, current task/turn/handle, expected Codex thread, unique Codex accounting record and canonical project workspace match. Each file read is capped at 256 KiB, with a 4 MiB report budget and no-follow file checks; a same-user parent-directory replacement can still race this nontransactional read. Output includes the diagnostic content SHA-256, direct parent exit code and whether a thread-compatible `turn.completed` event was observed. It excludes raw diagnostic text, path, arguments, stderr and events. `codex recovery status`, durable queue/accounting state, existing gap codes/counts and `launchAuthorized:false` are unchanged; the report evidence digest excludes these auxiliary diagnostic files.
- At isolated source `6d68cd9`, the focused recovery suite passed 22/22; strict all-target/all-feature workspace Clippy, `cargo fmt --all -- --check`, and the all-feature CatDesk build passed. Independent source review passed the final commit.
- At exact integrated `1d163fb` (tree `d27a775e3c07d822944246c16bea4e3e5ec8ff8b`), serial `cargo test --locked --workspace --all-features --quiet -- --test-threads=1` exited 0: main 1,379 passed, 0 failed, 28 ignored, and all enabled integration targets passed. Strict workspace Clippy, `cargo fmt --all -- --check`, diff check, and `cargo build --locked --all-features --bin catdesk` exited 0. The integrated debug image SHA-256 was `92EDC3D09CBD4B00248202CF36BBE96F9221AF3EB25394EDE2705E68E1F80881`. Read-only Desktop `codex recovery prepare` from that image still returned `NEEDS_OWNER_RECOVERY`, 35 queue blockers, 85 accounting review rows, 35 claim rows and `launchAuthorized:false`: four exact candidates, 20 absent diagnostics, six without a current handle, three without exact thread binding and two with non-Codex handles. All four exact candidates had a recorded direct parent exit; three showed a compatible `turn.completed`. This is local source and diagnostic acceptance only; no installed recovery, claim clearance, actual project dispatch, protected Wake or exact-source remote CI is established.
- One read-only actual Desktop `codex recovery prepare` using the isolated built image SHA-256 `5E5752CE1AE5E4DCBA4F7030931C75958413DA57A6D5EEF1AA5000723A36783A` reported 35 queue blockers, 85 accounting review rows, 35 claim rows and `launchAuthorized:false`. Four claims had exact current-turn diagnostic candidates with a recorded direct parent exit; three of those showed a thread-compatible `turn.completed`. A separate read-only forensic inventory of 11 exact session/task diagnostic files across eight claims has content-set digest `7be0cef5b8774bf0fdfbdfb1c59cc9e658743ddec1df569dcbdedbcdce8a290d` (SHA-256 of sorted lowercase file hashes joined by LF; no raw diagnostic contents). The four are plausible owner-review candidates only. Owner PID/creation identity, descendant exit, ended accounting and an exclusive reviewed reconciliation remain unproven; no claim was cleared, no native turn launched, and overall October MVP remains `PARTIAL`.

## October 16 local queue operator review decision (SOURCE ONLY; PARTIAL)

- Integrated source commits `a6af161dafa99bbd4ea0cab09c81d4c0294b7403` and `46912f7eec01d1002f4bcfbf053e7a041960e5d2` add `codex queue operator-review --project-id <id> --workspace <registered root> --receipt-id <uuid> --expected-package-sha256 <lowercase hash> --decision ACCEPT|REJECT|INCONCLUSIVE`, plus `codex queue operator-review-receipt --receipt-id <uuid>`. The first route requires a local interactive terminal and an exact decision/receipt/package-hash challenge with a 256-byte input limit. It records a create-once `OPERATOR_REVIEW_ONLY` decision about the immutable, locally verified captured package, even if the source worktree later changes. It validates receipt→candidate→verification→package, current registered project/target binding and package SHA. Receipt→queue→registry locks hold that comparison through create-once publication and readback; corrupted, partial, conflicting or drifted records fail closed. The package stays `independentReview=PENDING`, `wakeReady:false`; no ChatGPT independent decision, inbox event, Wake or next-task authority is granted.
- Initial independent review found a target-registry race between the terminal check and publication; the second commit closed it and independent rereview passed. Focused isolated tests passed 6/6, including no-terminal refusal, long input, busy registry, real target CAS drift, replay and corruption. At exact integrated `46912f7` (tree `d4803ae5992c82470d7ba4bc2dc6a37128bb7fd6`), serial `cargo test --locked --workspace --all-features --quiet -- --test-threads=1` exited 0: main 1,374 passed, 0 failed, 28 ignored, with every enabled integration target passing. Strict workspace Clippy, format check, diff check and `cargo build --locked --all-features --bin catdesk` exited 0. Debug image SHA-256: `4DFD465EA8C42D7B6A9EF453AB169EF4DC792F7B0C187F80580B5F032E4917A1`. A single read-only post-push check for exact source SHA `46912f7` returned `gh run list=[]` and zero check runs; repository Actions permission and workflow state were not changed. The earlier manual HTTP 422 disabled-Actions wording and missing `workflow_dispatch` trigger remain separate observations. This is source and disposable-fixture testing only; no real operator decision or installed route was exercised.
- At the final built image SHA-256 `4DFD465EA8C42D7B6A9EF453AB169EF4DC792F7B0C187F80580B5F032E4917A1`, read-only `codex status` against the actual Desktop control returned authenticated `codex-cli 0.160.0`, bound thread `019fe33e-5c82-7540-a41c-9ae497527e57`, `NEEDS_OWNER_RECOVERY`, 35 historical queue blockers, 85 accounting review rows, no pending writer marker and `launchAuthorized:false`; `codex queue status` returned `jobs:[]`. No owner reconciliation was performed. Three actual project jobs, installed recovery, independent review, protected Wake and exact-source remote CI remain open. A read-only Wake audit found no WakeHost process or fixed broker service; the legacy pointer still selects a hash-matching dev.84 host, while the source-built dev.59 status uses a different store. No protected install, target change, queue dispatch or workflow dispatch was made. Overall October MVP remains `PARTIAL`.
- At about 09:34 ET, bounded read-only CatDesk v4 connector calls reported `UNAVAILABLE`: transport and stable-supervisor status each returned MCP SSE probe HTTP 429, while provider status returned HTTP 404. These codes do not identify the failing gateway layer or an installed serving process. No tunnel, setting or process was changed.

## October 16 current-source authenticated scratch continuation (TESTED_LOCAL; PARTIAL)

- Source HEAD was `d51275438c815a72e3dcdb850a103ee883410d03` (only documentation changed since the source-built `0cafc07` image). The exact local `target/debug/catdesk.exe` SHA-256 was `8F22C6F6BB6026E4A811DEA366B80C88ED1D8BC43E20153CC7AD6110D696741E`; native `codex-cli 0.160.0` reported authenticated. From the previously approved, clean scratch Git checkout `C:\Users\Volap\OneDrive\Desktop\Projects\CatDesk-codex-native-cli-canary` at `3d4c612`, `catdesk.exe codex status --project-id catdesk --workspace <that exact root>` returned bound thread `01a14225-fc94-7d61-a6d5-b1733e8886d8`, zero recorded queue/accounting blockers and no pending writer marker. The command must run with that scratch checkout as its working directory so CatDesk selects its scratch registration.
- One actual `catdesk.exe codex run` in that bound thread requested `gpt-6.1-sol` at medium effort, workspace-write and approval `never`, and exited 0. Codex added only `turn_profile_evidence_enforces_length_and_effort_boundaries` in `src/delegated/codex_model_policy.rs`: a 128-byte ASCII turn ID and 64-character hex hash pass; a 129-byte ID, 63/65-character hashes and wrong effort fail. Its requested focused `cargo test --locked --bin catdesk turn_profile_evidence_enforces_length_and_effort_boundaries` exited 0 (1 passed). Independent rerun of all four model-policy tests exited 0 (4 passed), and an independent read-only reviewer passed the 29-line test-only diff, format and diff checks. The scratch branch was checkpointed clean at `24803f9d630d6ab7401a201acf94ff3adcebf2ec`; this is a fourth completed turn in the preserved native thread, not a new source-branch production change. Post-turn status remained authenticated with the same thread, zero recorded blockers and no pending marker. The status field `launchAuthorized:false` is a read-only diagnostic and does not by itself grant or deny a later turn.
- This is current-source local CatDesk CLI continuation evidence. It does not reconcile the actual Desktop root's 35 historical claims and 85 accounting rows, deliver three actual project jobs, establish an installed route or recovery, produce an independent review/Wake receipt, or replace exact-source GitHub CI. No installed service, Wake target, live project queue or other active project root was changed. Overall October MVP remains `PARTIAL`.

## October 16 MCP tools/call fixture (SOURCE ONLY; PARTIAL)

- Integrated commit `0cafc07e218a7c14920d1e0b24b29f018fd25d6b` cherry-picks independently reviewed `1733467`. A disposable queue fixture invokes the actual MCP `tools/call` handler with an injected fixture reader. It asserts generic denial without a grant, denial outside SupervisorOnly, then prompt and metadata readback after a fixture grant, denial after revocation, refusal to re-grant and byte-for-byte unchanged queue job/package files. Fixture grant setup directly uses the test store and does not exercise a local terminal challenge or the current user's queue.
- Isolated focused handler regression passed 1/1; seven adjacent reader tests passed 7/7. Isolated strict Clippy and format passed. Exact integrated serial `cargo test --locked --workspace --all-features --quiet -- --test-threads=1` exited 0 (main 1,368 passed, 0 failed, 28 ignored; every enabled integration target passed). Strict integrated `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings`, `cargo fmt --all -- --check`, and `cargo build --locked --all-features --bin catdesk` exited 0. Debug image SHA-256: `8F22C6F6BB6026E4A811DEA366B80C88ED1D8BC43E20153CC7AD6110D696741E`. These results are source/local fixture evidence only: no HTTP or tunnel call, installed reader, live disclosure grant, independent review decision or Wake approval is claimed. Overall acceptance remains `PARTIAL`.
- Read-only final integrated binary status remains authenticated but `NEEDS_OWNER_RECOVERY`: 35 historical queue claims, 85 accounting rows and `launchAuthorized:false`. Independent recovery audit found all 35 missing recorded owner identity, source-bound child-exit proof and ended accounting, with 13 open spans and 27 incomplete profiles. Existing `recovery.rs` reports the gaps; no reset or clear was performed.

## October 16 MCP review-package reader (SOURCE ONLY; PARTIAL)

- Source `5933b146113531309401b51d17615d46eb11152c` integrates the receipt-bound reader after independent review passed on `63304125`. The direct local operator grant command is `catdesk.exe codex queue review-disclosure-grant --project-id <id> --workspace <registered root> --receipt-id <uuid> --expected-package-sha256 <lowercase hash>`; an interactive terminal then requires `DISCLOSE <uuid>`. The revoke command is `catdesk.exe codex queue review-disclosure-revoke --project-id <id> --receipt-id <uuid> --expected-package-sha256 <lowercase hash>`, with no `--workspace`, followed by `REVOKE <uuid>`. The create-once grant lasts one hour by the local system clock; a create-once revocation tombstone permanently denies this receipt's later reads and re-grants.
- Shared MCP tool `catdesk_codex_queue_review_package_read` is exposed only with SupervisorOnly in Computer/Both mode. It requires exact project ID, canonical receipt UUID and lowercase package SHA-256, revalidates current registration and the full persisted evidence chain, and returns 8 KiB content pages or bounded metadata. Public denials are generic. The grant relies on same-user local file trust and does not identify the calling ChatGPT conversation. Reading evidence is not an independent review, inbox event, Wake approval or launch authority; package state remains `independentReview=PENDING` and `wakeReady:false`.
- Merged focused queue tests passed 43/43. The exact integrated serial `cargo test --locked --workspace --all-features --quiet -- --test-threads=1` exited 0: main 1,367 passed, 0 failed, 28 ignored, with every enabled integration target passing. Strict integrated `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings`, `cargo fmt --all -- --check`, and `cargo build --locked --all-features --bin catdesk` exited 0. The debug image SHA-256 is `BE831403C88926206F277B2D649E6A7BA56C77D5C77CCBCD88EB5216177E058A`. Exact-source remote CI acceptance remains open. The feature is source-only: no installed reader or real disclosure grant was used, and the live Desktop owner, installed Wake and three-project acceptance gates remain open.

## October 16 d6b queued prompt checkpoint (TESTED_LOCAL_SOURCE; PARTIAL)

- Exact integrated commit `d6b9a36de985b22a0ee508cc40ed1a5a89639ac0` (tree `b021f7b4b511d79bd4b598d10e4a5e82f781c447`) gives queued native Codex prompt delivery a bounded write/flush and an explicit stdin close. Failure preserves UNKNOWN writer/lease state for owner recovery; it never infers child exit or replays an uncertain attempt. Focused queue tests passed 39/39 and independent source review passed. The exact committed source passed serial all-feature Rust (main 1,360/0/28 plus all enabled integrations). Strict Clippy/format passed on the isolated source; the d6b debug build SHA-256 is `CF04792A4D842C09F6F5054E84092A4A3F77005454B924451EBE6F9CC2825637`.
- The actual Desktop root read-only `codex status` remains authenticated with `codex-cli 0.160.0`, but `NEEDS_OWNER_RECOVERY` holds 35 historical queue blockers and 85 accounting rows; `launchAuthorized:false`. This source checkpoint does not establish an authorized live Desktop turn, three-project deliverable review, installed recovery or Wake receipt.
- Read-only GitHub inspection found zero workflow runs/checks for d6b. Actions permissions report enabled/all and the CI workflow is active for push and PR, with no `workflow_dispatch` trigger. The earlier HTTP 422 disabled-Actions response is a distinct observation; its raw body was not retained. Billing/account restrictions remain unknown under the current token. No dispatch retry, settings change, protected install, Wake action or live queue mutation was made. Exact-source remote CI and overall MVP acceptance remain `PARTIAL`.

**Evidence snapshot:** 2026-10-16, including the receipt-bound local verifier, fixed Python verification profiles, a private advisory reviewer route, guarded profile binding, and disposable native continuations below (America/New_York). **Overall: `PARTIAL`.** This is an interim October 31 working-loop record; it is not installed, live three-project, Wake, or remote CI acceptance. Historical rows below retain their October 15 source identities.

## October 16 source and local verification continuation

- Integrated source commit `ec428e1` cherry-picks tested source `793967513340916f6545fc307433ba33e0d29196` (tree `31901d8f4f93387b2e8010cb2e6f2fcf8223461a`). It adds `codex queue verify --receipt-id`, a create-once receipt-bound source snapshot covering tracked/staged patches and changed file bytes, fixed `rust_full` commands with `--locked`, bounded command exit/output evidence, and exact resumed native model/effort readback. Review remains `PENDING`, and Wake readiness remains false. Historical review candidates without the new index witness remain readable but cannot be verified by this slice.
- The source worktree passed `cargo test --workspace --all-features --quiet` (main binary 1,314 passed, 0 failed, 28 ignored; all enabled integration targets passed), strict all-target Clippy, format check, and `cargo build --all-features --bin catdesk`. The final candidate-consistency regression landed after the full-suite compile and passed in the final focused queue suite, 23/23. `git diff --cached --check` passed before commit. The exact-source binary SHA-256 was `2BA73DE47E4F7FEB9B556CFB3FBFBC83A7B71725225043265B271860A0F00126`. The two changed source blobs match exactly between the tested branch and the integrated commit.
- Integrated commit `1afab98` cherry-picks tested source `895a2ed5f25a69ed8cc46ff3233f08d062e100a6` (tree `4c86e6d4ec4f91b153ba3dd97ab74cadcb8263a3`). It adds fixed `python_uv_group_dev_v1` for BYOVD and `python_uv_extras_dev_lab_v1` for Bug Bounty. Both require tracked `pyproject.toml`/`uv.lock`, a matching package/dependency selection, and pinned `uv run --locked --offline --no-python-downloads --no-env-file --no-config` commands. Ruff and pytest are fixed per project profile, with direct parent exit, executable/environment digest, and source recheck. Rust verification remains available. The integrated `queue.rs` blob `ab210e1166439f9d5e33f23eb16f8c8f8b69ed49` matches the fully tested branch. The queue suite passed 28/28; the full locked workspace test passed (main binary 1,320 passed, 0 failed, 28 ignored; all enabled integration targets passed), as did strict all-target Clippy, format, diff check, and `cargo build --locked --all-features --bin catdesk`. The tested binary SHA-256 was `F1CE2845250BF9B485E5C16E31B279F14AA6C77737E421D00499DA24352AE21C`. Installed `uv 0.12.23` accepted both command shapes in a dependency-free offline scratch smoke. This does not prove Ruff/pytest availability in the registered projects or their actual lock readiness.
- Integrated commit `4d9a39e` cherry-picks independently reviewed package source `d2c67b45a6b2d236ff6a7cdf26665b2426437583` (tree `7b42934ec14d2205e071708b4aec8dcf9bfe61c7`). `codex queue review-package --receipt-id` creates one private receipt-bound package of the bounded task prompt, validated receipt/candidate, source patches and file bytes, effective worker profile, and fixed local command outcomes. Its CLI summary omits the prompt. A surviving unbound package after interrupted publication moves the job to `ATTENTION`; tampered or failed verification cannot package. Historical readback is idempotent. The integrated `queue.rs` blob `2cb32d1976ac86e28f966b486e0059ae4befe956` matches the tested branch. The source branch passed 31/31 focused queue tests, strict Clippy, format, build, and a serial locked full workspace test (main 1,323 passed, 0 failed, 28 ignored; other enabled targets passed). An earlier concurrent full run failed a nested Wake installer-store lock test while another suite was running; the serial rerun passed. Package state is `independentReview=PENDING`, `wakeReady=false` and grants no reviewer or Wake authority.
- At the combined integrated source commit `4d9a39eaa068ff15ab2c3ca251f54a0f15303105` (tree `d6c8cd59546c066876b736aa44613aedc2542479`), `cargo test --locked --workspace --all-features --quiet` exited 0: main binary 1,333 passed, 0 failed, 28 ignored, with all enabled integration targets passing. Strict locked all-target Clippy, `cargo fmt --all -- --check`, `git diff --check`, and `cargo build --locked --all-features --bin catdesk` exited 0. The combined image SHA-256 is `61409F1AB3B5E9E307A13DDD4E953618C9F65977943B0C727939EF887DB7C60B`. The nested AppContainer negative helper prints an expected child failure while its enclosing test and top-level suite pass.
- Integrated commit `7b7e0fb` cherry-picks independently source-reviewed advisory route `f8c3c5263300a2dfd8c7673a9ac6f8bbfa1e8b84` (tree `5677b96bdc4559ca7c31307394f03db86fac998a`). `codex queue advisory-review --receipt-id` uses a create-once private copy of the verified package, a bounded same-user fresh native Codex reviewer turn, and a job-anchored advisory outcome. It binds package/request and physical file identities, one completed reviewer thread/turn, effective `gpt-6.1-sol`/`medium`, executable and stream digests, and direct child exit. Orphaned or ambiguous attempts require attention without replay. CLI output omits private reviewer text. This is an advisory opinion: `independentReview=PENDING`, `wakeReady=false`; model text cannot authorize ChatGPT review or Wake. The source branch passed 37/37 focused queue tests, a fresh-rollout test, strict Clippy, formatting, and diff checks; no native reviewer was launched on that branch.
- Integrated commits `22007f0` and `97c0cba` cherry-pick reviewed profile-binding source `3807a0bd5eccf10fed884b0dda30d3fc36727a4b` and repair `f173ae4cd52e36ed361162220047551e86d85a47` (final tree `0e49b1972b24604a2d9c90ba89d18d453720d831`). `codex queue profile-bind` can change a quiescent registered project's `rust_full` metadata to one exact fixed Python profile under queue, writer, scheduler, and registration locks, with full binding compare-and-swap, zero project jobs/leases, clean tracked manifest/lock and hidden Git index-bit refusal. It requires an explicit assertion naming the bound thread, but this is an **unverified operator assertion**, not proof that the chat is idle or owned by the caller. No BYOVD or Bug Bounty live registration was changed. The repaired source branch passed 34/34 focused queue tests, Cargo check, formatting, and diff checks; the independent reviewer signed it for a source-only, manual-gated path.
- At integrated commit `97c0cba4b7787076e40872b27fa1d1c1fd07e612` (tree `d7528c3ed1547bfcb2c41a96e34b956e6ba17f08`), a separate merge-resolution audit found no lost guard or parser branch. The combined focused queue suite passed 40/40. `cargo test --locked --workspace --all-features --quiet` exited 0: main binary 1,343 passed, 0 failed, 28 ignored, and all enabled integration targets passed. Strict locked all-target Clippy, `cargo fmt --all -- --check`, `git diff --check`, and `cargo build --locked --all-features --bin catdesk` exited 0. The exact local image SHA-256 is `FDBB8187A1E893D98EBFC58CE1B39F55B77BCD9474D27CB320F0A5FEE1B7399B`. The nested AppContainer negative child again printed its expected access-denied failure while the enclosing test and top-level Cargo run passed.
- A single-use disposable fixture under `%LOCALAPPDATA%\Temp\catdesk-queue-native-proof-20261016-982a1c6e` used registered `fixture_project`, a clean committed Rust project, and bound thread `01a142e9-e06a-78c2-a4b9-3e143ebf2c4e`. Its sealed preparation SHA-256 is `0A831DB208BD16768C5AFFF6490DFBB8D64F28867337EA5AE4B35A22E51265B4`. One `add` and one `dispatch` produced process receipt `f3bdaed5-e77d-46f3-adee-2892208890e9` with observed exit zero and native rollout readback `gpt-6.1-sol`/`medium`. The native turn changed only tracked `src/lib.rs` and new `src/proof.rs`; their captured and current SHA-256 values are `071FE38047263C6775F6F3F5C052315EC4970F0387ACB17476C0CFA18E5F2039` and `DD78A78F6165760782C415B4143D78C06F7834CAC728BCCD00CD7F9BA50CD892`. Control Git remained clean, both fixture HEADs and registry hash stayed fixed, and no scheduler lease remained.
- The verifier persisted artifact SHA-256 `418B03E09C2CB64B6AF8FF925F5E0454468E38B0AF9BA6F58FDCF61F0F04A7D4`, bound to receipt SHA-256 `A8C1536D4DA3574511A58B1C96EEACACCE42A48BC82F90EE06B5111D414D0DAD` and review candidate SHA-256 `E8E33DD9835DFFD93211D2432F2A879578A0D2BC5A2571B399EAE3FE86A86D90`. `cargo fmt`, strict Clippy, test, and build each recorded direct parent exit zero; artifact status is `LOCAL_COMMANDS_EXITED_ZERO_SOURCE_RECHECKED`, `independentReview=PENDING`, `wakeReady=false`. An idempotent `queue verify` readback returned the same artifact digest without another native dispatch.
- The combined `4d9a39e` image packaged that same disposable native receipt without another dispatch. The create-once package is 11,525 bytes, SHA-256 `43FBC4DB60B6157D0699EA02143652359CB9F179438FC26540E874AC71DD663B`. A second CLI readback returned `idempotent:true` and the same digest. Its private prompt is 872 UTF-8 bytes and absent from the CLI summary; `independentReview=PENDING`, `wakeReady=false`. A separate read-only reviewer checked the package against the receipt, candidate, verification artifact, Git witness and current fixture bytes and reported PASS for the bounded fixture edit. That reviewer did not rerun tests or authenticate a fresh native reviewer turn. No product review attestation was published; the package remains `PENDING` and Wake-ineligible.
- One native `codex queue advisory-review` invocation against receipt `f3bdaed5-e77d-46f3-adee-2892208890e9` created attempt `aba9f99b-4abe-4e31-a2c7-4a822ca6df8d` and its private package copy, then reached the bounded timeout with exit 2 and `CODEX_QUEUE_ADVISORY_ATTENTION: reviewer exit is unknown; descendants unproven`. No advisory outcome was published; both CatDesk and reviewer processes subsequently exited. The attempt marker remains `ATTEMPT_STARTED` and must not be replayed. Independent source analysis found that Windows Tokio `ChildStdin::shutdown()` did not close the stdin pipe before awaiting `codex exec -`; the native child waited for EOF. Commit `7889d37` integrates independently reviewed source `137210e8` (tree `de5dedef6e1334500980c27ef09803e11398756a`) that explicitly drops stdin before waiting and adds a subprocess EOF regression test. This is a source repair, not a successful native advisory result; a new disposable receipt is required for that proof.
- The first full-suite run of the repaired tree found a preexisting synthetic fresh-rollout test fixture race: on this Windows volume, a file created immediately after the test's captured launch clock sometimes had an NTFS creation timestamp 366 microseconds earlier. The production `created() >= launched_at` guard correctly rejected it. Test-only commit `5e1292f` cherry-picks independently reviewed `d4d47bb` and puts a 20 ms gap before fixture creation, then asserts the required ordering; the production guard is unchanged. At integrated `5e1292f9ba512a3948de52c1da152ec813a3877b` (tree `ff3272d989b7dba1b27168f6d84f2eb354971b3c`), the full locked workspace test exited 0: main binary 1,344 passed, 0 failed, 28 ignored, and all enabled integration targets passed. Strict locked all-target Clippy, format, diff check, and `cargo build --locked --all-features --bin catdesk` exited 0. The exact new image SHA-256 is `F99D67569050545DDED33B01970AFA85EA61AEDDF2181AF073A6BB51E0682586`. The nested AppContainer negative child printed its expected access-denied failure while its enclosing test and top-level Cargo run passed.
- A second, separately sealed disposable fixture under `%LOCALAPPDATA%\Temp\catdesk-queue-advisory-20261016-5a12683bd5b0` ran the exact `5e1292f` image once with runner SHA-256 `00857AE55E9AFA35A300250C878FF393E6F407A7BC539D054BBCA365AB988527`. Its result SHA-256 is `7EDC5A8BDE5E081EDF923829B386209DAE8F353B87CA230EA64BC3FA8AF3510F`; the one-shot claim predates bootstrap, and bootstrap/add/dispatch/verify/package/advisory call counters are each one. Fresh receipt `2c1dc398-95e0-4855-93dd-33a5b7335432` has observed worker exit zero, effective `gpt-6.1-sol`/`medium`, a released lease, and only the requested `src/lib.rs`/`src/proof.rs` change. Four fixed Cargo verification commands exited zero. Independently audited persisted SHA-256 values bind receipt `DD89842BC17218E69CE85EFA0AAA7247E11A742D9E8CAD49824431630AF84104`, candidate `9658D5CC178422857431E0E0EF52986D5E34E870317F210C094D3207C2DFAEAE`, verification `5ADB3DADE7C31ECBDDC98BB38B816B18819F6EF65DBEF777E4611F7450A9AFB9`, and private package `D527CD9DA9A3888F13EF0D856B733CDEB4635359388D596D7E11C60AE40640EB`. A distinct fresh reviewer thread and turn also used Sol/medium and exited zero, publishing advisory outcome SHA-256 `1254C6960949817D7DAA8F278E9E513E7BC0E0138B1EE6EB728A93AA806FB921` with verdict `INCONCLUSIVE`: its read-only sandbox rejected `exec_command` at process creation before it could inspect the private package. This is a completed *process* and an access failure, not a substantive review of the edit. `independentReview=PENDING`, `wakeReady=false`; this receipt is single-use and must not be replayed.
- The one-shot runner itself exited `CANARY_ARTIFACT_PATH_ESCAPE` **after** artifact publication: Windows exposed the same digest-bearing queue file through ordinary LocalAppData and a packaged `OpenAI.Codex` LocalCache path, while the runner required the first spelling. Both paths are 6,228 bytes with the same SHA-256, UTC write time, and Windows file ID `0x0000000000000000000200000112dfd3`; filesystem canonicalization of either resolves to the packaged path. This is a runner lexical-path assertion defect, not a second queue artifact. Runner SHA-256 is `717FF69A2BE01AB75B236E5EF80A51625D30DE3077240E74C24C48F30F5990E2`; its aborted progress record SHA-256 is `49D0BDAA8457FBE3A7753B26F469B326514AA38FF34B86E29820A23669852CB5`. The runner did not publish a `LOCAL_CANARY_PASS` result. Independent read-only audit and manual artifact/readback checks establish the narrower product facts above; this is a fixture result, not a completed harness result or live-project acceptance. The single-use claim prevents replaying the native task. A future runner should compare handle-backed canonical identities for the expected and returned artifact paths before checking content hashes.
- Integrated commit `e33b72a` cherry-picks independently source-reviewed `cfb1bde` (tree `e35900cc5e8c86411f9704bcac7e27988337e843`) to embed the exact verified UTF-8 review-package JSON in a v2 native reviewer request. It caps the raw package at 64 KiB before creating a reviewer directory or attempt, preserves byte-for-byte v1 prompt/readback and old no-replay behavior, and fixes the v2 no-findings output contract. Integrated `82ba2bb` cherry-picks independently source-reviewed `67a9c10` (tree `ba0aa5fff039796e861aa44c6c0e2694968a4cf8`) to pass `--disable shell_tool` while retaining read-only sandbox and approval `never`; the local CLI accepted that fixed argument preamble and reported `shell_tool=false`. This is a command-tool gate, not all-tools isolation. The full private package is included in the native request and retained in the user's Codex rollout under `CODEX_HOME`; CLI summaries omit it. Advisory outcomes still force `independentReview=PENDING`, `wakeReady=false`. At exact integrated `82ba2bbee81a6c37ca886d65b61287b7cf99678b` (tree `b8cc9ced4ac1f4f8192d05cb282fd14ce99b8902`), the full locked all-feature workspace suite passed serially (main 1,348 passed, 0 failed, 28 ignored; enabled integration targets passed); strict Clippy, formatting, diff check, and build exited zero. The exact binary SHA-256 was `D1D9CDAA5E5C12F15C25EE09E4C4F7D7125C82BE855214DE06AC812083A7D61F`. An earlier build attempt was blocked by a verified stale `catdesk.exe --help` process holding the old image; after closing that exact hung process, default parallel testing hit three legacy five-second job-manager waits. Those tests passed in isolation, and full serial runs at both `e33b72a` and `82ba2bb` passed. The nested AppContainer negative child printed its expected denial while its enclosing test and top-level suite passed.
- A third, separately sealed disposable fixture at `%LOCALAPPDATA%\Temp\catdesk-queue-advisory-v2-20261016-41bf7cf355c1` used one runner invocation (runner SHA-256 `62C212D5371B5E80D212642C17FA18C19047170D99DC7588058318836DD4864E`, gate SHA-256 `1DA894520C675D0458C73EA61B562404129DFD9C58725A3EEDCC51DDACD4EC9C`). Its result SHA-256 `D1713C279D4B1FB1C3DF14436D034919B1E681A8E8767DAC9E047DB6AE516743` records exactly one bootstrap, add, dispatch, verify, package and advisory call against source `82ba2bb` and the image above. Receipt `dd26dbeb-b71d-4cd9-b6b6-608924302362` records observed native worker exit zero and only the requested disposable `src/lib.rs`/`src/proof.rs` edit; four fixed Cargo verification commands exited zero. Persisted receipt/candidate/verification/package SHA-256 values are `23B1A2785F325865B81C8DC6D4ACB4CB6C1D45F7C7DF04C9A61AC018C8E10BB5`, `97745ECAA803D84352F4E33599D98E548EEDD5399B9C19F112D611BDAC17A396`, `AA6F4482719970F09B28950307F590942BDC273722CAA31BDA1DD8D56BC953CA`, and `0CD53A43FBACF6F886ED5A83769C8C461F6D7E5ABB61C78342D3FA6E6A6CCD69`. A separate fresh Sol/medium reviewer turn exited zero and produced v2 advisory outcome SHA-256 `B584C8EA4E036CF5BD96F510BD01ED4D63C6A917D03DDF9C1DA6ADA0DE1395D2` with verdict `NO_FINDINGS_REPORTED`. Its rollout had no tool-call events; the stored outcome remains `independentReview=PENDING`, `wakeReady=false`. The 48-byte schema-valid final verdict provides no independent proof of the model's thoroughness. This is a local fake-origin fixture, not an actual project review, installed Wake receipt, or remote CI acceptance.
- Integrated `1a823ffbee8d76594acbc43a9198bd53a0500818` (independently reviewed source `01122a8`, tree `edc68eacf880d083ff06f54a21924037b0722000`) bounds direct `codex run` prompt write/flush to 30 seconds, closes stdin before waiting, and bounds a failed prompt's parent kill attempt while retaining the durable writer marker for proof-based recovery. Its subprocess test observed EOF and forced a helper timeout with a 4 MiB nonreading-child prompt; production tasks remain capped at 24 KiB, and neither the helper test nor this change bounds a full model turn. Integrated `5237afa76f9326beb2eaadf3c484801b6f004c99` (independently reviewed source `862f416`, tree `78d27b5faa508a190aeae6fbb99a811279640ba2`) makes the public autostart supervisor classify only exact, scalar, command-bound redacted recovery gates: local MCP failures enter `DEGRADED` cooldown; external runtime failures enter `EXTERNAL_RUNTIME_PENDING`; malformed or unknown responses require attention. Three PowerShell fixture suites passed. At exact source `5237afa76f9326beb2eaadf3c484801b6f004c99`, the serial locked all-feature Rust suite exited zero (main 1,349 passed, 0 failed, 28 ignored; all enabled integration targets passed); strict all-target Clippy, format, diff check, and CatDesk build exited zero. The rebuilt image SHA-256 is `8A9D90244E1C1DBDCE539AABAA36D98C4C5612397CF44867E567DEF8F7B3B856`. These are source/local fixture results; the installed supervisor was not changed.
- Integrated `512e9efe81ebe00c583134bf3a11f744be5a40ab` (independently reviewed source `51b873e`, tree `0856746daee72a991fb839fc41ca9e40924ebdb3`) adds `recordedTurnGate` to scoped `codex status`: recorded session/queue/accounting counts, pending marker presence and explicit `launchAuthorized:false`, `writerLockChecked:false`, `transactional:false`. Malformed recovery evidence yields `EVIDENCE_UNAVAILABLE` with null counts; status remains an authentication result, not a turn permit. Integrated `2c47d0c7f7efc277b9ad46319349ef8d0fd9f972` (independently reviewed source `d6d88df`, tree `ca959cebdd7e351f5c1bdaea6e31aef9b5fa0a47`) persists an exact future queue-attempt marker digest, checks its physical marker before spawn, and leaves historical attempts readable but unbound. Neither marker identity nor parent exit establishes descendant quiescence or authorizes UNKNOWN lease release. Integrated `c124223a66cebc96658982086f36a52b816a125a` (independently reviewed test-only source `f82fa69`, tree `18f25fd05c1aa074760fcf4d1756d18bb68357b4`) fixes stale Wake fixture directory reuse. The first combined full run exited 1 on that nested fixture's `CONFIG_CAS_CONFLICT`; the exact failing test passed alone, and after the fixture repair the same full serial locked all-feature workspace command exited 0 (main 1,356 passed, 0 failed, 28 ignored; all enabled integration targets including nested Wake passed). Strict all-target Clippy, format, diff check, CatDesk build and three PowerShell fixture suites exited 0. Exact source tree is `18f25fd05c1aa074760fcf4d1756d18bb68357b4`; debug CatDesk SHA-256 is `EDB391BB645010C9375CC2C1CB1A502DD078C2EF72CAF3386E63011D70DAD1B1`. These remain source and local fixture results.
- Integrated `747e3cfffa2fc6376106aee6b2a55df3239a8539` (independently reviewed isolated source `83be20b`, tree `cefbbd33b43742969454c8f06be6733495546a49`) bounds noninteractive direct `codex run` wait to 12 hours after prompt delivery. Timeout or wait error attempts a bounded 20-second parent kill and retains the durable writer marker because descendants are unproven; interactive `resume` remains user controlled and unbounded. Integrated `532715de1786ba4b02c9cdffbbba4ef3139ce527` (independently reviewed source `1312237`, tree `243807ebebaeea9e2029f752d6a8f7f7622ffd0f`) makes queue status add a verification profile and SHA-256 Git-origin digest, and Binagotchy prints `VERIFIED` only for an exact typed current project binding. The digest avoids printing the raw origin but is a stable pseudonym, not a secrecy guarantee. Integrated `378ae174d745e08732c2824f89fd3caf61d4cf6e` (independently reviewed source `8b52a0e`, tree `bf0109446545e29cb69d796fe4153ad37732ff4a`) lets core recovery converge while Wake remains unavailable and reports `CORE_CONNECTED_WAKE_PENDING`/`WAKE_RUNTIME_NOT_READY`; it freshly rechecks core once after Wake work before writing evidence, and the supervisor enters `WAKE_PENDING`. A review-found stale-core proof path was repaired before integration. Canonical identity is checked before daemon work; a damaged pair can be repaired only through trusted interrupted-promotion/LKG authority and must be revalidated before daemon work.
- At exact integrated source `378ae174d745e08732c2824f89fd3caf61d4cf6e` (tree `785779be11325cd44462bbe95b2f6ba19db4daac`), `cargo test --locked --workspace --all-features --quiet -- --test-threads=1` exited 0: main binary 1,358 passed, 0 failed, 28 ignored, with all enabled integration targets including five recovery PowerShell tests passing. Strict all-target Clippy, `cargo fmt --all -- --check`, `git diff --check`, `cargo build --locked --all-features --bin catdesk`, and the three public PowerShell fixture suites exited 0. The exact debug image SHA-256 is `E5DA452AB15F2D19E2AE26C73F145CE822C8F3179E51F352F165C8D2CFA6CD4C`. The nested AppContainer negative child printed its expected access-denied failure while its enclosing test and top-level suite passed. These results are source/local; no installed daemon or WakeHost was changed.
- This verifier deliberately requires a clean preturn Git tree, unchanged HEAD, at least one changed file, and the tracked manifest/lock for its fixed profile. It records direct Cargo or `uv` parent exits and an immediate source recheck, without proving descendant quiescence, later worktree bytes, independent review, or Wake. Current registered roots have active or recovery-blocked work; the two external Python projects still need reviewed registry migration to their new fixed profiles before real three-project verification.

## October 16 read-only continuation

- At pushed `4d9a39e`, the repository is public, active and unarchived; its Actions setting is enabled with all actions allowed, and workflow `311012404` is active. The exact source commit had no run or check in immediate and delayed post-push readbacks (`gh run list --commit 4d9a39e` returned `[]`; check runs `total_count:0`; combined status had zero statuses). Earlier pushed `1afab98` had no run/check in immediate and delayed readbacks either. The workflow has unfiltered `push` and `pull_request` triggers and no `workflow_dispatch`; the source commit message has no skip annotation. The earlier manual HTTP 422 text, `Actions has been disabled for this repository`, is a separate, unproven administrative signal; its raw response was not retained. The authenticated billing read endpoint returned HTTP 404 because the CLI token lacks the `user` scope, so it cannot establish an account billing restriction. [GitHub's Actions settings guidance](https://docs.github.com/en/repositories/managing-your-repositorys-settings-and-features/enabling-features-for-your-repository/managing-github-actions-settings-for-a-repository) says that if the owner UI displays a separate repository/account disabled banner, changing ordinary Actions settings will not restore it and GitHub Support must review it. No workflow dispatch, token-scope refresh, or settings change was made.
- The actual CatDesk control root still reports `NEEDS_OWNER_RECOVERY` with 35 historical in-flight queue claims and 85 accounting review rows. A separate read-only owner-handoff audit found no recorded owner-process PID/creation identity; `recovery prepare` flags source-bound child exit as unproven for all 35 claims. That flag describes the report's unestablished proof, not an absence of external diagnostic files. All 35 accounting rows are unended, 13 have open spans, and 27 Codex claims have incomplete profile evidence. Of the other 50 review rows, 49 are unended and 44 have open spans. Twenty-nine snapshots retain a provider handle ID. A read-only search of 535 local Codex diagnostic files found 11 exact session/task JSON files across eight claims; all 11 record direct child exit. Five files match a snapshot's latest provider turn and current handle; four of those also match the expected Codex thread and show exit zero, and three of the four contain `turn.completed`. Three other sessions have only older-turn diagnostics. Those four are plausible owner-review candidates, not automatically clearable claims: owner PID/creation identity and descendant quiescence remain unproven, and the accounting rows remain unended. Terminal or paused labels and an absent pending writer marker likewise do not establish child exit. The existing restart reconciliation path does not apply to these claims. The original owners need claim-specific completion/cancellation and provider-specific disposition evidence; a future recovery mutation needs a reviewed, exclusive-lock compare-and-swap transaction. BYOVD and Bug Bounty have no recorded managed recovery blocker, but their bound Codex chats are active and both worktrees contain ongoing changes. No registered-project queue task was dispatched.
- Integrated commit `22baca6` cherry-picks independently reviewed recovery source `71c6d6d3acf5ed95fa96e514d18b7d7c7f4c23d7` (tree `ed553ca92a4cbea136610e3f2d719e051930cd5b`). `codex recovery status`/`prepare` now add bounded `claimEvidenceGapsV1` and aggregate counts without changing prior status, launch authority, or durable state. It validates checkpoint and accounting structure before joining evidence, distinguishes session-wide from per-task turn counts, and flags missing profile/thread/model bindings. The three integrated source blobs match the tested branch. Focused recovery tests passed 17/17; strict locked all-target Clippy, format, diff check, and full locked all-features workspace tests passed (main binary 1,325 passed, 0 failed, 28 ignored; remaining enabled targets passed). Built binary SHA-256 was `B98E0FF45D9F4EC4AA2FA03C9145812DAC6A320596C999DC5EA8FEC00BA61C50`. Read-only Desktop readback still reports 35 claims, 85 accounting review rows, and `launchAuthorized:false`; recorded control-state gaps include 35 without owner-process identity, 35 without source-bound child exit, 35 unended accounting records, 13 open spans, and 27 incomplete Codex turn profiles. Existing `queueBlockers` still expose sanitized session/task IDs; the new claim hashes do not anonymize the whole report. The diagnostic digest is not a recovery compare-and-swap token.
- The two Python projects are registered with `verificationProfile: rust_full` despite having root `pyproject.toml` and `uv.lock` rather than `Cargo.toml`. This profile must be migrated under reviewed project ownership before it can support their verified jobs; it must not silently select a different command.
- Read-only manifest checks at Bug Bounty `a5d7de3` and BYOVD `4341321` confirmed both `pyproject.toml` and `uv.lock` are tracked. Bug Bounty declares package `bug-bounty-security-research-platform` with `dev` and `lab` extras; BYOVD declares `byovd-driver-pipeline` with a `dev` dependency group. Their manifest/lock SHA-256 pairs are `2C342A91FF18CC06902077D2A881D841C08118232D50E8E2AB86C99EEA1FC7C0` / `3894D3973E7279EC1A1C2756E3D6CA330E7155C81559FD46D586C37A5936E0C1` and `488040B4F0762D4F44450390348EFFC468BFD48D4AD3AB6C1BB9A785E1478532` / `CECEC1B5852BC6D438B1DD99DBE65384353180A14F3FC2DAEEA072A10546D9FB`. No `uv` command ran in either active registered checkout.
- The installed Wake facade still reports `LKG_AUTHORITY_MISSING` and `CANONICAL_HASH_MISMATCH`. Its dev.84 pointer hashes to its named image, while the direct store remains at stale dev.59 `STOPPED` with an unpaired target. The protected install broker is absent. No safe activation or fresh Wake receipt was established.

## Source and entry points

| Item | Observed identity |
| --- | --- |
| Repository and branch | `vnath99/CatDesk`, `orchestrator/chatgpt-codex-autonomous-loop` |
| Last locally tested source revision | Integrated `378ae174d745e08732c2824f89fd3caf61d4cf6e` includes the earlier authenticated CLI, queue and recovery source through `c124223`, bounded direct noninteractive run `747e3cf`, full Binagotchy queue binding readback `532715d`, and core recovery with Wake pending `378ae17`. The exact combined source passed the local gates recorded above. |
| Last locally tested source tree | Integrated `378ae17` tree `785779be11325cd44462bbe95b2f6ba19db4daac`; isolated source trees and review limits are recorded above. |
| Remote branch readback | `git push` advanced the feature branch to exact tested source `378ae174d745e08732c2824f89fd3caf61d4cf6e`; `git ls-remote` returned the same SHA. Its immediate Actions list was empty and check-runs total was zero. This documentation checkpoint is later than the tested source tree. |
| Source version | root `Cargo.toml`: `0.1.6`; Wake `wake/Cargo.toml`: `1.0.0-dev.85` |
| Installed operator Codex | `codex-cli 0.160.0`; `codex login status` returned `Logged in using ChatGPT` |
| Local CatDesk entry point | `target/debug/catdesk.exe`; the initial route-reply canary image SHA-256 was `784C333BAF23CAA010A5D506013A8B7AABC39FF0D56167608B4A6ABFCA57F07D`. The exact `e945e59` edit/test image was `BBE2EF9A0705AD3023E80E66033CED3FBBE1DDBDC5A485A0AB13C092FDCCC00D`; the `6c6c798` manually ordered three-project image was `97C828B25CB7B2BDBA0216B8396F4C5FEE24A1F8A247BB6DA98F2831F2CCAAAF`; and the `f5c89c8` recovery inspection image was `7739CF3962F709BD800803EBD9C9B5F1DDCA44F044E4DE31EABA5EE5A1172936`. The first queue attempt used `12c25d0` image SHA-256 `548F76110E182DCCC79D12130FD4CE9DE2A5253703B5F4C874458307BBC08629`. The successful queue continuation used exact-`ab6a3f7` image SHA-256 `6BE6F7ABACDA1E74B52E738C1E548AEAA148524FD5938B8D4A3AC613122A7954`. The exact-`2636f78` source build image SHA-256 is `9D276D974220164A967175FF71053B1F70AAFE6BEC29E6153BA08280AD423A09`; it ran one independently audited clean-project scratch queue turn below. The exact `82ba2bb` source image SHA-256 is `D1D9CDAA5E5C12F15C25EE09E4C4F7D7125C82BE855214DE06AC812083A7D61F`. The exact `5237afa` source image SHA-256 is `8A9D90244E1C1DBDCE539AABAA36D98C4C5612397CF44867E567DEF8F7B3B856`. The exact `c124223` debug image SHA-256 is `EDB391BB645010C9375CC2C1CB1A502DD078C2EF72CAF3386E63011D70DAD1B1`. The exact `378ae17` debug image SHA-256 is `E5DA452AB15F2D19E2AE26C73F145CE822C8F3179E51F352F165C8D2CFA6CD4C`. Hash before another run; none is an installed-release version proof. |
| Native Codex entry point | The current user's authenticated `codex` CLI. Its scratch canary is separate from the proposed CatDesk `codex` operator route. |

The operator CLI source is committed at `8174ab9`; persisted fair lease rotation was committed at `e945e59` and repaired at `31890c9`; Binagotchy inspection at `ec7d824`; the shared-writer guard at `6c6c798`; read-only writer recovery at `f5c89c8`; scheduler-state bounds at `cb41768`; the explicit operator queue at `8b937e1`; Binagotchy scoped controls at `12c25d0`; absent-scheduler initialization at `ab6a3f7`; and accounting-aware prelaunch refusal at `2636f78`. A rebuilt `e945e59` binary completed a through-CatDesk edit/test canary. A rebuilt `6c6c798` binary completed manually ordered three-project turns; a rebuilt `f5c89c8` binary inspected actual Desktop control data. The `12c25d0` native queue canary exposed a preclaim scheduler initialization defect; the `ab6a3f7` continuation completed the preserved fixture tasks and passed independent result audit. A later `ab6a3f7` origin-drift failure-isolation fixture also passed independent audit. Both results are fixture only. The `2636f78` hardening is source-tested; it does not reconcile Desktop accounting or add a live canary. No older successful CI run validates these later changes.

## Real inputs and evidence

| Input or observation | Evidence and result | Label |
| --- | --- | --- |
| Approved CatDesk scratch worktree, branch `codex/native-cli-canary-20261015`, based on `da58c72634322d638291989fbca6465cabecbb6a` | Authenticated native Codex thread `01a14217-f972-7d62-90c1-4e626be4f536` made a 22-line test-only edit in `src/delegated/codex_model_policy.rs`, then continued by exact thread ID. The session `turn_context` records `gpt-6.1-sol`/`medium`, first with `workspace-write`, second with `read-only`; both record approval `never`. The second turn made no further file changes. The test-only diff is checkpointed as scratch commit `7c6272c56ec053704bbd675bc787d29261d6e768`, tree `46d22992140c65a17b8eb7435cb99fec957bc880`. The exact original native CLI invocation strings were not retained in this record. | `TESTED_LOCAL` for native Codex only; command transcript gap |
| First scratch source verification | At scratch commit `7c6272c`, `cargo test --bin catdesk delegated::codex_model_policy::tests::turn_profile_evidence_accepts_sol_medium_and_rejects_unicode_identifiers -- --exact` exited 0: one passed, 1,270 filtered. `cargo fmt --all -- --check` and `git diff --check` exited 0. Its source-file SHA-256 was then `0C5CC41BCD7374159222D63A20E5AD9B421CAF77624E8C7ED5C6F2C980D1FAFF`. A later scoped test addition changed that file. | `TESTED_LOCAL` |
| Private native session record | `%USERPROFILE%\.codex\sessions\2026\10\15\rollout-2026-10-15T20-23-55-01a14217-f972-7d62-90c1-4e626be4f536.jsonl`, SHA-256 `5E12F3734346A0CCB42275FEEA7631CF4BC0F80EF282D1CB39F9D00E351BC2E4`. This local file contains session content and is not attached to this repository. | `TESTED_LOCAL` |
| Standalone all-session picker | On October 16, `codex resume --all` from the CatDesk repository opened the authenticated interactive picker with the **All** filter selected and listed existing sessions. The PTY reported `TERM=dumb`; after continuing, Codex warned that the background server had incompatible feature settings. I selected **Run without daemon this time**, observed the picker, and quit with Ctrl+C (exit 0). No session was selected or resumed, and the background server was not restarted. This proves the direct CLI all-session picker is available; exact-session continuation is evidenced separately above. | `TESTED_LOCAL` for read-only picker only |
| CatDesk all-session picker | On October 16, the exact source-`82ba2bb` CatDesk image SHA-256 `D1D9CDAA5E5C12F15C25EE09E4C4F7D7125C82BE855214DE06AC812083A7D61F` ran `target/debug/catdesk.exe codex resume --all` from the control Git root. After the terminal's `TERM=dumb` prompt, the authenticated native **All** picker loaded existing sessions. I quit with Ctrl+C without selecting a session; CatDesk exited zero and returned `action:codex_global_resume_picker`, `projectClaim:false`, `threadId:null`. The read-only Desktop recovery evidence SHA-256 was `60b3ce930bcfcffbb7873fe3beaa83301c2c65099dc1f8d11a625a8aeb436d70` before and after, with 35 queue blockers, 85 accounting rows and no pending writer marker. This verifies the CatDesk picker route, without a project turn or daemon restart. | `TESTED_LOCAL` for global picker only; actual Desktop scoped `run`/`resume` remain blocked |
| First isolated CatDesk route canary | A separate, ignored one-record project registry in the scratch worktree bound a fresh native thread `01a14225-fc94-7d61-a6d5-b1733e8886d8`. `catdesk.exe codex status --project-id catdesk --workspace <scratch>` exited 0 and reported authenticated Codex CLI `0.160.0`. `catdesk.exe codex run --project-id catdesk --workspace <scratch> --task 'Reply exactly ROUTE_RUN_READY. Do not inspect or edit files.' --model gpt-6.1-sol --effort medium` exited 0 with exactly `ROUTE_RUN_READY`. An unregistered project was refused with exit 2, `CODEX_SCOPE`. The scratch Git tree remained clean. Focused operator CLI tests passed 4/4 with strict Clippy, format and diff checks before source commit `8174ab9`. | `TESTED_LOCAL` for the first route turn and scope refusal; later exact-source edit/test recorded below |
| Through-CatDesk edit/test canary | From the same isolated scratch control root, the exact invocation below continued thread `01a14225-fc94-7d61-a6d5-b1733e8886d8` with requested `gpt-6.1-sol`/`medium` and exited 0. It added only a 32-line test in `src/delegated/codex_model_policy.rs`, ran `cargo test --bin catdesk turn_profile_evidence_rejects_invalid_turn_hash_thread_and_model` successfully (one passed, zero failed, 1,271 filtered), and left a scoped diff that passed formatting and diff checks. That change is reviewed and checkpointed as scratch commit `3d4c612c4549767bad553600f9b01eb19fb75185`, tree `3876a34fba0dfafaed25ae8c02c5ae97a72d444a`; the worktree is clean. I independently reran the focused test with the same result. The current source-file SHA-256 is `5A68E428DEFA867ABAAE9FCF1854D57BC16D2DF7552ACC86D3417F2730792836`. | `TESTED_LOCAL` through exact-source rebuilt CatDesk; not installed or CI accepted |
| Private route session record | `%USERPROFILE%\.codex\sessions\2026\10\15\rollout-2026-10-15T20-39-08-01a14225-fc94-7d61-a6d5-b1733e8886d8.jsonl`, current SHA-256 `DFC169F9525ACA8F611B003D9A6707D2458CAF1B49CA46378D3F4019BF2803E6`. Its initial `turn_context` is `read-only`; both CatDesk continuations are `workspace-write`; all three have effective `gpt-6.1-sol`/`medium`, approval `never`, and the exact scratch working directory. The first route reply stdout is `%LOCALAPPDATA%\Temp\catdesk-route-run-462d341d-268b-4273-bfbc-3539e2637f6a.txt`, SHA-256 `7DEC41C6C84121E5F59E0326B256177F62EF5C76687D2299E74DC92C2E44721A`. These local records can contain session/task content and are not attached to Git. | `TESTED_LOCAL` with two CatDesk continuations |
| Three registered project identities | Local `.catdesk/projects/projects.json`, SHA-256 `6F06C8EE771D9B44D05216D079A7E51B7734F49ED20B56B085BE3FBE73A76FBF`, registers `catdesk`, `BYOVD_DRIVER_PIPELINE`, and `BUG_BOUNTY_RECON_PLATFORM` with distinct Git roots, Codex thread IDs and canonical conversation digests. The file is local private control data; its URLs and thread bindings are not reproduced here. No three-job routing receipt exists. | `IMPLEMENTED_SOURCE_ONLY` for read-only resolution; execution `BLOCKED` |
| Project identity and lease source | Commit `ee684bf` adds `resolve_registered_project_from_control_root` with temporary-Git-root tests and origin-drift refusal. Commit `e945e59` adds a persisted round-robin ready-project lease selector under a scheduler lock. Commit `31890c9` makes expired ownership fail closed and publishes complete leases no-replace. Commit `cb41768` validates bounded scheduler state; `ab6a3f7` initializes absent scheduler state before the queue claim. The first native queue run found that a valid registry with no `leases` directory refused before claim. The repaired continuation then rotated four fixture jobs `alpha→beta→catdesk→alpha` with exact lease release. Earlier focused scheduler checks passed 17/17; final queue-focused checks passed 15/15. Real-project fair rotation remains open. | `TESTED_LOCAL_FIXTURE_ONLY`; integrated routing pending |
| Actual Desktop CatDesk CLI readback | Against the actual Desktop CatDesk control root, `codex status` returned authenticated Codex CLI `0.160.0`. Both `codex run` and `codex resume` exited 2 with `CODEX_LOCK` before starting a native child: a paused `chatgpt-direct` session still had a `WORKER_RUNNING` queue claim and an open `CHATGPT_WEB_ACTIVE` span. The audit also found terminal-labeled historical sessions with unfinished queue claims or spans. The guard correctly treats the in-flight claim as a blocker even when a session label is terminal. Historical state was not cleared or rewritten. The earlier scratch success does not clear this lock. | `TESTED_LOCAL` safe refusal; integrated CLI progress `PARTIAL` |
| Shared project writer guard | Commit `6c6c798` coordinates scoped direct Codex CLI and managed provider operations with a project-root keyed OS lock and a durable pending marker outside worker-writable project roots. The implementation rechecks project binding after lock acquisition, records the marker before child spawn, and retires it only after observed process completion or kill; retained ambiguous markers and in-flight queue claims fail closed. Project autonomy paths receive symlink/reparse-point preflight. Focused operator tests, review corrections, the local full suite, and an isolated three-project binary canary passed. Same-OS-profile coordination only is evidenced; cross-account/service ownership and a public crash-marker recovery path are not. Manual global `codex resume --all` and goal resume remain unmanaged routes and cannot count as project-scoped CatDesk progress. | `TESTED_LOCAL_FIXTURE_ONLY` for scoped routing; integrated Desktop route `PARTIAL` |
| Accounting-aware native prelaunch guard | Commit `2636f78` adds the bounded, read-only managed recovery report to the shared direct/queued native prelaunch check. It refuses with `CODEX_LOCK` when recovery evidence is unavailable or unsafe, or when the status is not `NO_RECORDED_BLOCKER_OBSERVED`. This covers unended task accounting and open activity spans even if a historical session is terminal-labeled and its queue appears ready. Tests exercise direct refusal, closed-accounting allowance, and queued refusal before child spawn; focused operator checks passed 33/33. An exact-source clean-project scratch queue turn passed independent audit. The hardening does not clear the observed Desktop 35 queue blockers or 85 accounting review rows, grant Desktop launch authority, or prove an actual-control/installed turn. | `TESTED_LOCAL_FIXTURE_ONLY`; Desktop owner handoff pending |
| Explicit current-user operator queue | Commit `8b937e1` adds `codex queue add/status/receipt/dispatch/pause/resume`; `ab6a3f7` repairs first-claim initialization. Task payloads and create-new receipts live in a fixed current-user CatDesk state root, outside project workspaces, under a namespace hashed from the verified control Git root and identity. `add` is task-ID idempotent for the exact scope, prompt and profile; each explicit `dispatch` selects one ready project via the persisted fair cursor and exact lease, rechecks project/thread/target binding and the shared writer guard, and runs one native Codex turn. Only quiescent `READY`/`PAUSED` tasks may pause/resume. A receipt binds executable version/hash, prompt hash, root/thread/target and exit code; `NATIVE_EXIT_OBSERVED` proves **parent process outcome only**, not authorized edit, test, review, provider acceptance, or descendant absence. Pre-spawn refusal can return the task to `READY`; uncertain process state stays `UNKNOWN` with its lease retained and no automatic replay. The lease is not reclaimed by time alone. Queue-focused tests passed 15/15. The native scratch continuation below produced four process-exit receipts and exact lease releases; it did not execute real project jobs. Same-user coordination is not a protected cross-account broker. | `TESTED_LOCAL_FIXTURE_ONLY`; real-project acceptance pending |
| Binagotchy project queue and scoped controls | Commit `ec7d824` added read-only `projects` and `queue` views; real-control readback showed three verified registrations, five CatDesk historical rows, one scope rejection, and zero rows for the other projects. Commit `12c25d0` extends `queue` to render explicit operator jobs with verified project bindings, adds `resume <project-id>` for that project's bound native thread, and `recovery <project-id>` / `recovery prepare <project-id>` as bounded, read-only blocker views. Resume launches the existing scoped CatDesk operator as a child, which rechecks its registration and writer guard; it has no new privilege surface. Focused Binagotchy tests passed 14/14. In an isolated fixture profile, a Wake-unavailable REPL exited 0 after `projects` showed three verified scratch registrations, `queue` showed four READY fixture jobs with zero leases, and `recovery alpha` showed no recorded blocker; stdout SHA-256 `379BDC83BD1ADDF80BF86DBD30E294505B754E64A88938D67F8D0B2DD4A4BDA6`. This was read-only and did not launch Wake or a scoped resume. No installed REPL or actual-control live resume is accepted; Desktop `resume` still has historical `CODEX_LOCK`. | `TESTED_LOCAL_FIXTURE_ONLY` for read-only controls; live scoped resume pending |
| Local Rust and Python gates | Earlier source `6c6c798` passed the root full suite with main binary 1,265/0/28; recovery work later had 1,269/0/28 before final P1 fixes; `12c25d0` passed 1,300/0/28; `ab6a3f7` passed 1,302/0/28. At exact committed `2636f78`, `cargo test --workspace --all-features --quiet` exited 0: main binary **1,306 passed, 0 failed, 28 ignored**, and all other enabled targets passed. Focused operator tests passed 33/33; strict root `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo fmt --all -- --check`, and `git diff --check` exited 0. `cargo build --all-features --bin catdesk` exited 0 with binary SHA-256 `9D276D974220164A967175FF71053B1F70AAFE6BEC29E6153BA08280AD423A09`. The earlier `ab6a3f7` binary completed the audited four-turn queue fixture. An exact-`2636f78` single clean-project turn exited 0 in a scratch fixture and passed independent read-only audit. Earlier Python `pytest` exited 0, and independent Wake Rust passed 213/0/1 with strict Wake Clippy/format checks. An expected nested AppContainer feasibility helper printed a failure under its enclosing fail-closed parent test; the parent and overall command exited 0. None of these local results substitutes for GitHub CI or installed-release acceptance. | `TESTED_LOCAL` exact source checks and single-turn fixture; integrated acceptance open |
| Isolated three-project Codex canary | Disposable control root `%LOCALAPPDATA%\Temp\catdesk-3project-canary-20261015-211551-ee60ede1` had three distinct scratch Git roots, distinct native thread IDs and registry SHA-256 `AD8137F421F905024301EBD6C94CFD42B38AD9FD036F67650E65C9986660A8E9`. The `6c6c798` image SHA-256 `97C828B25CB7B2BDBA0216B8396F4C5FEE24A1F8A247BB6DA98F2831F2CCAAAF` completed explicit `alpha→beta→catdesk→alpha` native turns: all four exited 0, each created its unique proof file only under the intended root, and the second alpha turn reused its original thread. `alpha` with `beta`'s workspace was refused before execution, exit 2 `CODEX_SCOPE`. `result.json` reports `passed_fixture_only`, SHA-256 `E708FD946577BD98BC6FD492C619E99671BFF42AE3748C7E0F35584E2A09E4F0`. The order was explicitly invoked by the runner; its task IDs are fixture labels, not durable CatDesk queue receipts. No automatic scheduling, pause/resume, failure isolation or real-project execution was shown. | `TESTED_LOCAL_FIXTURE_ONLY`; E remains `BLOCKED` |
| First native operator queue attempt | Under the same three-root disposable control, the `12c25d0` binary SHA-256 `548F76110E182DCCC79D12130FD4CE9DE2A5253703B5F4C874458307BBC08629` added four distinct READY tasks, paused and resumed the beta task while quiescent, and observed zero Desktop operator jobs. First `codex queue dispatch` exited 2 with `CODEX_QUEUE: scheduler state is unavailable or unsafe` because the otherwise valid fixture registry had no `leases` directory. Failure occurred **before claim or native child launch**; all four READY tasks remained available. Result `queue-run-20261016T024835Z-29209e1c/result.json` SHA-256 `1495D2DA1EF2DE1884A44CB32D99B916FBB5581C40F84D1546A3E36D34A5C8EC`. Commit `ab6a3f7` repairs this initialization path. | `TESTED_LOCAL_FIXTURE_ONLY` failure with preserved state |
| Native queue continuation after repair | From those same four preserved READY tasks, the exact-`ab6a3f7` image SHA-256 `6BE6F7ABACDA1E74B52E738C1E548AEAA148524FD5938B8D4A3AC613122A7954` completed four explicit `codex queue dispatch` calls. The scheduler selected `alpha→beta→catdesk→alpha`, each native `codex-cli 0.160.0` turn exited 0 with `gpt-6.1-sol`/`medium` requested, four unique proof files appeared in only their bound scratch roots, the second alpha task reused its original native thread, each attempt produced a distinct `NATIVE_EXIT_OBSERVED` receipt and exact lease release, and Desktop queue status reported zero operator jobs before and after. Result `queue-continuation-20261016T025707Z-29209e1c/result.json` reports `passed_fixture_only`, SHA-256 `4781BB6A350E1BC1020C2B4E49E37600518939750470172687BCDF1D86C8FA1D`; continuation runner SHA-256 `FF7C21F0E173783846518478855C1D3C7598EEA9EDF843C00C1C65D457C6DEB9`. Independent audit matched the result to logs, proof files, native sessions and receipt hashes, and found no remaining fixture leases or native CLI children at that audit. Task add and quiescent beta pause/resume occurred under `12c25d0`; only the successful four-dispatch continuation was run under `ab6a3f7`. Receipt evidence proves observed native parent exit and scope, not independent deliverable quality or real-project dispatch. | `TESTED_LOCAL_FIXTURE_ONLY`; integrated E still open |
| Native queue failure isolation fixture | At exact source `ab6a3f7`, two additional scratch tasks were added. With alpha's scratch Git origin deliberately drifted, one explicit dispatch left alpha `ATTENTION` with no attempt ID, receipt, proof file or native claim, then selected beta. Beta's native Codex turn in its bound thread/workspace exited 0 with `gpt-6.1-sol`/`medium` requested, wrote only its intended proof file (SHA-256 `6901A0A0842F13C3BC5697FCDB3C4964E3F5F5C8A5D2D90668677F5B7328BBC1`), and produced `NATIVE_EXIT_OBSERVED` receipt `596b47c9-1c3a-489d-9839-7a095d7c21e6` with binding still matching and lease released. Alpha remains `ATTENTION` for operator review, with no automatic retry. The fixture restored alpha's origin; the lease directory was empty; Desktop queue status before/after had identical hashes and zero jobs. Result `queue-failure-isolation-20261016T030300Z-6646203c/result.json` reports `passed_fixture_only`, SHA-256 `CD401143467FD6932B1FD668BB16B2FA67FF792D3D662EED3BE7AAB9F9BA1DDF`; runner SHA-256 `ADB3FE83AC4BB657EB57133697422526BEF8BD51606E927D67A812B704270D4F`. Independent read-only audit passed for this one drift failure, not for all stuck-worker modes or real-project deliverables. | `TESTED_LOCAL_FIXTURE_ONLY`; integrated E open |
| Exact-`2636f78` clean-project queue fixture | In the same disposable control, one explicitly added catdesk scratch task dispatched through the source-built image SHA-256 `9D276D974220164A967175FF71053B1F70AAFE6BEC29E6153BA08280AD423A09`. Native `codex-cli 0.160.0` exited 0 in the bound catdesk thread/workspace with effective `gpt-6.1-sol`/`medium`, producing one proof file with its exact requested token and newline (SHA-256 `F1EF74C2EBC8DA04401FB4E6B484596B4568D6EFB5213DAE4554F1293D47C633`) and `NATIVE_EXIT_OBSERVED` receipt `e415091d-b3b4-4409-9b63-8a1600aa928b` (raw receipt SHA-256 `89A597AA24A0E388CED45A11A56F521B03C9820A4D23E6EF7BE3D1F985A5212E`); the exact lease was released. Independent read-only audit matched all eight invocation log hashes and the native session JSONL (SHA-256 `AA1C0393539E1D759291E78D43FE6E696535D43D551FF23A7C8EFAC40646A64F`), found the prior six fixture rows unchanged including alpha `ATTENTION`, no lease or writer marker, and identical Desktop queue status with zero jobs. Local result `queue-accounting-guard-2636f78-5f0c4e9a/result.json` reports `passed_fixture_only`, SHA-256 `4C390FCEFD90188FD4276C72F5E9BD2477B8AEBD3C798CA07B5FB0C6393818D2`; runner SHA-256 `FFABDF5B4351F6971561157D530C0D28D2B3B9E51670B27210D3EF7BF3438A17`. This clean-project process-exit receipt does not demonstrate reconciliation of actual Desktop blockers, deliverable review, installed health, Wake, or CI. | `TESTED_LOCAL_FIXTURE_ONLY`; integrated E open |
| Receipt-bound queue review source and local gates | `catdesk.exe codex queue review --receipt-id <canonical-receipt-UUID>` reads a persisted `UNREVIEWED` before/after Git witness bound to a native process receipt. It returns current registration and worktree rechecks; ignored files and submodule contents are excluded. A zero-exit native turn alone does not verify tests, model evidence, independent review, or Wake readiness. Missing/unsafe snapshots or binding/marker drift retain `ATTENTION` rather than an approved candidate. Focused queue tests passed 18/18. At integrated `ad7f499`, `cargo test --workspace --all-features --quiet` exited 0 (main 1,308 passed/0 failed/28 ignored; all other targets passed); strict workspace Clippy, format, diff check, and `cargo build --all-features --bin catdesk` exited 0. Built image SHA-256 `4DED18232F7B5690BD35E9E278DD10F8EE85C7E38DFE7CC752A4E0723B444817`. | `TESTED_LOCAL` source only; no remote CI or review approval |
| Single-project native review-candidate canary | Disposable source-`fc15603` image SHA-256 `54476011CE02D3FD6F06322ECBC840A01D15CE148B6D433FEC731FE7FE910B25` ran one authenticated `codex-cli 0.160.0` turn in one bound scratch root/thread. It created one proof file, a `NATIVE_EXIT_OBSERVED` receipt, a receipt-bound review candidate, and released the exact lease. `codex queue review --receipt-id f898b1d1-98d4-41a0-9a5e-3c68c20ed494` read back `UNREVIEWED`, `NOT_VERIFIED_BY_CATDESK`, and `wakeReady:false`. Independent postrun audit `%LOCALAPPDATA%\Temp\catdesk-review-candidate-canary-20261015-d2ddca04\review-candidate-run\postrun-audit.json` SHA-256 `6379CD6BCC737DAAFD758DB9C4E0AAF4B05EE85EA0139F4216588A26B2B314D5` reports `passed_fixture_only`. Its first runner failed a later control-file assertion because dispatch legitimately wrote `scheduler-cursor.json` and `scheduler.lock`; no second dispatch occurred. No CatDesk test verification, independent review, Wake approval, or actual registered-project job was produced. | `TESTED_LOCAL_FIXTURE_ONLY`; live review and three-project acceptance open |
| Verified native receipt review package | The exact integrated `4d9a39e` binary SHA-256 `61409F1AB3B5E9E307A13DDD4E953618C9F65977943B0C727939EF887DB7C60B` ran `codex queue review-package --receipt-id f3bdaed5-e77d-46f3-adee-2892208890e9` in the existing disposable control root without another dispatch. The create-once 11,525-byte package SHA-256 is `43FBC4DB60B6157D0699EA02143652359CB9F179438FC26540E874AC71DD663B`; the second readback returned `idempotent:true` and the same file hash. The private package contains the bounded 872-byte task prompt, while the CLI summary omits it. The package still says `independentReview:PENDING`, `wakeReady:false`; no reviewer verdict or Wake receipt was created. | `TESTED_LOCAL_FIXTURE_ONLY`; independent review and real-project acceptance open |
| V2 native advisory fixture | A separate clean fake-origin Rust root used one `82ba2bb` queue dispatch, one fixed local verification, one private review package and one fresh native advisory call. Its local result is `%LOCALAPPDATA%\Temp\catdesk-queue-advisory-v2-20261016-41bf7cf355c1\run-one-shot-advisory.result.json`, SHA-256 `D1713C279D4B1FB1C3DF14436D034919B1E681A8E8767DAC9E047DB6AE516743`. Receipt `dd26dbeb-b71d-4cd9-b6b6-608924302362` and advisory outcome SHA-256 `B584C8EA4E036CF5BD96F510BD01ED4D63C6A917D03DDF9C1DA6ADA0DE1395D2` bind a distinct `gpt-6.1-sol`/`medium` reviewer exit zero. The reviewer reported `NO_FINDINGS_REPORTED`; rollout structural audit found zero tool-call events. The record still says `independentReview:PENDING`, `wakeReady:false`. The verdict is an advisory model output and its short trace does not prove thorough inspection. | `TESTED_LOCAL_FIXTURE_ONLY`; actual project review, ChatGPT approval, Wake and CI open |
| Actual-control Codex recovery inspection | Commit `f5c89c8` adds `codex recovery status` and `prepare` as **read-only** inspections of the registered Desktop CatDesk project. With source-built image SHA-256 `7739CF3962F709BD800803EBD9C9B5F1DDCA44F044E4DE31EABA5EE5A1172936`, both commands exited 0 and returned `NEEDS_OWNER_RECOVERY`, `readOnly:true`, `launchAuthorized:false`. The snapshot observed 594 sessions, 35 in-flight queue blockers, 558 accounting records with 85 review rows, zero session blockers, and no pending writer marker. `prepare` lists the evidence needed for owner handoff; it does **not** clear a queue, close accounting, prove process exit, or authorize launching Codex. The snapshot is nontransactional, and its digest is diagnostic rather than a compare-and-swap token. Manifest SHA-256 `5E5B647ACF530EC78812823114550868379BBFF3BA1B1A57BD77651373C9D125`. | `TESTED_LOCAL_READ_ONLY`; integrated Desktop `run`/`resume` still `BLOCKED` |
| Public recovery facade fixture | At source `31890c9`, disposable fixture `%LOCALAPPDATA%\Temp\CatDeskRecoveryFixture-5eb879952a98478fa043496271be2f6c` called public root `catdesk.ps1` through fixture `TestSeams`. Initial `diagnose` was `HEALTHY`; an induced file-backed local-daemon fault gave `DEGRADED`, layer `LOCAL_DAEMON`, gate `LOCAL_MCP_LISTENER_MISSING`, next `RUN_RECOVER`; public `recover` returned `CONNECTED_VERIFIED` in 58.7 ms; final `diagnose` was `HEALTHY`. All four facade calls exited 0. Audit recorded exactly one `fixture-daemon-restart`; the before/after state-file hashes matched and differed from the fault hash. No installed daemon, WakeHost or external tunnel was restarted. | `TESTED_LOCAL_FIXTURE_ONLY`; installed recovery open |
| Recovery evidence files | Fixture `result.json` SHA-256 `BBC99415DC10AC89EA6C1958088C9C7F245D3517DADA127E403B39585007543C`; `run-recovery-fixture.ps1` SHA-256 `D0037AFE0894F4221C4A1BCE5AEF03855AE69EA4677A267879E7C62F53704A00`. The result binds public facade SHA-256 `3AF39540081BA7CDD925E618DD1C94B832C439F5CAFD273ED5B9C272731FD843` and lifecycle engine SHA-256 `EC7E67FED0CD851372C1FB928801614BCF3673C7A8044259E12920CF195034DD`. These local fixture files are not attached to Git. | `TESTED_LOCAL_FIXTURE_ONLY` |
| Protected Wake source | Commits `c8e9fa0` and `ab67d39` add a durable publication-attempt marker and Windows no-replace rename *fixture* proof; `da58c72` adds a private Sol burn-to-stage journal. Their scoped evidence is in `docs/orchestrator/review_bundles/T-0485_R29_PUBLICATION_ATTEMPT_MARKER_SOURCE.md`, `T-0485_R30_NATIVE_RENAME_FIXTURE_PROOF.md`, and `T-0483_R36_PRIVATE_SOL_BURN_STAGE_JOURNAL_SOURCE.md`. None installs Wake, publishes a package, or sends a fresh natural receipt. | `IMPLEMENTED_SOURCE_ONLY` |
| Installed facade and supervisor audit, 2026-10-15 22:29 ET | Public `catdesk.ps1 status` exited 0 with `LKG_AUTHORITY_MISSING`. Public `diagnose` exited 0 with `ACTION_REQUIRED`, primary `RECOVERY_AUTHORITY`, and `CANONICAL_HASH_MISMATCH`. HKCU Run autostart is enabled, but no matching scheduled task or supervisor mutex, CatDesk/WakeHost process, or port 3201 listener was observed. The available `target/release/catdesk.exe operator supervisor status` is an October 6 repository binary with root unavailable/startup unproven; it is **not** installed-process or current-source proof. No installed daemon or WakeHost was restarted. | `BLOCKED` for installed recovery and serving identity |
| Wake pointer and target audit, read-only | Local Wake `current.json` names a dev.84 image; the referenced binary SHA-256 `02431feba5ed73a5d40208b535d80cdc85c9a1c239058faf029f0311e51e89a2` matches that pointer. The separate status readback is stale dev.59, `STOPPED` with desired `STOPPED`. Wake target digest `81bc4d71d9a8927d08517cb70048572e86bd8b6fdc8516685f3fc38acdb3dfb8` differs from the repository registry target digest `b9c3c16a978e424ff4122f868450d4f7b8c540578dda2cc652cd3e04c557a3e3`; each record internally matches its own URL. These observations do not establish active owner, process, physical store authority or a paired target. No manual reconciliation or dev.85 installation occurred; raw target URLs are withheld. | `BLOCKED_WAKE`; no installed dev.85 or fresh receipt proof |
| Wake dev.85 source `BuildOnly` at `c124223` | At exact source `c124223`, `wake/install.ps1 -BuildOnly` exited 0 with offline locked Rust 1.97.0. Fresh `wake/target/release/CatDeskWakeHost.exe` is 1,653,248 bytes, SHA-256 `C770A7B90150120D7011D51C9FA1C69D11B1910843C707C1CDBB635531AE8726`; `wake/target/catdesk-gui/release/catdesk.exe` is 29,807,104 bytes, SHA-256 `E5E65E16F96E8CE0BD2BE5EFA4AEB446BDACDECC08E2C9D7C1523E7AE304DA4D`. Tracked `wake/adapter.py` and `scripts/wake_bridge.py` SHA-256 values are `F4E01F6E2FB392AE59D2A8AC173587A373A8B5FBB4106250A6E4302BA82ED20D` and `A1816D2DEC83D693300AA5D0FBB6B5C9B2FDFC0BC1B7303D66F1CBE57BDCBC9A`. No package manifest, stage, installation or activation was produced. | `TESTED_LOCAL` compile only; protected release blocked |
| CatDesk MCP v4 read-only transport | A single October 16 v4 `catdesk_transport_status` probe returned `UNAVAILABLE`: MCP SSE probe HTTP 404 from `openai.org`. No second connector call was made. Public installed `status` still reported `LKG_AUTHORITY_MISSING`; `diagnose` reported `ACTION_REQUIRED` with `CANONICAL_HASH_MISMATCH`. The source-built supervisor reported protected root unavailable, startup policy unproven and no port 3201 listener; no named CatDesk/WakeHost process was observed. The connector error does not prove its precise gateway cause. No external Secure MCP tunnel change was made. | `BLOCKED` for connected transport and installed recovery acceptance |
| Exact-source GitHub CI | After pushing source `378ae174d745e08732c2824f89fd3caf61d4cf6e`, `git ls-remote` returned that SHA; one read-only `gh run list --commit` returned `[]` and check-runs total was `0`. Repository Actions permissions previously read `enabled:true`, `allowed_actions:"all"`, and CI workflow `311012404` active. The workflow has push/PR triggers but no `workflow_dispatch`; prior manual HTTP 422 disabled-Actions wording is a separate unresolved signal. Billing API read previously returned HTTP 404 because the token lacks `user` scope. No remote CI acceptance is claimed. | `BLOCKED` for remote CI acceptance |

The subsequent 2026-10-15 registered-project audit was read-only. The registry bound CatDesk, Bug Bounty, and BYOVD to distinct Git roots, native thread IDs, and conversation targets. All three scoped `codex status` calls reported authenticated `codex-cli 0.160.0`; their operator queues held zero jobs and zero leases. CatDesk recovery still reported 35 in-flight queue blockers and 85 accounting review rows, so it did not authorize another native turn. Bug Bounty and BYOVD recovery views reported no recorded blocker, but their bound chats were actively in progress and their checkouts were dirty. No actual project job was enqueued or dispatched; a clear recovery view alone did not establish exclusive ownership of those active projects.

The subsequent Wake read-only audit found the public facade still at `LKG_AUTHORITY_MISSING`; `diagnose` reported `ACTION_REQUIRED` with canonical hash mismatch. The public release binary hash differed from its fingerprint, so it was not accepted as a reviewed last-known-good image. The dev.84 pointer's referenced files hash-matched, but no Wake host or listener was observed; separate status remained stale dev.59 `STOPPED` with desired `STOPPED`, and the target was unpaired. The protected broker SCM service, image, and root were absent, and `wake/install.ps1` intentionally refused non-`BuildOnly` installation. Exact-source dev.85 BuildOnly and CI proof were absent. These checks neither installed Wake nor established a fresh natural receipt.

The 22:29 installed audit was read-only and has no independently sealed audit manifest in this record. The pointer and target digests above bind specific local observations but do not identify an active owner or serving store; the October 6 repository release binary is not an installed-process receipt. The two target records' raw URLs are deliberately omitted.

The three-project fixture result is `%LOCALAPPDATA%\Temp\catdesk-3project-canary-20261015-211551-ee60ede1\run-20261016T020246Z-c3e6c480\result.json` (SHA-256 `E708FD946577BD98BC6FD492C619E99671BFF42AE3748C7E0F35584E2A09E4F0`). Its runner `run-three-project-canary.ps1` has SHA-256 `B4347C77F03E6E6291236ED26DB229D97D7627909808B20CA4F319437326687A`. The result identifies per-turn stdout/stderr hashes, three private native session-file hashes, scoped Git status after each turn, and four proof-file hashes:

| Fixture project | Native thread | Proof-file SHA-256 |
| --- | --- | --- |
| `alpha` | `01a14247-eee5-76b2-baba-ae74b0f003d4` reused for both turns | `canary-proof-alpha-01.txt`: `2A3F00842F0574D59E5271759FD524BB7DD0D2C608F1F93B155DE3FBC5FF580B`; `canary-proof-alpha-02.txt`: `663412955B1ABB92C1D01CE011BED139D49999829DB9B1084AE9A61B7769B104` |
| `beta` | `01a14248-0ff0-7cf1-a9b1-ea4a133a2a2c` | `canary-proof-beta-01.txt`: `04D22FCC2B4FB98B428FF971ABEB0706254C8757E7F9A5CA6E57867FE7FBC079` |
| `catdesk` fixture | `01a14247-c725-7c40-9f11-a8202821c9a1` | `canary-proof-catdesk-01.txt`: `4E14533688275E397D622F1671DC94E113599A5B7DF92E4FC163D02FA21AEA1D` |

The first native queue result is `%LOCALAPPDATA%\Temp\catdesk-3project-canary-20261015-211551-ee60ede1\queue-run-20261016T024835Z-29209e1c\result.json` (SHA-256 `1495D2DA1EF2DE1884A44CB32D99B916FBB5581C40F84D1546A3E36D34A5C8EC`). The repaired continuation result is sibling `queue-continuation-20261016T025707Z-29209e1c\result.json` (SHA-256 `4781BB6A350E1BC1020C2B4E49E37600518939750470172687BCDF1D86C8FA1D`). The latter binds source `ab6a3f75f7ad92b9183a3461a1b36eb83f03a5ad`, CatDesk image SHA-256 `6BE6F7ABACDA1E74B52E738C1E548AEAA148524FD5938B8D4A3AC613122A7954`, native Codex executable SHA-256 `fdda5fa3cf3fb3d000b876720742857676293e4315e4b045fae6f8bd7e866d1d`, per-call stdout/stderr hashes, scoped Git snapshots, and private native session hashes. Each result is a local fixture file, not a GitHub artifact.

An independent audit of the queue continuation matched all four fixture turns to their logs, proof files, native session records and process-exit receipts. It found no remaining fixture leases or native CLI children at that audit. The isolated Binagotchy read-only REPL stdout is `%LOCALAPPDATA%\Temp\catdesk-binagotchy-queue-readonly-20261015-2252\stdout.log`, SHA-256 `379BDC83BD1ADDF80BF86DBD30E294505B754E64A88938D67F8D0B2DD4A4BDA6`. The later failure-isolation result is sibling `queue-failure-isolation-20261016T030300Z-6646203c\result.json`, SHA-256 `CD401143467FD6932B1FD668BB16B2FA67FF792D3D662EED3BE7AAB9F9BA1DDF`. Its independent read-only audit matched the alpha attention/no-claim state, beta receipt and proof, restored origin, empty leases, and unchanged Desktop queue.

| Queue-selected turn | Attempt and receipt ID | New proof-file SHA-256 |
| --- | --- | --- |
| `alpha` 01 | `dceb41e8-d8db-43c0-9818-33fc884add99` | `500974CFA7A1C5692A3BA193ECA7BD6B5E3702BBB7D174E37D7FA2A98E22044E` |
| `beta` 01 | `5f05372b-1358-4cd5-9d52-143d871ccfcc` | `FE6740E6132B5B3FEDE2ABC467EEC362721C8A23E6CF39FF09D33F897789FD14` |
| `catdesk` 01 | `6eaf541f-63d6-4dc9-8a67-59ba877b539d` | `DA766D4B94394FEB64AC38C35A248654BB2E628B6E7A5F50EBD338F23672F081` |
| `alpha` 02, same thread | `2e7379b1-c19c-48a2-a0b6-b8756ebee945` | `A0C80840C5A09A5A8E5C4814BFE333C904F38DB2F2F9047B472B49AE5972761A` |

The live read-only recovery checkpoint is `%LOCALAPPDATA%\Temp\CatDeskCodexRecoveryP1-20261015-222051-c119b955\manifest.json`, SHA-256 `5E5B647ACF530EC78812823114550868379BBFF3BA1B1A57BD77651373C9D125`. It binds source `f5c89c85e50860d60b4a87ffff957dd92052586e`, binary SHA-256 `7739CF3962F709BD800803EBD9C9B5F1DDCA44F044E4DE31EABA5EE5A1172936`, `codex-recovery-status.json` SHA-256 `12FE90DCCB567E4C1686BC817B62891D1D238D1E89EEAEC3B90A9239D4C80070`, and `codex-recovery-prepare.json` SHA-256 `B1F3CAEAD542190705D8AF884BA62DFB6A0ABE1315DFDE99132749F0013425F3`. Both outputs share diagnostic evidence digest `1f04954f01b6328cb5a7bd06f369e405f98a221f8481541d80bc1ebd56194820`. The observed commands, each exit 0, were `catdesk.exe codex recovery status --project-id catdesk --workspace <Desktop CatDesk root>` and the same with `prepare` in place of `status`. The outputs may contain private session and task IDs; keep the full JSON local and use this aggregate record for sharing.

GitHub reports repository Actions `enabled:true`, `allowed_actions:"all"`, and workflow `311012404` active. The committed `.github/workflows/ci.yml` has `push` and `pull_request` triggers and no `workflow_dispatch`. A prior `gh workflow run` attempt was recorded as HTTP 422, `Actions has been disabled for this repository`; the raw response body and headers were not retained. Missing `workflow_dispatch` explains why that manual command is not a valid acceptance probe, but does not explain the absence of push runs. A fresh read-only check after pushing documentation commit `3eff5db` on October 16 still returned no workflow runs and zero check runs for that SHA, while the repository Actions permission endpoint again returned `enabled:true`, `allowed_actions:"all"`. Billing or an administrative restriction is not established. See `docs/orchestrator/review_bundles/T-0485_R14_CI_SCHEDULING_READ_ONLY_TRIAGE.md`. No dispatch retry or settings change was made for this record. If the repository owner sees an administrative disable banner despite the enabled settings, they should take that evidence to GitHub Support.

At exact `c124223`, the rebuilt source CLI's unsandboxed read-only `codex status` for the registered Desktop CatDesk root exited 0 with `authenticated:true`, native `codex-cli 0.160.0`, and `recordedTurnGate` of `NEEDS_OWNER_RECOVERY`: 35 queue blockers, 85 accounting review rows, no pending writer marker, `launchAuthorized:false`, `writerLockChecked:false`. A follow-up read-only recovery status exited 0 with the same diagnostic evidence SHA-256 `60b3ce930bcfcffbb7873fe3beaa83301c2c65099dc1f8d11a625a8aeb436d70` as the earlier checkpoint. A sandboxed probe lacked access to the registered Git scope and returned `CODEX_SCOPE`; the normal-access readback is the product observation. No model turn, queue claim, marker or managed record was created.

A second October 16 read-only `codex recovery status`/`prepare` audit at the same source and debug image returned the same 35 queue blockers, 85 accounting rows and diagnostic digest. All 35 claim rows lack recorded owner process identity and source-bound child-exit proof, and their accounting records are unended; 13 have open spans and 27 have incomplete Codex turn profiles. `prepare` lists missing proof but does not reconcile a claim or authorize a turn. Claim-specific original-owner/child-exit evidence and a reviewed accounting disposition are prerequisites to any exclusive-lock reconciliation; all historical claims remain frozen.

A read-only installed-release audit found a canonical `target/release/catdesk.exe` SHA-256 `9A8F4EBF6EA57CBB61EFAA12E8AFA54D18B8DB7374B9884067325BC84565568C` against sidecar SHA-256 `A264C8DC85FA55311E74114158F4F4CD598F6E9E9D0A6CA535E14D18AA649939`. Slot-a matches the sidecar but its manifest uses legacy `operational_verified`, which the current recovery policy rejects. Reviewed promotion and authorization/claim records are absent. The ordinary promotion helper checks the prior canonical pair before planning, so a fresh built candidate and approval alone cannot repair this host. Protected signed image epoch 1 remains accepted and epoch 2 is pending without an installed rotation receipt; today's readback did not cryptographically reverify that signed history. An owner-approved signed bootstrap or pending-rotation decision and a separately reviewed canonical-pair recovery transaction are required. No installed files or process were changed.

A read-only three-project readiness audit found zero operator queue jobs and an empty scheduler lease directory. Registry SHA-256 `6F06C8EE771D9B44D05216D079A7E51B7734F49ED20B56B085BE3FBE73A76FBF` still binds three distinct roots and threads. The CatDesk root is blocked as above; Bug Bounty's bound chat was active with 237 untracked files and unconfirmed external program scope; BYOVD's bound chat was idle, but this audit established no owner handoff; its root had six untracked documents. Both Python registrations still specify `rust_full` despite tracked `pyproject.toml` and `uv.lock`. Queue verification requires a clean initial Git witness and matching profile. None of these actual roots is ready for safe verifiable dispatch. After owner coordination, the next bounded slices are an owned localhost Bug Bounty negative pilot and a BYOVD pinned metadata-only corpus report with static review of already retained evidence. Any fresh driver-byte analysis requires separately reviewed isolated-input handling; no production-host driver loads. External program requests require confirmed written scope.

The edit/test turn used this task and argument order. On this host `$repo` and `$scratch` expand to the original literal absolute executable and workspace paths. It is historical evidence; rerunning it in the already edited thread would request a duplicate change.

```powershell
$repo = Join-Path $env:USERPROFILE 'OneDrive\Desktop\Projects\CatDesk-codex-loop'
$scratch = Join-Path $env:USERPROFILE 'OneDrive\Desktop\Projects\CatDesk-codex-native-cli-canary'
Set-Location -LiteralPath $scratch
& (Join-Path $repo 'target\debug\catdesk.exe') codex run --project-id catdesk --workspace $scratch --task 'In src/delegated/codex_model_policy.rs add one focused unit test for CodexCliTurnProfileEvidenceV1::matches that rejects an empty turn ID, a turn ID longer than 128 bytes, a nonhex evidence hash, a wrong thread ID, and a wrong model ID. Keep the production code unchanged. Run only that new test with cargo test --bin catdesk and report the command and result. Do not edit other files or commit.' --model gpt-6.1-sol --effort medium
```

## October CatDesk requirements

| Requirement from `01_CATDESK_CODEX_HANDOFF.md` | Result at this snapshot | Evidence needed to close |
| --- | --- | --- |
| A. CatDesk connects to authenticated Codex and completes a real authorized edit, test and review in a preserved thread | **PASS — `TESTED_LOCAL`.** The approved scratch CatDesk route edited and tested a focused source test, preserved the bound native thread, and checkpointed the reviewed edit as `3d4c612`. The October 16 `d512754` source image then completed another test-only edit in that same thread with `gpt-6.1-sol`/medium; the focused test and four-test policy suite passed, independent read-only review passed, and scratch commit `24803f9d630d6ab7401a201acf94ff3adcebf2ec` is clean. The later `46912f7` image has read-only status proof, not another model-turn proof. | Repeat against the intended installed entry point before assigning `INSTALLED_LIVE`; exact-source remote CI is still open. |
| B. CLI accepts another turn without Codex lock or starvation | **PARTIAL.** The October 16 scratch run was the fourth completed turn in the preserved thread, and post-turn status showed the same thread with zero recorded blockers and no pending writer marker. Earlier isolated scratch routing also completed four turns across three roots, including a repeated alpha thread. Standalone and through-CatDesk `codex resume --all` opened the authenticated All-session picker; both exited without selecting a session. The final `46912f7` image's read-only actual Desktop status remains authenticated but `NEEDS_OWNER_RECOVERY` with 35 queue blockers, 85 accounting review rows and `launchAuthorized:false`. Previously observed Desktop scoped `run`/`resume` attempts refused with `CODEX_LOCK`; no latest-image Desktop turn was attempted. Source `378ae17` bounds noninteractive run wait in a scratch child fixture; interactive resume remains user controlled. | Provide a proof-backed owner handoff/reconciliation for historical in-flight state without clearing an ambiguous live child; demonstrate Desktop `run` and exact-thread `resume`, cancellation/orphan handling and lock release. |
| C. Direct progress survives CatDesk/Wake failure | **PASS, local scope.** Native authenticated Codex edited and tested while the local supervisor readback was unavailable and no WakeHost was observed. The local CatDesk CLI route also completed independently of installed Wake. | Retain the native fallback and recheck it during installed recovery work. |
| D. Operator diagnoses and recovers one safely induced layer failure | **PARTIAL — `TESTED_LOCAL_FIXTURE_ONLY` for recovery; installed recovery blocked.** The public facade diagnosed an induced file-backed local-daemon fault and returned `CONNECTED_VERIFIED` after exactly one fixture restart in 58.7 ms. All calls exited 0 and final fixture health was `HEALTHY`. Actual-control `codex recovery status`/`prepare` only read writer blockers. The installed public `status`/`diagnose` calls exited 0 but reported `LKG_AUTHORITY_MISSING` and `ACTION_REQUIRED` with `RECOVERY_AUTHORITY`/`CANONICAL_HASH_MISMATCH`; no serving process or port 3201 listener was observed. No real daemon, WakeHost or external runtime was restarted. Source `378ae17` additionally passes a core-ready/Wake-pending public fixture without claiming full recovery. | Establish current installed authority and canonical image/store identity, then repeat authorized bounded public recovery under the reviewed release and ownership gates and verify the intended adapter without changing the external Secure MCP tunnel. Reconcile writer ownership only through a separately reviewed, source-bound handoff. |
| E. Three independent project jobs route and resume without path, thread, lock or receipt cross-talk | **BLOCKED for integrated real-project acceptance; native scratch queue continuation and one failure-isolation fixture passed independent audit.** Three real registrations, control-root resolver, fair selector, writer guard and explicit same-user operator queue are committed. A first fixture dispatch failed before claim on absent scheduler state, leaving four READY tasks; `ab6a3f7` repaired it. The continuation then selected and ran four native Codex turns through the queue in `alpha→beta→catdesk→alpha` order, with three distinct scratch roots/threads, repeated alpha thread, four root-scoped proof files, process-exit receipts and exact lease releases. Its independent audit matched logs, proofs, sessions and receipts. A separate earlier fixture established wrong-workspace refusal; the beta task paused/resumed quiescently during queue setup under `12c25d0`, so exact-`ab6a3f7` live enqueue/pause/resume remains source-test-only. A later audited `ab6a3f7` fixture left origin-drifted alpha in `ATTENTION` without claim and ran beta with a process receipt and released lease; alpha needs operator review, not automatic retry. Desktop operator queue status stayed at zero jobs before/after. These are disposable registrations and process-outcome receipts. The actual CatDesk/BYOVD/Bug Bounty jobs, their canonical conversation receipts, real fair rotation, and real-project failure isolation remain unproven. Source `378ae17` makes Binagotchy identity readback match the complete dispatch binding, but did not launch an actual project job. | Safely reconcile Desktop writer blockers, then demonstrate three authorized actual project tasks with bound roots/conversations/threads/locks, deliverable review receipts, fair rotation, pause/resume and failure isolation. |
| F. Fresh natural Wake receipt, or explicit unresolved status | **BLOCKED_WAKE, explicitly unresolved.** Local pointer data names dev.84 and matches an on-disk image hash, while status is stale dev.59 `STOPPED`/desired `STOPPED`; the Wake and repository registry target digests differ. No active owner/process, paired target, fresh manual or natural event, current-generation exact receipt, or dev.85 installation was verified. | Independently reconcile installed WakeHost owner/store and paired project/Wake target through the approved guarded policy, then one fresh authorized event and clean receipt if the protected gate permits it. Preserve ambiguous historical events without replay; do not edit either target store manually. |

`QUALIFIED_REAL_SITE`, `INSTALLED_LIVE`, `ACCEPTED_MVP` and overall `MVP_ACCEPTED` are **not** assigned to this CatDesk snapshot. The overall disposition is `PARTIAL`.

## Historical three-minute evidence replay at 46912f7

This historical read-only sequence replays the proof available at `46912f7` and stops at its recorded blockers. It is not an end-to-end October MVP demo. At that checkpoint, the tested source and tree were `46912f7eec01d1002f4bcfbf053e7a041960e5d2` / `d4803ae5992c82470d7ba4bc2dc6a37128bb7fd6`; checkpoints through `17c8144` changed documentation only at that time. The current tested source is `31d72faaa07e842c36c028012ffa7d806900376e` above. The fourth scratch turn is commit `24803f9d630d6ab7401a201acf94ff3adcebf2ec`. The commands display any subsequent source changes rather than silently treating a newer HEAD as tested. They use no credentials in arguments and do not send a Wake event.

```powershell
$repo = Join-Path $env:USERPROFILE 'OneDrive\Desktop\Projects\CatDesk-codex-loop'
$scratch = Join-Path $env:USERPROFILE 'OneDrive\Desktop\Projects\CatDesk-codex-native-cli-canary'
$testedSource = '46912f7eec01d1002f4bcfbf053e7a041960e5d2'
Set-Location -LiteralPath $repo
git rev-parse HEAD
git rev-parse ($testedSource + '^{tree}')
git diff --name-only $testedSource HEAD
Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $repo 'target\debug\catdesk.exe')
codex --version
codex login status
Set-Location -LiteralPath $scratch
git show --check --oneline --stat 24803f9d630d6ab7401a201acf94ff3adcebf2ec
git log -1 --format='%H %T %s'
Set-Location -LiteralPath $repo
Push-Location -LiteralPath $scratch
& (Join-Path $repo 'target\debug\catdesk.exe') codex status --project-id catdesk --workspace $scratch
Pop-Location
& .\catdesk.ps1 status
& .\catdesk.ps1 diagnose
& .\target\debug\catdesk.exe operator supervisor status
Get-Content -LiteralPath (Join-Path $env:LOCALAPPDATA 'Temp\CatDeskRecoveryFixture-5eb879952a98478fa043496271be2f6c\result.json') -Raw | ConvertFrom-Json | Select-Object label,sourceRevision,actions,actualRestart
Get-Content -LiteralPath (Join-Path $env:LOCALAPPDATA 'Temp\catdesk-3project-canary-20261015-211551-ee60ede1\run-20261016T020246Z-c3e6c480\result.json') -Raw | ConvertFrom-Json | Select-Object outcome,fixtureOnly,catdeskBinarySha256,intendedOrder
Get-Content -LiteralPath (Join-Path $env:LOCALAPPDATA 'Temp\catdesk-3project-canary-20261015-211551-ee60ede1\queue-continuation-20261016T025707Z-29209e1c\result.json') -Raw | ConvertFrom-Json | Select-Object sourceCommit,binarySha256,outcome,fixtureOnly,intendedOrder
Get-Content -LiteralPath (Join-Path $env:LOCALAPPDATA 'Temp\catdesk-3project-canary-20261015-211551-ee60ede1\queue-failure-isolation-20261016T030300Z-6646203c\result.json') -Raw | ConvertFrom-Json | Select-Object sourceCommit,binarySha256,outcome,fixtureOnly,alphaOriginRestored
Get-Content -LiteralPath (Join-Path $env:LOCALAPPDATA 'Temp\catdesk-3project-canary-20261015-211551-ee60ede1\queue-accounting-guard-2636f78-5f0c4e9a\result.json') -Raw | ConvertFrom-Json | Select-Object sourceCommit,binarySha256,outcome,fixtureOnly,receiptId,proofSha256,leaseReleased
Get-Content -LiteralPath (Join-Path $env:LOCALAPPDATA 'Temp\catdesk-queue-advisory-v2-20261016-41bf7cf355c1\run-one-shot-advisory.result.json') -Raw | ConvertFrom-Json | Select-Object sourceCommit,binarySha256,status,receiptId,advisoryVerdict,advisoryOutcomeSha256
Get-Content -LiteralPath (Join-Path $env:LOCALAPPDATA 'Temp\CatDeskCodexRecoveryP1-20261015-222051-c119b955\manifest.json') -Raw | ConvertFrom-Json | Select-Object sourceHead,binarySha256,status,observedSessionCount,queueBlockerCount,pendingWriterMarker,launchAuthorized
& .\target\debug\catdesk.exe codex status --project-id catdesk --workspace $repo
& .\target\debug\catdesk.exe codex queue status
gh run list -R vnath99/CatDesk --commit $testedSource --json databaseId,headSha,status,conclusion,workflowName
```

The scoped edit/test and exact-thread continuations are proven locally. This replay reads the fourth scratch commit and historical fixture evidence without rerunning a test, sending a duplicate model task or creating a review package. The third fixture's result readback shows an advisory verdict while the package and outcome remain Wake-ineligible. Do not rerun any dispatch or advisory script: each one-shot claim prevents a second attempt, and an ambiguous attempt must remain for owner inspection. Public recovery, manual three-root routing, and queue routing results remain fixture-only; actual-control writer recovery has **read-only diagnosis only**. The public facade status/diagnose calls are read-only and currently report missing recovery authority and a canonical hash mismatch. The source-built `operator supervisor status` does not establish an installed serving process. For the eventual three-minute *installed acceptance* demonstration, add an authorized public recovery command and actual three-project queue/deliverable receipt readback. Capture the installed image identity, effective Codex model/effort, sanitized receipts and exact-source CI. Those steps remain unproven today.

## Safety, privacy and next fixes

Outside the documented repository source and documentation commits, native canary/runtime mutations were confined to test-only scratch source edits, disposable file-backed fixtures, and their separate same-user queue namespace. The failed first dispatch preserved four READY fixture tasks; repaired dispatches created only scratch proof files and process receipts. Desktop queue status read back zero operator jobs before and after. Actual Desktop writer recovery, public facade, process/port, Wake pointer/target and MCP transport checks only read state; they did not hand off ownership, clear the 35 queue claims or 85 accounting review rows, close spans, launch a child in the Desktop project, reconcile either target store, or restart a process. No installed daemon, WakeHost, protected package/pointer or external Secure MCP tunnel was changed, no historical `SUBMITTING` event was replayed, and no job application, bounty action, driver load or funded trade was performed. Private native session logs and local project registries stay outside Git; this record publishes only aggregate recovery counts, hashes/digests and scratch canary thread identifiers, without raw target URLs. Preserve user authentication and OS/admin prompts, approved workspace boundaries, and separate protected-release authorization.

The current source bounds direct prompt delivery and improves supervisor layer classification, but neither is installed acceptance. A direct prompt failure retains its writer marker until source-bound owner cleanup is proven. At source `378ae17`, noninteractive `run` has a 12-hour post-prompt wait ceiling; interactive `resume` remains unbounded. The public supervisor fix was verified only against local fixtures, and the installed public facade still reports missing recovery authority.
The October 16 source adds a truthful status gate and exact marker identity for future queue attempts; it does not reconcile the 35 historical claims or prove descendants have exited. At source `c124223`, `BuildOnly` produced release binaries; source `378ae17` has a tested local debug image, while the protected broker, owner-reviewed promotion, exact Windows CI and paired Wake target remain absent.
The immediate CatDesk fixes are: obtain claim-specific source-bound completion or cancellation evidence from the original owners before attempting a reviewed transaction to reconcile the 35 historical claims and accounting rows. Preserve claims whose ownership cannot be proven; any quarantine/abandonment requires an explicit scoped human decision and must retain their output and accounting. The native queue continuation and one origin-drift failure-isolation fixture passed independent audits. A separate read-only reviewer passed the first verified fixture package's bounded edit. The first native advisory attempt hung during stdin close; a second returned `INCONCLUSIVE` because its read-only shell could not open the private package. The third, v2 fixture produced a valid `NO_FINDINGS_REPORTED` advisory and passed independent structural and artifact audits. Its short model response does not establish inspection thoroughness or an approved independent product review, and its complete private package is retained in the reviewer rollout. Actual project deliverable edit/test/review evidence remains open. Demonstrate three authorized real-project jobs with fair routing, scoped resume, and one project failure leaving the other two available; the scratch origin-drift case does not satisfy that integrated gate. Read-only `recovery status`/`prepare` identifies blockers but does not grant launch authority or reconcile them. Shared writer coordination remains scoped to the same OS profile; cross-account ownership and public crash-marker recovery are not proven. For installed recovery, resolve `LKG_AUTHORITY_MISSING` and `CANONICAL_HASH_MISMATCH`, identify the actual serving process/authority, and verify bounded public recovery before any installed acceptance. For Wake, independently establish owner/store identity and the paired target through approved policy before any event; the dev.84 pointer alone cannot authorize dev.85 or a fresh receipt. Exact-source CI remains open. The local Codex route also needs an installed-entry-point demonstration. Binagotchy's scoped resume and recovery controls remain source/fixture-tested, with actual installed behavior open. Automatic context rollover, polished Binagotchy UI, broad parallel scheduling, and optional isolated Sol-worker hardening beyond the safe source lane remain later work under the October directive.

**What a human can actually do today:** use authenticated native Codex in an approved workspace, run a scoped authenticated edit/test through the locally built CatDesk CLI in an isolated scratch registration, inspect the committed scratch diff and rerun its test, read the queue-selected four-turn three-root fixture results and proof files, and read the private package for the already verified native fixture without repeating its dispatch. A separate reviewer checked the first fixture package read-only. The third CatDesk native reviewer fixture now reports `NO_FINDINGS_REPORTED`, with zero tool-call events in its rollout and a matching inline package; the stored result remains advisory, `independentReview=PENDING`, `wakeReady=false`. They can inspect three real registrations and historical queue rows through Binagotchy, use its Wake-unavailable fixture REPL, inspect actual Desktop writer evidence gaps with read-only `codex recovery status`/`prepare`, read public installed `status`/`diagnose` authority failures, and replay the bounded file-backed recovery facade drill. Fixed Python verification profiles are in source but not migrated into the active project registry. Previously observed actual Desktop CatDesk `run`/`resume` attempts refused with `CODEX_LOCK`; exact-`c124223` status still denies launch authority. Installed recovery, durable real-project execution, Wake, remote CI and an installed production route remain unproven.
