use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::EXECUTION_CONTRACT_SCHEMA_VERSION;
use super::context::{ContextBudgetPolicyV1, ContextBuilderV1};
use super::contracts::{
    ExecutionContractV1, RunId, RunState, ToolCallId, TurnId, WorkerSessionId, validate_contract,
};
use super::events::{
    AgentMessageItem, EventEnvelopeV1, EventPayloadV1, LifecycleEvent, ToolCallItem,
    ToolResultItem, TurnItemV1,
};
use super::journal::{
    DelegatedJournal, JournalError, ToolCallRecordV1, ToolCallStatus, ToolMutationKind,
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProviderType {
    LocalApi,
    RemoteApi,
    Browser,
    Fake,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCapabilitiesV1 {
    pub provider_id: String,
    pub provider_type: ProviderType,
    pub supports_streaming: bool,
    pub supports_native_tool_calls: bool,
    pub supports_session_continuity: bool,
    pub supports_cancellation: bool,
    pub context_limit: usize,
    pub output_limit: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProviderHealthStatus {
    Available,
    Unavailable,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderHealthV1 {
    pub provider_id: String,
    pub status: ProviderHealthStatus,
    pub available_models: Vec<String>,
    pub detail: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSessionV1 {
    pub provider_id: String,
    pub provider_session_id: String,
    pub model_id: String,
    pub keep_alive: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderTurnRequestV1 {
    pub run_id: RunId,
    pub worker_session_id: WorkerSessionId,
    pub turn_id: TurnId,
    pub model_id: String,
    pub context_json: Value,
    pub tool_definitions: Vec<ToolDefinitionV1>,
    pub max_output_bytes: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderTurnResultV1 {
    pub terminal: bool,
    pub output_bytes: usize,
    pub events: Vec<NormalizedProviderEventV1>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum NormalizedProviderEventKind {
    TextDelta,
    ToolCall,
    CompletionClaim,
    MalformedResponse,
    CancelAck,
    TerminalError,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NormalizedProviderEventV1 {
    pub provider_id: String,
    pub turn_id: TurnId,
    pub kind: NormalizedProviderEventKind,
    pub text: Option<String>,
    pub tool_call: Option<NormalizedToolCallV1>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NormalizedToolCallV1 {
    pub tool_call_id: ToolCallId,
    pub tool_name: String,
    pub arguments: Value,
    pub arguments_hash: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolDefinitionV1 {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
    pub mutation_kind: ToolMutationKind,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerRuntimeBudgetV1 {
    pub max_turns: u32,
    pub max_tool_calls: u32,
    pub max_elapsed: Duration,
    pub max_input_bytes: usize,
    pub max_output_bytes: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum WorkerRunStatus {
    Running,
    Completed,
    Cancelled,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerSessionSnapshotV1 {
    pub schema_version: u32,
    pub run_id: RunId,
    pub worker_session_id: WorkerSessionId,
    pub provider_session: ProviderSessionV1,
    pub status: WorkerRunStatus,
    pub completed_turns: u32,
    pub completed_tool_calls: u32,
    pub last_checkpoint: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum RuntimeError {
    Journal(String),
    Validation(String),
    BudgetExceeded(String),
    Cancelled,
    MalformedOutput(String),
    Provider(String),
}

pub trait CatDeskToolDispatcher {
    fn execute(&mut self, tool_call: &NormalizedToolCallV1) -> Result<String, RuntimeError>;
}

#[derive(Clone, Debug)]
pub enum FakeProviderTurn {
    Text(String),
    ToolCall(String),
    Complete(String),
    Malformed(String),
}

#[derive(Clone, Debug)]
pub struct FakeProvider {
    turns: Vec<FakeProviderTurn>,
    index: usize,
}

impl FakeProvider {
    pub fn new(turns: Vec<FakeProviderTurn>) -> Self {
        Self { turns, index: 0 }
    }

    pub fn capabilities() -> ProviderCapabilitiesV1 {
        ProviderCapabilitiesV1 {
            provider_id: "fake".into(),
            provider_type: ProviderType::Fake,
            supports_streaming: true,
            supports_native_tool_calls: true,
            supports_session_continuity: true,
            supports_cancellation: true,
            context_limit: 64 * 1024,
            output_limit: 16 * 1024,
        }
    }

    fn create_session(&self, model_id: &str) -> ProviderSessionV1 {
        ProviderSessionV1 {
            provider_id: "fake".into(),
            provider_session_id: "fake-session".into(),
            model_id: model_id.into(),
            keep_alive: None,
        }
    }

    fn send_turn(
        &mut self,
        request: &ProviderTurnRequestV1,
        allowed_tools: &BTreeSet<String>,
    ) -> Result<ProviderTurnResultV1, RuntimeError> {
        let Some(turn) = self.turns.get(self.index).cloned() else {
            return Ok(ProviderTurnResultV1 {
                terminal: true,
                output_bytes: 0,
                events: vec![NormalizedProviderEventV1 {
                    provider_id: "fake".into(),
                    turn_id: request.turn_id.clone(),
                    kind: NormalizedProviderEventKind::CompletionClaim,
                    text: Some("fake provider completed".into()),
                    tool_call: None,
                }],
            });
        };
        self.index += 1;
        match turn {
            FakeProviderTurn::Text(text) => {
                Ok(provider_text_result("fake", &request.turn_id, text, false))
            }
            FakeProviderTurn::Complete(text) => {
                Ok(provider_text_result("fake", &request.turn_id, text, true))
            }
            FakeProviderTurn::ToolCall(text) => {
                let tool_call = parse_strict_tool_call(&text, allowed_tools)?;
                Ok(ProviderTurnResultV1 {
                    terminal: false,
                    output_bytes: text.len(),
                    events: vec![NormalizedProviderEventV1 {
                        provider_id: "fake".into(),
                        turn_id: request.turn_id.clone(),
                        kind: NormalizedProviderEventKind::ToolCall,
                        text: None,
                        tool_call: Some(tool_call),
                    }],
                })
            }
            FakeProviderTurn::Malformed(text) => Err(RuntimeError::MalformedOutput(text)),
        }
    }
}

pub struct WorkerRuntimeHarness<'a> {
    journal: &'a DelegatedJournal,
    provider: FakeProvider,
    cancelled: bool,
}

impl<'a> WorkerRuntimeHarness<'a> {
    pub fn new(journal: &'a DelegatedJournal, provider: FakeProvider) -> Self {
        Self {
            journal,
            provider,
            cancelled: false,
        }
    }

    pub fn cancel(&mut self) {
        self.cancelled = true;
    }

    pub fn run(
        &mut self,
        contract: &ExecutionContractV1,
        dispatcher: &mut dyn CatDeskToolDispatcher,
        budget: WorkerRuntimeBudgetV1,
    ) -> Result<WorkerSessionSnapshotV1, RuntimeError> {
        validate_contract(contract).map_err(RuntimeError::Validation)?;
        let run_snapshot = self
            .journal
            .create_run(contract)
            .map_err(|error| RuntimeError::Journal(format!("{error:?}")))?;
        self.journal
            .update_run_state(&run_snapshot.run_id, RunState::Running)
            .map_err(|error| RuntimeError::Journal(format!("{error:?}")))?;
        let session = self
            .provider
            .create_session(&contract.provider_policy.primary_model_id);
        let worker_session_id =
            WorkerSessionId::new("worker-session-1").map_err(RuntimeError::Validation)?;
        let allowed_tools = catdesk_tool_definitions()
            .iter()
            .map(|tool| tool.name.clone())
            .collect::<BTreeSet<_>>();
        let started = Instant::now();
        let mut next_sequence = 1;
        let mut snapshot = WorkerSessionSnapshotV1 {
            schema_version: EXECUTION_CONTRACT_SCHEMA_VERSION,
            run_id: run_snapshot.run_id.clone(),
            worker_session_id: worker_session_id.clone(),
            provider_session: session,
            status: WorkerRunStatus::Running,
            completed_turns: 0,
            completed_tool_calls: 0,
            last_checkpoint: "run started".into(),
        };

        while snapshot.completed_turns < budget.max_turns {
            if self.cancelled {
                snapshot.status = WorkerRunStatus::Cancelled;
                self.journal
                    .update_run_state(&snapshot.run_id, RunState::Cancelled)
                    .map_err(|error| RuntimeError::Journal(format!("{error:?}")))?;
                return Ok(snapshot);
            }
            if started.elapsed() > budget.max_elapsed {
                return Err(RuntimeError::BudgetExceeded(
                    "elapsed time budget exceeded".into(),
                ));
            }

            let turn_id = TurnId::new(format!("turn-{}", snapshot.completed_turns + 1))
                .map_err(RuntimeError::Validation)?;
            let mut context_builder = ContextBuilderV1::new(
                snapshot.run_id.clone(),
                turn_id.clone(),
                ContextBudgetPolicyV1::local_default(),
            );
            context_builder.add_contract_summary(contract)?;
            let context = context_builder.build()?;
            if context.total_bytes > budget.max_input_bytes {
                return Err(RuntimeError::BudgetExceeded("input budget exceeded".into()));
            }
            let request = ProviderTurnRequestV1 {
                run_id: snapshot.run_id.clone(),
                worker_session_id: worker_session_id.clone(),
                turn_id: turn_id.clone(),
                model_id: contract.provider_policy.primary_model_id.clone(),
                context_json: serde_json::to_value(&context)
                    .map_err(|error| RuntimeError::Validation(error.to_string()))?,
                tool_definitions: catdesk_tool_definitions(),
                max_output_bytes: budget.max_output_bytes,
            };
            let result = self.provider.send_turn(&request, &allowed_tools)?;
            if result.output_bytes > budget.max_output_bytes {
                return Err(RuntimeError::BudgetExceeded(
                    "output budget exceeded".into(),
                ));
            }
            for event in result.events {
                let run_id = snapshot.run_id.clone();
                next_sequence = self.record_provider_event(
                    next_sequence,
                    &run_id,
                    &worker_session_id,
                    event,
                    dispatcher,
                    &mut snapshot,
                )?;
            }
            snapshot.completed_turns += 1;
            snapshot.last_checkpoint = format!("completed {}", turn_id.as_str());
            if result.terminal {
                snapshot.status = WorkerRunStatus::Completed;
                self.journal
                    .update_run_state(&snapshot.run_id, RunState::Verifying)
                    .map_err(|error| RuntimeError::Journal(format!("{error:?}")))?;
                return Ok(snapshot);
            }
        }
        Err(RuntimeError::BudgetExceeded("turn budget exceeded".into()))
    }

    fn record_provider_event(
        &self,
        sequence: u64,
        run_id: &RunId,
        worker_session_id: &WorkerSessionId,
        event: NormalizedProviderEventV1,
        dispatcher: &mut dyn CatDeskToolDispatcher,
        snapshot: &mut WorkerSessionSnapshotV1,
    ) -> Result<u64, RuntimeError> {
        match event.kind {
            NormalizedProviderEventKind::TextDelta
            | NormalizedProviderEventKind::CompletionClaim => {
                self.append_event(
                    sequence,
                    run_id,
                    worker_session_id,
                    Some(event.turn_id),
                    LifecycleEvent::Delta,
                    EventPayloadV1::Item {
                        item: Box::new(TurnItemV1::AgentMessage(AgentMessageItem {
                            item_id: super::contracts::ItemId::new(format!("item-{sequence}"))
                                .map_err(RuntimeError::Validation)?,
                            text: event.text.unwrap_or_default(),
                        })),
                    },
                )?;
                Ok(sequence + 1)
            }
            NormalizedProviderEventKind::ToolCall => {
                let tool_call = event.tool_call.ok_or_else(|| {
                    RuntimeError::Validation("missing normalized tool call".into())
                })?;
                let request_hash = stable_value_hash(&tool_call.arguments);
                self.journal
                    .record_tool_call_requested(ToolCallRecordV1 {
                        schema_version: EXECUTION_CONTRACT_SCHEMA_VERSION,
                        run_id: run_id.clone(),
                        tool_call_id: tool_call.tool_call_id.clone(),
                        tool_name: tool_call.tool_name.clone(),
                        request_hash: request_hash.clone(),
                        arguments_hash: tool_call.arguments_hash.clone(),
                        mutation_kind: tool_mutation_kind(&tool_call.tool_name),
                        status: ToolCallStatus::Requested,
                        result_hash: None,
                        outcome_summary: None,
                    })
                    .map_err(map_journal_error)?;
                self.journal
                    .transition_tool_call(
                        run_id,
                        &tool_call.tool_call_id,
                        ToolCallStatus::PolicyAllowed,
                        None,
                        None,
                    )
                    .map_err(map_journal_error)?;
                self.journal
                    .transition_tool_call(
                        run_id,
                        &tool_call.tool_call_id,
                        ToolCallStatus::Executing,
                        None,
                        None,
                    )
                    .map_err(map_journal_error)?;
                let tool_result = dispatcher.execute(&tool_call)?;
                let result_hash = stable_text_hash(&tool_result);
                self.journal
                    .transition_tool_call(
                        run_id,
                        &tool_call.tool_call_id,
                        ToolCallStatus::Completed,
                        Some(result_hash.clone()),
                        Some(tool_result.clone()),
                    )
                    .map_err(map_journal_error)?;
                snapshot.completed_tool_calls += 1;
                self.append_event(
                    sequence,
                    run_id,
                    worker_session_id,
                    Some(event.turn_id.clone()),
                    LifecycleEvent::Delta,
                    EventPayloadV1::Item {
                        item: Box::new(TurnItemV1::ToolCall(ToolCallItem {
                            item_id: super::contracts::ItemId::new(format!("item-{sequence}"))
                                .map_err(RuntimeError::Validation)?,
                            tool_call_id: tool_call.tool_call_id.clone(),
                            tool_name: tool_call.tool_name.clone(),
                            request_hash,
                        })),
                    },
                )?;
                self.append_event(
                    sequence + 1,
                    run_id,
                    worker_session_id,
                    Some(event.turn_id),
                    LifecycleEvent::Delta,
                    EventPayloadV1::Item {
                        item: Box::new(TurnItemV1::ToolResult(ToolResultItem {
                            item_id: super::contracts::ItemId::new(format!(
                                "item-{}",
                                sequence + 1
                            ))
                            .map_err(RuntimeError::Validation)?,
                            tool_call_id: tool_call.tool_call_id,
                            result_hash,
                            summary: tool_result,
                        })),
                    },
                )?;
                Ok(sequence + 2)
            }
            NormalizedProviderEventKind::MalformedResponse => Err(RuntimeError::MalformedOutput(
                event.text.unwrap_or_default(),
            )),
            NormalizedProviderEventKind::CancelAck => Err(RuntimeError::Cancelled),
            NormalizedProviderEventKind::TerminalError => {
                Err(RuntimeError::Provider(event.text.unwrap_or_default()))
            }
        }
    }

    fn append_event(
        &self,
        sequence: u64,
        run_id: &RunId,
        worker_session_id: &WorkerSessionId,
        turn_id: Option<TurnId>,
        lifecycle_event: LifecycleEvent,
        payload: EventPayloadV1,
    ) -> Result<(), RuntimeError> {
        self.journal
            .append_event(
                &EventEnvelopeV1 {
                    schema_version: EXECUTION_CONTRACT_SCHEMA_VERSION,
                    event_sequence: sequence,
                    run_id: run_id.clone(),
                    worker_session_id: Some(worker_session_id.clone()),
                    turn_id,
                    item_id: None,
                    lifecycle_event,
                    request_hash: "fnv1a64:runtime".into(),
                    result_hash: None,
                    payload,
                }
                .with_result_hash()
                .map_err(RuntimeError::Validation)?,
            )
            .map_err(map_journal_error)
    }
}

pub fn restore_checkpointed_sessions(
    journal: &DelegatedJournal,
) -> Result<Vec<RunId>, RuntimeError> {
    let runs = journal
        .restore_active_runs()
        .map_err(|error| RuntimeError::Journal(format!("{error:?}")))?;
    Ok(runs.into_iter().map(|run| run.run_id).collect())
}

pub fn catdesk_tool_definitions() -> Vec<ToolDefinitionV1> {
    vec![
        ToolDefinitionV1 {
            name: "read".into(),
            description: "Read a bounded file excerpt through CatDesk.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "startLine": { "type": "integer" },
                    "endLine": { "type": "integer" }
                },
                "required": ["path"]
            }),
            mutation_kind: ToolMutationKind::ReadOnly,
        },
        ToolDefinitionV1 {
            name: "search".into(),
            description: "Search the workspace through CatDesk.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string" },
                    "path": { "type": "string" }
                },
                "required": ["pattern"]
            }),
            mutation_kind: ToolMutationKind::ReadOnly,
        },
        ToolDefinitionV1 {
            name: "patch.preview".into(),
            description: "Preview a proposed patch through CatDesk policy.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "patchId": { "type": "string" },
                    "parentPatchId": { "type": "string" },
                    "targetPaths": {
                        "type": "array",
                        "items": { "type": "string" }
                    },
                    "operations": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "path": { "type": "string" },
                                "old": { "type": "string" },
                                "new": { "type": "string" }
                            },
                            "required": ["path", "old", "new"]
                        }
                    },
                    "rationale": { "type": "string" }
                },
                "required": ["patchId", "operations"]
            }),
            mutation_kind: ToolMutationKind::ReadOnly,
        },
        ToolDefinitionV1 {
            name: "patch.apply".into(),
            description: "Apply an approved patch through CatDesk policy.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "patchId": { "type": "string" },
                    "confirmation": { "type": "string" }
                },
                "required": ["patchId"]
            }),
            mutation_kind: ToolMutationKind::Mutating,
        },
        ToolDefinitionV1 {
            name: "patch.compare".into(),
            description: "Compare a revised child patch against a parent patch.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "parentPatchId": { "type": "string" },
                    "candidatePatchId": { "type": "string" }
                },
                "required": ["parentPatchId", "candidatePatchId"]
            }),
            mutation_kind: ToolMutationKind::ReadOnly,
        },
        ToolDefinitionV1 {
            name: "diff.actual".into(),
            description: "Capture the authoritative Git diff for selected paths.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "paths": {
                        "type": "array",
                        "items": { "type": "string" }
                    }
                }
            }),
            mutation_kind: ToolMutationKind::ReadOnly,
        },
        ToolDefinitionV1 {
            name: "verify.run".into(),
            description: "Run CatDesk-controlled verification for the workspace.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "timeout": { "type": "integer" }
                }
            }),
            mutation_kind: ToolMutationKind::ReadOnly,
        },
        ToolDefinitionV1 {
            name: "job.start".into(),
            description:
                "Start a CatDesk-controlled long-running job after command-policy validation."
                    .into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string" },
                    "cwd": { "type": "string" },
                    "maxLogBytes": { "type": "integer" }
                },
                "required": ["command"]
            }),
            mutation_kind: ToolMutationKind::Mutating,
        },
        ToolDefinitionV1 {
            name: "job.status".into(),
            description: "Read status for a CatDesk-controlled long-running job.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "jobId": { "type": "string" }
                },
                "required": ["jobId"]
            }),
            mutation_kind: ToolMutationKind::ReadOnly,
        },
        ToolDefinitionV1 {
            name: "job.poll".into(),
            description: "Read a bounded stdout or stderr slice for a CatDesk-controlled job."
                .into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "jobId": { "type": "string" },
                    "stream": { "type": "string", "enum": ["stdout", "stderr"] },
                    "offset": { "type": "integer" },
                    "maxBytes": { "type": "integer" }
                },
                "required": ["jobId"]
            }),
            mutation_kind: ToolMutationKind::ReadOnly,
        },
        ToolDefinitionV1 {
            name: "job.cancel".into(),
            description:
                "Cancel a CatDesk-controlled long-running job using process-tree termination."
                    .into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "jobId": { "type": "string" }
                },
                "required": ["jobId"]
            }),
            mutation_kind: ToolMutationKind::Mutating,
        },
    ]
}

