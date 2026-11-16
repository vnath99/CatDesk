# CatDesk important commands

**Verified follow-up 2026-11-16T00:09:22.940282+00:00:** source `1ae346351db6c4659d08f1d0e755a46ddd49b411` adds journal-derived remaining tool-call budget to task metadata/resume. Counts survive close/reopen and conversation rollover, include failed calls, and remain unknown when the journal cannot be read. Elapsed scope is explicitly per serving connection; ChatGPT turn usage is unknown. Four targeted tests, strict workspace Clippy and formatting pass. This is source-only readback; no budget reset, grant change, selected-package rebuild or protected upgrade. The separately authorized actual-runtime RX repair and native version check passed. Full Web edit/checkpoint/self-Wake remains OPEN. [Evidence](docs/verification/NATIVE_RUNTIME_REPAIR_APPLIED_2026_11_15.json).

**Approved runtime repair 2026-11-15T23:00:48.899963+00:00:** the frozen DACL-only operation applied once under explicit user authorization. The actual installed interpreter now passes its bounded native version check; images are unchanged, write/owner/audit rights were not granted, and both owners exited normally. No original-task replay, package, production release or v5 change. This supersedes the earlier pending runtime-owner decision. [Actual result](docs/verification/NATIVE_RUNTIME_REPAIR_APPLIED_2026_11_15.json).

## 2026-11-15T22:34:51Z — Runtime-owner repair is separate from release promotion

The existing native sandbox accounts need OS read/execute access to the approved interpreter. Logical native root-read permissions did not provision a genuinely private fixture. A speculative application policy change was discarded; the production runner remains byte-for-byte unchanged. Existing accounts plus a DACL-only read/execute grant repaired a private fixture, including the exact previously refused copy, while an actual write probe was denied.

The private frozen proposal is preflight-ready: directory identity/physical path, owner WRITE_DAC, five core image hashes and both existing native reader identities are checked. Only its exact resource-owner approval remains; do not run Execute, setup, elevation, fingerprint edits or original-task calls before that decision. Proposal SHA-256 `75b134ce8158328e22cc0b996cf7b6ae35cefc9493efb3e8633d8c482a159099`; script SHA-256 `99e25101082f52fe001cca93dbb546c97cf0d075df023e78191cf71fbc1d7f62`. Private path/SDDL/SIDs remain in the established local audit checkpoint, not GitHub. Timestamps/log references do not invalidate it; changed resource/ACL/image/account identities do.

The source-only ignored probes `native_private_runtime_without_access_is_refused` and `native_private_runtime_with_access_is_read_only` require distinct unprovisioned/provisioned disposable parent states, exact approved interpreter path/SHA and test-only `CATDESK_TEST_PRIVATE_RUNTIME_PARENT`. Never point these fixture preparations at an existing install, project, profile or authentication directory. They preserve negative-control and private receipt evidence; ordinary tests skip live probes. No new installation, account, connector or release manager. [Evidence](docs/verification/NATIVE_RUNTIME_READ_ACCESS_2026_11_15.json).

## 2026-11-15T21:34:58Z — Bounded real-runner qualification

From the authorized source/build worktree, with the existing Rust toolchain and target directory configured:

```text
cargo test --locked --offline --bin catdesk project_task_command::windows::tests::native_system_lookup_uses_actual_command_owner -- --ignored --nocapture
```

This ignored Windows integration probe calls the actual fixed-command runner, not a separate sandbox approximation. It performs one system-only lookup in disposable state with a 15-second command bound, retains exact private failure receipts and emits only bounded metadata. No task/grant, model call, setup, install or Web send. The separate ignored `native_python_version_uses_actual_command_owner` requires test-only `CATDESK_TEST_NATIVE_PYTHON` and `CATDESK_TEST_NATIVE_PYTHON_SHA256` from the already approved interpreter identity; it runs only isolated/bytecode-disabled version inspection. Do not repeat its current known failure without a changed prerequisite. Never substitute these probes for the original verifier or full Web acceptance.

Readiness was qualified through the existing app-server proxy using the documented WebSocket HTTP Upgrade, initialize and `windowsSandbox/readiness` only. Raw JSONL is not the Unix control-socket wire protocol. No new daemon, connection configuration, setup or credential was used. READY is scoped to that existing 0.169.0 daemon; it does not qualify the original 0.163.0 verifier or interpreter. [Evidence](docs/verification/NATIVE_SANDBOX_READINESS_AND_COMMAND_QUALIFICATION_2026_11_15.json).

**Development update 2026-11-15T21:01:12Z:** future native verification receipts now have a bounded advisory sandbox-spawn diagnostic; source 7b4e1d6d passed five targeted command tests, strict Clippy and formatting. Original failed receipt/task and production/v5 remain unchanged. No new connection, package selection, replay, setup or release authority. [Evidence](docs/verification/NATIVE_COMMAND_FAILURE_DIAGNOSTIC_2026_11_15.json).

