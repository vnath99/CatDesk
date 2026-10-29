## October 28 — existing Secure MCP owner bearer bridge qualified locally

Source `344ff8167e38c9bee59deffbe0bb9b20f679f5a7` adds explicit `-ForwardLocalMcpBearer` to the existing Secure MCP setup helper. It consumes the already supplied process-local CatDesk bearer and passes an Authorization environment reference only to the owned native connect child. Default setup behavior stays unchanged. Invalid tokens and conflicting header policies refuse before connect; validated loopback targeting and bounded native execution remain required. No token is put in arguments, CatDesk presentation records or the parent environment. Managed-runtime inheritance and persistence still require separate live qualification.

Seventeen checks passed on each of PowerShell 7 and Windows PowerShell 5.1, including real child processes and Connect-Runtime's validation/command boundary. Existing route/migration regressions passed on both hosts. An actual trusted native-client loopback test through the new helper observed the correct MCP bearer at the MCP origin and no MCP bearer at a separate control-plane origin. The existing native timeout stopped the fixture client after two seconds; both local servers and the enclosing 30-second Job cleaned up. Earlier doctor runs returned success without forwarding the header, so doctor success is not authentication proof. A prior five-second native diagnostic exceeded its output bound and is preserved as partial evidence, not a pass.

This is source plus an actual native-client local fixture, not official runtime recovery or two-ChatGPT-conversation acceptance. No official profile, authentication setting, tunnel, browser, Wake target, native project owner or installed executable changed. The new setup helper is not integrated into the canonical installation. GitHub source SHA is verified; exact-commit CI has zero visible checks/runs and remains UNVERIFIED. The connector's last HTTP 429 was not retried. Next is identifying the existing supported installed serving path for the intended control-registry scope, without merging registries or adding another release mechanism. Evidence: `docs/verification/NATIVE_MCP_BEARER_FORWARDING_2026_10_28.json`.

## October 28 — actual default-profile backend qualified; central dispatch prerequisites exposed

One monitored, 180-second-bounded run used the installed C254 executable and the existing normal configuration, without a bearer overlay. Public backend-only startup, initialize, a 101-tool catalog, read-only canonical registry access, wrong-route HTTP 404 and public shutdown passed. The existing opaque-route capability accepts requests at the exact route without a bearer; no-header and intentionally wrong-bearer requests returned 200. This is not bearer-authentication acceptance. Earlier temporary-token qualification separately established 401/200 behavior and 102 tools.

Preservation initially reported one changed configuration among 10,278 protected files. Independent analysis subtracted the exact single-call accounting delta in memory and reconstructed the complete prior file's SHA-256. This proves the only change was usage accounting; authentication, route and all other settings stayed identical. The valid accounting update was retained. All other 10,277 protected files and the complete file set were unchanged. No test-owned backend/browser/Wake process or port-3200 listener remains. The installed executable/fingerprint and trusted previous release remain preserved.

The installed canonical registry contains four native-bound projects, three with Web URLs; canonical Wake2 is Web-unbound and PortfolioDesk is absent in that scope. The separate controlled Web scope contains the five original project conversations plus canaries. They were not merged or overwritten. Existing native ownership must survive any supported scope reconciliation.

Source `56b7093a44bc2a8052e3e33dc6fb64ff186dceb8` adds `centralProjectExecution` to transport status using the same bearer and mode prerequisites as central dispatch. It reports the exact blocker while retaining task-grant requirements, `executionAuthorized=false` and no claim of provider-conversation or installed Web acceptance. All 45 server regressions (one opt-in test ignored), including the HTTP status/catalog matrix and existing authentication gates, formatting and strict all-target Clippy passed. This status addition is source-qualified and is not installed in C254. The source GitHub SHA was read back exactly; zero exact-commit check/workflow runs are visible, so CI remains UNVERIFIED.

The normal profile's 101 tools omit central task dispatch because no bearer is configured; the earlier authenticated profile advertised 102. Authentication was not changed. A qualified authenticated serving connection and intended control-registry scope are prerequisites for real two-project Web coding. The separate connector's last result remains HTTP 429 and its supported owner last reported stopped/not ready; neither was retried. The stable port-3201 supervisor proxies traffic and does not start the port-3200 daemon, so its installation is not a new prerequisite for local backend recovery. Persistent serving, connector recovery and two independent ChatGPT Web coding conversations remain OPEN. No additional sign-in or repeated promotion is required by this result. Evidence: `docs/verification/DEFAULT_PROFILE_INSTALLED_QUALIFICATION_2026_10_28.json`.

## October 28 — central active Web work attribution qualified

Source `b8b42754ae59fc972e148e71a80a70d70b03b072` corrects the central report's omission of `source_review` work requests. Each active work intent now includes its exact delivery snapshot, checked against project, event, target generation/digest and message digest. SUBMITTING/ATTENTION require reconciliation; no status grants replay, execution or task completion. Historical queue records cannot be mistaken for another active request.

Advisory registry/Wake status reuses the existing target/history consistency validator without repeating workspace Git admission for every row. It explicitly reports `workspaceIdentityVerified=false` and `executionAuthorized=false`. Execution, task ownership and binding mutation retain their existing current-Git and lifetime checks. Two bounded diagnostics exceeded 45 seconds while making progress through project rows; the corrected reader passed against the same actual metadata in 29.95 seconds with a 45-second command / 60-second outer bound. All 369 protected metadata files remained unchanged. Four focused tests, eleven Web composition/gate tests, one target-identity regression, the actual metadata read, formatting and strict all-target Clippy passed.

The actual read distinguishes CatDesk's completed historical no-replay disposition from its current ambiguous review; Wake2 has its own separate ambiguous review. The other three original projects have no active Web request. Existing receipts also verify a later CatDesk delivery and completed assistant turn after the original disposition. This proves subsequent transport resumed; the original provider outcome remains UNKNOWN and no old message was resent.

This is tested source and current durable-metadata qualification, not installed two-conversation Web coding acceptance. C254's installed executable/fingerprint, all bindings and native project ownership are preserved; no browser, Wake worker, recurring task, tunnel, authentication or new release candidate was started/changed. These new report fields are not yet installed in C254. Connector HTTP 429 and qualified unattended serving remain separate open gates and were not re-probed. Source remote SHA is verified; CI has zero visible exact-commit runs and remains UNVERIFIED. Evidence: `docs/verification/ACTIVE_WEB_WORK_PROGRESS_2026_10_28.json`.

## October 28 — startup owner lookup corrected; installed strict caller PASS

Reviewed source `c014db86bbd872d07d99160694534299df4c3af2` fixes two observed startup defects. A bounded read-only Windows Task Scheduler query proves the fixed supervisor task is absent and returns `0x80070002`. The native classifier previously expected `0x8004130F`, which actually represents missing account information. It now recognizes only file-not-found with no returned object as absence; account, access, RPC and malformed-object failures still refuse ownership. Twelve focused tests and one opt-in test through the actual native COM path passed. No task was created, updated, run or deleted. This native fix is qualified source, not a new installed executable or supervisor activation.

The public backend readiness parser also depended on the caller's strict-mode setting: a normal successful JSON-RPC response omits `error`, and reading that absent property threw. It now checks property existence and actual JSON object shapes before reading. Eighteen success/malformed cases pass on each PowerShell engine; the complete bootstrap suites pass on PowerShell 7 and Windows PowerShell 5.1. Formatting and strict all-target Clippy pass.

Only the published launcher source was integrated into the canonical checkout under the existing stopped-lifecycle lease. The attested C254 executable and fingerprint remain unchanged. One actual installed public start/authenticated-MCP/stop cycle with caller strict mode left at Latest passed: HTTP 401 for missing/wrong credentials, HTTP 200 for initialize and 102-tool listing, verified installed process identity and cleanup. The final 8,502-file/two-metadata-pin preservation check passed. Prior actual rollback qualification remains preserved; it was not repeated for this source-only change.

The installed supervisor still requires qualified installation/startup authority; do not expect its old classifier to change until a future approved executable release. Do not create a recurring task or relax ownership gates merely to obtain green startup status. The official runtime/connector dependency is unchanged and was not re-probed: last connector result HTTP 429, official runtime stopped/not ready. Real two-project ChatGPT Web coding and unattended operation remain OPEN. No browser, Wake, native project task, tunnel or authentication change occurred. Source publication is verified on the existing branch; exact-commit CI has zero visible runs and remains UNVERIFIED. Evidence: `docs/verification/STARTUP_OWNER_AND_STRICT_CALLER_2026_10_28.json`.

## October 28, 23:12 UTC — installed startup, authentication and actual rollback PASS

C254 is now installed and qualified through the public local-backend lifecycle. Two `start -BackendOnly` / `stop` cycles verified the actual installed path and SHA-256. Missing and incorrect bearer credentials received HTTP 401; authenticated initialize and tools/list received HTTP 200 with 102 tools. The qualified one-line launcher correction is published in `741e4aaa3407680006901fe7d3b982a92a260d30`; it changes launcher source only, leaving the genuinely attested C254 executable unchanged.

Actual rollback also passed. A retained exclusive read handle temporarily made the current recovery-slot artifact unavailable without editing bytes or ACLs. Public `recover -BackendOnly`, using existing release authority, restored the independently verified previous generation 2 pair. That older release correctly reports that modern backend-only startup is unsupported; its existing bounded read-only headless MCP served authenticated requests with 10 tools and exited naturally. Releasing the fault allowed public recovery to restore generation 3/C254, verify authenticated 102-tool MCP, then stop. Both immutable release slots, manifests, authority records and selection pointer remained unchanged. No fingerprint was independently rewritten and no recovery authority was fabricated.

The final scan passed all 8,502 hashed files and two metadata pins. All five registered project workspaces exist and their registry/Wake conversation URL, digest and generation pairs agree. Bindings and native-session ownership were preserved. Final inventory found no CatDesk image, test-profile browser owner or port-3200 listener. The backend is intentionally stopped after these bounded tests; persistent serving and unattended operation are separate open gates.

One post-recovery connector probe returned `MCP SSE probe returned 429 from openai.org`. The pre-existing official runtime's supported read-only status separately reports process stopped and not ready. No official runtime start, tunnel creation, authentication change, Wake start or recurring automation occurred. These observations do not establish that starting that runtime would resolve HTTP 429. Real two-conversation ChatGPT Web coding remains UNVERIFIED; completed rollover canaries were not replayed and unknown submissions remain preserved.

Source bootstrap tests pass on PowerShell 7 and Windows PowerShell 5.1, and the unmocked native capability regression passes on both. GitHub readback verified source commit `741e4aaa3407680006901fe7d3b982a92a260d30` on the existing development branch; zero exact-commit check/workflow runs are visible, so remote CI remains UNVERIFIED. Next eligible work is supported reconciliation of pending Web evidence without resubmission; two-project Web execution awaits connector availability and a qualified serving owner. No additional sign-in or operator terminal experiment is requested by these results. Sanitized evidence: `docs/verification/C254_INSTALLED_BACKEND_AND_ROLLBACK_2026_10_28.json`.

## October 28 — C254 protected promotion completed; installed startup qualification

The guarded production promotion completed at 22:55 UTC. The installed executable and recorded fingerprint match the genuinely attested C254 artifact SHA-256 `608c2e9735bfcf8373dcc72e62dcf34336118efab4db1557560e45fd3eeac3cf`, built from reviewed source `e4a99a8b6913c3db689a216f4ae4e0a7f6fff612`. Native RESULT verified `PROMOTION_COMPLETED` with `STOPPED_AFTER_BOUNDED_OWNER`; no disk swap transaction remains. The current verified release is generation 3, with the independently verified previous generation 2 retained. Final wrapper preservation and cleanup checks passed. This is actual protected installation evidence, separate from development-package selection.

The first public `start -BackendOnly` test refused `LOCAL_MCP_BACKEND_ONLY_UNSUPPORTED` and left no daemon. The executable itself correctly reports support. The launcher incorrectly read `Output` from the bounded helper, whose actual field is `Stdout`. The one-line correction retains the closed native capability query, identity checks, bounded execution and complete five-field validation. A new regression invokes the actual executable through the unmocked helper; it passes on PowerShell 7 and Windows PowerShell 5.1. A Windows-only fixture adjustment recognizes that .NET Framework cannot represent present-but-empty environment variables; production authentication rules are unchanged.

Installed public startup/restart, authenticated MCP, actual rollback and real two-project Web coding remain separate acceptance gates. No external Secure MCP owner, browser, Wake binding, recurring automation or native project task was changed by promotion. Source tests and retained rollback bytes alone do not establish serving or unattended acceptance. See `docs/verification/C254_PRODUCTION_PROMOTION_2026_10_28.json` for sanitized evidence and remaining gates.

## October 28 — installed source qualified; handoff status defect reproduced

The trusted C208 image passed bounded installed headless startup: unauthorized
401, authenticated initialize/list 200, exact ten-tool read-only catalog and whole
owned-Job exit. The first failed attempt and superseded historical handoff were
then retired by the supported native transition, preserving all seven original
files byte-for-byte and recording durable completion. No executable changed.

A fresh existing promotion preflight passed but confirmation stopped before a
disk swap, leaving a new local-backend `INTENT_RECORDED` receipt. The installed
C208 executable/fingerprint remain intact. Read-only reconciliation verified all
8,485 prior pins, six new failure/intent files, two metadata records and zero
owners/listeners; no disk transaction exists. No blind retry occurred.

