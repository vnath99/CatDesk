# Minimal CatDesk Worker Runtime

Status: T-0013B architecture freeze
Date: 2026-07-25

## Purpose

This document defines the minimum provider-neutral runtime CatDesk needs before implementing persistence, context management, Ollama transport, patch execution, approvals, and supervisor polling.

The runtime is deliberately small. It is a repeated model/tool loop with durable events and strict policy checks, not a general agent platform.

## Conceptual Interfaces

The exact Rust modules may differ during implementation, but production code should preserve these boundaries.

```rust
trait WorkerRuntime {
    async fn start_run(&self, contract: ExecutionContractV1) -> Result<RunHandle, RuntimeError>;
    async fn resume_run(&self, run_id: RunId) -> Result<RunHandle, RuntimeError>;
    async fn cancel_run(&self, run_id: RunId, reason: CancellationReason) -> Result<(), RuntimeError>;
    async fn poll_events(&self, run_id: RunId, cursor: EventCursor) -> Result<Vec<EventEnvelopeV1>, RuntimeError>;
}

trait WorkerProvider {
    fn provider_id(&self) -> &str;
    fn capabilities(&self) -> ProviderCapabilitiesV1;
    async fn health(&self) -> Result<ProviderHealthV1, ProviderError>;
    async fn create_session(&self, request: CreateProviderSessionRequestV1) -> Result<ProviderSessionV1, ProviderError>;
    async fn send_turn(
        &self,
        session: &ProviderSessionV1,
        request: ProviderTurnRequestV1,
        sink: &mut dyn ProviderEventSink,
    ) -> Result<ProviderTurnResultV1, ProviderError>;
    async fn cancel(&self, session: &ProviderSessionV1) -> Result<CancelResultV1, ProviderError>;
    async fn checkpoint(&self, session: &ProviderSessionV1) -> Result<ProviderCheckpointV1, ProviderError>;
}

trait ProviderEventSink {
    fn emit(&mut self, event: NormalizedProviderEventV1) -> Result<(), RuntimeError>;
}
```

Provider adapters do not call CatDesk tools directly. They return normalized events. The worker runtime validates those events and routes approved tool calls through the CatDesk policy and tool dispatcher.

## Core Data Types

### ProviderCapabilitiesV1

- `provider_id`
- `provider_type`: `LOCAL_API`, `REMOTE_API`, or `BROWSER`
- `supports_streaming`
- `supports_native_tool_calls`
- `supports_session_continuity`
- `supports_cancellation`
- `supports_provider_checkpoint`
- `supports_usage_accounting`
- `supports_attachments`
- `context_limit`
- `output_limit`
- `privacy_boundary`: `LOCAL_ONLY` or `REMOTE_DISCLOSURE`
- `credential_requirement`: `NONE`, `LOCAL_SERVICE`, `API_KEY`, or `BROWSER_LOGIN`

### ProviderSessionV1

- `provider_id`
- `provider_session_id`
- `created_at`
- `model_id`
- `capabilities_hash`
- `provider_checkpoint`
- `disclosure_classification`

CatDesk's `worker_session_id` remains authoritative. Provider session IDs are adapter metadata.

### ProviderTurnRequestV1

- `run_id`
- `worker_session_id`
- `turn_id`
- `model_id`
- `system_contract_summary`
- `bounded_context`
- `tool_definitions`
- `recent_tool_results`
- `checkpoint_summary`
- `remaining_budgets`
- `strict_tool_call_mode`
- `disclosure_notice`

### NormalizedProviderEventV1

- `TEXT_DELTA`
- `REASONING_SUMMARY`
- `TOOL_CALL`
- `FINAL_TEXT`
- `COMPLETION_CLAIM`
- `RATE_LIMIT`
- `LOGIN_REQUIRED`
- `USER_TAKEOVER_REQUIRED`
- `CONTEXT_LIMIT`
- `TIMEOUT`
- `PROVIDER_UNAVAILABLE`
- `MALFORMED_RESPONSE`
- `CANCEL_ACK`
- `TERMINAL_ERROR`

Every normalized event includes:

- `provider_id`
- `provider_event_id`
- `run_id`
- `turn_id`
- `provider_sequence`, when available
- `received_at`
- bounded payload or artifact reference

### NormalizedToolCallV1

- `tool_call_id`
- `tool_name`
- `arguments`
- `arguments_hash`
- `turn_id`
- `provider_event_id`
- `requires_approval`
- `risk_class`
- `policy_decision`