Source-only follow-up 2026-11-15T20:32:08Z: `9d96f8e32f3fdf8b528d15eda453e9398f74a392` additionally validates retained operations against the original full preview-diff hash without executing or recording a tool call. The changed-operations denial, all eight native recovery tests, strict Clippy and formatting pass. The live-tested selected app remains the exact `79a449b` build; no candidate rebuild or protected promotion was performed for this follow-up.

## 2026-11-15T20:23:03Z — Use the existing event-bound body reader

```text
<qualified-controller> operator web review read --project-id <registered-id> --event-id <exact-SENT-event>
```

Despite its historical `review` name, this existing reader handles the exact ordinary diagnostic event after delivery. It requires matching project, paired target generation/digest, SENT receipt and message hash, then uses the selected qualified browser owner for bounded read-only snapshotting. Require `eventVerified: true`; extract only necessary markers/errors, never arbitrary fallback snapshots. This grants no review verdict, execution, retry or release authority. `operator web work response` is installed timer re-observation and can return already-COMPLETE metadata without a body; do not treat its development refusal as proof that the body reader is missing.

The original local task now has one completed patch and a failed native sandbox verifier receipt. Keep its exact assignment, original HEAD, uncommitted assigned-file edit and call IDs. Do not repeat the completed read/preview/apply/verify, change its command profile or create a replacement grant. After genuine new environment evidence, revalidate the existing task through supported controls before any new verification action. The observed CLI is 0.163.0; its help does not support the newer documented `sandbox setup` command, so no setup command is currently prescribed. Preserve all managed permissions and existing sandbox selection. [recovery and live evidence](docs/verification/SETTLED_READ_ONLY_TASK_RECOVERY_2026_11_15.json).

Latest decision 2026-11-15T19:33:36Z: one system and canonical v5 connection; frozen releases are production, source/builds are development. Existing local test facilities require no external setup or production upgrade. Local task test reached accepted read/preview only, then stopped on a harness receipt-parsing error; its original task remains fenced, with no edit/verify/Git/Wake acceptance. Production/profiles preserved and test-process cleanup verified. See [dated audit](CATDESK_ACTIVITY_LOG.md) and [qualification](docs/verification/DEVELOPMENT_PRODUCTION_SEPARATION_2026_11_15.json).

## 2026-11-15T19:18:45Z — Use the existing development owner, not production promotion

From the already prepared development control workspace, invoke the hash-qualified development app/controller, never a production launcher:

```text
<qualified-dev-controller> operator web-dev-runtime status
<matching-source-dev-controller> operator web-dev-runtime update-package --package <exact-four-file-directory>
<qualified-dev-controller> operator web-dev-runtime record-verification --input-file <in-workspace-evidence.json>
<qualified-dev-controller> operator web-dev-runtime run-backend --max-runtime-seconds 60
<qualified-dev-controller> operator web-dev-runtime run-project-backend --project-id <registered-dev-id> --project-target-generation <n> --project-target-sha256 <digest> --max-runtime-seconds 180 --project-task-contract <absolute-contract>
```

`run-backend` requires a valid process-only `CATDESK_MCP_AUTH_TOKEN`; do not print it or put it on the command line. Use its issued random-loopback readiness URL, not production port 3200. The existing foreground owner bounds and drains the child; no recurring service is enabled. Package update requires stopped Host/browser/backend owners, exact inventory and scripts matching the controller's compiled source. A mismatch is a refusal, not permission to relax the check. Source build/test facade `scripts/catdesk-dev-lane.ps1` stays source-only; the existing operator commands above already supply the runtime lane. Local development verification is never production review authority. [development qualification](docs/verification/DEVELOPMENT_PRODUCTION_SEPARATION_2026_11_15.json).

The installed-adapter refusal documented below blocks only that production send. Keep that original queued task unchanged; use separate registered development projects and bindings for development acceptance. Keep the single canonical v5 connection and its matching bootstrap; test newer functionality through local MCP harnesses without external infrastructure changes. Actual Web use of missing installed operations requires their genuine reviewed release authority. Protected review/migration is a separate track, not an ordinary build/test prerequisite.

November 15 fresh central-chat qualification: the operator's one-time setup is complete. Native history verified one exact frozen setup USER and its completed acknowledgment. The installed `operator web target bind --project-id <new-project> --conversation-url <verified-issued-url> --expected-current-target-sha256 none --expected-wake-generation 0` paired only the new project at generation 1; do not rerun it. One request and its scoped assignment are now recorded and associated, without a grant/key or delivery. The original prepared zero retry budget failed validation; an unissued compatible revision changes only that field to the legacy schema minimum 1, retaining no-resend instructions and every existing replay fence. The old input is preserved.