Actual Windows PowerShell tests reproduced a false-failure check: the promoter
used unset/stale native LASTEXITCODE after a successful PowerShell-script call.
Source fix `958e8d73903d9661051d992ee47e948efadea6f0` captures the script's own
status inside the array expression. Both engine matrices and the existing suite
pass. Recovery now admits five mandatory records with optional attestation copy
and an exact expired, quiescent pre-swap local intent. It preserves UTF-8 BOM/raw
bytes and refuses unknown, completed or foreign receipts. Fourteen Rust checks,
14 worker-classifier cases, formatting and strict Clippy passed. This does not
clear Web rollover quarantine or create new source/build/LKG approval.

Next: qualify the updated controller against that same preserved intent, complete
reviewed retirement, integrate the exact tested helper correction and use fresh
normal promotion authority for the unchanged attested C254 candidate. Production
startup, rollback, connector and independent two-project Web coding remain open.
No operator action or repeated sign-in is currently requested. See
`docs/verification/PROMOTION_HANDOFF_STATUS_AND_INTENT_RECOVERY_2026_10_28.json`.

## October 28 — bounded promotion source bridge qualified in source

The native reviewed-promotion worker now reuses the existing installed-foreground
read-only command and exact ten-tool MCP verifier to supply a temporary old listener
from the already qualified stopped release. The normal guarded promotion/handoff
remains the only installation writer. Existing trust, original sealed config,
matching prior/candidate retirement and exclusive lifecycle ownership are required.
Its armed Job is never released; the source lifetime is limited to 120 seconds.
No browser, Wake, scheduler or tunnel starts. Tokens remain off command lines.

Qualification passed 66 selected tests (including two subprocess fixture entry
points), formatting and strict Clippy. Live qualification, metadata retirement and
production promotion have not yet occurred. Next: build the qualified controller
once, qualify the existing old image before retirement, and verify cleanup and
protected state. All project/native ownership and release artifacts remain intact.
See `docs/verification/PROMOTION_SOURCE_BRIDGE_2026_10_28.json` and the release
lifecycle bridge procedure. Installed and two-project Web acceptance remain open.

## October 28 — real failed-promotion inspection qualified; stopped startup still open

The qualified controller from source `c980eed0d299fe4bae3e661c543d6574f3e7f693`
completed one bounded read-only inspection of the actual failed promotion at
21:23:45–21:24:00 UTC. Native result: `ELIGIBLE`, including the exact superseded
historical receipt and existing trusted release/producer authority. All 8,484
protected files, two metadata-only records, profile identity and project state
were preserved; zero test owners remained. No metadata retirement, binary change,
browser action, new message or recurring process was performed.

A second prerequisite remains: the existing promotion's phase-one entry requires
a running canonical backend, while the current trusted C208 image lacks supported
backend-only startup. Retiring the records alone cannot restore service. Next:
qualify the existing guarded promotion/lifecycle path for a stopped current release,
with a retained trusted rollback and bounded owner, before any new promotion.
Do not start the broader GUI/Wake/tunnel stack as a workaround. No operator decision
is requested. Installed health, rollback, connector and independent two-project
ChatGPT Web coding remain open; zero remote CI checks/runs means CI unverified.
See `docs/verification/STOPPED_PROMOTION_RETIREMENT_2026_10_28.json`.

## October 28 — stopped failed-promotion recovery, source qualification

Source adds an explicit, stopped-only retirement branch to the existing release-pair
operator. It preserves the six failed promotion records and a provably superseded
historical handoff through pinned, no-replace native renames and a durable journal.
An interrupted transition fences new promotion until the same evidence resumes.
The original trusted executable, fingerprint, LKG authority and reviewed candidate
are retained; this operation creates no source review or build authority.

Qualification passed 86 selected tests (including two subprocess fixture entry
points), formatting and strict all-target Clippy. Actual Windows tests terminated
an owned process at both archive boundaries, recovered with a fresh owner and
refused retirement while the candidate was mapped despite an empty backend port.
Changed records, unknown receipts, collisions and uncertain ownership fail closed.
The existing independent-workspace lock behavior also passed.

This is source/native-fixture qualification only. The stopped installation has
not undergone this transition or a new promotion. Next: one bounded read-only
inspection of the real failed transaction, then independently qualify the exact
recovery evidence before any mutation. Installed startup, rollback, connector and
two-project ChatGPT Web coding acceptance remain open. No operator decision is
currently requested; browser readiness remains separately blocked and completed
canaries are not replayed. See the release lifecycle recovery section and
`docs/verification/STOPPED_PROMOTION_RETIREMENT_2026_10_28.json`.

## October 28 — attested reviewed-source candidate; promotion failure preserved

The exact acknowledged C254 snapshot at
`e4a99a8b6913c3db689a216f4ae4e0a7f6fff612` completed a supported producer retry
using the qualified helper correction in controller
`28af7fe8a8aa5dc066df0de0263b34d52fab85d7`. Native RESULT independently validated
`BUILD_ATTESTED`; all observed producer processes exited. The 37,000,704-byte
candidate has SHA-256
`608c2e9735bfcf8373dcc72e62dcf34336118efab4db1557560e45fd3eeac3cf`.
No new Web review or review authority was manufactured. Later development changes
are outside this candidate's accepted source scope.

The candidate passed actual backend-only capability, 401 for missing/wrong
credentials, authenticated 200 initialization/tool listing, its exact ten-tool
read-only catalog, natural exit and cleanup. Guarded production PREFLIGHT accepted
the exact authority. CONFIRM returned `PROMOTION_FAILED_OR_AMBIGUOUS`. It was not
retried. Reconciliation preserved the failed claim and all history, proved no disk
promotion transaction, no remaining owners/listener, and the original installed
C208 executable/fingerprint still matching
`91eea19e6e030e80900d43114f800c3642f5cc063ef2c969b83eb20d2c28d14b`.
Only the expected promotion-control records changed; 8,484 protected pins now
capture that reconciled state. The attested candidate remains retained.

Supported read-only `-ValidatePhase1 -BackendOnly` returned
`OPERATOR_ATTENTION_PHASE1`. A historical handoff receipt for a different image
cannot establish current candidate/process continuity. Do not delete that receipt,
clear the claimed attempt, rerun CONFIRM or copy the candidate into production.
Installed startup, rollback and connector testing remain pending restoration.
The connector was not rechecked against an unchanged stopped backend.

The expired, never-submitted C259 event was safely archived/retired and its native
claim cancelled, preserving history and quota. The operator confirms BYOVD history
loaded after sign-in; automation's `HOME` observation is not proof of failed login.
The development inspector now selects only a unique returned CDP tab whose target
metadata and independently read document URL both match the registered target,
then checks the selected document again. It does not type, navigate or close tabs
while selecting. Ambiguity, drift and unavailable inventory fail closed. Existing
bounded empty-Home recovery and all production probe controls remain intact.
Eleven Python tests and four actual-Windows probe tests passed; Wake formatting and
strict Clippy passed. One bounded live inspection observed TARGET_TAB_DRIFT then CHROME_ERROR; no ready authority or message was produced. The specific Chrome error code was not collected. All 8,484 protected files and two metadata-only records were preserved; zero test owners remained. No automatic retry occurred.

For unchanged valid acknowledged snapshots, the supported
`operator reviewed-build prepare --review-record-id <record>` / `confirm` / `result`
chain can retry an audited terminal producer failure without another Web message.
This does not make failed production promotion replayable or authorize later source.
See the C254/C262 verification records and `OPERATOR_ACTIONS.md`. Genuine two-project
ChatGPT Web coding and full October MVP acceptance remain open.
## October 28 C262 — bounded inspection of the existing development browser

Source adds `operator web-dev-runtime inspect-readiness --project-id <project>`.
It reuses guarded development-login preflight and the existing read-only readiness
probe, with the exact registered target and existing selected profile. It accepts
no profile, URL, message or timeout override. No new profile, text entry, submission,
queue mutation, host, scheduler or production policy change occurs. The legacy
production-only probe retains its PAUSED-host/acknowledgment requirements.

The shared probe runner requires START before browser imports, bounds the helper,
filters output to fixed labels and proves the whole owned Job empty before reading
its pipe. Unverified cleanup retains the existing sticky owner marker. Target
pairing is checked before/after inspection; observations grant no send permission
and do not establish authenticated identity. Qualification: 15 focused tests,
root/Wake formatting and strict Clippy passed. Actual Windows fixtures exercised
timeout cleanup and a descendant holding the output pipe; Python fixtures verified
profile selection and readiness labels without starting a browser.

This is source/native fixture qualification. Next: build the standalone controller
and perform one bounded read of the existing selected profile. No live readiness,
sign-in cause, ChatGPT review, new release authority or production acceptance is
claimed yet. The original C259 event remains prepared, with no browser resumption.

## October 28 C261 — actual no-send preparation of the original review request

The qualified standalone controller performed `review retry-pre-submit` against
the real, exactly frozen C259 request. It changed only that confirmed pre-submit
delivery from `ATTENTION` to `CLAIMED`. The original attention bytes are archived;
the request, reserved message, grant, native claim and queued event remain intact.
One of 8,471 protected files changed as authorized; the other 8,470, two metadata-only
files, all five bindings, profile identity and native sessions were preserved.
No new event/file, browser, message, timer or receipt was created. The bounded native
process exited successfully and zero test owners remain. This qualifies local
recovery metadata, not a ChatGPT review, authenticated session or production release.

The eligibility transition itself needs no browser authentication. Actual bounded
`review resume` remains waiting for browser readiness and fresh claim/grant checks;
it must reuse the existing frozen request. Do not repeat the preparation or create
another review identity. The legacy profile diagnostic requires a production
PAUSED host plus acknowledgment and is ineligible for the stopped isolated runtime;
do not change production policy to run it. Next: reuse the guarded development
profile preflight and existing read-only readiness probe for one bounded diagnostic.
No sign-in cause is asserted from the prior attention code alone. Production,
connector and two independent Web coding acceptance remain open.

## October 28 C260 — scoped recovery for a confirmed pre-submit review refusal

C259 exposed a recovery gap: Web review `resume` requires `CLAIMED`, but a confirmed
pre-submit login/profile refusal remains `ATTENTION`. The existing development
retry deliberately accepts only `dev-` events. Source now adds
`operator web review retry-pre-submit --project-id <project> --input-file <frozen-review.json>`.
It reuses the existing grant/reservation and exact queued event, preparing only
`ATTENTION` → `CLAIMED`; it never opens a browser, sends, refunds quota or starts a
host. Only the observed `LOGIN_OR_PROFILE_REQUIRED` pre-submit case is eligible.
Registered target, payload, audit reference, expiry, revocation and dispatch checks
remain mandatory, under existing project/registry/host/transaction locks.

A regression proved the old low-level retry could ignore a surviving submission
start witness. Wake now refuses a start witness, timer or archive under its existing
transaction lock. Qualification passed 70 focused tests, both formatting checks
and strict Clippy including tests. Fifteen unsafe workflow cases preserve fixture
bytes; another blocked project stays unchanged while the eligible project prepares.
These are source/isolated controls, not actual Web or installed production proof.

C259's frozen request, reservation, native claim and `ATTENTION` journal remain
untouched. No authentication/profile change or replay occurred. Standalone source
`5964217a250a6e6c39880c05092766029a73b33a` was built separately and its real Windows
process rejected a deliberately changed material payload with
`WEB_REVIEW_GRANT_SCOPE_CHANGED`. All 8,471 protected files, two metadata-only files,
five bindings, native sessions and the frozen request were preserved. No browser
opened, no message was submitted, no package was selected and zero test owners
remain. This is actual local rejection evidence, not installed/Web acceptance.
After operator browser inspection and actual readiness recovery, revalidate the
original request before any explicit retry preparation
and bounded resume. Production still needs genuine review and producer authority;
connector, installed rollback and two independent Web coding acceptance remain open.

## October 28 C259 — corrected controller healthy; fresh review held before submission

Source `28af7fe8a8aa5dc066df0de0263b34d52fab85d7` is published with exact remote
SHA readback. Its selected development app has SHA-256
`df0cab8196c889b72fa3fd3c7dc0b5c1ccc1b202897d8c161655ce7a0593496b`.
Supported six-second backend qualification verified the actual selected executable,
401 without authentication, 200 with authentication, 79 unique tools, natural
parent/child exit and 8,459 preserved files. Previous verified development artifacts
remain retained. This is local development qualification, not production promotion.
The new controller separately validated the native failed-build RESULT and retained
trusted C208 artifact without scheduling another producer or changing authority.

The existing registry admitted one frozen, bounded, read-only public-source review
under standing authorization. It returned `LOGIN_OR_PROFILE_REQUIRED` before the
browser submission boundary. Reconciliation proves `ATTENTION`, no dispatch marker,
no submission-start witness, no receipt, no timer and zero remaining test owners.
All 8,469 prior state pins and five project bindings were verified; two scoped
queue/delivery records produce a new 8,471-file baseline. No request was replayed.
The native review claim remains waiting, without source acceptance or a new build.
This differs from a submitted request with an unknown outcome. No authentication
expiry cause is asserted from the attention code alone.

Recovery uses the existing `operator web-dev-runtime login --project-id <project>`
from an interactive terminal, reusing the selected development profile. An operator
must inspect/sign in only in that browser if needed, preserve unknown drafts, and
press Enter in the terminal to close it through its owned cleanup procedure.
`LOGIN_WINDOW_CLOSED_UNVERIFIED` is not authentication proof. Fresh supported review
resume admission and immutable identity checks remain mandatory before delivery.
The development-only `retry` command requires a `dev-` event; do not use it for a
`web-work-` review or relax that restriction. No recurring host or scheduler starts.

