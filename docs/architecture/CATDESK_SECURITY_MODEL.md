# CatDesk — Security and Authority Model

**Decision 2026-11-16T00:09:22.940282+00:00:** The explicit operator decision authorized exactly one DACL-only RX application to the existing runtime readers plus one bounded native version check. Both completed; no write/owner/SACL rights, accounts, credentials, release authority, original task replay or grant change. Budget readback is advisory and cannot refill or authorize work. [Evidence](../verification/NATIVE_RUNTIME_REPAIR_APPLIED_2026_11_15.json).

**Decision 2026-11-15T22:34:51Z:** Only new disposable ACLs were changed during testing. Changing the existing interpreter ACL remains a separate exact resource-owner decision. DACL-only access preserves owner/SACL and requests no elevation; it creates no signer, account, grant or replay authority. [Evidence](../verification/NATIVE_RUNTIME_READ_ACCESS_2026_11_15.json).

**Decision 2026-11-15T21:34:58Z:** Interpreter launch denial is reproduced by version-only inspection; neither this output nor daemon readiness grants replay, ACL/configuration changes, grant replacement or protected promotion. Disposable access tests must preserve original task and installation. [Evidence](../verification/NATIVE_SANDBOX_READINESS_AND_COMMAND_QUALIFICATION_2026_11_15.json).

**Decision 2026-11-15T21:01:12Z:** Reported sandbox error text is untrusted diagnostic evidence, never proof that a target did not run or authority for retry, permission changes or recovery. Original prelaunch allowlist and ownership fences are unchanged. [Evidence](../verification/NATIVE_COMMAND_FAILURE_DIAGNOSTIC_2026_11_15.json).

**Source validation 2026-11-15T20:32:08Z:** retained proposal operations are now checked against their original full diff hash within the existing non-writing recovery validation. This adds no tool call, replay, authority, install or service. Source `9d96f8e32f3fdf8b528d15eda453e9398f74a392` passes eight native recovery tests and strict checks; selected live test app remains source `79a449b`.

**Decision 2026-11-15T20:23:03Z:** Native Job/owner exit, exact task/registration/assignment, clean worktree and immutable journal snapshots still gate retirement. Only completed READ_ONLY read/search/preview calls with matching state/history/proposal/event evidence are newly eligible; original claims and completed call IDs are preserved. Any command, mutation, unsettled/unknown result, legacy owner or inconsistent receipt remains outside that path. Sandbox error text is a diagnostic, never authority to weaken permissions or infer a safe replay. See [dated proof](../verification/SETTLED_READ_ONLY_TASK_RECOVERY_2026_11_15.json).

**Release-model clarification 2026-11-15T19:33:36Z:** one system and one unchanged canonical v5 interface. Frozen independently verified releases are production; authorized source/worktrees/builds and isolated local fixture tests are development. Reuse current selection, immutable artifacts, existing fallback/promotion/rollback and bounded owners. No new installation, release manager, tunnel, plugin, external connection or backend infrastructure. Local source work proceeds independently of protected-release acceptance.

## 2026-11-15T19:18:45Z — Development/production separation decision

Standing authorization covers source inspection/editing, local tests/builds, isolated development fixtures and recovery, inert review drafts and reviewed feature publication under existing workspace ownership. Preserve independent protected release/signing, credential, destructive, external-submission, cross-project and uncertain-replay boundaries. The existing realm predicate now refuses production supervisor activation from valid or incomplete development state, both in publication and cached dispatch. No platform rule or original exact review-start decision was weakened. See [dated qualification](../verification/DEVELOPMENT_PRODUCTION_SEPARATION_2026_11_15.json) and [current handoff](../../CATDESK_PROJECT_HANDOFF.md) for exact maturity and remaining gates.

> **Revision timestamp (UTC):** `2026-11-14T18:47:56Z` · **Status:** Reviewed consolidation reference; defensive design reference, not a grant to execute local commands, select releases, change browser state or send messages.
> **Security objective:** Give ChatGPT Web useful *bounded* local capabilities while preserving project isolation, exact task attribution, idempotent side-effect handling and externally owned trust boundaries.