pub fn parse_strict_tool_call(
    text: &str,
    allowed_tools: &BTreeSet<String>,
) -> Result<NormalizedToolCallV1, RuntimeError> {
    if text.trim() != text {
        return Err(RuntimeError::MalformedOutput(
            "tool-call envelope must not have surrounding text".into(),
        ));
    }
    let value: Value = serde_json::from_str(text)
        .map_err(|error| RuntimeError::MalformedOutput(error.to_string()))?;
    let schema = value
        .get("schema_version")
        .and_then(Value::as_str)
        .ok_or_else(|| RuntimeError::MalformedOutput("missing schema_version".into()))?;
    if schema != "catdesk.tool-call.v1" {
        return Err(RuntimeError::MalformedOutput(
            "unsupported tool-call schema".into(),
        ));
    }
    let tool_name = value
        .get("tool")
        .and_then(Value::as_str)
        .ok_or_else(|| RuntimeError::MalformedOutput("missing tool".into()))?;
    if !allowed_tools.contains(tool_name) {
        return Err(RuntimeError::MalformedOutput(format!(
            "unknown or disallowed tool `{tool_name}`"
        )));
    }
    let tool_call_id = ToolCallId::new(
        value
            .get("tool_call_id")
            .and_then(Value::as_str)
            .ok_or_else(|| RuntimeError::MalformedOutput("missing tool_call_id".into()))?,
    )
    .map_err(RuntimeError::MalformedOutput)?;
    let arguments = value
        .get("arguments")
        .cloned()
        .ok_or_else(|| RuntimeError::MalformedOutput("missing arguments".into()))?;
    if !arguments.is_object() {
        return Err(RuntimeError::MalformedOutput(
            "arguments must be an object".into(),
        ));
    }
    Ok(NormalizedToolCallV1 {
        tool_call_id,
        tool_name: tool_name.into(),
        arguments_hash: stable_value_hash(&arguments),
        arguments,
    })
}

