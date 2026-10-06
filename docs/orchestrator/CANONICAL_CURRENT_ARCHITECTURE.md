# CatDesk Canonical Current Architecture and Security Model

This is the current architecture and security authority for CatDesk. It
summarizes accepted durable state; it does not authorize a host operation.
For current milestone status, active control state, and ticket eligibility,
read [`CATDESK_MILESTONES.md`](../../CATDESK_MILESTONES.md),
[`.catdesk/current_plan.md`](../../.catdesk/current_plan.md), and
[`.catdesk/todo.md`](../../.catdesk/todo.md) first. The companion
[architecture/document index](ARCHITECTURE_DOCUMENT_INDEX.md) identifies
superseded and historical material.

## Current end-to-end architecture

```text
Operator-designated ChatGPT conversation
  -> official OpenAI Secure MCP tunnel/client       [external/operator owned]
  -> CatDesk fixed loopback control transport       [CatDesk owned]
  -> canonical provider turn / exact Codex thread   [CatDesk controlled]
  -> durable contract, accounting, review, and continuity state
  -> reviewed output and canonical review-inbox event
  -> bounded one-shot wake delivery when separately eligible
```

The project registry's designated conversation is a protected continuity
binding. CatDesk CLI/app-server continuation reuses the exact canonical Codex
thread and durable continuity metadata; it does not create, select, scrape, or
retarget a ChatGPT conversation. Browser wake is not a continuation mechanism
and is never required to establish thread identity.

The current stable control transport is recorded as `CONNECTED_VERIFIED` in
the durable plan. A legacy `CatDesk_Local` HTTP 404 is not evidence to replace,
restart, or reconfigure the healthy stable tunnel.

## Ownership and network boundary

| Surface | Current owner | Boundary |
| --- | --- | --- |
| Official Secure MCP tunnel/client | Operator/external runtime | CatDesk must not create, duplicate, configure, stop, remove, or own it. Bounded redacted health observation is not ownership. |
| CatDesk loopback MCP transport | CatDesk | Fixed local listener and product-controlled direct children only. No route, credential, tunnel ID, or browser data belongs in source-controlled diagnostics. |
| Stable supervisor source chain | CatDesk, source accepted | Intended fixed surfaces are `127.0.0.1:3201`, one private Windows control pipe, and versioned worker registration at exact `127.0.0.1:3200`. Host-live activation is still a separately reviewed/operator boundary. |
| Ordinary versioned worker | CatDesk | The supervisor pipe admits the OS-derived same `TokenUser` and nonzero exact `TokenSessionId` before decoding registration, then applies executable/image, reviewed-manifest, generation-CAS, and old-backend-preservation checks. |
| Browser/wake adapter | Isolated CatDesk wake boundary | It has no tunnel or provider authority and is invoked only for an eligible canonical event. |

The supervisor must never become an owner of Secure MCP/tunnel lifecycle, and
the external tunnel must never be treated as an authority to alter CatDesk
supervisor state. No alternate listener, pipe, worker endpoint, service, or
Scheduler task is a valid substitute for the reviewed fixed control path.

## Provider, authentication, and contract boundary

Codex is preferred while eligible and remains tied to the current user's
operator authentication context. CatDesk validates availability without reading
authentication contents. A confirmed Codex allowance/credit exhaustion is a
route transition within the **same logical session and task**:

1. Persist the provider-attested reset boundary and exact Codex thread metadata.
2. Hand the same work to healthy contract-approved local Qwen.
3. Keep Qwen sticky and make no Codex probe before that boundary.
4. At a safe provider/task boundary at or after reset, restore only the exact
   preserved Codex thread.

If Codex is still exhausted, refresh the durable boundary and remain on (or
return to) Qwen. A transient Codex 429 remains bounded Codex backoff. An
unavailable Qwen fails closed to `WAITING_FOR_CHATGPT`; cloud, paid, browser,
and other remote fallback are forbidden.

Autonomous work is limited by the approved task contract: workspace scope,
allowed paths, provider policy, approved objective, and completion/review
ordering. Contract, thread, accounting, review-inbox, and output-attribution
state are durable CatDesk authority. A provider must not create a replacement
ticket or conversation merely because a quota boundary is reached.