## 1. Threat and trust assumptions

ChatGPT Web, local and remote model outputs, repository text, uploaded artifacts, browser content, logs and review responses are **untrusted inputs**. CatDesk's backend, permissions, journals and ownership checks make the final execution decision. The local machine and current Windows user account are powerful environments; a nominal shell allowlist is *guardrails, not an operating-system sandbox*.

Threats include: prompt-injection from source files or websites; model requesting foreign-workspace operations; spoofed project or ChatGPT conversation identity; stale target generation; browser draft/editor drift; duplicated tool or USER sends after timeouts; credential leakage; cross-agent worktree conflict; privilege/signer misuse; malicious/incorrect Git publication; and desynchronized installed Wake/release state.

**No implicit security guarantee:** A shared bearer token or a passed MCP catalog proves transport/session authentication under its scope; it does **not** independently cryptographically prove which ChatGPT conversation originated a call. Exact target URL, digest, browser receipt, task/assignment and original journal must be correlated, and stronger origin attestation remains an explicit design gap where unavailable.

## 2. Boundaries and owners

| Boundary | Owner / policy | Security invariant |
|---|---|---|
| ChatGPT conversation | External provider and user | Content is untrusted; remote USER and assistant receipts are evidence only within their authenticated observation scope |
| Official Secure MCP client/tunnel | OpenAI/operator-managed external runtime | CatDesk must not create a duplicate, kill/reconfigure a foreign process, or treat route secrecy as authentication |
| Local CatDesk v5 server | Qualified installed image and authenticated user/service controls | Only declared tools with exact args; no arbitrary model-provided backend mutation |
| Registered project workspace | Project capability/approval boundary | Physical path identity, scope, exact task grant; refuse cross-project access, symlinks/reparse/hardlink alias violations where applicable |
| Tool executor / shell | CatDesk-owned guarded tools; optional OS-level containment | Explicit allowlist and task permission; unrestricted shell is **not** confined by file-tool checks |
| Task and side-effect journal | Durable local store | Original task/event/call ID and state are authoritative; UNKNOWN is a fence, not a replay permit |
| WakeHost / SeleniumBase | Independent local runtime/owned browser profile | One browser owner/lease; exact target and generation; never export authentication artifacts |
| Release/installer | Protected release workflow and legitimate signer/operator authority | Immutable hashes/manifest, physical roots, old-owner stop and reviewed transaction; source-only tests do not grant promotion |
| GitHub/source publication | Remote code host and branch approvals | Branch source is recoverability evidence, not production artifact trust; public repo must not contain secrets |
| Native Codex/Qwen/DeepSeek | Optional provider-specific sessions and credentials | No provider receives unrestricted tool authority or remote disclosure by default |

## 3. Least-privilege project and task execution

Every mutable operation should be bound to: project ID; approved actual workspace/file root; original source event and request; task/assignment SHA; active target generation/digest; allowed tool+arguments; execution key/call ID; owner/lease; and any required explicit action-specific grant. Validate before effect and at any critical post-lease boundary.

A generic CatDesk workspace tool may operate inside its own configured root, but such success must not count as **registered-project** isolation evidence. For multiple projects, verify exact project-scoped tools/registered root, not merely a path whose string looks correct. Reject foreign paths, unexpected symbolic/reparse redirects and privilege or identity drift. Do not parse a serialized task as shell text; the v5 `CATDESK_WEB_TASK=` cached call is a dedicated closed command envelope whose fields and original grant remain mandatory.

**Verification:** run only approved fixed command profiles or explicitly admitted commands, with bounded resources and captured real exit/result. The model must not be able to change the verification command into a file write or release promotion by prompt text. Use actual Git diff and test outcomes, not an assistant success claim.

## 4. Durable no-replay semantics

A tool/API timeout does not mean no effect occurred. Preserve the exact event, call ID and original immutable arguments; never fabricate a new equivalent task or grant to force success.