The installed source does not parse `operator web work deliver`. One bounded attempt using the retained hash-qualified development controller reached the existing selected-package preflight and refused `SCOPED_WORK_INSTALLED_ADAPTER_UPGRADE_REQUIRED` before browser creation. That source-controller operation is not installed-backend qualification. Do not repeat delivery, change its adapter/bridge allowlist or route around the guard. The selected package lacks the exact supported scoped native-entry pair; its reviewed upgrade is now a demonstrated delivery prerequisite. The backend's separate reviewed promotion is required for its missing delivery CLI and scoped Git checkpoint capability. Future first-chat creation must use CatDesk automation after legitimate installed capability qualification; repeated operator setup is not the intended workflow. [Actual results](docs/verification/FRESH_CENTRAL_WEB_PAIRING_2026_11_15.json).

Source follow-up: `ExecutionContractV1` and its advertised MCP schema now accept `retryBudget: 0` so a contract can request no retries. Positive turn/tool/time budgets, path validation, approval gates and durable no-replay checks remain unchanged. The legacy installed validator still needs its minimum 1; this source change neither edits the associated revision nor qualifies a new release. Nine targeted contract/schema checks pass, including the original approval/hash/expiration and immutable-intent tests. [Dated evidence](docs/verification/FRESH_CENTRAL_WEB_PAIRING_2026_11_15.json).

November 15 timer observation: Wake already starts a project-bound timer at submission. Reuse `catdesk_turn_timer` with `{"action":"STATUS","timerId":"<original-event-id>"}` for that exact stored timer; do not start a second manual timer to observe it. The source correction uses the existing read-only store and no longer requires a central CatDesk conversation for STATUS. It returns stored project/generation/digest metadata, not a current-binding assertion, delivery proof or execution grant. START/STOP retain their existing central-project/manual-timer restrictions. Missing stores remain missing; no initialization, browser or scheduler is performed by STATUS. This correction is source-tested and not installed. [Evidence and limitations](docs/verification/PROJECT_TIMER_STATUS_2026_11_15.json).

November 15 installed registration qualification: the existing canonical executable successfully ran `operator web project register --project-id <new-id> --workspace <clean-source-worktree> --git-identity <exact-origin>` from the actual serving control workspace. The result is `REGISTERED_WEB_ONLY`, unbound and without a native owner. Task edits must use a distinct, unregistered linked execution worktree; registering a source workspace does not grant edits there. The completed new-project operation must not be rerun. Its fresh-chat setup awaits one actual issued URL before guarded pairing or work admission. Installed source has no scoped Git-checkpoint capability; do not use general global-root Git tools as a project-task workaround. [Actual scope](docs/verification/CENTRAL_WEB_PROJECT_PREPARATION_2026_11_15.json).

November 15 direct Web preflight clarification: doctor now says `PREPARE_NEW_SCOPED_ASSIGNMENT_WITH_EXISTING_LOCAL_PREFLIGHT` or `VERIFY_ORIGINAL_ASSIGNMENT_GRANT_AND_OWNER_WITH_EXISTING_LOCAL_PREFLIGHT`. These replace misleading native-preflight advice, not authorization checks. Local scope/ownership validation does not invoke a Codex model. Missing optional Codex tooling leaves independently eligible project metadata checks available; grant validation and actual execution remain separate. `pwsh -NoProfile -File scripts/test-catdesk-doctor.ps1` passed 213 assertions through the real facade with disposable fixtures. This source script qualification does not prove installed or Web execution acceptance.

November 15 development authorization: existing permissions cover scoped source inspection, implementation, local tests, isolated builds, documentation, Git commits and reviewed feature-branch publication. Protected production promotion is not a prerequisite. Preserve project/task ownership and separate authority for credentials, signing, protected releases, external browser submissions, destructive changes and uncertain-operation replay. Do not alter Codex/platform rules to unblock a command. Record a refused operation once and continue independently eligible work.

The existing `autonomy_contract_create` and `autonomy_contract_validate` tools prepare and validate an inert DRAFT; they do not approve, claim or execute it. Their allowed directories must already exist within the approved workspace. Safely preparing the exact intended empty report directory can satisfy that prerequisite without changing frozen arguments. Source diagnostics now distinguish `AUTONOMY_DRAFT_ALLOWED_DIRECTORY_REQUIRED`, scope denials, approval requirements and replay denials; this change is not installed. A generic command-runner `blocked by policy` refusal before process creation is not a CatDesk authorization verdict, and its undisclosed rule must not be guessed. The completed f7 draft must not be recreated or advanced under draft-only permission.

November 15 policy audit:

