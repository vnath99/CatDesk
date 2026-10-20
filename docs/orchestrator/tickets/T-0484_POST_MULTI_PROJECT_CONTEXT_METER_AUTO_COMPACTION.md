# T-0484 — Per-conversation context pressure, token accounting and safe automatic rollover
**Priority:** October 31 combined P3/P4 user milestone. The operator's October 17 revision supersedes the earlier deferral below. Independent ChatGPT Web project continuation and a demonstrated project rollover are required for overall acceptance.
**Date:** 2026-10-14; priority revised 2026-10-17; acceptance updated 2026-10-20 | **Status:** Controlled development rollover and continuation verified; installed and fresh multi-project live acceptance pending.
**Owner:** Codex Desktop, alongside continued development in each project's existing Codex conversation.
**Dependencies:** accepted project-bound sessions and fair Wake dispatch, versioned WakeHost/owned SeleniumBase, durable event/receipt identity, one authoritative target generation per project, source/trust boundary acceptance.  
**Read with:** `docs/orchestrator/CATDESK_CODEX_DESKTOP_HANDOFF_2026-10-14.md`; `docs/orchestrator/CATDESK_COMPETITOR_BROWSER_CONTINUITY_2026-10-14.md`; `CATDESK_MILESTONES.md`.

## Operator requirements

### October 20 current acceptance and contention correction

Current standalone capture uses the same witnessed bounded worker as MCP.
Three real non-canary reads verified existing BYOVD, Wake2 and PortfolioDesk
targets, independent visible estimates and normal reader cleanup. An earlier
60-second outer limit cancelled startup prematurely; the fixed total budget is
180 seconds, retaining the existing internal timeouts and cancellation. Timeout
cleanup was also observed with all 14 recorded reader descendants gone.
The v19 development package remains stopped; only two expected context ledgers
changed. No message, target change, repeated canary or recurring start occurred.
These reads do not establish fresh delivery or central connector parity.

The controlled fast canary completed replacement creation, paired binding,
history preservation and continuation with assistant response proof. Preserve
that completed operation. Earlier failure descriptions below are historical.
The central MCP context family exposes policy, observations and bounded capture.
Online mutations and snapshot ingestion now refuse a held ledger lock with
`WEB_CONTEXT_BUSY`, retaining exact target guards and idempotent event/revision
accounting. HTTP and fake-reader tests verify sibling status, capture-slot
release and unchanged state after refusal. Final root gate: 1,536 passed / 28
ignored, strict Clippy/build/format pass. No browser, WakeHost, recurring worker
or native conversation was started or changed to run these fixtures.

The stopped development candidate is v19; this is not protected installed
acceptance. Fixed broker preflight reports `WAKE_BROKER_SERVICE_UNPROVISIONED`.
Independent release approval, closed old-store/launcher evidence, protected
bootstrap/reader cutover, remote CI, connector serving parity (last 429) and
fresh independent project delivery remain separate gates. The four other native
project chats remain active. See `docs/OCTOBER_2026_MVP_ACCEPTANCE.md` for evidence.

### October 17 running-host implementation update

The new source driver connects periodic per-project context capture, handoff
request/collection and rollover progression to the production service and
non-isolated headless MCP host. Observation is explicitly enabled per project;
rollover remains separately configurable. A 120-second cadence and up to eight
independent project workers share short browser sessions. Stopped/paused Wake
does not start a browser action. The installed host must support the shared
browser-session protocol and prove current store ownership.

Replacement submission now saves `RESPONSE_PENDING` with the actual issued URL
and fresh-document exact USER receipt. Subsequent probes are read-only and
never submit the handoff again. `SUBMITTING` without a receipt remains an
unknown outcome. New source activity must be reconciled before changing a
target; pending evidence and events are preserved. `operator web auto tick`
exercises one driver step and central Web status includes durable per-project
progress. This supersedes the earlier source descriptions below that leave
periodic integration and cooperative bootstrap outstanding.