| Evidence state | Permitted treatment |
|---|---|
| No admitted durable operation | If independently proven, submit one eligible original operation |
| Pre-submit/claimed with exact known no-effect boundary | Resume only via the supported original state machine and current target identity |
| SUBMITTING, UNKNOWN, unverified browser/UI result | **No replay**; inspect original journal and target read-only, then obtain a supported resolution |
| Exact SENT USER receipt | Terminal send; continue observing response, do not send again |
| Verified assistant COMPLETE | May advance response observer; does **not** establish native task success or file change |
| File edit result unverified | Inspect actual files/diff/journal; do not repeat non-idempotent mutation |
| Protected release outcome unknown | Keep original transaction/marker; no new replacement pointer or LKG escrow |

**Atomicity statement:** CatDesk can atomically commit its own registry+Wake target records through a guarded local compare-and-swap transaction. It cannot atomically commit a remote ChatGPT send/new-chat creation with local storage. Use durable prepare/attempt/receipt/reconciliation phases, at-most-once external submission policy and honest uncertainty. Avoid calling remote delivery “atomic.”

## 5. Browser profile, URL, timer and rollover privacy

- SeleniumBase uses a dedicated, operator-authenticated local profile under its existing owner; no browser cookies, tokens, local storage, profile snapshots or ChatGPT drafts in GitHub, logs, handoff or MCP status.
- A ChatGPT URL may be exposed to the operator in a trusted local GUI, but persistent/remote reports should prefer project ID, generation and digest rather than arbitrary full private routes or message contents.
- Validate exact expected ChatGPT conversation destination, associated project, digest/generation and active composer before any submit. Login/CAPTCHA, draft present, editor missing, unknown click outcome and generation drift are all **ATTENTION/UNKNOWN**, not reasons to bypass checks.
- Manual Wake may be initiated only on an eligible task/turn boundary; the timer neither grants file-execution rights nor excuses unknown results. Do not use silent hourly deadmen as a substitute for the user-approved manual Wake model.
- Context ledger keeps safe hashes/counts/attribution, not raw private conversation text by default. Unknown hidden/model context and incomplete capture stay unknown; summaries for rollover must be task-bound and disclose only authorized content.
- The successor chat starts as **unverified** until its URL and actual initialization receipt are observed, then locally owned target state transitions under CAS. Old chat and task history remain retained.

### Existing successful send is immutable evidence