## Workspace and protected-state confinement

Repository-visible state contains source and redacted review artifacts, not
credentials, tunnel IDs, routes, tokens, browser storage, cookies, or full
sensitive endpoints. Workspace-relative paths are normalized and contained.
Required outputs are regular files under the approved workspace and are
measured with bounded hashing; dangling links, reparses, special objects,
permission/metadata ambiguity, and baseline corruption fail closed.

For accepted supervisor install/state work, authority-bearing ProgramData
operations are rooted in pinned protected parents and use no-follow,
handle-relative opens and same-parent commits. Reparse/identity drift and
foreign or ambiguous state are rejection conditions. T-0281's protected
current/LKG plus fixed startup-task transaction is accepted source evidence;
it is not a license for ad-hoc live ProgramData, Task Scheduler, service, or
port mutation.

## MCP, shell, and Git authority

Normal MCP command handling is allowlist-shell mode, not a general authority
grant. The narrow typed lifecycle intercept accepts only exact public
`catdesk.ps1` forms, resolves the canonical workspace and script itself, and
forwards no user-selected command, path, flag, pipe, environment, or shell
syntax. Near-misses remain blocked. `shell_mode = "unrestricted"`, where a
user explicitly enables it, is ordinary local-shell authority and is outside
file-tool containment; it is not a safe substitute for an approved contract.

Git staging, commits, and publication require their own explicit authority and
gates. Current reliability, architecture, and review tickets do **not**
authorize publication. Do not use a review bundle, lifecycle helper, provider
turn, or wake event as implied permission to publish, change branches, or
access credentials.

## Review, wake, and recovery model

`.catdesk/autonomy/review-inbox.json` is the canonical review-event authority.
The newest unread actionable event for the exact session is the only candidate
for delivery; acknowledgement and claims enforce one-writer/idempotent
handling. T-0224 live natural-delivery acceptance is closed through T-0298 with
an exact current-target schema-4 `SENT` receipt and acknowledged review record.
The currently deployed owner path must preserve that accepted one-writer,
project-scoped contract. Any later Rust-owner cutover/restart experiment is a
separate implementation/acceptance concern and must not manufacture evidence by
manual browser wake or silently reopen the accepted T-0224 result.

`catdesk.ps1` remains the public lifecycle facade for its supported fixed,
redacted lifecycle states. Its status path is read-only. Public recovery may
restart/verify an exact valid canonical binary+fingerprint pair and preserve the
externally owned runtime, but runtime/process/transport health is not reviewed-
promotion authority. T-0322 restores the accepted provenance boundary:
`operational_verified` may only reuse an already-matching LKG and cannot mint or
advance rollback escrow. Broken-pair rollback still requires exact durable
reviewed authority; missing, mismatched, damaged, or ambiguous authority remains
fail closed for rollback. Recovery does not grant tunnel ownership, compile or
promote mutable workspace source, or bypass the parked T-0223 host-live
lifecycle boundary.

## Current acceptance map

- Accepted deterministic chains include T-0134 attribution continuity,
  T-0137 lifecycle parameter scope, T-0223 source work through T-0281,
  reset-aware Codex/Qwen routing logic through T-0284, and T-0285 deterministic
  Codex GUI/CLI exact-thread revalidation. T-0224 live natural delivery is also
  accepted through T-0298.
- Fresh runtime evidence can narrow a prior deterministic claim without erasing
  its historical test result. On 2026-09-07, a bounded `qwen3.8:27b` run failed
  on turn 3 with Ollama HTTP 500 `no user query found in messages`; T-0324 must
  reconcile deployed history serialization before Qwen implementation use.
- Still open or operator/live constrained: T-0223 host-live activation, T-0222
  visible/deployed GUI acceptance, T-0319 watchdog-only independent review,
  T-0324 Qwen runtime continuation reconciliation, T-0141 multi-project live
  isolation, and the final T-0152 sweep.
- T-0285 is `COMPLETED_VERIFIED` evidence and still requests independent
  T-0142 acceptance; it does not license any ChatGPT target mutation.

Use the milestone tracker and current plan for the authoritative status at the
time of work. Historical diagrams, plans, and review bundles explain how a
boundary was reached but cannot override this model or authorize a new action.
