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

# CatDesk operator actions — October 28–31

This is an execution/status record, not an authorization registry. Existing native
contracts, review grants, journals, platform permissions and release authority
remain authoritative. Routine source work, tests, reviews, isolated builds,
non-destructive diagnostics and reviewed publication have standing authorization
within existing ownership. CatDesk retains its explicit public-repository
exception; private project data and runtime artifacts must not be published.

Track each task separately using `READY_TO_EXECUTE`, `RUNNING`,
`WAITING_FOR_OPERATOR`, `WAITING_FOR_EXTERNAL_SERVICE`,
`BLOCKED_BY_TECHNICAL_DEFECT`, `BLOCKED_BY_POLICY` or `COMPLETED_VERIFIED`.
An unchanged dependency does not stall unrelated eligible work. Do not repeat
completed tests/builds or ambiguous submissions. Preflight and freeze any required
exact scope before requesting a new decision; batch compatible requests where the
existing application supports it. Standing permission never renews expired grants
or replaces mandatory platform/OS approvals.

## Current action register

| Project/task | Status | Operation and reason | Scope, effects and expiration | Success and safe recovery | Independent eligible work |
|---|---|---|---|---|---|
| CatDesk existing-profile sign-in | COMPLETED_VERIFIED (human action/cleanup only) | Operator reported signing in; no further sign-in request pending. Exact target readiness remains unverified. | Existing isolated profile only; credentials entered by operator. Browser closed; no message sent. | Login owner exited; subsequent read-only inspection preserved all protected state. Do not interpret `HOME` as proof of failed authentication. | Fix and qualify bounded exact-target inspection. |
| CatDesk C259 review | COMPLETED_VERIFIED (retirement only) | Supported exact retirement archived the unsubmitted event, closed transport and cancelled the native claim at version 5; no review result or release authority. | Original grant/lease expired October 28 at 17:53:03 UTC; no renewal or replay permitted. No operator approval is currently requested. | Historical request remains auditable; no receipt, submission-start witness or timer may be present before retirement. If evidence conflicts, preserve and stop that operation. | Current-source qualification and read-only browser diagnostics. |
| CatDesk exact-target readiness | BLOCKED_BY_TECHNICAL_DEFECT (cause unproven) | Operator saw BYOVD history. The new bounded native check reported TARGET_TAB_DRIFT then CHROME_ERROR; no readiness or message. Cleanup and all 8,484 pins passed. No repeated sign-in or blind browser retry is requested. | Existing registered project/profile; bounded owner; no typing, sending, draft clearing, binding change or recurring automation. | Exact target plus editor observed, or fixed failure label; all owned processes exit. Preserve unknown drafts and quarantine uncertain cleanup. | Documentation and source inspection. |
| CatDesk current-source Web review | WAITING_FOR_EXTERNAL_SERVICE / exact-target prerequisite | Fresh review of later source waits for verified target readiness; no new approval request is prepared for a known downstream refusal. | Frozen public-source template, exact verified registered destination, valid lease/grant, one bounded submission. Platform checks still apply. | Genuine task-attributable response, snapshot and acknowledgment before attested producer. Ambiguous outcome is never blindly resent. | Readiness correction and source tests. |
| CatDesk stopped failed-promotion recovery | COMPLETED_VERIFIED (installed headless qualification / first retirement); READY_TO_EXECUTE (new intent inspection) | Installed read-only startup and first byte-preserving retirement passed. A fresh promotion failed before swap; its new local intent is preserved. Script-status defect reproduced/fixed; exact five-record/intent/BOM recovery is source-qualified. | No executable, fingerprint, LKG, binding, browser or tunnel changes. Stable transaction and evidence digest required. | Refuse unknown owners/records; recover only the same journal; preserve all original bytes. | Existing browser source diagnosis. |
| CatDesk retained-reviewed-source producer / promotion | COMPLETED_VERIFIED (build/qualification); BLOCKED_BY_TECHNICAL_DEFECT (promotion) | Exact C254 retry is BUILD_ATTESTED and candidate live checks passed. Promotion preflight passed; confirmation failed before disk replacement. Read-only phase-one validation reports OPERATOR_ATTENTION_PHASE1; preserve historical receipt and failed claim. | Existing guarded preflight/promotion/lifecycle only. No fingerprint rewrite, Job bypass, replacement tunnel or arbitrary executable copy. | Exact installed identity/fingerprint, trusted previous version, authenticated health, cleanup and rollback evidence. Preserve trusted installation on failure. | Review/producer work above. |
| CatDesk connector and two-project Web acceptance | WAITING_FOR_EXTERNAL_SERVICE / backend prerequisite | Test connector after qualified backend restoration; previous HTTP 429 is a separate dependency. | No external tunnel/authentication ownership changes. No unchanged-service polling. | Actual independent project tool calls, correct workspace routing and ownership evidence. Stop only the blocked test if unavailable. | Qualified release and browser work. |

No discretionary operator decision is pending at this checkpoint. Ask only when a
new nondelegable prerequisite is established. Record exact UTC action times and
private evidence hashes in the established audit trail; publish sanitized results
and verified remote commit readbacks. Other projects retain their native sessions,
bindings, histories and assigned work throughout.