pub struct OllamaAdapter {
    client: reqwest::Client,
    base_url: Url,
    keep_alive: Option<String>,
}

impl OllamaAdapter {
    pub fn new(base_url: &str, keep_alive: Option<String>) -> Result<Self, RuntimeError> {
        Ok(Self {
            client: reqwest::Client::new(),
            base_url: Url::parse(base_url)
                .map_err(|error| RuntimeError::Provider(error.to_string()))?,
            keep_alive,
        })
    }

    pub async fn health(&self) -> Result<ProviderHealthV1, RuntimeError> {
        let models = self.list_models().await?;
        Ok(ProviderHealthV1 {
            provider_id: "ollama".into(),
            status: ProviderHealthStatus::Available,
            available_models: models,
            detail: "Ollama local API responded".into(),
        })
    }

    pub async fn list_models(&self) -> Result<Vec<String>, RuntimeError> {
        let url = self
            .base_url
            .join("/api/tags")
            .map_err(|error| RuntimeError::Provider(error.to_string()))?;
        let value: Value = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|error| RuntimeError::Provider(error.to_string()))?
            .error_for_status()
            .map_err(|error| RuntimeError::Provider(error.to_string()))?
            .json()
            .await
            .map_err(|error| RuntimeError::Provider(error.to_string()))?;
        Ok(value
            .get("models")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|model| model.get("name").and_then(Value::as_str))
            .map(ToOwned::to_owned)
            .collect())
    }

    pub async fn chat_once(
        &self,
        model: &str,
        prompt: &str,
        allowed_tools: &[ToolDefinitionV1],
    ) -> Result<ProviderTurnResultV1, RuntimeError> {
        let url = self
            .base_url
            .join("/api/chat")
            .map_err(|error| RuntimeError::Provider(error.to_string()))?;
        let body = json!({
            "model": model,
            "stream": false,
            "keep_alive": self.keep_alive,
            "messages": [{ "role": "user", "content": prompt }],
            "tools": ollama_tool_definitions(allowed_tools),
        });
        let value: Value = self
            .client
            .post(url)
            .json(&body)
            .send()
            .await
            .map_err(|error| RuntimeError::Provider(error.to_string()))?
            .error_for_status()
            .map_err(|error| RuntimeError::Provider(error.to_string()))?
            .json()
            .await
            .map_err(|error| RuntimeError::Provider(error.to_string()))?;
        normalize_ollama_chat_response(
            &value,
            TurnId::new("ollama-turn").map_err(RuntimeError::Validation)?,
        )
    }

    pub async fn chat_stream_text(
        &self,
        model: &str,
        prompt: &str,
    ) -> Result<Vec<NormalizedProviderEventV1>, RuntimeError> {
        let url = self
            .base_url
            .join("/api/chat")
            .map_err(|error| RuntimeError::Provider(error.to_string()))?;
        let body = json!({
            "model": model,
            "stream": true,
            "keep_alive": self.keep_alive,
            "messages": [{ "role": "user", "content": prompt }],
        });
        let body_text = self
            .client
            .post(url)
            .json(&body)
            .send()
            .await
            .map_err(|error| RuntimeError::Provider(error.to_string()))?
            .error_for_status()
            .map_err(|error| RuntimeError::Provider(error.to_string()))?
            .text()
            .await
            .map_err(|error| RuntimeError::Provider(error.to_string()))?;
        let mut events = Vec::new();
        let turn_id = TurnId::new("ollama-stream-turn").map_err(RuntimeError::Validation)?;
        for line in body_text
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
        {
            let value: Value = serde_json::from_str(line)
                .map_err(|error| RuntimeError::Provider(error.to_string()))?;
            if let Some(text) = value
                .get("message")
                .and_then(|message| message.get("content"))
                .and_then(Value::as_str)
                .filter(|text| !text.is_empty())
            {
                events.push(NormalizedProviderEventV1 {
                    provider_id: "ollama".into(),
                    turn_id: turn_id.clone(),
                    kind: NormalizedProviderEventKind::TextDelta,
                    text: Some(text.into()),
                    tool_call: None,
                });
            }
        }
        Ok(events)
    }
}