Current ChatGPT UI support recognizes semantic USER/ASSISTANT units and isolates
message text from Copy controls. Completed assistant content remains mandatory.
The controlled replacement diagnostic produced its requested ACK, but its
assistant body showed `This response couldn’t load` after reload. Exact live
USER and turn-anchor probes succeeded; the full snapshot/completion probes
could not run through the available read-only CUA DOM surface. Offline DOM
fixtures cover those probes. This is partial browser evidence, not installed
Wake delivery or completed user-facing rollover acceptance.

Installed dev84 remains stopped, with only a CatDesk Wake target that differs
from the central registry. BYOVD and Bug Bounty have registered Web URLs but no
independent Wake targets; Wake2 has no Web URL. Reviewed package installation,
authority alignment, three live projects and a durable real rollover remain
required. See the current acceptance ledger for exact checks and limitations.

### October 17 priority and execution correction

The primary user capability is autonomous work across independent ChatGPT Web
conversations. Native Codex thread coordination supports that capability. Keep
Bug Bounty, BYOVD, Wake2 and PortfolioDesk progressing toward their own MVPs;
do not hold their existing conversations idle for a queue demonstration.

Implement the smallest connected sequence using the existing registry, Wake
store, receipts and SeleniumBase owner: per-project paired target binding and
history; project event delivery; an advisory per-conversation visible-content
ledger with configurable pressure thresholds; durable handoff/rollover intent;
one controlled New Chat bootstrap with an observed issued URL and exact USER
receipt; atomic one-project binding update; continuation with predecessor links.
Exercise a controlled conversation early. Keep source implementation, local
tests, installed authority and actual browser receipts as separate evidence.

Overall P3/P4 remains incomplete until at least three registered project Web
conversations can make independent progress and one project has demonstrated
handoff, replacement creation, URL/Wake verification and continuation while
the old history and other project sessions remain usable. If authentication,
UI behavior or installed Wake authority blocks automatic execution, demonstrate
the supported partial workflow and record the precise remaining limitation.
The older ordering language below is historical and does not defer this work.

1. Use CatDesk's **existing Rust MCP gateway and SeleniumBase Python/WakeHost browser**; do not introduce a Chrome extension as a prerequisite.
2. Automatically monitor estimated **current ChatGPT conversation** context pressure (not lifetime MCP byte counts).
3. At a configurable, conservative threshold, arrange a safe summary/handoff in Chat A, create a new native ChatGPT conversation in the existing authenticated Selenium browser, verify its first USER receipt, then atomically rebind that **one project's** project/Wake/session/worker routing to Chat B. Never move another project's target or queue.
4. Display estimated tool/model-inbound traffic and full context estimates separately; mark inferred/unobservable areas clearly. The tool token meters are not provider-authoritative context or dollar charges.
5. Preserve current one-writer, no duplicate USER send, immutable event ID, reviewed install, old forensic records and original-chat fallback rules.
6. October 17 revision: implement context-aware continuation alongside multi-project scheduling. Both are required for October 31 user-facing acceptance.

## Existing CatDesk assets — verified at design time

- `src/mcp.rs::estimate_turn_token_usage` computes `inputTokens` using the `o200k_base` tokenizer over JSON-serialized tool name/arguments, `outputTokens` over sanitized JSON result (excluding internal `_meta`), and `totalTokens` as their sum.
- `src/state.rs::UsageTotals` and `src/server.rs::extract_turn_token_usage / attach_history_usage` maintain displayed tool/session/history totals; `src/widget/catdesk_dashboard.html::renderTurnToken` renders ↓, ↑, Σ, $, ƒ. These counts **are neither authoritative ChatGPT usage nor per-conversation context**. Tool ↓ represents model→tool commands; tool ↑ represents tool→model content. The GUI illustrative dollar rate must not be interpreted as actual billing.
- Today CatDesk's project registry includes `catdesk`, `BUG_BOUNTY_RECON_PLATFORM`, `BYOVD_DRIVER_PIPELINE`; registration alone does **not** prove a fair multi-project Wake scheduler.
- `wake/src/runtime.rs`, `wake/adapter.py`, `scripts/wake_bridge.py` and review records already implement guarded existing-chat target, timer, read-only SUBMITTING reconciliation, exact USER receipt and assistant completion. Current installed dev84 is behind source dev85. The operator has authorized a controlled rollover test; production installation and Wake authority still require their existing evidence.