Next: resolve the browser readiness gate, obtain genuine review of the frozen source,
then complete the existing source/snapshot/ACK/producer and guarded promotion chain.
Installed startup/rollback, separate connector connectivity and two-project Web
coding acceptance remain open. Exact-source CI still has no check runs. Production,
external Secure MCP ownership, native sessions and historical unknown requests are
preserved. No new operator permission decision is needed for the routine review;
actual browser sign-in, if required, remains a human action.

## October 28 C256 — instrument the existing unloadable-response read

C254 remains delivered once to its verified target, with no accepted assistant
body. Inspection found that the existing private DOM-shape diagnostic was skipped
when a load-error card appeared structurally complete. C256 now includes that same
bounded diagnostic for normalized load-error text. Text, target, receipt and refusal
rules remain unchanged; it adds no navigation, editor operation, retry or submission.

The regression reproduced two missing-diagnostic cases. All 32 adapter and 79
rollover/snapshot tests passed. Source review and publication do not qualify an
installed runtime. Next is a guarded development-package update and one instrumented
read of the existing C254 event, using explicit UTF-8 capture. C254's frozen request,
C249's unknown outcome, predecessor histories and native project work stay preserved.
Production promotion and two-project Web coding acceptance remain open.

## October 28 C253–C255 — live development health, unavailable Web review, diagnostic fix

The guarded C253 development package from published source
`e4a99a8b6913c3db689a216f4ae4e0a7f6fff612` is selected with its verified
predecessor retained. Its six-second live backend returned 401 without authentication
and 200 with 79 unique MCP tools when authenticated; actual child identity and
natural cleanup passed. This remains development qualification.

C254 delivered one standing-authorized read-only review with an exact USER receipt.
Bounded reobservation refused `RESPONSE_TARGET_DRIFT`; the separate supported read
reached the correct conversation and latest request but found an assistant load-error
placeholder. No substantive review, acceptance, ACK, snapshot or build authority
was created. The request remains preserved without resend. Final checks preserved
8,212 files, all five bindings and native session state, with no test owners left.
The separate connector HTTP 429 was not rechecked or identified as this UI failure's cause.

C255 fixes a reproduced source defect: wrapped load-error text could pass the review
reader. Whitespace/quote variants now remain unverified with the fixed diagnostic
`WEB_REVIEW_RESPONSE_LOAD_ERROR`; unfinished or incomplete bodies retain their
existing refusal. All 26 relevant Web CLI tests, formatting and strict Clippy passed
on the final source. Production and the selected development executable are unchanged
by C255; its installed/live qualification and independent source review remain open.
Captured diagnostic text showed a private harness encoding limitation; original
captures are retained and no repaired bytes were used as review authority.

Next: recover substantive evidence for the already delivered C254 request through
supported read-only mechanisms after the Web dependency changes. Genuine acceptance
can qualify its exact reviewed revision for the existing producer/promotion chain;
C255 needs separate qualification before inclusion in production. Do not create an
equivalent review or replay an unknown request. Recurring Wake/browser automation
remains off. Production startup, rollback, connector and two-project Web coding
acceptance remain open; no operator action is required for continued source work.
See the C253/C254 live-development and C255 source verification records.

## October 28 review and runtime qualification update

C252 was a verified independent Web review with a FAIL verdict; it establishes no
release authority. C253's route-CAS correction passed source tests and awaits fresh
review and development update. Keep the Sol OS-identity launch guard closed;
CLI login status is not qualification. Preserve C249's unknown request without
replay. Current evidence is in the C252/C253 verification records and project handoff.
## October 26 C249 — bounded helper rechecks qualified in source

A real regression reproduced the one-shot helper-cleanup defect: an unknown Job
member exited naturally after the first check, but the remaining pinned helper
was never reconsidered and the four-second fixture deadline expired. The red
test exited 101; the corrected test exited 0 with verified fixture cleanup.
Source now repeats the complete existing helper identity/membership check no more
often than every 250 ms within the unchanged deadline. Unknown members are never
selectively stopped. Timeout diagnostics distinguish root running, descendants
active and stderr EOF pending, using the existing failure record only after
confirmed cleanup and EOF; the timeout error code and authority gates stay intact.

Qualification passed 130 reviewed-build tests, 14 protected-filesystem tests,
formatting and strict Clippy, with eight manual tests ignored. This proves the
regression correction in source; the original production-build timeout's cause
is still unproven. A qualified controller containing this new code and genuine
review of its new frozen source are required before a fresh supported producer
attempt. See [C249 source evidence](../verification/C249_BOUNDED_HELPER_RECHECK_AND_TIMEOUT_DIAGNOSTICS_2026_10_26.json).

## October 26 C248 — actual Cargo probes did not reproduce the timeout

Four isolated Cargo probes crossed version/tiny-build cases with both console
settings. All exited naturally with code 0 in 206–406 ms, closed stderr and left
empty owned Jobs. They created no release authority. The diagnostic overlay was
removed and the original source restored before C249. These small probes did not
reproduce or explain C247's full-build timeout.
See [C248 diagnostic evidence](../verification/C248_ACTUAL_CARGO_GATE_PROBES_2026_10_26.json).

## October 26 C247 — managed review accepted; real producer timed out

Exact public source `98248023c8cd359476e5943d991c960354c5e168` has a verified
remote SHA readback. Its genuine 5,389-byte review passed the strict frozen schema
without an exception, then managed finalization and acknowledgment retained a
217-entry snapshot. The real producer subsequently returned
`REVIEWED_BUILD_TIMEOUT` without attestation. Temporary compiled bytes are not a
release candidate; the validated terminal failure and original C208 remain
preserved. Web work is now `COMPLETE`/`CLOSED`, with 8,159 protected pins verified;
transport completion remains caller-reported separately from native review.
See [C247 actual evidence](../verification/C247_MSVC_READ_ONLY_DIRECTORY_PIN_2026_10_26.json).

No new executable was promoted. Keep the existing PREPARE/CONFIRM/RESULT workflow,
using fresh qualified source and authority rather than repeating a failed CONFIRM.
Installed health, different-version rollback and two actual independent Web coding
conversations remain open. Exact-source CI is unverified, the connector's last
HTTP 429 is separate, and recurring automation remains stopped.

## October 26 C247 — read-only MSVC pin qualified in source

The real C246 producer terminated `BUILD_FAILED_OR_AMBIGUOUS` before Cargo:
`reviewed MSVC helper directory identity is unavailable`. No candidate or
attestation was produced. A bounded native probe established that directory
access mask 7 requested add rights and failed with Win32 error 5; read-only mask
129 opened the same non-reparse directory and read its identity. No ACL or
directory metadata changed. C247 uses an explicit read-only MSVC directory pin;
protected writable-directory guards retain their existing rights and checks.

Qualification passed 125 reviewed-build tests, 14 protected-filesystem tests,
formatting and strict Clippy; eight manual tests remain ignored. The first run's
one new test-only granted-access failure is preserved. Correcting its native
56-byte buffer and signed 32-bit information-class ABI enabled the full passing second run.
These are source results, not a new producer or installed acceptance.
See [C247 evidence](../verification/C247_MSVC_READ_ONLY_DIRECTORY_PIN_2026_10_26.json).

## October 26 C246 — actual review acknowledged; producer refusal retained

The genuine cumulative review of exact published source
`d9aed85d6a45ddb1b4a1dcb0a0d5205c5870188e` passed managed verification and
acknowledgment with its faithful 4,960-byte response and 217-entry source snapshot.
The original auxiliary schema failed because 23 limitation notes exceeded its
20-item cap. Two independent reviews accepted only that count exception: the
approved native contract imposed no cap and all other frozen rules passed.
The raw reply, schema failure, review decision and all 4,585 prior native file pins
remain preserved. A bounded fresh reobservation at 19:50:12 UTC proved the
previously `OBSERVING` Wake timer `COMPLETE`, preserving 7,914 other pins and
retaining two immutable witness logs with zero test owners left. No message was
resent. Supported closure then marked its transport record `CLOSED`/`completed`
at 19:54:54 UTC, preserving 7,916 pins with only the expected three ledger fields
changed. The API's `taskCompletionVerified=false` remains caller-reported transport
completion, separate from the accepted native review. No browser, send, native
execution or production action occurred. See [C246 actual evidence](../verification/C246_GENUINE_REVIEW_AND_MANAGED_ACK_2026_10_26.json).

Next: qualify a controller containing C247, obtain genuine acknowledged review and
snapshot for its new frozen source, then use the existing supported failed-build
generation succession with fresh PREPARE/CONFIRM evidence. Do not repeat the failed
CONFIRM or relabel C246 authority. Installed C208, five bindings and native owners
remain preserved. Promotion, installed health, different-version rollback and two
actual Web coding conversations remain open; CI is unverified and the connector's
last HTTP 429 is separate. Recurring automation stays stopped.

## October 26 C246 — local promotion handoff qualified in source

Normal reviewed-promotion preflight now has an explicit local-backend-only
profile, pinned into authorization and the authenticated handoff receipt.
Candidate capability, fixed loopback port 3200, configuration and authentication
are checked before stopping the old owner. Review-hash and PowerShell timestamp
compatibility corrections retain the existing integrity checks. Eight Rust
promotion tests, both PowerShell suites under versions 5 and 7, formatting and
strict Clippy passed. Actual read-only executable queries confirmed support in
the qualified development image and refusal by installed C208.
See [C246 source evidence](../verification/C246_LOCAL_PROMOTION_HANDOFF_SOURCE_2026_10_26.json).

This is source qualification. New material source still needs its frozen review,
producer attestation, candidate qualification and protected promotion. A bounded
promotion result reports serving separately; it does not establish a persistent
backend. Use the documented public `start -BackendOnly` only after qualifying a
supporting installed release. C208 remains installed; different-version rollback,
actual two-project Web coding and exact-source CI remain open. The connector's
last HTTP 429 is separate, and recurring automation remains stopped.

## October 26 C245 — genuine review acknowledged and transport completed

The selected development backend passed 401/200 authentication checks with 79
tools and natural cleanup. A new cumulative public-source review of exact
`e5a3ca9a9fc713bfba5d6b7c21f2d6e8e32780d1` passed with a verified 3,206-byte
reply and no actionable findings. Managed verification retained 217 snapshot
entries covering all 202 required build inputs. Finalization committed despite
an HTTP timeout; durable inspection established completion without replay.
The exact review was acknowledged, then its transport record was closed through
the supported completion API. That API still reports task completion as
caller-reported, separately from managed review acceptance. All 7,545 protected
files passed the closure check; only the expected transport-record fields changed.
See [C245 actual evidence](../verification/C245_CURRENT_SOURCE_REVIEW_ACKNOWLEDGED_2026_10_26.json).

Build preparation stopped in read-only harness preflight before any product
request or attempt marker; the C208 producer selection stayed unchanged. No new
candidate or production promotion followed. All five bindings, histories and
native owners remain preserved. Do not resend the completed C245 review.

## October 26 C244 — actual review snapshot verified

The C243 frame correction passed a real bounded read of the existing completed
review: exact USER attribution and the complete 6,397-byte assistant body were
verified. No message was resent. Matching normal-debug development artifacts
were built and selected through the existing guarded update. The older
controller's source-mismatch refusal is preserved; no state changed on that
failed preflight. The working previous package, profile, all five bindings and
4,577 native-session files are preserved, with zero test-owned processes left.
See [actual development evidence](../verification/C244_DEVELOPMENT_REVIEW_SNAPSHOT_LIVE_2026_10_26.json).

The reply reports source-scope PASS with eight positive observations and all
twelve required paths. It lists five additional public documentation paths,
which fail the frozen report schema. Its nonrenewable managed claim also
expired before finalization. The raw response and original contract are retained;
no report, source integration, acknowledgment, new producer or promotion followed.

The expired claim was cancelled through the supported API; its completed Web
work was explicitly retired. Other native files and accounting remain unchanged.
This closure does not grant review acceptance. Next: bounded development backend
health and a fresh scoped review of the new transport fix under standing authorization. Production installation, trusted different-version rollback and
two independent Web coding conversations remain separate open gates. Recurring
automation stays stopped; the connector's last HTTP 429 remains separate.

## October 26 C243 — private review snapshot frame

One bounded routine review was delivered exactly once and its turn completed.
The completed reply remains **unverified**: the old 8 KiB JSON frame trimmed
its preceding USER witness, although both texts fit the raw text limit.
No report finalization, new producer authority or promotion followed.

Source now gives only private SOURCE_SNAPSHOT frames a bounded 64 KiB wire
allowance. Other frames retain 8 KiB; text limits and exact USER/target checks
remain unchanged. Qualification passed 32 adapter and 71 conversation Python
tests, focused Rust framing/runtime/deadline checks, formatting and strict
Wake Clippy. The initial legacy-fixture failures and zero-match filter are
recorded with their corrections in [C243 evidence](../verification/C243_PRIVATE_REVIEW_SNAPSHOT_FRAME_2026_10_26.json).

Next: qualify matching development artifacts and re-read the existing event
without resending. Production, all five bindings and native owners are preserved.
Source tests do not establish installed acceptance. Current-source managed
review/snapshot, trusted different-version rollback and two Web coding
conversations remain open; the connector's last HTTP 429 is separate.
## October 26 C240/C241 — legacy producer compatibility verified