fn normalize_ollama_chat_response(
    value: &Value,
    turn_id: TurnId,
) -> Result<ProviderTurnResultV1, RuntimeError> {
    let message = value
        .get("message")
        .ok_or_else(|| RuntimeError::MalformedOutput("missing Ollama message".into()))?;
    if let Some(tool_calls) = message.get("tool_calls").and_then(Value::as_array)
        && let Some(first) = tool_calls.first()
    {
        let function = first
            .get("function")
            .ok_or_else(|| RuntimeError::MalformedOutput("missing Ollama function".into()))?;
        let name = function
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| RuntimeError::MalformedOutput("missing Ollama tool name".into()))?;
        let arguments = function
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!({}));
        let tool_call = NormalizedToolCallV1 {
            tool_call_id: ToolCallId::new("ollama-tool-1").map_err(RuntimeError::Validation)?,
            tool_name: name.into(),
            arguments_hash: stable_value_hash(&arguments),
            arguments,
        };
        return Ok(ProviderTurnResultV1 {
            terminal: false,
            output_bytes: serde_json::to_string(value)
                .map_err(|error| RuntimeError::MalformedOutput(error.to_string()))?
                .len(),
            events: vec![NormalizedProviderEventV1 {
                provider_id: "ollama".into(),
                turn_id,
                kind: NormalizedProviderEventKind::ToolCall,
                text: None,
                tool_call: Some(tool_call),
            }],
        });
    }
    let text = message
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    Ok(provider_text_result("ollama", &turn_id, text, true))
}