### October 17 implemented source and controlled live evidence

Source now contains per-project current/predecessor URLs, paired registry/Wake
readback and compare-and-set binding, exact-generation event publication,
independent advisory context ledgers, a guarded new-chat adapter and installed
runtime bridge. Local `operator web` commands connect these to durable
PREPARED → SUBMITTING → CREATED → BOUND → CONTINUATION_QUEUED records. A SUBMITTING
record never authorizes resend; a CREATED record resumes binding and continuation
without opening another conversation. Current policy is rechecked before submit.

The local command surface is:

```text
catdesk operator web status
catdesk operator web target read --project-id PROJECT
catdesk operator web target bind --project-id PROJECT --conversation-url URL --expected-current-target-sha256 DIGEST_OR_none --expected-wake-generation N
catdesk operator web context configure --project-id PROJECT --input-file POLICY_JSON
catdesk operator web context observe --project-id PROJECT --input-file OBSERVATION_JSON
catdesk operator web context capture --project-id PROJECT
catdesk operator web context status --project-id PROJECT
catdesk operator web rollover prepare --project-id PROJECT --handoff-file HANDOFF_JSON
catdesk operator web rollover execute --project-id PROJECT
catdesk operator web rollover status --project-id PROJECT
```

Run these from the central control workspace. Policy uses
`configuredBudgetTokens`, `safetyHeadroomTokens`, `unknownSourceReserveTokens`,
and `automaticRolloverEnabled` (default false). Observations supply schemaVersion
1, an exact `key` (`projectId`, `conversationUrl`, `targetGeneration`) and `events`
with `eventId`, `revision`, `dedupKey`, `sourceKind`, `visibleText` and
`observedAtUnix`. The independently read paired target must match that key.
Raw observations are never returned in diagnostics or stored by the meter.
`capture` obtains a private bounded excerpt through the reviewed installed
adapter, checks the paired target again, and ingests complete explicitly keyed
messages with locked revision updates. Empty, clipped, unkeyed and response-load
error bodies cannot replace a previously counted complete answer. It sends no
USER message. Cross-document anchor stability remains unverified; a fixed
unknown reserve does not establish coverage of a preexisting long conversation.
Periodic host capture and initial-history accounting remain required work.

Handoff input requires schemaVersion 1, projectId, sourceTaskId, objective,
completed/remaining/ownedPaths/verification/blockers arrays and nextAction. It is
private local state, capped at 10 KiB. The generated bootstrap includes the
workspace, native owner, predecessor URL and instructions to await the bound
continuation event. Preparing a handoff is not permission to duplicate native
work. The initial MVP command consumes an explicit handoff; automatic extraction
from project state and browser observations is still outstanding.

The source defaults are a local 96k token budget with 24k safety headroom and an
8k reserve per unknown source. These are configurable operating assumptions,
not a provider context-window claim. The optional browser snapshot is only a
bounded recent excerpt and cannot stand for whole-chat measurement; automatic
observation must preserve unknown/truncation reserves and stable message IDs.

On October 17 an authenticated Chrome computer-use canary created a predecessor
and a distinct replacement conversation, carried the assistant-generated
handoff and source link forward, and resumed the synthetic coding task. The
returned code passed five local behavior checks. See
`docs/acceptance/web_rollover_canary_20261017.json` for the observed URLs and scope.
This canary did not run the installed CatDesk rollover path or rebind Wake.

Remaining live blockers are specific: installed dev84 lacks the new adapter;
the central/independent Wake targets are not coherent; and the one-shot bootstrap
requires Wake's free singleton host lease. Production integration must schedule
rollover alongside the running host so one response cannot block other projects.
Do not stop other projects to turn this partial result into a demonstration.

## Semantics: tool traffic versus model context