C239's bounded read-only check returned `attention`: four authentic legacy
failure audits omitted an optional field that current serialization added as
null. Their original raw hashes and retained fields matched. Source
`47922f3a6ad5af85a85e1bbd619cb758b24966f2` permits only that explicit schema1
omission; raw audit/plan hashes and all other fields remain mandatory. Schema2
success checks stay byte-exact. No historical file or digest was rewritten.
All 123 reviewed-build tests, four review-authority checks, six release-pair
checks, formatting and strict Clippy passed. [Source evidence](../verification/C240_LEGACY_AUDIT_COMPATIBILITY_2026_10_26.json).

The rebuilt debug controller then returned `BUILD_ATTESTED` for real C208
through 46 retained families/45 transitions. All 2,701 prior pins matched; no
test process remained. This was one read-only result command within a 60-second
owner, after the specific correction. C239 remains preserved, and no producer build,
generation selection, promotion, native-session action, browser, Wake or
recurring process ran. [Live read-only evidence](../verification/C241_EXISTING_PRODUCER_READONLY_2026_10_26.json).

C238's qualified-controller-only release-writer migration rule still applies.
Current source still needs genuine acknowledged review and an exact immutable
snapshot before a new producer or promotion may run. Old C196/C208 authority
cannot authorize it. The last connector result remains HTTP 429, and actual
two-conversation Web coding/installed acceptance remain OPEN. No operator action
is required for preparing the next source review within standing authorization.

## October 26 C238 — successful producer succession source qualified

Published source `f1f0fe24cf57581fac81fe959ef3aa384a137f4a` fixes a release-path
gap: an attested successful build previously blocked a later reviewed binding,
and producer replay could only read the active generation. The existing immutable
generation/plan/atomic-pointer mechanism now preserves a validated successful
predecessor and can replay exact reachable historical candidate authority.
An exact already-successful review returns `BUILD_ATTESTED` without rebuilding
or changing the active selection. No new registry or release command is added.

The first suite passed 121 tests; one static guard rejected safe test-call
syntax. Two test-only calls were retyped without weakening the guard or changing
production code. The corrected guard, five succession rechecks, four authority
checks, six release-pair checks, formatting and strict Clippy then passed.
All 2,701 prior state pins matched; test owners were absent. GitHub showed zero
Actions runs for this source; CI acceptance is not claimed. [Evidence](../verification/C238_SUCCESSFUL_BUILD_SUCCESSION_2026_10_26.json).

**Migration:** before any live succession, retain the exact qualified controller
and one release-management owner. Route all build and producer-dependent
promotion/reconcile operations through it after cutover. C208 can still read the
schema1 pointer but cannot replay old candidates; do not mix release writers.
Ordinary project MCP serving and existing terminal-receipt/LKG startup checks
remain independent. Existing PREPARE/CONFIRM/RESULT syntax is unchanged; do not
confirm a prepare already returning `BUILD_ATTESTED`. Historical compiler
identity checks remain strict.

This is source/fixture qualification only. No live producer generation, installed
binary, binding, native session, browser, Wake or recurring process changed.
Fresh managed review/snapshot authority for current source is still required;
old C196/C208 evidence cannot authorize it. Next is a bounded read-only current
producer compatibility check using a separately built debug controller. Connector
HTTP 429 and actual two-conversation Web coding remain open. No operator action
is needed for that read-only development check.

## October 26 C237 — development executable cold recovery LIVE PASS

The normal debug executable built from published source
`9eaa88c80cfd00432b34c37d3183c218a21b2a49` passed the 180-second local
canary. A new isolated task reached READY with zero calls and was terminated
through its retained process handle. Restarting the same assignment performed
source-managed recovery, preserved the old claim, archived its marker and
recorded all three recovery phases before creating a new owner claim.
The recovered task applied one assigned probe and passed its fixed verification
with zero model calls. It then exited naturally; its Job, process and pending
marker were absent, and the writer lock reopened. [Evidence](../verification/C237_DEVELOPMENT_TASK_RECOVERY_2026_10_26.json).

All 1,977 fixed prior pins remained unchanged; the one permitted context ledger
preserved its prior events and appended attributable MCP traffic. Native
sessions, original workspaces, legacy quarantine, installed C208 and all
bindings remained intact. No browser, Wake, recurring job or promotion ran.
Two preparation errors were corrected before any task launched. The completed
canary must not be replayed. This advances development-runtime recovery only;
installed qualification and genuine two-conversation ChatGPT Web coding remain
open. The last connector result remains HTTP 429 and was not polled again.

## October 26 C236 — zero-call task recovery source qualified

Published source `922b8056651f1ec6aa78bd9844aea306d69c9e99` adds cold recovery
for newly claimed, dedicated, bounded project-task processes. It reuses the
existing project lease and workspace writer lock. Exact owner/Job death,
unchanged assignment, clean worktree and zero tool activity are required.
Durable authorization precedes exact-handle marker archival; interrupted
recovery resumes without replay. Legacy or ambiguous markers remain blocked.
Normal dirty-worktree resumption retains its prior behavior.

All 60 focused tests, formatting and strict Clippy passed. Real Windows child
fixtures cover interruption, both sides of archival, recorded-call refusal,
legacy refusal and a warm resume with 90 retained dirty files. The first run's
unsupported fixture tool name was corrected before this passing frozen run.
All 1,978 prior state pins matched; test owners and test executables were absent
afterward. The source commit has no visible GitHub Actions run; CI acceptance
is not claimed. [Detailed evidence](../verification/C236_ZERO_CALL_TASK_RECOVERY_2026_10_26.json).

This is source/process-fixture qualification, not installed or ChatGPT Web
acceptance. C208 remains installed; no Wake, browser or recurring process was
started. The old C234 quarantine remains preserved and ineligible. The next
executable step is a bounded normal-debug canary using a fresh claimed task,
then the existing reviewed release procedure where required. Connector HTTP
429 and genuine two-conversation Web acceptance remain separate open gates.
No operator action is required for the next development canary.

## October 25 C235 — installed isolated task writes and verification LIVE PASS

The existing trusted C208 executable completed two concurrent operator-assigned
local tasks in separate linked worktrees of two registered projects. Each
previewed/applied one scoped probe file, returned its actual diff and passed a
fixed sandboxed verification command. Duplicate apply, wrong task, sibling
read and sibling patch-preview were refused; the peer continued afterward.
Both 120-second endpoints exited naturally within the 180-second test owner.
New writer markers retired, locks reopened and test processes/listeners were
absent. The test made no native-session call, original-checkout write, Wake/browser start or promotion.

C234 had stopped before any tool call when its harness tried reading the
endpoint's legitimate exclusive project lease. Its zero-call task, worktree,
journal and pending writer marker remain quarantined. C235 uses fresh task
identities; it exempts only the verified live endpoint's exact lease hash and
rechecks after exit. Nineteen prior quarantine records remained unchanged.

This is installed local write/command acceptance, not authenticated ChatGPT Web
coding or delivery of a project MVP feature. The original marker exposes a
remaining cold-task recovery gap: no supported retirement command exists. Never
delete it or replay the task. Next is a narrow owner-proven recovery transition
using the existing writer gate. Connector HTTP 429 remains separate and was not
retried. The complete multi-project Web milestone remains OPEN.
[Evidence](../verification/C235_INSTALLED_TASK_WRITES_2026_10_25.json).

## October 25 C233 — independent installed project controllers LIVE PASS

The corrected controller from `cf902361718ec36304c06b1d92aabdeb979dec3f`
passed the monitored 180-second test against the existing trusted installed
executable. One registered project's endpoint stayed active while the second
controller started. Each released its startup lease before READY and retained
separate process, port, project scope and target witness. Own-workspace reads
passed; wrong witness, sibling path and wrong bearer were refused. The peer
continued useful work after the refusal.

Both 120-second-bounded controllers exited naturally with code 0. Owned Jobs,
listeners and installed test processes were absent afterward. All 1,936 fixed
prior pins and 15 historical runtime files were preserved; the two context
ledgers only appended verified MCP traffic. Six attributed runtime files were
added. All project bindings and native sessions remain intact. C231's failed
history remains preserved; no browser, Wake, recurring automation or promotion
was started. The exact source's Actions query returned zero runs.

This qualifies the C230 `run-project` command using the fixed C232 controller
(SHA-256 `0226b6e04cc996c35ed8082832bf38420ff03566b98f720fe58a691b1f09ace0`).
It is actual installed local read-only operation, not authenticated ChatGPT Web
coding, write ownership, unattended serving or different-version rollback.
Simultaneous startup admission remains untested. Connector HTTP 429 remains a
separate dependency; the full October multi-project Web milestone stays OPEN.
[Evidence](../verification/C233_INDEPENDENT_PROJECT_CONTROLLERS_2026_10_25.json).

## October 25 C232 — Windows owner-probe correction PASS; live retry pending

The first C231 installed controller refused before READY. Its helper failure
was not retained, so the original exact cause remains unknown. The second
project never started. Exact-owner reconciliation preserved both new history
files and all 1,938 prior pins; the Job, installed processes and listeners were
gone, and the lifecycle lease was available. No message or binding changed.

A real Windows listener fixture then exposed an unavailable `Get-FileHash`
command in the probe environment. The correction reuses the existing pinned
reconciliation helper's .NET hash routine and established process-path lookup.
It preserves PID/path/hash/start/listener checks and adds bounded sanitized
failure diagnostics. Ten project tests, the real listener/wrong-hash fixture,
formatting and strict Clippy passed. Legacy release helpers are unchanged.

Build a fresh fixed controller before the next bounded installed test. C230's
controller is not live-qualified. Web coding, unattended service and a trusted
different-version rollback remain OPEN; connector HTTP 429 was not retried.
[Evidence](../verification/C232_OWNER_PROBE_CORRECTION_2026_10_25.json).

## October 25 C230 — bounded per-project operator source PASS

The existing registry now feeds independent `run-project` controllers. A short
startup lease protects release selection; it is released before READY while
immutable release/configuration pins remain. Scope, target witness, read-only
access, finite ownership and per-project cleanup are enforced. Configuration
requiring initialization or route migration refuses before launch.

Forty-eight focused tests, formatting, strict Clippy and two reviews passed.
Actual fixed-controller build/live qualification remains pending. Current
installed recovery and C229 local MCP evidence are preserved. Authenticated
Web coding, write ownership, unattended service and different-version rollback
remain OPEN; no new connector check or browser/Wake startup was performed.
[Evidence](../verification/C230_PROJECT_OPERATOR_SOURCE_2026_10_25.json).

## October 25 C229 — existing project route reused without another release

The current installed artifact already supports project-bound headless read-only
MCP. Two real registered workspaces passed concurrent operation and independent
context accounting using the existing approved control profile. No candidate,
development selection, release authority, registry, binding or ownership setting
changed. Productizing these flags should reuse that path; actual Web coding and
write permissions still need their own live qualification.
[Evidence](../verification/C229_TWO_PROJECT_LOCAL_MCP_2026_10_25.json).

## October 25 C228 — installed foreground ownership LIVE PASS

The separately built qualified controller now serves the existing trusted
installed image with bounded read-only operation. Natural exit and deliberate
controller interruption both released the lifecycle lease and reaped the owned
tree. No installed binary upgrade, development selection, release authority,
binding or external runtime change was required. Native/unattended operation and
project/Web integration remain separate gates.
[Evidence](../verification/C228_INSTALLED_FOREGROUND_LIVE_2026_10_25.json).

## October 25 C227 — installed foreground ownership implemented in source

A distinct bounded operator action now serves the already trusted installed
headless engine. It reuses existing pointer/slot/receipt readers, the lifecycle
lease and armed process ownership. The development selector remains unchanged;
no new candidate, release authority or competing registry was introduced.
The whole command has a deadline with teardown reserve and fixed read-only
scope. Source qualification passed; normal build and actual action acceptance
remain pending. [Evidence](../verification/C227_INSTALLED_FOREGROUND_SOURCE_2026_10_25.json).

## October 25 C226 — installed operation verified; foreground ownership gap

The trusted installed image now has two natural bounded headless restart/read
cycles with state preservation and complete cleanup. The remaining local gap is
a public foreground serving action for that image. Reuse the existing retained
child/Job owner loop with current trusted-release checks; keep the development
selector and detached-serving contract unchanged. No new candidate is needed
merely to demonstrate this already supported headless engine.
[Evidence](../verification/C226_INSTALLED_RESTART_2026_10_25.json).

## October 25 C225 — first trusted stopped production selection restored

The existing release store now accepts slot B generation 2 from the genuinely
reviewed C208 artifact, with a distinct stopped qualification/exit/receipt chain.
The installed executable and sidecar agree; original divergent bytes and slot A
are preserved. This repairs integrity and recovery authority without inventing
a serving-owner receipt or changing the legacy ownership guard.

Actual installed local MCP passed 401 rejection and authenticated initialize and
tool listing, followed by complete cleanup. Persistent native serving and trusted
previous-version rollback remain separate gates. The official connector still
returned HTTP 429 on its single fresh probe. Development, browser/Wake bindings
and native project sessions remain independent and unchanged.
[Evidence](../verification/C225_INSTALLED_STOPPED_RECOVERY_2026_10_25.json).

## October 25 C224 — repair and serving have separate lifetime contracts