fn provider_text_result(
    provider_id: &str,
    turn_id: &TurnId,
    text: String,
    terminal: bool,
) -> ProviderTurnResultV1 {
    ProviderTurnResultV1 {
        terminal,
        output_bytes: text.len(),
        events: vec![NormalizedProviderEventV1 {
            provider_id: provider_id.into(),
            turn_id: turn_id.clone(),
            kind: if terminal {
                NormalizedProviderEventKind::CompletionClaim
            } else {
                NormalizedProviderEventKind::TextDelta
            },
            text: Some(text),
            tool_call: None,
        }],
    }
}

fn ollama_tool_definitions(tools: &[ToolDefinitionV1]) -> Value {
    Value::Array(
        tools
            .iter()
            .map(|tool| {
                json!({
                    "type": "function",
                    "function": {
                        "name": tool.name,
                        "description": tool.description,
                        "parameters": tool.input_schema,
                    }
                })
            })
            .collect(),
    )
}

fn tool_mutation_kind(tool_name: &str) -> ToolMutationKind {
    catdesk_tool_definitions()
        .into_iter()
        .find(|tool| tool.name == tool_name)
        .map(|tool| tool.mutation_kind)
        .unwrap_or(ToolMutationKind::Mutating)
}

fn stable_value_hash(value: &Value) -> String {
    stable_text_hash(&serde_json::to_string(value).unwrap_or_default())
}