A tool invocation's serialized arguments are text emitted by the model; their appearance in future input is provider-managed and cannot be double-counted as newly ingested text each time. Tool result bytes **available to the model** generally contribute to its next input context. Historical total tool tokens count **traffic over time**; context occupancy is the particular window of content considered on a model request. Provider truncation, cache/reuse, summaries, tool schemas, system/developer instructions, hidden reasoning and native document/image handling are not completely visible.

**Never compute `contextTokens = history.inputTokens + history.outputTokens`.** Nor directly add an entire tool result estimate to a copy of the same result in a rendered assistant message. Instead build a deduplicated, source-attributed content ledger keyed by project + conversation generation + turn ID + message/tool-call ID with stable ownership.

### Baseline ledger, estimates and uncertainty

Proposed schema (versioned; exact naming can change in reviewed implementation):
```text
ContextEvent {
  eventId, projectId, conversationId, targetGeneration, turnId,
  sourceKind: USER_TEXT | ASSISTANT_TEXT | MCP_ARGS | MCP_RESULT | WORKER_MESSAGE
            | ATTACHMENT_VISIBLE_TEXT | NATIVE_ATTACHMENT_UNKNOWN | CONTEXT_CONTROL,
  upstreamToolCallId?, contentSha256?, sourceLength?, observedAt,
  estimatedTokens?, estimationMethod?, reliability: OBSERVED | DERIVED | UNKNOWN,
  providerVisibility: LIKELY_IN_CONTEXT | UNCERTAIN | NOT_CONTEXT,
  dedupKey, sourceRevision?, providerMetadata?
}
ConversationPressure {
  projectId, conversationId, generation, estimatorVersion,
  visibleUniqueTokens, knownUnknownSources, estimatedRange?, thresholdTokens,
  warningBand, effectiveModelHint?, measuredAt, advisoryOnly
}
```
- Prefer CatDesk's existing `o200k_base` tokenizer for observed text and JSON, but test language/code/Unicode and revise estimates if the selected model's actual encoding is known. Document that it is a proxy, not an exact GPT-6 tokenizer.
- Attribute each MCP tool response to the **originating conversation and project** at the server boundary, authenticated by existing opaque caller/session context, not a supplied arbitrary `projectId` field.
- Persist outcome/truncation/character count/estimated tokens even for a truncated visible result, but count only the provider-visible bounded payload under clear conventions. Do not conflate full local file bytes with the subset actually surfaced to the model.
- Track USER and ASSISTANT visible messages exactly once even when DOM streaming updates the same turn (replace revision/token delta, do not sum snapshots). A native attached PDF/image may imply unobserved content: mark unknown and reserve safety headroom rather than inventing extracted token quantity.
- Track model-output tool arguments separately from model-input results; calculate estimated current-context pressure from retained **unique content** and model-visible tools, not lifetime generation totals.
- Lifetime statistics stay separate from active-conversation statistics; on verified A→B migration, preserve original ledger as historical and initialize B with only its actual handoff/bootstrap plus B's future tokens (do not reset or erase historical traffic).
- No per-model fixed claim of 400k. Select conservative operator-configured thresholds per model/profile, with safety headroom and a **disabled-by-default** compaction control until source tests and live evidence establish useful triggering. An illustrative 70–80% budget is only a starting hypothesis. Provider/access refusals override the threshold.
- Provide three metrics: `Observed tool tokens`, `Estimated visible conversation pressure`, and `Uncertainty/unknown sources`; dashboard should warn when estimate becomes untrustworthy.

## Why CoS is useful but not a drop-in estimator