Duplicate `tool_call_id` values are rejected. Unknown tools, invalid arguments, out-of-scope paths, and shell smuggling attempts fail closed before execution.

### WorkerCheckpointV1

- `checkpoint_id`
- `run_id`
- `last_event_sequence`
- `state`
- `current_step`
- `remaining_budgets`
- `context_summary`
- `recent_artifact_ids`
- `unresolved_failures`
- `outcome_unknown_items`
- `provider_handoff`

### ProviderHandoffV1

- `from_provider_id`
- `to_provider_id`
- `reason`
- `safe_context_summary`
- `last_verified_state`
- `pending_tool_call_ids`
- `latest_patch_artifact_ids`
- `disclosure_policy`

Provider handoff may continue reasoning, but it must not replay completed mutations.

## Model-Turn Lifecycle

```text
READY
-> TURN_PREPARING
-> CONTEXT_BUILT
-> PROVIDER_REQUESTED
-> PROVIDER_STREAMING
-> TOOL_CALL_PROPOSED or COMPLETION_CLAIMED or PROVIDER_FAILED
-> TOOL_VALIDATING
-> TOOL_EXECUTING
-> TOOL_RESULT_RECORDED
-> TURN_COMPLETED
-> next turn, VERIFYING, NEEDS_SUPERVISOR, CANCELLED, or FAILED
```

CatDesk may mark a run `COMPLETED_VERIFIED` only after tests and acceptance checks pass and no `OUTCOME_UNKNOWN` item remains.

## Failure States

| State | Meaning | Required behavior |
| --- | --- | --- |
| `MALFORMED_PROVIDER_OUTPUT` | Provider returned text that cannot be parsed under the active mode | Record event, ask for repair within retry budget, or fail |
| `TOOL_POLICY_REJECTED` | Tool call violates CatDesk policy | Record rejection and continue only if safe |
| `APPROVAL_REQUIRED` | Tool call needs supervisor approval | Pause in `NEEDS_SUPERVISOR` |
| `OUTCOME_UNKNOWN` | Mutation may have occurred but result was not durably recorded | Do not replay automatically; require supervisor handling |
| `PROVIDER_UNAVAILABLE` | Adapter cannot complete turn | Retry, switch provider, or fail according to policy |
| `CONTEXT_LIMIT_REACHED` | Context bundle cannot fit | Compact or escalate |
| `BUDGET_EXHAUSTED` | Turn, time, tool, or token budget is spent | Stop or escalate |
| `CANCELLED` | User or coordinator cancelled run | Persist cancellation and stop execution |

## Event Mapping From T-0013

| Runtime event | T-0013 event representation |
| --- | --- |
| Run created/started/completed/failed/cancelled | `EventPayloadV1::RunStateChanged` and lifecycle events |
| Text streaming | `EventPayloadV1::Delta` |
| Provider message | `TurnItemV1::AgentMessage` |
| Tool request | `TurnItemV1::ToolCall` |
| Approval request | `TurnItemV1::ApprovalRequest` |
| Tool result | `TurnItemV1::ToolResult` |
| Patch or actual diff | `TurnItemV1::DiffArtifact` |
| Test/verification result | `TurnItemV1::VerificationArtifact` |
| Escalation | `TurnItemV1::EscalationArtifact` |
| Ambiguous mutation | `EventPayloadV1::OutcomeUnknown` |

T-0014 will make these events durable. T-0015 will make context bundles and checkpoints compact. T-0016 will attach the first Ollama provider adapter.

## Network And Context Efficiency

Each provider turn must:

- send only the bounded context needed for the next decision;
- include hashes for file ranges and artifacts;
- summarize or reference large command outputs instead of retransmitting them;
- cap file ranges, command output, provider input, provider output, turn count, and elapsed time;
- avoid sending unchanged raw data repeatedly;
- record byte estimates and token estimates when available;
- stream provider deltas when supported;
- preserve constraints, acceptance criteria, budgets, and unresolved risks through compaction.

## Local Versus Remote Disclosure

`LOCAL_ONLY` providers receive context over loopback or local IPC. `REMOTE_DISCLOSURE` providers receive selected context through an API or browser-controlled website.

Remote or browser disclosure requires explicit execution-contract permission. The default posture is local-only. Provider adapters must never receive secrets, environment-variable values, private keys, tokens, repository credentials, or unbounded repository dumps.

## Deferred OpenClaw Adapter

OpenClaw can be revisited only as an adapter behind this contract. It must prove, before any model turn, that CatDesk can construct or verify the exact worker-visible tool surface and can observe structured session events well enough to preserve the CatDesk journal invariants.