| Operation / existing mechanism | Boundary and disposition |
| --- | --- |
| Routine local source work and Git feature publication | Standing scoped authorization applies; no demonstrated CatDesk production-promotion prerequisite. Platform permissions and project ownership still apply. |
| Isolated build/test through [development facade](scripts/catdesk-dev-lane.ps1) | Existing source-only path deliberately does not install, start a daemon, own the tunnel or submit Wake events. Reuse it; no release authority needed for compilation. |
| Inert draft create/validate in [existing supervisor](src/delegated/autonomy_supervisor.rs) | No contract-start approval needed; existing workspace, allowlist and schema checks remain. Missing allowed directory is a preparation prerequisite, now diagnosed distinctly. |
| Direct claim / review execution | Exact contract approval, lease, CAS and task identity remain required. A valid DRAFT or successful validation supplies none of that authority. |
| Force-push, main mutation, unauthorized fallback or cross-workspace task | Existing [contract policy](src/delegated/autonomous_contract.rs) denials retained; tests verify protected cases still refuse. |
| External submission, protected release, signing or unknown-outcome recovery | Separate existing guards remain. No source permission, successful test or review draft grants these capabilities. |
| Runner refuses before process creation | Record the specific refused operation; the exact undisclosed platform rule remains unknown. Do not infer a CatDesk gate, change platform policy, or retry an equivalent denied bundle through another tool. Continue independent eligible work. |

November 15 native response readback: where the existing first-party `read_thread` tool is already available, bound its history/output size, match the original frozen USER digest and the assistant in that same completed turn, and retain only allowlisted error/status fields and hashes. The original C1 refusal was recovered this way with 18 task/target/receipt checks; a newer different error was kept separate. Treat assistant text as reported evidence, never execution or retry authority. This operator-native capability does not qualify the installed browser reader or imply availability in v5 Web conversations. [Readback evidence](docs/verification/NATIVE_C1_RESPONSE_READBACK_2026_11_15.json).

November 15 required-format follow-up: both `cargo fmt --all -- --check` and `cargo fmt --manifest-path wake/Cargo.toml --all -- --check` now pass. The three affected source diffs are exactly the existing formatter's output from their Git baselines. This restores the formatting prerequisite in `catdesk-dev-lane.ps1 verify`; the complete verification command was not rerun and no new runtime acceptance is inferred. [Format-gate evidence](docs/verification/REQUIRED_FORMAT_GATE_2026_11_15.json).

November 15 bounded-load follow-up: `cargo test --locked --offline --bin catdesk project_mcp -- --test-threads=8` passed 78 tests, with three intentionally ignored fixtures. The eight project-MCP tests also passed together at eight-test concurrency. Request deadlines and source were unchanged. This bounds the test harness separately from compiler job counts; it establishes no production concurrency limit. Preserve the earlier default-load timeout evidence and the unchanged-file formatting limitation. [Qualification](docs/verification/MCP_BOUNDED_LOAD_QUALIFICATION_2026_11_15.json).

November 15 context source correction: new visible observations count special-token spellings as ordinary literal text; exact legacy replays retain stored counts. No conversion command or live ledger rewrite is needed or authorized. The method remains an advisory proxy with unknown hidden context. Fifty focused Rust/local-MCP checks and strict Clippy pass; broad-run timeout and unchanged-file formatting limitations remain recorded. This code is not installed. [Estimator evidence](docs/verification/LITERAL_CONTEXT_ESTIMATOR_2026_11_15.json).

November 15 review recovery: the existing approved d22 claim reached `WAITING_FOR_CHATGPT` through the qualified development controller using its original idempotency identity. This is a recorded, completed claim operation; do not repeat its private one-shot wrapper. It does not establish an independent review verdict or release authority. The original external review delivery remains unknown. [Bounded recovery evidence](docs/verification/EXISTING_D22_CLAIM_RECOVERY_2026_11_15.json).

Run shell examples in an ordinary PowerShell terminal from the intended checkout. Confirm `Get-Location` and ownership first. These are command references, not authorization to run protected lifecycle operations. Do not put credentials or private contexts on a command line. Installed executable, source facade and development packages are distinct.

## Environment and source identity

| Command | Purpose / prerequisites / effect | Expected result and failure |
| --- | --- | --- |
| `git status --short --branch` | Read-only; any Git checkout | Branch, tracked/untracked work; preserve other owners' changes |
| `git rev-parse HEAD` | Read-only exact source identity | Full commit; failure means no established checkout |
| `$PSVersionTable.PSVersion; git --version; rustc --version; cargo --version; python --version` | Read-only toolchain inventory | Installed versions; missing tool is a prerequisite, not test failure |
| `codex --version` | Read-only native CLI version | Version; optional for direct Web workflow |
| `codex resume --all` | Interactive native CLI session selector; reuse supported owner | Select original task; never steal app writer or replay work |

