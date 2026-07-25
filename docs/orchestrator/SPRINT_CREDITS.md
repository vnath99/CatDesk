# Sprint Credit Ledger

Date: 2026-07-25

The Codex desktop interface in this session does not expose a per-ticket credit counter. To avoid inventing exact usage, this ledger records the visible value as unavailable and preserves the roadmap planning range for each completed ticket.

| Ticket | Status | Displayed credits used | Planning range | Notes |
| --- | --- | ---: | ---: | --- |
| T-0012 | COMPLETED_VERIFIED | unavailable | 55-85 | Completed before sprint branch checkpoint; local commit `d0cc25e`. |
| T-0013 | COMPLETED_VERIFIED | unavailable | 60-95 | Protocol schemas, events, state machine, docs, and fixtures. |
| T-0013A | FAIL_CLOSED | unavailable | 35-60 | Disposable Gateway validation proved auth/RPC/event control but failed the pre-model `tools.effective` CatDesk MCP tool-policy gate. |
| T-0013B | COMPLETED_VERIFIED | unavailable | 45-75 | CatDesk-owned provider-neutral worker-loop architecture frozen; local commit recorded in Git history. |
| T-0014 | COMPLETED_VERIFIED | unavailable | 80-125 | Durable JSON/JSONL journal and idempotent tool-call replay protections added; local commit recorded in Git history. |
| T-0015 | COMPLETED_VERIFIED | unavailable | 80-125 | Bounded context, compaction, redacted inspection, and provider handoff primitives added; local commit recorded in Git history. |
| T-0016 | COMPLETED_VERIFIED | unavailable | 120-190 | Worker runtime harness, fake provider, strict tool calls, CatDesk tool definitions, and local Ollama adapter added; local commit recorded in Git history. |
| T-0017 | COMPLETED_VERIFIED | unavailable | 90-145 | Patch preview/apply/compare/diff engine and disposable repair-cycle evidence added; local commit recorded in Git history. |
| T-0018 | COMPLETED_VERIFIED | unavailable | 90-145 | Run coordinator readiness, locking, budget, approval, escalation, cancellation, and final-review gates added; local commit recorded in Git history. |
| T-0019 | COMPLETED_VERIFIED | unavailable | 80-130 | Provider routing, fallback handoff, disclosure enforcement, and fake API/browser validation added; local commit recorded in Git history. |
| T-0020 | COMPLETED_VERIFIED | unavailable | 65-105 | Supervisor tool surface, event polling, bounded artifact/diff retrieval, patch inspection, and final review lookup added; local commit recorded in Git history. |
| T-0021 | COMPLETED_VERIFIED | unavailable | 90-140 | Long-running job manager, durable records, bounded log polling, rotation, restart recovery, cancellation, and command-policy integration added; local commit recorded in Git history. |
| T-0022 | COMPLETED_VERIFIED | unavailable | 135-210 | Fault-injection and security harness added for patch, journal, context, provider, supervisor final-review, long-job, Git staging, disclosure, and redaction scenarios; local commit recorded in Git history. |
| T-0023 | COMPLETED_VERIFIED | unavailable | 50-85 | Setup, operator tutorial, security limitations, and release-review documentation added; local commit recorded in Git history. |
| T-0023A | COMPLETED_VERIFIED | unavailable | 170-260 | Integrated delegated service, complete tool dispatcher, MCP supervisor transport wiring, setup scripts, journal durability review, fake restart recovery, and live Qwen closure evidence added; local commit recorded in Git history. |
| T-0023B | COMPLETED_VERIFIED | unavailable | 190-300 | Production delegated-run loop, provider history, durable integrated recovery, explicit MCP supervisor errors, supervisor-only startup mode, job containment, failure-state hardening, and live Qwen autonomous-loop evidence added; local commit recorded in Git history. |
| T-0023C | COMPLETED_VERIFIED | unavailable | 220-360 | MCP create/start now drives the production integrated worker, journal-backed registry rehydration was added, malformed run IDs fail safely, final review data comes from real worker state, and live Qwen MCP-to-worker evidence was captured. |
| T-0023D | COMPLETED_VERIFIED | unavailable | 260-420 | Functional-release closure added authenticated loopback MCP, bounded contract schemas, RunStart-only approval, lifecycle/cancel/restart hardening, bounded provider context, completion gates, SHA-256 patch safety, structured event IDs, startup-script auth, and live external MCP-to-Qwen evidence. |

If the UI later exposes exact ticket credit usage, update this ledger without changing ticket scope.
