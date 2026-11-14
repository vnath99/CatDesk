# CatDesk important commands

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

The source-tested development operator action `operator web-dev-runtime reconcile-root-view --expected-authority-sha256 <sha256>` performs a bounded preflight against the workspace selected by the existing `WORKSPACE_ROOT` mechanism. Use only a qualified immutable development controller, not an arbitrary executable from PATH. Expected output is `ROOT_VIEW_PREFLIGHT_ONLY` with old-authority, observed-root-view and proposed-authority hashes. Retained-runtime qualification is still pending; this syntax is not a claim of installed availability.

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

`python scripts/catdesk_docs.py check` validates the five documents, references/status/identity and privacy. `python scripts/catdesk_docs.py record --event <reviewed-event.json>` validates and previews a task-bound update; add `--apply` only to write the checked activity/ticket/handoff/milestone patch. Exact repeat is a no-op; a reused event ID with changed bytes refuses. This records evidence, never grants review/release authority. Use at task completion even when CatDesk is unavailable. Commands/results must reference real retained evidence.

The existing source-only `.\scripts\catdesk-dev-lane.ps1 verify` now runs the documentation check before its unchanged Rust checks. Python 3.9+ is required for this development command; no daemon or external connector is involved. Automatic record mode updates the current handoff task only; acceptance/DONE, new live maturity, another task or conflicting history requires separate review. Interrupted partial document writes refuse reconciliation rather than inventing completion; normal Git diff/history is the recovery record.

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