`/goal resume` is a Codex chat/session command, not a PowerShell command. Model/session continuation must preserve original ownership and held outcomes.

## Health and serving lifecycle

| Command | Availability / permissions / effect | Expected result and failure |
| --- | --- | --- |
| `.\catdesk.ps1 doctor -Workspace (Get-Location).Path -Human` | Source and canonical checkout facade parse this command; read-only bounded diagnosis; installed compatibility depends on paired files | Layered canonical/serving/auth/tunnel/Wake checks; failed layer is distinct from overall readiness |
| `.\catdesk.ps1 diagnose -Workspace (Get-Location).Path` | Public read-only lifecycle diagnosis | Structured state; ROOT_UNAVAILABLE/authority mismatch requires reconciliation |
| `.\catdesk.ps1 status -Workspace (Get-Location).Path` | Public read-only owner/status | Existing owner health; no duplicate start |
| `Get-NetTCPConnection -LocalPort 3200 -State Listen -ErrorAction SilentlyContinue` | Read-only local listener check | Listener identity or absent; does not establish MCP authentication or connector readiness |
| `.\catdesk.ps1 start -Workspace (Get-Location).Path -BackendOnly` | State-changing; existing qualified canonical owner and reviewed release required | Guarded ready state; never creates release authority or a replacement tunnel |
| `.\catdesk.ps1 recover -Workspace (Get-Location).Path -BackendOnly` | State-changing; existing approved current/previous release authority required | Existing release-only recovery; missing LKG/transaction/root proof refuses safely |
| `.\catdesk.ps1 stop -Workspace (Get-Location).Path` | State-changing bounded owned-backend stop; preserve active work | Confirm owner/descendant exit before restart |

Restart uses supported stop â†’ status â†’ start, with verified owner and original release metadata. Do not run it merely because a client UI timed out. Stable supervisor automatic reboot/failure restoration is NOT qualified. External Secure MCP is separately owned; this document does not authorize its settings or replacement.

### Interpreting Wake and connector status

On November 14, the supported Wake lifecycle status returned STOPPED/PID 0, while the older installed transport tool returned an October 15 RUNNING snapshot with an absent PID. Check the host lease/process and observation timestamp before treating a stored projection as live. Current source annotates file age; the older installed binary does not. Do not clear old errors or resume a held queue to make status green.

The native catalog publishes `catdesk_turn_timer`, but its current handler starts timers on the default `catdesk` target. It is not a caller-selected project timer. Verify the exact target before START; a timer is bookkeeping, not send authority. No fresh timer/Wake was started during the [November 14 baseline](docs/verification/WAKE_LIFECYCLE_BASELINE_2026_11_14.json). A healthy official owner and local authenticated catalog do not establish that a particular ChatGPT connector works; preserve the exact outer error without inventing an HTTP status.

## Development, release and rollback

For a pending first conversation, `operator web initialize status --project-id <registered-id>` reads the existing journal without opening a browser or sending. A `SUBMITTING` / `SEND_ATTEMPTED` result with `submissionOutcomeUnknown: true` is a no-replay fence. Do not call `initialize execute` again, re-register the task or invent a replacement URL. The November 14 canary originally entered that state; qualified no-send recovery and its fresh two-turn Wake are now complete. Preserve their receipts and history and do not rerun either operation. Other unknown initializations retain their no-replay fence. [Actual attempt and cleanup](docs/verification/WAKE_LIFECYCLE_LIVE_RECOVERY_2026_11_14.json).

The development operator action `operator web-dev-runtime reconcile-root-view --expected-authority-sha256 <sha256>` performs a bounded preflight against the operator process current working directory. Use only a qualified immutable development controller, not an arbitrary executable from PATH. Expected output is `ROOT_VIEW_PREFLIGHT_ONLY` with old-authority, observed-root-view and proposed-authority hashes. The retained-runtime preflight and explicit one-file transition passed on November 14 using the controller from `0f6adeb8cff47a3aaa8a2edb65bc8f6e19e29e5a`; the unchanged selected dev.85 controller then passed status. This is development metadata qualification, not a protected installation upgrade.

The explicit commit additionally requires `--expected-root-view-sha256 <preflight-hash> --execute`. It admits only the known package-alias split, requires stopped/exclusive owners and profile quiescence, retains the original development authority, and atomically changes only that authority's exclusion-root list. `ROOT_VIEW_RECONCILED_STOPPED` means this metadata transition passed; `ROOT_VIEW_ALREADY_RECONCILED` is a read-only exact-result recognition. Neither starts a browser/host, changes a package/profile/binding, nor repairs protected T-0485 state. Changed hashes, lost anchors, active owners, altered artifacts and pending login/package markers refuse. [Qualification](docs/verification/DEV_ROOT_VIEW_RECONCILIATION_2026_11_14.json).