The new source uses one existing release-pair transaction/store with an explicit
`QUALIFIED_STOPPED` mode. A dedicated finite controller enters its own armed,
non-breakaway Job before subprocesses or writes. Actual image/process/MCP
qualification precedes complete tree termination and the stopped receipt. This
removes the need for a detached serving owner during bounded integrity repair;
legacy serving operations retain their existing owner and live-proof gates.

Fresh schema2 mode authority wraps the existing reviewed artifact evidence.
Pure preparation publishes only that mode; foreign preflights are preserved.
Same-transaction interruption recovery retains escrow and unknown bytes. Public
executing startup shares the exclusive lease through readiness, while plans are
read-only. No new release registry, service, tunnel or approval system was added.

Source qualification passed 36 Rust cases, one opt-in helper ignored by direct
selection, five PowerShell invocations across four suites, formatting and strict
Clippy. Actual Windows canaries cover normal exit and hard controller death in
the pre-private-assignment gap. Source/fixture evidence does not prove protected
migration, installed recovery or serving. The unchanged reviewed candidate can
be used only after the new controller and canonical helper migration qualify.
See [the supported procedure](CATDESK_IMPORTANT_COMMANDS.md) and
[C224 evidence](../verification/C224_BOUNDED_STOPPED_RECOVERY_2026_10_25.json).

## October 25 C223 — compiled checks and lifetime limits

The new normal debug image has passed actual capability and pre-state
missing-config checks. It has no release authority and has not been selected or
installed. The manual ordinary-child result is now negative: that launcher is
ineligible, and the legacy detached serving worker's ownership guard remains
intact. No existing alternate supervisor qualified for release-pair work.

Assessing a bounded repair that ends stopped requires a distinct honest receipt,
actual owned qualification/quiescence and supported interruption recovery; it
cannot reuse legacy live-serving proof or leave a staged transaction as success.
There is no such supported command yet. Preserve the frozen C208 artifact and
all existing release authority. See the current execution log and C223 evidence.

## October 25 C222 — reuse the lifecycle for an opt-in local backend

The new source `-BackendOnly` option on public `start` and `recover` separates
local backend startup from automatic GUI, Wake, browser, scheduler and external
tunnel startup. It reuses canonical executable/fingerprint checks, reviewed
recovery/LKG authority and the existing local readiness/owner checks. It adds
no release store, supervisor, tunnel owner or alternate promotion mechanism.
The unflagged full lifecycle keeps its existing behavior.

Before a new launch, the verified native image must return the fixed capability
for `--catdesk-daemon --backend-only --check-support`. This exact tuple exits
before loading configuration or starting services; unsupported executables are
refused rather than launched in full mode. Backend startup reads the existing
initialized configuration and current route, uses ephemeral launch settings,
and preserves pending route and saved UI mode. Ordinary authorized MCP work and
usage accounting are not converted into read-only operations. A process-only
bearer, when configured, is validated and used only for the exact approved local
readiness endpoint; external authentication is unchanged.

Source qualification passed: ten Rust tests, both PowerShell suites, formatting
and strict root Clippy. No new candidate or production release
has been created, and neither C208 nor the selected C218 runtime has acquired
this source behavior. C208's reviewed `409513a` source lacks the new tuple;
the actual old executable safely refused the query once. Installed startup/restart
remain untested. Source tests
cannot establish a trusted previous production release or rollback acceptance.
The C221 manual-child result remains pending and every existing ownership
guard stays intact.

## October 25 C219 — recovery needs independent serving-process ownership

The outside-Job requirement protects the promoted daemon from an unrelated
outer owner's termination. Creating a nested Job would not remove that outer
ownership. Keep the existing gate and use the supported outside-Job operator
process for the divergent-pair repair. Ordinary MCP promotion first checks the
canonical executable/fingerprint pair and cannot repair the current mismatch.

The refreshed private wrapper preserves 486 current-state paths and retains
C196/C208 authority. Its preservation-only qualification passed; production
preflight, installation and serving health have not run. The C218 development
package remains locally qualified, with its previous verified version retained.
Restoring a valid pair removes this particular ordinary-promotion mismatch
gate; later releases still require the existing review and producer evidence.

## October 25 C218 — locally verified development package is selected

The normal release from source `c56d3b4d17ad0070742b4e2d5888a2c20a85633b`
passed a six-second selected-image health check (authenticated 200, unauthorized
401, 79 tools and normal exit). Existing stopped-package selection and local
verification commands retained the previous verified version and all project
state. This does not create reviewed-promotion or production recovery authority.
One metadata-only browser diagnosis passed without sending or changing state.
The pending review remains unknown, and protected production remains unpromoted.

## October 25 C215 — review consent stays separate from release evidence

Routine read-only review sends can use the user's standing scoped authorization
through the existing project WebWork ledger; see the
[operator contract](CATDESK_ROUTINE_REVIEW_AUTHORIZATION.md). This does not require
production promotion, new browser authentication or a Wake binding change.
A real independent review and the existing acknowledged contract/snapshot/build
chain remain necessary where the release procedure requires them. The chain does
not inherently require another ChatGPT Web message; never relabel a local verifier,
queue advisory, grant or delivery receipt as independent review authority.

## October 25 C209 — keep authentication and cleanup within existing owners

Current source gives the native MCP listener an optional process-only bearer.
Invalid configuration fails before services start; missing configuration keeps
legacy ordinary MCP but cannot enable central project-task dispatch. Local
health checks authenticate with that same value, remote checks do not receive
it, and status reports only the mode. No persistent credential registry or new
transport owner is introduced.

The reviewed producer handles only its pinned MSVC helper inside the exact Job,
after successful Cargo exit and full member validation. Retained process handles
prevent PID reuse from changing cleanup targets; a proved natural exit is
accepted. Final Job emptiness, stream completion and release-authority checks
remain unchanged. Both source changes passed independent static review and
source tests. A real producer run of this new code remains pending. C208's
attested `409513a` candidate remains separate and production is unpromoted.
The outside-Job owner-launch limitation is not new approval debt.
Upstream static-header support is documented, but current runtime ingress,
installed health, rollback and real two-project Web operation remain unproved.

Validation: 144 source tests passed (8 manual tests ignored), with strict Clippy and bounded Job cleanup passing. Counts: native auth 5, project dispatch 6, bearer 1, headless 16, reviewed build 116. These are source/fixture results, not installed acceptance. Evidence: `docs/verification/C209_NATIVE_AUTH_AND_COMPILER_HELPER_2026_10_25.json`.


## October 25 C208 — build authority recovered; installation remains separate

The existing producer, with its ownership gate intact, naturally attested the
candidate after exact orphaned compiler-helper cleanup. Independent product
readback and two bounded candidate authentication/lifecycle runs passed. The
stale-PID monitor was stopped separately; unrelated processes were preserved.

The candidate is built from reviewed snapshot `409513a`; the `83a2d98` controller
supplied the build fix but is not the candidate source. No production promotion
has occurred. Normal and PTY tool sessions are inside a Windows Job; UI terminal
automation is not permitted. One checked command requires operator execution in
a normal PowerShell 7 window outside that Job. The launcher withholds external-runtime credential references only
for its process tree. It enables no recurring browser/Wake work. Installed
health, rollback, Secure MCP and actual two-conversation coding remain open.
`docs/verification/C208_ATTESTED_CANDIDATE_AND_PROMOTION_PREFLIGHT_2026_10_25.json`.

## October 25 C207 — preserve reader compatibility and release boundaries

Disposition now leaves the original `REQUESTED` context ledger unchanged for
existing readers. Matching immutable disposition/completion receipts authorize
new work; a later rollover needs a fresh completed source task and distinct
stable intent. After 112 tests and both strict Clippy checks, the rebuilt
controller completed live prepare/confirm while preserving all 452 pins and
original context bytes. New/legacy journal reads and legacy context decoding
passed. No browser, host, message or binding changed; ordinary target binding
retains its guard. Actual new Web task execution remains open.

Two bounded diagnostics passed `ring` without a complete Cargo result. One
confirmed supported producer retry failed again with Cargo 101 in 37 seconds,
no attestation and zero workers. Later exact replay exposed `ring` / MSVC
`C1083 SOURCE_FILE` in producer paths; ordinary paths compiled Ring. The source
fix normalizes five external values using the existing helper, preserving
canonical identities and closed V5 policy. Its 110 tests and strict Clippy passed;
fixed-controller and producer requalification remain pending. Diagnostic,
source-qualified, producer-attested and installed states remain separate;
production promotion and two-project Web coding are open.
Evidence: `docs/verification/C207_COMPATIBLE_QUARANTINE_AND_BUILD_DIAGNOSIS_2026_10_25.json`.

## October 25 C206 — separate review recovery, producer health and installation

The genuine C196 source review and local ACK/snapshot chain completed. Its
normal Wake observer/work ledger did not: they remain `ATTENTION` / `OPEN`.
The supported reviewed producer then failed with Cargo 101 and no attestation.
None of these outcomes changes the installed executable or establishes rollback.

This source increment provides explicit operator disposition of an unknown
rollover while preserving its journal and unknown outcome, plus an 8,192-byte
explicit source-snapshot read. Ordinary 4,096-byte context excerpts and overall
wire limits remain unchanged. These are source-tested changes; the live
quarantine, bindings, native sessions and recurring-automation state remain
unchanged. Diagnose the producer failure before another confirmed attempt.
The bounded reproduction has not launched Cargo; preparation failures do not
establish the producer failure's cause.
See `docs/verification/C206_REVIEW_RECOVERY_AND_BUILD_DIAGNOSIS_2026_10_25.json`.

## October 25 C197 — review approved; actual project admission blocks delivery

The user approved C184's frozen message and exact destination. Its nonrenewable
lease had expired, so supported MCP cancellation and a fresh successor preserved
the old contract/history. The claim committed before a ten-second local HTTP
timeout. Durable status, queue, baseline and accounting proved that commitment;
it was not replayed. Supported supersession was recorded. All 594 earlier session
files remain exact, with 559 prior accounting records preserved. The fresh bounded
controller readback exited naturally and released its listener.

The approved review remains **unsent**. The qualified application actually refused
publication with `WEB_WORK_ROLLOVER_PENDING`, exit 2, empty queue and no state drift.
CatDesk retains its original October 18 quarantine. One read-only inspection using
the existing authenticated profile returned `RESPONSE_EXPECTED_USER_NOT_FOUND`;
it produced no recovery authority. The quarantine, original binding and unknown
outcome remain preserved. This attempt did not contact the external connector and
is not a new HTTP 429. No WakeHost or recurring worker was started. Browser cleanup
passed with zero test owners; all 361 pinned files remain exact.

Private preparation now checks routing and unresolved rollover state before claim
mutations, retaining the normal publisher's admission guard. A distinct frozen
public-source review for Wake2's existing conversation passed metadata checks only.
Its different message/destination require separate approval; C184 approval is not
reused. Nothing was sent there, and native Wake2 work and all project bindings
remain unchanged. No product code changed. Production remains unpromoted; genuine
review, producer authority, installed health, trusted rollback and two-project Web
coding remain open. The goal remains active. Evidence:
`docs/verification/C197_ACTUAL_REVIEW_ROUTE_REFUSAL_2026_10_25.json`.

## October 25 C189 — real direct admission works; one Web send awaits automatic approval

The qualified controller advertised the direct mode against the real canonical
workspace. Read-only supervisor probing exited naturally with listener cleanup.
A new review contract then validated, was approved and claimed once, without a
native thread. This is local operator initialization, **not ChatGPT Web execution**.
All 594 older session states remain byte exact. All values/keys of 558 historical
accounting records are retained; normal serialization added only null requested
models and empty native-profile arrays. Original bytes are archived. The new
claim accounts for one added record. Its first audit stop was reconciled without
repeating the claim or inventing historical native-turn proof.

Only canonical autonomous Wake was set to `MANUAL_OFF` for bounded testing.
All 361 pinned files and independent project bindings remain exact. No browser
or message was launched. Automatic approval review blocked the generated
publication script pending approval for this exact new message and destination;
the frozen payload remains unchanged. This adds no application approval framework.
Production is unpromoted, genuine Web review/producer authority and installed
rollback remain pending, and actual two-conversation Web coding remains open.
The goal stays active. Evidence:
`docs/verification/C189_LIVE_DIRECT_ADMISSION_2026_10_25.json`.

## October 25 C181 — direct controller built and qualified; protected promotion pending

Published source `061051fe9e6e4493ab0ccf24c58b41cbe81b74a6` now has a separate
normal release controller, SHA-256
`701221a979e73e95c81a7853bec661add68ba814c0ac68919c1e357c49254a7a`.
The locked offline app build finished within its 600-second bound. Two twelve-second
read-only loopback starts passed missing/wrong auth 401, valid auth 200 with ten
tools, natural exit and listener cleanup. This is an actual normal executable
qualification, **not producer attestation, installed startup or Web coding proof**.

Canonical source/docs fast-forwarded to that commit. The installed executable and
fingerprint, ten preserved untracked files, all 361 pinned state/history files,
v77/v76 and project/native bindings remain exact. No browser, WakeHost, recurring
automation, external Secure MCP owner or existing native session was started.
Exact-source GitHub Actions/check counts remain zero: remote CI is unverified.

