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

If the UI later exposes exact ticket credit usage, update this ledger without changing ticket scope.