| Command | Availability / prerequisites / effect | Expected result and failure |
| --- | --- | --- |
| `.\scripts\catdesk-dev-lane.ps1 status` | Source-only developer facade; read-only | Source/debug identity; not runtime selection or installation |
| `.\scripts\catdesk-dev-lane.ps1 build` | State-changing local compilation; reviewed workspace/toolchain/offline cache | Debug artifact SHA; never production promotion |
| `cargo fmt --all -- --check` | Source formatting; no daemon | Pass/fail for current source only |
| `cargo test --locked --offline --bin catdesk <exact_test_filter>` | Bounded task-specific tests; use real eligible filter and existing process owner | Actual test evidence, not installed acceptance |
| `.\scripts\promote-reviewed-catdesk-build.ps1 -BuildPath $candidate -Workspace $canonical` | Read-only plan is default; set exact already-qualified paths first | Current promotion prerequisites/plan; does not approve candidate |

Protected promotion `-Execute`, rollback and interrupted promotion use the [existing production lifecycle](docs/orchestrator/CATDESK_PRODUCTION_RELEASE_LIFECYCLE.md). Exact artifact review, producer, signature/authority, fingerprint, independent process ownership and rollback checks remain mandatory. No protected action is performed by this documentation change. Do not rewrite a fingerprint, self-sign, copy development bytes into production or discard an unresolved transaction.

## Secure MCP, projects and Wake

Use v5 `catdesk_transport_status` once for a fresh read-only connector check. A successful local catalog/resource response does not prove that Web client or embedded renderer. Credentials stay in the existing approved mechanism; unauthorized MCP must return 401. `CONFIGURED_UNVERIFIED`, HTTP 429 and `Session terminated` describe different observations; retain exact error/receipt and do not loop.

Use registered-project read/status and guarded project/Wake pairing through the [existing Web operator parser](src/operator_web_cli.rs) and [installed project controller](src/operator_installed_project.rs) when available. Verify the actual command parser in the selected revision before mutation. Exact URL, digest, generation and independent Wake counterpart must agree before delivery. Global `read`/`write` does not prove project-scoped authentication. The installed cached dispatcher envelope is documented in the [architecture](docs/orchestrator/CANONICAL_CURRENT_ARCHITECTURE.md); immutable task/key/grant controls still apply.

Manual Wake uses existing bounded event/receipt APIs; queue admission is not SENT or completed response. Do not enable WakeHost, hourly tasks, new profiles or recurring automation for a UI check. Authentication uses the existing qualified login owner; the browser is its interactive step. Do not clear unknown drafts. T-0485 root comparison/signing is BLOCKED; expired proposals must not be signed or collected again without reconciliation.

## Git and GitHub-only recovery

These require Git/network access, not CatDesk. `main` is stable source; canonical development/docs are `codex/web-rollover-oct17`. The primary compatibility publication is `codex/docs-primary-sync-nov14`. Clone into a new unused destination, never over another owner's checkout.

```powershell
git clone --branch codex/web-rollover-oct17 https://github.com/vnath99/CatDesk.git CatDesk-recovery
Set-Location CatDesk-recovery
git status --short --branch
git fetch --no-tags origin
git rev-parse HEAD
git ls-remote origin refs/heads/codex/web-rollover-oct17 refs/heads/codex/docs-primary-sync-nov14 refs/heads/main
python scripts/catdesk_docs.py check
```

Inspect development without changing active files: `git log -5 --oneline origin/codex/web-rollover-oct17`. Before synchronization: `git rev-list --left-right --count HEAD...origin/codex/web-rollover-oct17`. Zero local-only commits permits an ownership-checked fast-forward; any divergence requires review, not blind pull/rebase/reset. Preserve all stashes/untracked/ignored work and do not prune other worktrees.

After explicit scoped staging/review and local checks on an owned feature branch, publish that branch and integrate through the ordinary GitHub PR workflow. Read back its exact remote SHA and compare with local HEAD. Do not blindly push a feature HEAD over canonical development. A commit alone is not publication. No direct main push, force push or automatic merge. Keep private journals, signing/profile data and machine-specific logs out of commits.

## Documentation maintenance

### Recover an unknown first-conversation initialization (development source)

`operator web initialize recover --project-id <registered-project> --input-file <frozen-recovery.json>` observes the original candidate without sending a message. It requires the original `SEND_ATTEMPTED` journal, its exact file hash, original registration/message digests, a retained native-history proof hash, candidate URL and provider turn-start time. The native candidate is navigation evidence only; the owned browser independently verifies the complete original USER, one completed acknowledgment and persistence after refresh. An unknown draft prevents refresh. No native dispatch or execution grant is created.