The explicit direct lane is implemented and source-reviewed. Next: actual fresh
ChatGPT Web review and verified response after an approved direct claim, followed
by the existing completion/ACK/snapshot/producer/release-pair chain. Native source
review JSON is not Web work. Production remains stopped and unpromoted; trusted
installed rollback and actual two-conversation Web coding remain open. No new
operator action or date hold is identified; the goal remains active.
Evidence: `docs/verification/C181_DIRECT_CONTROLLER_QUALIFICATION_2026_10_25.json`.

## C151 — explicit ChatGPT Web work contracts; production admission still pending

Implemented the explicit `chatgpt_web_direct_autonomous` contract using the existing
session, verifier, review inbox, immutable snapshot and reviewed-build producer.
Its exact provider tuple is `chatgpt-web` / `conversation-selected` with routine
provider/model `none`; paid/cloud fallback remains forbidden. The actual Web model
is not inferred. Native contracts, Sol isolation/profile gates and existing native
writer ownership remain unchanged.

Direct work requires exact approval, a pre-edit baseline and explicit verification. Fresh verification requires task-specific queue and accounting ownership; a planner wait cannot complete an unclaimed task, even without output files.
Lease expiry, approval drift, dependent tasks, task-turn budgets and planner replies
are checked. An interrupted gated claim reuses its immutable baseline and existing accounting/queue claim. Accounting is idempotent for the same direct task; native turn counting is preserved. Repairs retain the original claim. Non-CatDesk
projects retain checkpoint hashes without CatDesk build inputs; CatDesk DAGs create
their existing release snapshot at final completion. No second authority store exists. Direct transitions share the existing session lock; Windows retains an exclusive delete-on-close handle, with an actual abrupt-exit fixture proving release without stale-path recovery. Exact accounting identity collisions are refused. Interrupted verification returns to explicit retry using its original claimed task.

The current 183 controller/contract/supervisor/state/accounting/native-restart tests, strict lint
and format passed, followed by limited independent native source review. The earlier
full root suite and integrations passed before the final controller/accounting pending-intent guards; those final guards and their regression assertions passed the current focused qualification. Failed runs and review findings
remain recorded. Fixture completion/ACK/producer PREPARE is source evidence only.

Next: build and qualify this controller, obtain actual fresh ChatGPT Web work and
its response after claim, then use the original ACK/snapshot/producer/release-pair
chain. A native reviewer report or coordinator copy cannot become Web execution.
Production is still stopped and unpromoted; installed rollback and two-conversation
Web coding remain open. v77/v76, 361 state/history files and all project/native
bindings remain preserved. No browser/Wake/recurring/external runtime was started.
No new operator action or date hold is identified. The goal remains active.
Evidence recorded 2026-10-25T04:41:18.5413838+00:00: `docs/verification/C151_EXPLICIT_DIRECT_CHATGPT_SOURCE_2026_10_25.json`.

## October 24 EDT C134 — corrected controller built and qualified; production admission open

The corrected normal-release controller is built from published source
`3f272767c96b4113902a547efea804b76cd46eae`. SHA-256:
`86f00f6e333a8988171fb30f2a1c09547c746586d339fc340a6785664acbb7cc`.
Both twelve-second read-only loopback starts passed: missing/wrong auth 401,
valid auth 200 with ten tools, natural exit and listener cleanup. All 361 pinned
development/history files remain exact; v77/v76 and project/native ownership stay
preserved. No browser, Wake, recurring automation or external runtime was started.

This is a qualified separate controller, **not a producer-attested or installed
production release**. The concrete admission gap is the legacy completed-review
path's deliberate Sol OS-isolation refusal. Native queue operator decisions and
external source-review JSON cannot replace its acknowledged review/snapshot chain.
Resolve that integration through reviewed source work; do not switch models,
fabricate legacy records, rewrite checksums or relabel a development predecessor
as trusted canonical rollback authority. No new operator action is identified.
Remote source CI remains unverified (zero exact-SHA runs/checks). Production
reconciliation, installed rollback and actual two-conversation Web coding remain
open; the October 27 date deferral stays superseded and the goal stays active.
Evidence: `docs/verification/C134_CORRECTED_CONTROLLER_QUALIFICATION_2026_10_24.json`.

## October 24 EDT C130 — producer gates fixed; parallel target-read regression resolved

Independent source review exposed three producer defects. Snapshot creation and
replay now check overlapping completion hashes; PREPARE/CONFIRM reuse the existing
crash-safe admission lock; output admission waits for root exit, whole-Job exit
and bounded stderr completion. Two snapshot regressions failed before the fix.
The full Rust suite also exposed Git probes held under the shared Wake-target
lock. Status probes now run outside that lock, with exact prepared-root checking
and paired binding readback retained. A delayed-project regression reproduced the
old sibling failure. The four-file follow-up source review passed its stated scope.

The complete local Rust commands, strict Clippy and format passed. Earlier failed
runs remain in the audit. A separate controller from published base `5fb1529`
passed two short read-only MCP starts (401/401/200, natural exit and cleanup),
but predates these fixes and is held. None of this is installed production proof.
All 306 development and 55 historical files remain exact; v77/v76, five project
bindings and native sessions are preserved. No recurring automation was activated.

Production remains stopped and unpromoted. Next: rebuild the corrected controller,
establish the genuine current completed-review/snapshot/producer chain, then use
the existing guarded release-pair route. This external review is not producer
authority; Sol gates remain enforced. No date hold or new operator action is
identified. Connector 429 and official runtime remain separate, unchanged
dependencies. Installed rollback and actual two-conversation Web coding remain
open. Evidence: `docs/verification/C130_PRODUCER_AND_PARALLEL_TARGET_FIXES_2026_10_24.json`.

## October 24 EDT C118 — complete promotion path and resumed-owner evidence

Fixed the two C117 findings: nested candidate paths now use the Rust producer's
forward-slash representation, and Phase1 resume retains the verified timestamp,
image path and hash needed by actual handback. The full consumer regression also
exposed an unavailable hashing cmdlet in the fixed native interpreter; promotion
now delegates to the existing shared helper using streaming framework SHA256.

18 canonical Rust tests, strict Clippy/format and both Windows PowerShell release
fixture suites passed. The expanded wire test uses an actual Rust authorization
and claim, a nested candidate, real script identity and no authorization seam.
It forces the optional hash cmdlet unavailable. Resumed metadata failed before
the fix and passes with complete identity afterward. Initial failures and fixture
loader corrections remain recorded; no isolated wire-path RED claim is made.

Production remains unchanged. Fresh independent review, the fixed normal controller
build and a genuine reviewed producer/snapshot chain are next; no October27 hold
remains. All 306 runtime and 55 historical files are exact, v77/v76 and native
sessions preserved. Source/fixture results do not establish installed rollback,
connector availability or actual Web coding. Evidence:
`docs/verification/C118_PROMOTION_INTEROPERABILITY_2026_10_24.json`.

## October 24 EDT C115/C116 — production authorized now; two release blockers fixed

The operator superseded the October 27 deferral. Canonical source fast-forwarded
to published `af2ad2e` without changing its executable/sidecar or ten untracked
files; the checksum-verified normal candidate was staged separately. A bounded
independent public-source review identified a stale PowerShell authorization schema
and unproven whole-Job cleanup after spawn setup failure. Both are fixed in source.
The candidate is held; the corrected source needs fresh review and producer build.

59 targeted Rust tests, strict Clippy/format and both release PowerShell fixture
suites passed. New regressions use actual Rust serialization and a real resumed
child/descendant. Production was not promoted, installed rollback was not tested,
and actual Web coding remains open. All 306 runtime and 55 historical records
remain exact; no Wake/browser/tunnel or other native-session action occurred.
The old ordinary plan correctly refuses this divergent pair. The supported route
is reviewed release-pair reconciliation once its real gates pass, with no date hold.

Complete commands, parameters, prerequisites, failure states and rollback limits:
`docs/orchestrator/CATDESK_PRODUCTION_RELEASE_LIFECYCLE.md`.
Evidence: `docs/verification/C116_PRODUCTION_REVIEW_BLOCKER_FIXES_2026_10_24.json`.

## October 24, 2026 — focused simplification and compatibility proposal

This section supersedes the historical deployment plan below. The operator has
requested a smaller release model. Protected installation and recovery actions
remain separately authorized; this proposal grants no runtime activation.

### Consolidate existing components incrementally

| Existing component | Proposed disposition | Required compatibility condition |
| --- | --- | --- |
| Git branch history, exact source/build/test/review evidence | Keep GitHub as the source and development record. Bind immutable artifact manifests to exact commits and full hashes. | A commit or CI result alone does not authenticate unknown executable bytes. Preserve unverified CI labels. |
| Development `versions/`, package manifests, `current.json`, workspace authority | Reuse this functioning nonprivileged runtime. Consolidate package-selection metadata behind its existing transaction, with current/previous verified release as the target model. | Existing schema-1 selections, profile, Python runtime, targets and journals must remain readable. No production root or tunnel access. |
| `package-update-attempt.json` and staged immutable package | Reuse one selection/recovery journal. Remove dependence on the original build directory when completing an already validated staged update. | Revalidate exact workspace, package, pointer, authority and exclusive STOPPED ownership; unknown state remains held. First implementation below. |
| `user_worker_release.rs` current/previous immutable manifests and atomic active state | Reuse this existing current/previous representation for the eventual stable worker cutover; do not create another competing version registry. | Keep existing protected admission intact until the composed startup, rollback and migration tests pass. |
| Legacy canonical executable/sidecar and alternating LKG slots | Retain as compatibility inputs; stop treating them as the development runtime's release store. Retire their mutating role only after migration to a verified versioned release passes. | Preserve legacy bytes and receipts. `operational_verified` must not be relabeled as independently reviewed. |
| Reviewed build, promotion and October 18 release-pair procedure | Consolidate operator presentation into one admitted-release transaction. Reuse existing review/build validators and the bounded reconstruction route where needed. | Do not introduce a parallel trust framework or bypass existing protected policy. The current divergent production pair still needs approved authority. |
| Product-root signed installation / pending rotation | Keep only for the protected launcher/install boundary where required. Exclude it from ordinary source work, Wake URL updates and development-package selection. | Preserve existing signed artifacts and receipts; no rotation or protected cutover is authorized here. |
| Independent Wake, guarded paired URL binding, native Codex and external Secure MCP | Keep their existing independent ownership and lifecycle. | No implicit Wake launch, native writer takeover, tunnel replacement or chat replay during release selection. |

### Minimal end state and staged migration

A release manifest should identify exact source commit, artifact hashes, tests,
independent review and (separately) observed working health. Keep immutable current
and one independently verified working predecessor. Reuse the existing atomic
selection and journal; a folder's presence, newest timestamp or prior selection
alone must not make it trusted rollback material. Public startup should choose a
verified current release, or restore the verified predecessor after a bounded
failure. If neither is eligible, return one clear reconstruction-required result.

Reconstruction must bind one explicit operator-approved source/build candidate
and preserve the divergent old pair and escrow. Use the existing reviewed-build /
release-pair procedures; do not rewrite a fingerprint or require a newly invented
signing/authority mechanism. Existing protected requirements apply only where the
chosen production admission actually requires them.

Migration order:

1. Make the existing development selection recoverable from local immutable
   artifacts without any privileged promotion or retained build-output directory.
2. Add commit/review/health qualification and explicit current/previous selection
   to the existing versioned model. Preserve older schema readers. Verify corrupt
   current, valid predecessor, absent/corrupt predecessor, interrupted selection,
   cold restart and normal startup before automatic fallback is enabled.
3. Qualify the composed normal developer backend startup and rollback independently
   of protected production. Retain bounded ownership and approved-resource scope.
4. On an authorized production maintenance turn, admit a qualified version through
   the existing reconstruction/cutover route. Prove process identity, protocol,
   bindings and recovery. Only then retire redundant legacy writers; retain
   historical readers and evidence for compatibility.

No legacy mechanism is retired by the first increment. The October 27 historical
plan remains the production compatibility path until a migration is qualified.
No recurring process is enabled. Actual two-conversation Web coding remains the
next live priority once the approved serving path is restored.

### First independently eligible implementation

`operator web-dev-runtime recover-package` takes no path, version or hash input.
It finishes only an existing package-update journal against the already staged
immutable package, using the same completion validator as `update-package`.
This removes the unnecessary original-build-directory dependency and creates no
second LKG store, new approval type or production authority. It never starts a
host or browser. With no pending update, it validates the current stopped
selection without changing its bytes. Unknown pointers, changed package bytes,
active owners or incompatible state remain explicit refusals.

This increment passed 17 development-runtime and 12 operator tests, both strict lint
gates and formatting. It is source/fixture qualification, not current/previous
health fallback, installed recovery or normal unattended acceptance. The existing
selected development package and protected runtime are unchanged.

### C102 normal executable qualification

The exact C101 source also built as normal app/host release images. A bounded
normal CLI test passed a real Windows pointer-first replacement failure and cold
recovery with the build directory unavailable, using two genuine pinned packages.
Nine commands exited naturally; profile/history and old package survived, with
all real runtime/historical records unchanged and no browser/host launch.
This advances command qualification only. Backend startup, independent working
predecessor admission, automatic fallback and protected migration remain separate.
See `docs/verification/C102_NORMAL_RELEASE_SELECTION_RECOVERY_2026_10_24.json`.

### C103 existing versioned rollback correction

The existing worker store no longer requires healthy current executable bytes
before restoring its intact retained predecessor. Current manifest and predecessor
validation, atomic selection, stale-request refusal and failed-image preservation
remain. 18 source tests, strict Clippy and format pass. This corrects a storage
primitive; working-health qualification and normal startup integration remain
required. No protected release or automatic fallback was activated.
See `docs/verification/C103_VERSIONED_ROLLBACK_CURRENT_IMAGE_FAILURE_2026_10_24.json`.

