# Execution Protocol V1

CatDesk Delegated Orchestrator v1 uses a small typed protocol between the ChatGPT supervisor, CatDesk, provider adapters, and worker models. The protocol is intentionally conservative: CatDesk owns contracts, approvals, the model/tool loop, local tool policy, durable run state, verification, and audit events. Provider adapters own transport to one model surface. Worker model output may request work, but it cannot rewrite the original execution contract or bypass CatDesk policy.

## Ownership

| Component | Owns | Must not own |
| --- | --- | --- |
| ChatGPT Web | objective, architecture, escalation decisions, final review | local file/shell execution |
| CatDesk | contract validation, tool construction, approvals, Git safety, model/tool loop, verification, event journal | provider-specific transport details |
| Provider adapter | model transport, streaming normalization, health, cancellation where supported | repository mutation authority, shell access, Git, approvals, verification |
| Worker model | proposals, tool-call requests, explanations | policy decisions or contract changes |
| Provider/router | model selection, retry/fallback routing | bypassing CatDesk tool policy |
| SQLite store | durable run metadata, event cursor, idempotency records | secrets or unredacted credentials |

## Domain Model

```text
DelegatedRun
  WorkerSession
    Turn
      AgentMessageItem
      ToolCallItem
      ApprovalRequestItem
      ToolResultItem
      DiffArtifact
      VerificationArtifact
      EscalationArtifact
```

Every object uses conservative string identifiers: `run_id`, `worker_session_id`, `turn_id`, `item_id`, `tool_call_id`, `approval_id`, and `artifact_id`. Events additionally carry `event_sequence`, `request_hash`, and optional `result_hash`.

## Contract

`ExecutionContractV1` has `schemaVersion: 1` and includes the task objective, workspace, feature branch, path policy, command profiles, ordered steps, acceptance criteria, budgets, provider policy, escalation rules, approvals, expected artifacts, and verification profile.

CatDesk rejects:

- unknown schema versions;
- empty objectives, steps, budgets, or verification profile;
- unsafe feature branch names;
- absolute, traversal, or `.git` contract paths;
- worker output that changes immutable intent fields such as objective, workspace, feature branch, path policy, or acceptance criteria.

## State Machine

Allowed run states are:

```text
DRAFT
AWAITING_APPROVAL
READY
STARTING
RUNNING
PAUSED
NEEDS_SUPERVISOR
VERIFYING
COMPLETED_VERIFIED
FAILED
CANCELLED
```

Terminal states cannot transition back to active states. Invalid transitions fail deterministically and are not journaled as successful state changes.

## Events And Cursor

Events are append-only and ordered by `event_sequence`, starting at 1. Polling uses `after_sequence` and `limit`; the same cursor returns the same ordered page until new events are appended. Out-of-order appends are rejected.

Lifecycle events are:

- `CREATED`
- `STARTED`
- `DELTA`
- `COMPLETED`
- `FAILED`
- `CANCELLED`
- `OUTCOME_UNKNOWN`

`OUTCOME_UNKNOWN` is reserved for cases where a mutating action may have happened but the result was not durably recorded. Such actions require supervisor handling and must not be replayed automatically.

## Approvals

`ApprovalRequestV1` binds a request to one run, one `approval_id`, one `request_hash`, and one expiration sequence. `SupervisorDecisionV1` is valid only when it matches that run, approval, hash, and expiration window, and the approval has not already been consumed.

## Runtime Handshake

`ProtocolHandshakeV1` declares:

- protocol version: `catdesk.delegated.v1`;
- contract schema version: `1`;
- worker runtime name;
- delta-event support;
- cursor support.

Unknown protocol or schema versions fail closed.

## Current Architecture Decision

T-0013B moves the required v1 worker loop into CatDesk. T-0012 and T-0013A remain preserved as OpenClaw research evidence, but no required v1 execution contract assumes OpenClaw, OpenClaw MCP warm-up, or OpenClaw event cursors.

CatDesk constructs the complete model-visible tool surface before every provider request. The authoritative event cursor, patch lineage, idempotency records, and verification state are CatDesk-owned.