Add `--execute` only for the authorized local receipt/admission/paired-target commit after these checks. A saved completed receipt allows interruption recovery without another browser send. Provider turn-start time is explicitly recorded as such, never represented as an exact observed send time. Wrong identities, changed records, duplicate USER messages, missing bodies/login or ambiguous observations refuse. This command requires the matching reviewed development package with `initialization_readback`; the older selected/protected package is not upgraded by documentation. Preserve the original unknown journal until this supported recovery succeeds; never rerun `initialize execute` for it.

November 14 controlled qualification: the matched package was selected in the existing unprotected development store; its predecessor is retained. The first no-send readback refused `RESPONSE_TARGET_DRIFT` and cleanup passed. The original unknown remains fenced, with no binding commit. This is not successful installed observation, direct Web work or production promotion. [Exact acceptance limits](docs/verification/BOOTSTRAP_READBACK_RECOVERY_2026_11_14.json).

November 15 controlled qualification: known `INITIALIZATION_READBACK_*` errors now survive the CLI's closed allowlist. Private `initial-readback-<digest>.json` diagnostic artifacts in the existing project journal directory bind the original record hash, project/intent/message identity and candidate digest to fixed phase/route/failure/surface categories. They contain no transcript or draft text and cannot authorize delivery, binding or retry. Identical evidence is retained once. Home-only recovery cannot fall through to reload or another CDP context and refuses unknown/nonempty state. Actual source `c43265ad` reports `INITIALIZATION_READBACK_HOME_STATE_UNPROVEN` with a complete Home document and missing editor/history; authentication remains unverified. Preserve the original pending journal. [Live limits and next source task](docs/verification/READBACK_HOME_DIAGNOSTICS_2026_11_15.json).

`python scripts/catdesk_docs.py check` validates the five documents, references/status/identity and privacy. `python scripts/catdesk_docs.py record --event <reviewed-event.json>` validates and previews a task-bound update; add `--apply` only to write the checked activity/ticket/handoff/milestone patch. Exact repeat is a no-op; a reused event ID with changed bytes refuses. This records evidence, never grants review/release authority. Use at task completion even when CatDesk is unavailable. Commands/results must reference real retained evidence.

The November 15 matched development package also reuses the existing verified-tab selector. A unique metadata match whose document reports Home remains unproven; selection never navigates, clears a draft or submits. Its private navigation diagnostic exports only closed document/history route classes, title/body categories, fixed Home-only notices and optional observed HTTP status. It does not export URLs, draft text or transcripts. Actual qualification identified `LOGIN_REQUIRED` on HTTP-200 Home, while stale tab metadata still named the candidate. [Exact scope](docs/verification/VERIFIED_TAB_READBACK_2026_11_15.json).

The subsequent source correction maps that explicit notice to `LOGIN_OR_PROFILE_REQUIRED` only when document and independent browser history both prove Home. Draft/generation refusals keep precedence; missing or conflicting observations retain the original uncertainty. This mapping was subsequently built and selected only in the unprotected development runtime; original no-send recovery then passed. This is not protected installation acceptance. [Source scope and timer limitation](docs/verification/LOGIN_REFUSAL_CLASSIFICATION_2026_11_15.json).

For sign-in, enter the prepared control workspace explicitly and use its selected, checksum-verified development controller, then `operator web-dev-runtime login --project-id <existing-paired-development-project>`, in an interactive PowerShell 7 terminal. This is the standard bounded login procedure, not an installation or binding change. Its native preflight requires exact pairing, production isolation, stopped host and exclusive profile ownership. An unbound project is not a valid login target; an already paired development entry opens the same profile. Sign in only in the browser, send nothing, preserve drafts, then return to the launching terminal and press Enter there. The whole interaction is limited to 120 seconds; guarded Job cleanup verifies descendant exit. `LOGIN_WINDOW_CLOSED_UNVERIFIED` means cleanup, not authenticated readiness. If it refuses or cleanup is uncertain, preserve the error/marker and do not force-clear or repeatedly relaunch. Once closed, independently observe the original pending candidate through `initialize recover`; never resend initialization.

Operator actions use the process current directory. For example: `Push-Location -LiteralPath $preparedControlWorkspace; try { & $qualifiedDevelopmentImage operator web-dev-runtime login --project-id $registeredProjectId } finally { Pop-Location }`. The first launch from the wrong directory returned DEV_RUNTIME_NOT_PREPARED without a browser; entering the prepared directory opened the same existing profile and closed cleanly. Authentication is complete for the latest controlled test; no new login is requested.