### C104 local development evidence and predecessor retention

Reuse `versions/<packageId>/verification.json` for immutable local operator-provided
references to source commit, build evidence, review evidence and observed working
health. Bind them to the exact package ID, manifest hash and artifact hashes.
Recording does not execute those checks or grant production review authority.
An identical replay is harmless; conflicting or mismatched records are refused.

Keep the qualified predecessor in optional `previousVerifiedRelease` metadata in
the existing `current.json`; its bytes remain bound by the unchanged authority.
Reuse the same update journal. Do not introduce another release selector/store.
Keep the last qualified predecessor across unqualified updates, while treating
invalid predecessor metadata as ineligible rather than blocking healthy current
operation. The legacy authority/manifest format remains unchanged. Existing Wake
pointer readers accept extra metadata (source inspection); old binary readback
still requires normal executable qualification. Legacy pending journals recover.
Use the current controller for new updates and new-format pending journals:
older controllers may refuse the pending journal or discard optional predecessor
metadata during a later update. They are not the migration writer.

The supported source command is:

```text
catdesk.exe operator web-dev-runtime record-verification --input-file evidence.json
```

Run in the already prepared development workspace while host/browser ownership is
free and control is STOPPED. Input must be a plain file inside that workspace.
Schema 1 fields are `packageId`, `manifestSha256`, `artifacts` (the four immutable
package hashes), `sourceCommit` (40 lowercase hex), `buildEvidenceSha256`,
`reviewEvidenceSha256`, `workingHealthEvidenceSha256` (64 lowercase hex each),
and `workingHealthObservedUtc` (nonfuture Unix seconds), plus `schemaVersion: 1`.
Evidence digests reference actual retained operator-reviewed records; merely
inventing digest strings is not release qualification. Status labels these
references `OPERATOR_RECORDED`, not independent automatic attestation.

23 runtime tests and 13 operator tests, strict root/Wake Clippy and formats pass.
Source fixture receipts are synthetic; no real release is newly qualified.
Automatic predecessor restoration and composed normal backend startup remain
the next integration, before any legacy writer retirement. The existing selected
package and protected installation remain unchanged. Evidence:
`docs/verification/C104_DEV_RELEASE_EVIDENCE_PREDECESSOR_2026_10_24.json`.

### C105 predecessor restoration through the same selection journal

The existing `recover-package` command now restores only the exact recorded
predecessor when current package files are damaged or missing. Current manifest,
pointer, Python and workspace identity remain strict. Predecessor evidence,
manifest identity/hash and every artifact are revalidated. The optional restoration
record lives in the same update journal; the authority and manifest formats stay
unchanged. Legacy prepared package IDs are restored in place without repackaging.
Failed current bytes/history are preserved and are not eligible for oscillation.

With no valid predecessor, return `DEV_RELEASE_RECONSTRUCTION_REQUIRED_...` and
preserve the selection. Use the existing operator-approved source reconstruction
path; never invent a fingerprint. Recovery requires STOPPED/exclusive ownership
and starts no host, browser or external transport. Older source descriptions that
say this command only completes pending updates are superseded by this section.

An expected old-code failure and 28 runtime plus 13 operator tests establish source
behavior, including all four damaged/missing artifacts, three journal boundaries,
ownership/metadata refusal, changed pending evidence/assets and no failed-version
oscillation. Strict root/Wake lint and formats pass. Normal binary, actual process
interruption, automatic backend startup and production migration remain separate
qualification. No legacy writer is retired. Evidence:
`docs/verification/C105_DEV_PREDECESSOR_RESTORATION_2026_10_24.json`.

### C106 normal executable startup and recovery qualification

The exact C105 source built as normal app/host release images. In a bounded
22-second disposable test, both current and previous images served authenticated
loopback MCP catalogs and exited naturally. The old normal binary read new pointer
metadata. A real Windows sharing failure interrupted predecessor restoration;
a fresh normal process completed it and the restored backend started. Profile,
history, evidence and failed bytes were retained. Healthy replay was byte-stable;
no predecessor produced reconstruction-required without selection mutation.

Two harness path-limit failures remain recorded. A shorter disposable checkpoint
root permits normal Windows process launch; the real selected app path is already
below that limit and was not changed. All test-owned processes exited. Real state,
bindings, protected artifacts and selected v76 remain unchanged. Health here is
loopback MCP only, not independent external review, Wake or production acceptance.

The explicit recovery command is normal-binary qualified. One supported bounded
developer-backend start that executes the selected image and automatically restores
on current-image failure remains the next source integration. No legacy writer
is retired before that composed startup/migration qualifies. Evidence:
`docs/verification/C106_NORMAL_BACKEND_PREDECESSOR_RECOVERY_2026_10_24.json`.

### C107 one bounded selected-backend command

Source implementation adds:

```text
catdesk.exe operator web-dev-runtime run-backend --max-runtime-seconds 180
```

Run in the prepared isolated development workspace with its existing approved
process-scoped `CATDESK_MCP_AUTH_TOKEN`. The default active runtime is 180 seconds;
explicit bounds are 1–900. The owner includes bounded readiness/shutdown grace.
It emits validated `DEV_BACKEND_READY` with the loopback MCP URL and redacted auth
metadata while active, then `FOREGROUND_BACKEND_EXITED` after exact tree cleanup.
No recurring activation, tunnel, browser, Wake startup or authentication change.

Reuse existing selection/recovery and the existing suspended-child Job owner.
The exact selected app runs, including the restored predecessor after failed
current files. A local backend lease protects one workspace's selection during
its foreground lifetime; it does not serialize project tools or other workspaces.
Healthy startup can coexist with Wake/browser ownership. Actual recovery retains
the existing STOPPED/exclusive requirements. Read-only image/pointer/authority
pins remain until owned-tree drain. Current controllers must perform future
package mutations; older controllers do not honor this new backend lease.

159 source tests, strict root/Wake lint and formats pass. Source tests do not
establish normal command, actual automatic startup fallback or parent-exit proof.
These are the next normal-release tests. Fallback when current bytes hash correctly
but the backend fails before readiness remains a further source requirement.
Existing selected/protected runtime and
all project bindings remain unchanged; no legacy writer is retired. Evidence:
`docs/verification/C107_BOUNDED_SELECTED_DEV_BACKEND_2026_10_24.json`.

### C108 normal startup, file-failure fallback and parent-exit proof

The C107 command is now built and normal-executable qualified in a disposable
runtime. It started the exact selected app, served authenticated loopback MCP,
rejected unauthorized requests and refused duplicate startup/package mutation
without interrupting service. A current-file failure caused automatic restoration
and actual execution of the exact recorded C105 predecessor. Existing C106 health
evidence was reused; no redundant prior health experiment was needed.

Two operators exited naturally. A third, still serving, was killed by exact parent
handle without a tree-kill. Its retained child handle reached exit, the listener
closed, and a cold command verified lease release with unchanged selection. All
three child image paths/content hashes were independently checked. The monitored
test took 19 seconds inside its 180-second bound. All real runtime/history records,
bindings, selected v76 and protected artifacts remain unchanged, with no orphans.

Normal file-failure fallback is qualified; valid-hash pre-ready startup failure
fallback still needs one bounded source implementation and distinct test. No
production or unattended operation is qualified, and no legacy mechanism is
retired. Actual two-conversation Web coding remains the live priority after its
approved serving dependencies return. Evidence:
`docs/verification/C108_NORMAL_SELECTED_BACKEND_AUTORECOVERY_2026_10_24.json`.

### C109 one bounded attempt after pre-ready startup failure

Source now handles a selected package whose hashes validate but whose backend
cannot start: after proven native spawn rejection with no child, or pre-ready
timeout/EOF/exit and complete private-Job drain, restore only the exact recorded
predecessor and attempt it once. The backend lease stays held across the same
selection journal. A sealed private authority snapshot prevents display fields
from retargeting rollback. Current metadata and all predecessor checks stay strict.

After readiness or partial publication, never replace/retry the backend
automatically. Unknown cleanup, scope/auth violations, unsafe output, busy Wake/
browser ownership or absent/damaged predecessor also refuse. Failed bytes remain
preserved, never a rollback target. Existing spawn API/messages and Job policy are
retained; a typed failure result distinguishes no-child rejection from owner
failures. Fallback metadata appears with the validated ready/result record.

The old behavior was confirmed with the normal C107 executable. The new source
passes 51 focused runtime/backend/owner tests, both strict lint gates and format.
New normal fallback, failed-predecessor stop and after-ready refusal still need
bounded executable qualification. C108 file-failure and owner-exit acceptance is
preserved. Protected/unattended migration and real Web coding remain separate;
no legacy mechanism is retired. Evidence:
`docs/verification/C109_ONE_PRE_READY_BACKEND_FALLBACK_2026_10_24.json`.

### C110 normal one-fallback qualification

The C109 source is normal-release built and exercised in three disposable cases.
The exact former failing package now restores and executes the recorded trusted
predecessor. When a controlled shared configuration blocks both valid images
before ready, one restoration occurs and the command exits with bounded failure.
After a verified ready record, killing the exact owned child produces an error
without replacement, replay or selector mutation. Ready image paths/hashes were
verified by retained process handles; the failed pair's exact spawn count was not
independently traced beyond source control flow and observed single restoration.

A harness prefix-matching assertion stopped the first job despite correct negative
behavior. Its record was retained; a bounded continuation parsed the real status,
verified unchanged state and performed only the unfinished case. The successful
fallback was not replayed. About 15 active seconds spanned two separately bounded
jobs; no process remained during the diagnostic gap. Real v76, every paired target,
profile/task journal, historical artifact and protected installation stayed intact.

Next, inspect existing v76 evidence and the supported stopped-development selector.
If qualification is exact, retain that package as the local working predecessor,
select the new qualified source in the isolated dev runtime and perform one bounded
normal MCP qualification. This is independent of protected production admission or
external Secure MCP ownership. No legacy mechanism is retired or recurring process
enabled. Actual two-conversation Web coding remains the live priority once its
approved serving dependencies return. Evidence:
`docs/verification/C110_NORMAL_PRE_READY_FALLBACK_2026_10_24.json`.

### C111 existing development migration completed

The current/previous model now operates in the existing isolated development root.
The normal C109 package is selected, and v76 is retained with exact build/review
and freshly observed MCP health references. Current v77 received its own write-once
record only after actual selected-image MCP health passed. Both used the existing
selection journal and schema-compatible metadata; no competing registry or launcher
was introduced. The actual normal backend exited naturally in each bounded test.

Ten paired target tuples, the five-project central view, 304 unmodified runtime
records and 55 historical files were verified. Only the authority/selection pair
changed, with both baseline generations preserved. Browser/Wake stayed stopped,
profiles and native sessions were untouched. These local development records do
not create production review authority or retire the legacy canonical writers.
The worker release store remains a storage primitive for the separate worker role;
it must not be presented as an already composed canonical daemon startup mechanism.

The next production boundary remains the October 27 existing-authority plan.
Admission must preserve existing signed/legacy artifacts and pass actual composed
startup/recovery before any old writer is retired. Once the serving dependencies
return, actual two-conversation Web coding takes priority over more synthetic demos.
Evidence: `docs/verification/C111_SELECTED_DEVELOPMENT_CURRENT_PREVIOUS_HEALTH_2026_10_24.json`.

# T-0476 — CatDesk fast development, safe Wake recovery and simple production releases

## CURRENT 2026-10-11 — Chat52 three-lane development and recovery status (supersedes history below)

- **Wake continuity lane:** the guarded paired CatDesk project registry and independent WakeHost both identify Chat52 `https://chatgpt.com/c/6acba3bf-e740-8331-8896-8a25fefdbf16`, SHA `d82593bf8a7eb82936cd63d1ed52f77243c46afa3f9aa57ca2d9a642a31b73ce`, independent generation33. WakeHost dev84 RUNNING/queue0. Manual test `manual-wake-mcp-1791732042693` was terminal SENT/EXACT_USER_MESSAGE_APPENDED/COMPLETE; later test `manual-wake-mcp-1791737671996` reached visible Chat52 and now has an exact message-appended receipt, but check its terminal timer/delivery after response. **Manual != natural Wake acceptance.** All CatDesk hourly deadmen are disabled at user request; do not recreate without user instruction.
- **Source-only development lane:** T0478 R2 authenticated temp-root read/search fixture, R3A handle-pinned no-follow single-link and in-memory regex, R3B1 config hardlink/reparse preflight, R3B2 entirely in-memory fixture configuration accepted by GitHub CI `38157083333` 3/3 success. R3B3 source commit `04806d8` retains no-delete-share directory handle for full disposable fixture listener lifetime, real local HTTP negative rename/release test passes; its independent GitHub CI `38158118618` was still running when recorded. R3B4 **design only** `docs/orchestrator/review_bundles/T-0478_R3B4_SOURCE_SNAPSHOT_PROVENANCE_DESIGN.md` defines immutable Git-commit-derived source manifests, content-hash checking and negative integrity tests. Still **NOT** an autonomous coding agent, signed installation or production worker.
- **Protected production/recovery lane:** operational supervisor root `SUPERVISOR_ROOT_UNAVAILABLE`, startup policy `SUPERVISOR_STARTUP_POLICY_UNPROVEN`, startup definition read failed, port3201 unprobed. The serving binary still rejects some new typed recovery commands. Signed installed T0215 epoch1 and authentic pending T0366 epoch2 must remain untouched until reviewed protected release/LKG authority exists. Official Secure MCP runtime has external ownership, never instantiate a duplicate. No private signer online.
- Next: verify R3B3 GitHub CI, implement R3B4 trusted source snapshot and bounded owner PID/port stop/restart, then T0479 versioned stable worker/LKG. This supersedes older Chat48/Chat51 target and hourly-fallback statements in this architecture file; those paragraphs are historical only.