fn stable_text_hash(text: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("fnv1a64:{hash:016x}")
}

fn map_journal_error(error: JournalError) -> RuntimeError {
    RuntimeError::Journal(format!("{error:?}"))
}

impl From<super::context::ContextError> for RuntimeError {
    fn from(error: super::context::ContextError) -> Self {
        Self::Validation(format!("{error:?}"))
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    struct EchoDispatcher;

    impl CatDeskToolDispatcher for EchoDispatcher {
        fn execute(&mut self, tool_call: &NormalizedToolCallV1) -> Result<String, RuntimeError> {
            Ok(format!("executed {}", tool_call.tool_name))
        }
    }

    fn fixture_contract() -> ExecutionContractV1 {
        serde_json::from_str(include_str!(
            "../../tests/fixtures/delegated/execution_contract_v1.json"
        ))
        .expect("fixture parses")
    }

    fn temp_journal(name: &str) -> DelegatedJournal {
        let root =
            std::env::temp_dir().join(format!("catdesk-runtime-{name}-{}", std::process::id()));
        if root.exists() {
            fs::remove_dir_all(&root).expect("clear temp journal");
        }
        DelegatedJournal::open(root).expect("open journal")
    }

    fn budget() -> WorkerRuntimeBudgetV1 {
        WorkerRuntimeBudgetV1 {
            max_turns: 4,
            max_tool_calls: 4,
            max_elapsed: Duration::from_secs(30),
            max_input_bytes: 64 * 1024,
            max_output_bytes: 16 * 1024,
        }
    }

    #[test]
    fn fake_provider_completes_deterministic_multi_turn_scenario() {
        let journal = temp_journal("fake-complete");
        let tool_envelope = json!({
            "schema_version": "catdesk.tool-call.v1",
            "tool_call_id": "tc-read",
            "tool": "read",
            "arguments": { "path": "src/main.rs" }
        })
        .to_string();
        let provider = FakeProvider::new(vec![
            FakeProviderTurn::ToolCall(tool_envelope),
            FakeProviderTurn::Complete("done".into()),
        ]);
        let mut runtime = WorkerRuntimeHarness::new(&journal, provider);
        let mut dispatcher = EchoDispatcher;
        let snapshot = runtime
            .run(&fixture_contract(), &mut dispatcher, budget())
            .expect("runtime completes");
        assert_eq!(snapshot.status, WorkerRunStatus::Completed);
        assert_eq!(snapshot.completed_turns, 2);
        assert_eq!(snapshot.completed_tool_calls, 1);
    }

    #[test]
    fn malformed_output_is_rejected() {
        let allowed = catdesk_tool_definitions()
            .into_iter()
            .map(|tool| tool.name)
            .collect::<BTreeSet<_>>();
        let malformed = "before {\"schema_version\":\"catdesk.tool-call.v1\"}";
        assert!(matches!(
            parse_strict_tool_call(malformed, &allowed),
            Err(RuntimeError::MalformedOutput(_))
        ));
    }

    #[test]
    fn cancellation_works_at_safe_boundary() {
        let journal = temp_journal("cancel");
        let provider = FakeProvider::new(vec![FakeProviderTurn::Complete("done".into())]);
        let mut runtime = WorkerRuntimeHarness::new(&journal, provider);
        runtime.cancel();
        let mut dispatcher = EchoDispatcher;
        let snapshot = runtime
            .run(&fixture_contract(), &mut dispatcher, budget())
            .expect("cancelled snapshot");
        assert_eq!(snapshot.status, WorkerRunStatus::Cancelled);
    }

    #[test]
    fn restart_restores_checkpointed_session_runs() {
        let journal = temp_journal("restore");
        let mut contract = fixture_contract();
        contract.task_id = "run-restore".into();
        let run_id = journal.create_run(&contract).expect("create run").run_id;
        journal
            .update_run_state(&run_id, RunState::Running)
            .expect("running");
        let restored = restore_checkpointed_sessions(&journal).expect("restore");
        assert_eq!(restored, vec![run_id]);
    }

    #[test]
    fn catdesk_constructs_tool_definitions_without_provider_authority() {
        let tools = catdesk_tool_definitions();
        assert!(tools.iter().any(|tool| tool.name == "read"));
        assert!(tools.iter().any(|tool| tool.name == "patch.apply"));
        assert!(tools.iter().all(|tool| {
            !tool.name.contains("shell")
                && !tool.name.contains("exec")
                && !tool.description.to_ascii_lowercase().contains("provider")
        }));
    }

    #[test]
    fn ollama_tool_response_normalizes_to_provider_event() {
        let value = json!({
            "message": {
                "role": "assistant",
                "content": "",
                "tool_calls": [{
                    "function": {
                        "name": "read",
                        "arguments": { "path": "src/main.rs" }
                    }
                }]
            },
            "done": true
        });
        let result =
            normalize_ollama_chat_response(&value, TurnId::new("turn-ollama").expect("turn id"))
                .expect("normalize");
        assert_eq!(result.events[0].kind, NormalizedProviderEventKind::ToolCall);
        assert_eq!(
            result.events[0]
                .tool_call
                .as_ref()
                .expect("tool call")
                .tool_name,
            "read"
        );
    }

    #[tokio::test]
    #[ignore = "requires local Ollama with qwen3.5:9b"]
    async fn ollama_qwen_live_smoke_returns_normalized_response() {
        let adapter =
            OllamaAdapter::new("http://127.0.0.1:11434", Some("5m".into())).expect("adapter");
        let models = adapter.list_models().await.expect("list models");
        assert!(
            models.iter().any(|model| model == "qwen3.5:9b"),
            "qwen3.5:9b must be installed for the live T-0016 smoke"
        );
        let result = adapter
            .chat_once(
                "qwen3.5:9b",
                "Reply with exactly this text and no extra words: catdesk-ok",
                &[],
            )
            .await
            .expect("chat response");
        assert!(!result.events.is_empty());
        assert!(matches!(
            result.events[0].kind,
            NormalizedProviderEventKind::TextDelta
                | NormalizedProviderEventKind::CompletionClaim
                | NormalizedProviderEventKind::ToolCall
        ));
    }
}