The retained fresh Wake is now SENT and its read-only canary CLOSED after one qualified recovery; do not run it again. New source `f7f1dede` extends the existing local `operator web work retry-pre-submit --project-id <registered-id> --request-id <original-id> --expected-delivery-sha256 <frozen-prior-sha>` with the paired options `--timeout-evidence-file <original-native-frame.json> --expected-timeout-evidence-sha256 <frozen-frame-sha>`. This is a local no-send metadata transition, qualified in the isolated development runtime, not protected installation authority. Only the exact closed ordinary SCOPED_REVIEW_TIMEOUT frame at BEFORE_SUBMIT is admitted, within 16 KiB, with matching project/event/message/generation/digest and elapsed deadline. The existing stopped-host, exclusive profile, registry/task lease, paired-target and CAS checks remain mandatory; any surviving submission witness, timer or archive refuses. The original failed delivery and proof hash are archived before CLAIMED. The frame itself still reports UNKNOWN and grants no retry permission; it is not sufficient without the complete current local proof. Generic UNKNOWN, SUBMITTING, changed payloads and missing evidence cannot enter this recovery. Without these paired evidence options, the old installed-only BROWSER_ATTEMPT_TIMEOUT recovery behavior is unchanged. A separate, explicitly authorized deliver operation is required after successful recovery; never automatically loop it. [Actual one-delivery qualification and preserved failure](docs/verification/FRESH_WAKE_RECOVERY_ACCEPTANCE_2026_11_15.json).

The existing source-only `.\scripts\catdesk-dev-lane.ps1 verify` now runs the documentation check before its unchanged Rust checks. Python 3.9+ is required for this development command; no daemon or external connector is involved. Automatic record mode updates the current handoff task only; acceptance/DONE, new live maturity, another task or conflicting history requires separate review. Interrupted partial document writes refuse reconciliation rather than inventing completion; normal Git diff/history is the recovery record.

### Project-specific work diagnostics

The reviewed development facade's existing `doctor` now includes bounded `PROJECT_REGISTRY.facts.workReadiness` rows and concise per-project human output. Invoke the qualified source facade with explicit observation context: `& $qualifiedSourceFacade doctor -Workspace $canonicalWorkspace -ProjectWorkspace $projectControlWorkspace -WakeRoot $observedWakeRoot -Human`. These flags select **read-only observations**, not serving workspace or recovery authority. No network request, browser, service, grant or repair is started. This source-facade capability is not claimed as installed-production qualification.

Rows distinguish no/open/multiple work requests, target mismatch, missing delivery, SUBMITTING_UNKNOWN, ATTENTION_UNRESOLVED, source/receipt tuple mismatch and PRESENT_UNVERIFIED grant presence. The report never reads grant contents and always denies execution/retry authority. A matching row is only a candidate for existing native assignment/owner preflight; it does not authorize a call. At most 32 projects and 256 records per ledger are observed, with explicit coverage limits and a 1 MiB file bound. A malformed or unknown sibling does not suppress another project's preflight diagnostics. [206 assertions and actual Windows observations](docs/verification/PROJECT_WORK_READINESS_2026_11_15.json).

`operator web rollover status --project-id <registered-project>` is the supported no-send record reader. The original CatDesk record is still QUARANTINED by design, with a validated completed ABANDONED_UNKNOWN_NO_REPLAY disposition. It does not require another recovery or submission. Keep UNKNOWN and the abandoned source-task fence intact; completion of that disposition is not proof of a new work lease or successful provider turn. [Recorded status](docs/verification/RETAINED_QUARANTINE_STATUS_2026_11_15.json).

## Other optional providers and common errors

`ollama --version` and `ollama list` are read-only local diagnostics when already installed; Qwen remains optional and no server/model download is started. Missing provider is not a direct-Web blocker.

| Error | Layer / safe next action |
| --- | --- |
| WEB_TASK_CACHED_ARGUMENTS_INVALID | Native task schema/invariant; reconcile original cached arguments/journal; no blind retry |
| WEB_WORK_ACTIVE_REQUEST / WEB_WORK_ROLLOVER_PENDING | Existing task/rollover fence; retain original operation, investigate exact evidence |
| WAKE_INSTALL_BROKER_UNPROVISIONED | Protected installation authority/functionality; direct backend work can continue independently |
| FIXED_DIRECT_WAKE_ROOT_IDENTITY_CHANGED | Physical root mismatch; do not rewrite IDs or sign expired proposal |
| CatDesk app view timed out | Client/resource/rendering evidence needed; does not itself prove tool failure; never replay mutating call to test UI |
| HTTP 429 / Session terminated | Connector/upstream/session; diagnose once after serving/auth ready and preserve exact error |

For completed integration, primary edition differences and preservation evidence, read the [final synchronization record](docs/verification/DOC_SYNC_STATE_2026_11_14.json). These are documentation/source checks, not fresh runtime health or installed product acceptance.