## 2026-10-11 actual live progress (supersedes historical states below)

- T0477 split Chat48 registry / Chat51 independent Wake was successfully reconciled via source-current guarded paired recovery. Both authoritative Chat51 URL/digest match; independent Wake target generation32. No signed-image rotation was needed.
- WakeHost dev84 is RUNNING; exactly one manual generation32 event `manual-wake-mcp-1791691944140` ended `SENT`, `EXACT_USER_MESSAGE_APPENDED`, timer `COMPLETE`, queue0. This is confirmed **manual**, not natural automatic Wake acceptance.
- The **production Recovery** milestone still has unverified stable supervisor startup definition/policy and unavailable supervisor root; old serving controller cannot execute the current typed nine-layer diagnose via MCP. It must not be marked COMPLETE.
- T0478 starts the parallel **nonprivileged source-only development loop** `scripts/catdesk-dev-lane.ps1 status|build|verify`. It never starts a daemon/browser or installs a production executable; `isolatedRuntimeReady=false` until R2 proves state/port/tunnel separation and safely enables a development worker.
- Retain all protected signing/rollback boundaries for actual production installs. Do not serialize ordinary source engineering on pending T0366 epoch2 rotation.



Date: 2026-10-10 local / 2026-10-11 UTC. Canonical human/hourly Chat51:
https://chatgpt.com/c/6acaae13-4b18-83e9-b6ca-3c5e00cf47a8.

Status: **ARCHITECTURE PIVOT APPROVED BY USER IN CHAT / FIRST SOURCE SAFETY PATCH IMPLEMENTED**.
This document is a forward design and specific acceptance plan, not authority to
change protected Program Files state or run browser Wake events.

## The architectural diagnosis

CatDesk currently treats six logically distinct operations as one serial
deployment gate: editing source -> Windows verification -> independently reviewed
build -> product-root signed immutable Program Files image -> installed
version/provenance and protected promotion -> serving daemon / WakeHost event
delivery and target rebinding. Signed main-image rotation is itself stuck:
T-0215 epoch 1 is installed and trusted, T-0366 epoch 2 is cryptographically
signed and persisted as pending with an intact incoming image, incoming signed
envelope and staging image, but no installed rotation receipt. The historical
T-0373 attempt returned a broad INSTALL_FAILED. T-0474 ordinary (nonadmin)
Win32 probes returned read-open0 for all three images, DELETE ACCESS 5 for the
two Program Files objects, and DELETE ACCESS 0 for ProgramData incoming.
This proves non-admin Program Files delete rights are absent (expected), *not*
that the elevated MoveFileExW would fail. In-place swapping a possibly running
executable is unnecessarily fragile; hard-coded production keys/epochs should
not block routine local development.

A **separate**, proven obstacle is that the live CatDesk control daemon is
older than current source, and its guarded current-chat target rollover rejected
the current Chat51 request. Independent WakeHost is STOPPED. The freshest operator-run independent
WakeHost readback now shows **Chat51 generation32**, but the CatDesk project
registry remains at **Chat48**; the older serving daemon's cached gen31 Wake
snapshot must not override the fresh independent readback. Until the split
pair is reconciled, only the hourly Chat51 deadman is a confirmed
continuation mechanism. Signing a new main image is not intrinsically required
to update a ChatGPT conversation target if the already-existing operator-owned,
guarded project+Wake target update is used. Conversely unpaired target setters,
hand-editing registry/Wake JSON, spoofed browser events and continuing on the
old target are not acceptable shortcuts.

## New operating model: 3 independent lanes

### A. Developer loop — default for ordinary milestones

- Source of truth: Git branch/commit, review notes, CI (Rust + Python + Wake),
  and the local tested binaries. Git provides source history and rollback; it
  does not by itself authenticate an arbitrary installed administrator binary.
- Developer runs current source in a separate non-elevated identity and
  nonproduction instance, on distinct state roots/listeners, with a clearly
  marked DEVELOPMENT identity. Never point this instance at the externally
  owned Secure MCP tunnel or overwrite production protected receipts.
- Use one command to build, test, start/stop that isolated worker, inspect its
  status, and revert to the last verified working *development* build. No
  Ed25519 private signer and no administrator access for each source edit.
- The development instance must fail closed on any production state root,
  external tunnel ownership, privileged install, or real production event
  delivery request. It may use fixture Wake profiles for browser adapter
  testing but must not emit Wake events to an unapproved live target.
- This lane's full implementation is NEXT SOURCE TASK, not yet present.
  First acceptance: verified isolated no-tunnel/no-protected-state startup,
  then bounded provider work/review and recoverable restart after crash.

### B. Independent Wake/continuity — not coupled to binary rotation

- WakeHost is its own versioned, hash-pinned host with an independent
  production profile. CatDesk source already supports a **paired CAS**
  transaction `mcp::operator_update_designated_chat_target`, exposed through
  Binagotchy `target set <url>`. It reads coherent old project/Wake identity,
  performs a guarded generation increment, compensates a failed second stage,
  and rejects mismatches. Current source also has a special correction for
  a known earlier registry-only digest defect.
- **Immediate T0476 code repair (THIS COMMIT):** opening
  `--catdesk-binagotchy-cli` no longer calls
  `start_installed_only_if_desired_running` at all. Status and a guarded
  `target set` must NEVER launch a stopped WakeHost; only explicit
  `wake start`/`wake resume` may do so. This directly removes the old
  accidental-start risk from the operator console's paired rollover path.
- Operator may run the **local source-built, tested CLI in normal PowerShell**
  without installing it as the protected production main image, only to
  perform this user-owned **narrow paired target operation**. The CLI explicitly
  does not start a second daemon or Secure MCP tunnel and does not invoke the
  signing/rotation machinery. This is not authority for unsigned production
  daemon deployment. The same safeguards (canonical URL, exact old CAS,
  source-current paired transaction, independent registry+Wake readback) apply.
- For Chat51, the expected target is
  `https://chatgpt.com/c/6acaae13-4b18-83e9-b6ca-3c5e00cf47a8`, hash
  `7faf9cc96669e240362da0dd4266bec8b2585f0ef3e03b4b56ae4c8f66db1b22`.
  The **registry** still shows the old Chat48 URL with SHA
  `3a1d4cc69dfa3d05cb2bc33ef1197ac5a7433a120cfb4214a6996edb5a9d29e9`,
  while independent Wake is **already Chat51 generation32**. T0477's guarded
  wake-first recovery source is specifically designed to commit only the
  outstanding registry side when Wake and the caller's exact requested
  target/digest already agree; it must NOT advance Wake to generation33.
  A separate preexisting helper still handles same-URL/damaged-registry-SHA
  correction. No blind retry of an old paired rollover is permitted.
- Precise live gated steps: focused tests + all Windows CI green for the
  **T0477 R2 partial-recovery fix** -> assistant first tries the currently
  supported, guarded paired CatDesk tool (old serving gateway has rejected
  this exact split state) -> only if still unavailable, a **rare operator**
  one-line locally rebuilt source CLI issues `target set <Chat51>`, `target`,
  `wake status`, `exit`. It must return the exact success receipt and
  current Wake generation32 without advancing it, plus the assistant's
  independent CatDesk registry readback must show Chat51 URL/hash. Do not
  rely on old-serving `catdesk_transport_status` if its Wake snapshot is
  stale. Only when both authorities have been proven equal with WakeHost STOPPED ->
  **separate explicit authorization** to start WakeHost if auth/session ready ->
  one MANUAL canary, explicit receipt, then actual natural event acceptance.
  Never infer success from CLI output alone. A singleton Binagotchy mutex can
  make a one-shot command exit harmlessly without executing; require
  explicit success and full independent verification.
- Failure condition is STOPPED with exact diagnostic; don't repair by writing
  either protected store directly or running old installed source.
- Production-ready future improvement: move the signed/guarded paired target
  authority into the independent Wake control plane, not the app-image updater,
  with exact two-store compatibility/rollback tests.

### C. Production update — intentionally infrequent, simple and guarded

- Retain signature verification where genuinely necessary to protect privileged
  production execution. GitHub commit/CI identifies candidate source, but an
  installed binary needs its own measured executable/manifest assurance.
  For a one-person operator system, exact pinned hash + local explicit
  approval is sufficient for development; don't demand an offline root-key
  signature for ordinary source testing.
- Converge on the **already designed stable supervisor** T-0223: a small,
  infrequently updated trusted launcher/control plane with versioned worker
  directories. Do not replace an executable image currently mapped by
  Windows. Stage a new version beside the old one; review + verify hash;
  quiesce only the CatDesk-owned worker; atomically select the new version;
  restart; health-check; restore previous version automatically if not ready.
- Use one operator-facing release command and one retained last-known-good
  release. State shows `currentCommit, currentImageSha256, priorImageSha256,
  status, rollbackAvailable, wakeTargetGeneration`. A single bounded failure
  reason is better than multiple ambiguous review/epoch/promotion states.
- Protected authority remains: no private Ed25519 key accessible to CatDesk,
  no signing fakes, no direct Program Files image overwrite or pending receipt
  deletion, no Secure MCP tunnel lifecycle claim. Existing signed pending
  T0366 epoch2 must be reconciled safely as historic provenance, not secretly
  discarded; this is a separate production maintenance task with a reviewed
  quiesce/upgrade flow and bounded Windows failure diagnostics.
- Consolidate redundant build/attestation/promotion/supervisor gates behind
  one transaction which produces source SHA, built SHA, tests, review approval,
  installation receipt, health/rollback receipt. Internal integrity checks
  remain, but the user doesn't need a new manual ceremony for each substage.

## Priority reversal and measurable next milestones

1. **T0476 — No implicit Wake activation.** Completed source fix and all
   three GitHub Windows CI jobs passed (commit `4213ba8`,
   run `38105874271`); the non-daemon local CLI cannot start WakeHost
   merely by opening.
2. **T0477 — Complete the interrupted Chat51 target pair.** Source commit
   `58c4bfc` keeps Wake generation32 as-is while reconciling old registry
   Chat48 to Chat51 under exact CAS, preserving legacy same-URL digest-only
   repair. Windows CI run `38108654054` PASSED all three Windows jobs, including
   both designated-chat repair regression tests, before live recovery;
   require explicit updated CLI receipt and independently verified paired
   readbacks, then separate Wake start/canary approval.
3. **T0478 — Isolated dev runner.** One explicit `catdesk dev ...` interface,
   separate state and ports, no production transport or signer, build/test,
   stop/restart and measurable task progression, no accidental session
   duplication. CI and safety tests prove forbidden production access.
4. **T0479 — Stable supervisor versioned worker release.** Inventory what
   T-0223 already implements; implement missing operator activation, versioned
   source-current rollout, LKG and bounded recovery. Avoid new overlapping
   release systems or a second tunnel owner.
5. **T0480 — Production signed-state cleanup.** Review T0366 pending stage,
   file-sharing/ACL evidence, identify the real elevated replacement failure;
   preserve signed receipts, provide reviewed one-shot completion or
   monotonic supersession before any new signed release. Stop the current
   unbounded series of bespoke T03xx scripts.
6. Resume Recovery nine-layer doctor and core milestones, then multi-project
   scheduling / Binagotchy UI / natural Wake maturity. Source implementation
   is allowed to progress via dev lane while production signing is parked.

## Explicit rollback and gate criteria

- Target binding success requires **both** project registry and independent
  Wake target exact canonical Chat51 + SHA, Wake generation **32 retained**
  through the recovery (not incremented again), WakeHost STOPPED,
  no old Chat48 submission, no duplicate.
- The first live Wake MANUAL canary is not natural acceptance; check one exact
  new generation receipt and stopped/ready status before declaring reliability.
- The old source/current official Secure MCP client remains externally owned
  and healthy; no duplicate service or tunnel introduced.
- Do not claim the new dev runner, stable supervisor or one-command production
  release already exist; this document defines their implementation standard.
- If source-only CLI rollover is blocked on the operator machine, the branch
  remains safe and the hourly deadman remains the fallback; continue source
  milestones without waiting for signed production rotation.

## Evidence

- T0474 operator Win32 non-admin probe result at
  `.catdesk/t0474-rotation-open-probes.json`: installed/staging READ0 DELETE5;
  incoming READ0 DELETE0; `tokenIsAdministrator=false`. No Win32 32
  observed *for those exact access modes*. This cannot prove elevated
  MoveFileExW success/failure.
- Existing T0473 fresh inventory at `.catdesk/t0372-rotation-state.json`
  and signed T0472 readback verified T0215 installed1/T0366 pending2.
- Source `binagotchy_cli.rs` has a paired updater. T0476 removed
  `start_installed_only_if_desired_running` from console startup.
  Independent Wake was later observed already changed to Chat51 generation32
  without a corresponding CatDesk registry commit; no WakeHost launch/browser
  event, protected main-image rotation, production daemon reload or private
  signer use was performed by this recovery work.