A historical CatDesk manual-wake event has a retained `EXACT_USER_MESSAGE_APPENDED` receipt. Its presence proves an earlier specific USER append under the original target witness, not a blanket right to submit again. Recovery, tests, new chats and provider handoff must never replace the original event identity or reuse its message; a present `SUBMITTING` state remains fenced even if an older successful event exists. The [architecture historical section](CATDESK_ARCHITECTURE.md#historical-acceptance-preserved--single-chat-wake-and-timer) records the dates and source boundary. This example is evidence of an invariant, not an authorization grant.

## 6. Protected release integrity and recovery refusal

The protected runtime is defined by an exact immutable executable hash, signed/reviewed or otherwise legitimate manifest/authority per the existing release protocol, selected pointer, source/build provenance and preserved predecessor. The release chain is not equivalent to the GitHub development branch.

Do not create signer keys on behalf of the operator, reuse purpose-incompatible signing grants, bypass elevated admission, accept a mutable source checkout as LKG, silently overwrite protected `current.json`, or install a debug binary into production. An installed-store root mismatch or absence of `current.json` cannot be fixed safely by copying arbitrary matching config/status bytes from another physical directory.

`recover` must require fresh exact owner/root/lock proofs, expected unchanged current/previous artifacts, eligible transaction phase, known stopped predecessor and supported bounded restart. It must **fail closed** when authority is absent, damaged, ambiguous or held by a different owner. Original snapshots/receipts, even failed ones, are evidence and must not be deleted to obtain a green diagnostic.

## 7. Model/provider and disclosure boundary

Native Codex can remain available but is optional for the primary ChatGPT Web workflow. The separate Codex CLI process may be a local sandbox or verification helper without sending a Codex model turn; report the distinction in telemetry. Ollama/Qwen is a local provider with its own capability and resource limits; a prior local model execution has failed at history serialization and is not assumed production-ready. Future DeepSeek/Gemini-style clients require explicit authenticated MCP support, least-privilege project grants and separate verification.

No automatic fallback from `LOCAL_ONLY` to remote/external models. If provider output would disclose repository contents or personal paths, require policy-authorized remote disclosure and log only safe bounded evidence. Account credentials and API keys are never embedded in source, command arguments or public documentation.

## 8. Git, audit and multi-agent boundary

- Protect `main` as stable reviewed source; use non-main feature/worktree changes, checks and reviewed merges. No force push, blind reset, unscoped `git add -A`, or unrelated-worktree staging.
- Record source SHA, branch, changed paths, tests, exit codes, review outcome and actual installed artifact separately. Publish only sanitized docs and source to the public repo.
- Preserve historical ticket reports and private original journals rather than migrating them into public Markdown. Activity log may describe redacted evidence references, not raw secrets or task contexts.
- A new ChatGPT/Claude/Qwen instance may **read** public recovery docs but does not inherit authority to replay incomplete native tasks or control protected production. Absence of local journals means effects are UNKNOWN, not NOT_RUN.
- The serial project lane is an authorization and scheduling boundary. A paused/failing project may release the lane only through its supported task/lease state machine, not by stealing another AI instance's lock.

## 9. Security acceptance checklist

1. **Authentication:** missing/wrong credentials rejected; correct MCP initialization/catalog can be verified without leaking secrets. The external connector is independently checked.
2. **Scope:** cross-project file access, wrong root, stale generation, foreign task ID, wrong execution key, mismatched assignment, changed URL and malformed envelope all refuse without side effects.
3. **No replay:** test timeout before send, during submit, after remote acceptance, after response, after local edit, during rollover and during protected pointer switch. Unknown outcomes remain fenced across restarts.
4. **Ownership:** no duplicate listener, WakeHost, browser owner or external tunnel takeover; process exit and handle release are measured rather than inferred from PID.
5. **Protected state:** missing/malformed LKG, mismatched binary, stale signed input, physical path alias, incomplete migration and unauthorized elevation must fail closed.
6. **Privacy:** redacted status/logs, no bearer/key/profile/cookie/text leakage and safe public GitHub publication.
7. **Correct completion:** actual effects and tests match claimed result; a completed assistant message never substitutes for task acceptance.
8. **Concurrency:** one active mutable project in MVP; other projects retain distinct queued events; pause/yield does not erase held unknown effects.

Tests must identify their verification maturity and exact artifact. This checklist is a design acceptance contract, not a claim of current test pass.

## 10. Dated security decisions and references

**Decision at `2026-11-14T18:47:56Z`:** consolidate project/workspace capability, transport ownership, no-replay, privacy and protected release trust into one security-model reference while retaining the source's existing guards. Changes to authority, authentication, external effects or release signing require an independently reviewed milestone/ticket and a new UTC decision entry here. Routine incident audit stays in [CATDESK_ACTIVITY_LOG.md](../../CATDESK_ACTIVITY_LOG.md).

Evidence/background: [original current architecture](../orchestrator/CANONICAL_CURRENT_ARCHITECTURE.md), [transport security](../tunnel/TRANSPORT_SECURITY_MODEL.md), [stable transport plan](../tunnel/STABLE_MCP_TRANSPORT_ARCHITECTURE.md), [wake independence](../orchestrator/WAKE_INDEPENDENCE.md), [release lifecycle](../orchestrator/CATDESK_PRODUCTION_RELEASE_LIFECYCLE.md), [T-0484 rollover](../orchestrator/tickets/T-0484_POST_MULTI_PROJECT_CONTEXT_METER_AUTO_COMPACTION.md), and [project MCP files](../../src/project_mcp.rs). Original task journals and protected state are local authority and are not published in documentation.