[CoS `src/shared/session.ts::eventTokens`](https://github.com/totec448-spec/chat-on-steroids/blob/main/src/shared/session.ts) approximates ~4 characters per token; it includes visible USER/ASSISTANT messages, MCP arguments, up to **10,000 estimated tokens per individual MCP result**, agent messages; it omits repeated transient progress snapshots and nested tool calls. The 10k cap may **undercount** large actual model-visible MCP results. [CoS `src/main/session/store.ts::autoCompactionReady`](https://github.com/totec448-spec/chat-on-steroids/blob/main/src/main/session/store.ts) uses a configurable local pressure line (fresh defaults 400k), not provider-authenticated remaining context. [CoS `src/main/bridge.ts::considerAutomaticCompaction`](https://github.com/totec448-spec/chat-on-steroids/blob/main/src/main/bridge.ts) also checks a current working turn/eligible failure and continuation fences. CatDesk can adopt these **semantics** while retaining SeleniumBase, and reuse its superior tokenization for observable tool text.

## Safe A→B automatic compaction using Selenium, phase gated

1. Detect pressure through per-conversation ledger and conservatively classify evidence. Do not auto-complicate an inactive/unknown-owner/project chat simply because aggregate MCP history is large. Require exact target and authority, and no ongoing competing review/session.
2. Open durable **rollover intent** with `projectId`, current chat A/generation, source-turn ID, pressure snapshot, idempotency key and deadline. Only one per project/chat; multiple projects can remain queued without starvation under already-approved fairness.
3. Choose a safe quiescent point: let bounded calls finish when possible; if preempting generation, use reviewed Stop/settle with exact source turn, no silently lost tool state. If no safe point, defer and signal attention.
4. Ask Chat A for a structured comprehensive handoff. Require exact handoff request receipt, completed final assistant response, minimum provenance/completeness/safety checks, and capture authoritative plan/task/review pointers from the local durable state. A handoff brief provides **model-facing context**, not primary durable task authority.
5. Using the existing authenticated, owned SeleniumBase instance, navigate to native New Chat (or target approved ChatGPT Project), prepare the id-less first message with the handoff and project anchor, authorize Send at most once with an irreversible write-ahead checkpoint. Never fabricate a /c UUID, never resend an ambiguous first USER.
6. Read ChatGPT's resulting `/c/<id>` and verify a unique exact latest USER bootstrap, assistant acceptance/response and correct account/project. Keep A canonical until this proof is complete.
7. Atomically rebind CatDesk project-chat registry, per-project independent Wake target/digest/generation and session/worker queue ownership using an existing reviewed guarded transaction; ensure B becomes the sole designated active frontend. On failure, A remains authoritative and B is quarantined/inspected without a second Send.
8. Resume unfinished project tasks and reviews in B; retain A's historical forensic evidence. Never convert a provider rate limit, CAPTCHA or policy restriction into a new-chat bypass.

## Tests / independent acceptance required

- Tokenizer: code/Unicode/JSON; huge tool result; partial response vs full result; streamed update snapshots; identical response in multiple recorded surfaces; nested invocation; same-call retries; tool arguments versus results; two projects concurrently; old generation late result; missing turn attribution; native image/document unknown; provider-side absent/changed model metadata; user quota/provider refusal.
- Threshold: just under/over, growing stream, active-turn/idle gating, stalled tool/timeout, no competing writer, different model/profile budgets, manual opt-out, persistent after restart, no duplicate compaction ticket, no cross-project bleed.
- Handoff: Chat A request/complete final verification, new tab still id-less, lost tab creation ACK, first Send ambiguity, source versus destination wrong chat, app crash mid-WAL, duplicate destination claim, source-only rollback, new URL and generation guard, token counter reset only for B, continuous worker/review state, read-only reconciliation and no second USER send.
- Independent **exact commit** Windows CI 3/3, source security review, reviewed Wake installation, real-browser current-generation receipt/SENT/COMPLETE acceptance; CoS examples are not validation of CatDesk behavior. Never alter signed main-image/T0366 or external Secure MCP owner during source implementation.

## Ordering / deliverables

**Prior milestone:** Binagotchy CLI + **multi-project scheduling, per-project Wake queues and fairness**, fully accepted.  
**This milestone:** T-0484 context estimator → per-conversation meter → safe automatic compaction ticket → Selenium New Chat and session/Wake rebind → tests and CI → one genuine live project-scoped rollover canary.  
**Success is not:** adding the sums from the tool-call widget to a fixed 400k number, printing a fake “remaining tokens” gauge, switching to an extension, or creating another chat to evade restrictions.

Codex Desktop should not begin T-0484 early unless the operator reprioritizes explicitly; keep the existing handoff/dev85/T0483/worker and multi-project priorities unchanged.
