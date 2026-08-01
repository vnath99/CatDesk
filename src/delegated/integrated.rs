use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::delegated::advisor::{
    AdviceBrokerConfigV1, AdviceDisclosureClassification, AdviceRequestDraftV1, AdviceResponseV1,
    AdviceTrigger, AdvisorAdapter, AdvisorBroker, AdvisorStatus, BoundedSourceExcerptV1,
    DeepSeekProcessAdvisor, DeepSeekProcessAdvisorConfig, redact_and_bound,
};
use crate::delegated::context::{ContextBudgetPolicyV1, ContextBuilderV1};
use crate::delegated::contracts::{
    ApprovalRequirementKind, ExecutionContractV1, ItemId, PatchId, RunId, RunState, ToolCallId,
    TurnId, WorkerSessionId,
};
use crate::delegated::coordinator::{
    FinalReviewPackageV1, RunCoordinator, VerificationStatusV1, VerificationSummaryV1,
};
use crate::delegated::events::{EventCursor, EventEnvelopeV1, EventPayloadV1, LifecycleEvent};
use crate::delegated::job_manager::{JobId, JobManager, JobSpecV1, LogStream};
use crate::delegated::journal::{
    DelegatedJournal, PatchApplicationRecordV1, PatchApplicationStatus, PatchProposalRecordV1,
    ToolCallRecordV1, ToolCallStatus, ToolMutationKind,
};
use crate::delegated::patch_engine::{
    ActualDiffArtifactV1, FileHashV1, PatchEngine, PatchError, PatchProposalV1, ReplaceOperationV1,
    compare_patches, stable_text_hash, verify_model_completion_claim,
};
use crate::delegated::provider_router::{
    ProviderAvailabilityV1, ProviderConfigV1, ProviderRegistryV1, ProviderRoutingPolicyV1,
    fake_provider_config,
};
#[cfg(test)]
use crate::delegated::runtime::{FakeProvider, ProviderClientV1, ProviderTurnRequestV1};
use crate::delegated::runtime::{
    NormalizedProviderEventKind, NormalizedToolCallV1, OllamaAdapter, ProviderMessageV1,
    ProviderType, RuntimeError, ToolDefinitionV1, catdesk_tool_definitions,
    ollama_error_indicates_malformed_tool_syntax,
};
use crate::delegated::supervisor::SupervisorSurface;
use crate::{verification, workspace_tools};

use super::EXECUTION_CONTRACT_SCHEMA_VERSION;

#[derive(Debug)]
pub enum IntegratedError {
    Io(String),
    Json(String),
    Runtime(String),
    Journal(String),
    Patch(String),
    Tool(String),
    Verification(String),
    Supervisor(String),
}

impl From<std::io::Error> for IntegratedError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value.to_string())
    }
}

impl From<serde_json::Error> for IntegratedError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value.to_string())
    }
}

impl From<PatchError> for IntegratedError {
    fn from(value: PatchError) -> Self {
        Self::Patch(format!("{value:?}"))
    }
}

impl From<RuntimeError> for IntegratedError {
    fn from(value: RuntimeError) -> Self {
        Self::Runtime(format!("{value:?}"))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntegratedRunConfigV1 {
    pub journal_root: PathBuf,
    pub job_root: PathBuf,
    pub ollama_base_url: String,
    pub model_id: String,
    #[serde(default)]
    pub advisor: Option<IntegratedAdvisorConfigV1>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntegratedAdvisorConfigV1 {
    pub enabled: bool,
    pub advisor_id: String,
    pub disclosure_classification: AdviceDisclosureClassification,
    pub maximum_response_length: usize,
    pub maximum_consultations_per_run: u32,
    pub advice_required: bool,
    #[serde(default)]
    pub local_runtime: Option<IntegratedAdvisorLocalRuntimeConfigV1>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntegratedAdvisorLocalRuntimeConfigV1 {
    pub python_executable: PathBuf,
    pub adapter_script: PathBuf,
    pub profile_dir: PathBuf,
    #[serde(default)]
    pub selectors_path: Option<PathBuf>,
    #[serde(default = "default_advisor_headed")]
    pub headed: bool,
    #[serde(default)]
    pub allow_env_login: bool,
}

fn default_advisor_headed() -> bool {
    true
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntegratedToolResultV1 {
    pub tool_name: String,
    pub summary: String,
    pub payload: Value,
    pub bounded_text: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntegratedRunSnapshotV1 {
    pub run_id: RunId,
    pub state: RunState,
    pub checkpoint: String,
    pub completed_tool_call_ids: Vec<String>,
}

pub struct IntegratedDelegatedService {
    workspace_root: PathBuf,
    contract: ExecutionContractV1,
    config: IntegratedRunConfigV1,
    journal: DelegatedJournal,
    job_manager: JobManager,
    supervisor: SupervisorSurface,
    provider_registry: ProviderRegistryV1,
    patch_proposals: BTreeMap<String, PatchProposalV1>,
    last_verification: Option<VerificationSummaryV1>,
    last_diff: Option<ActualDiffArtifactV1>,
    completed_tool_call_ids: Vec<String>,
    provider_history: Vec<ProviderMessageV1>,
    completed_tool_calls: u32,
    consecutive_failed_verifications: u32,
    advice_consulted_for_current_failure: bool,
    advisor_consultations_used: u32,
    recent_read_excerpts: Vec<BoundedSourceExcerptV1>,
    advisor_process: Option<DeepSeekProcessAdvisor>,
    next_event_sequence: u64,
    worker_session_id: WorkerSessionId,
    current_turn_id: Option<TurnId>,
    corrective_turns_used: u32,
    json_tool_recovery_turns_remaining: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct IntegratedDurableStateV1 {
    schema_version: u32,
    run_id: RunId,
    patch_proposals: Vec<PatchProposalV1>,
    last_verification: Option<VerificationSummaryV1>,
    last_diff: Option<ActualDiffArtifactV1>,
    completed_tool_call_ids: Vec<String>,
    provider_history: Vec<ProviderMessageV1>,
    completed_tool_calls: u32,
    #[serde(default)]
    consecutive_failed_verifications: u32,
    #[serde(default)]
    advice_consulted_for_current_failure: bool,
    #[serde(default)]
    advisor_consultations_used: u32,
    #[serde(default)]
    recent_read_excerpts: Vec<BoundedSourceExcerptV1>,
    next_event_sequence: u64,
    #[serde(default)]
    corrective_turns_used: u32,
    #[serde(default)]
    json_tool_recovery_turns_remaining: u32,
}

const MAX_CORRECTIVE_TURNS_PER_RUN: u32 = 2;

impl IntegratedDelegatedService {
    pub fn new(
        workspace_root: impl AsRef<Path>,
        contract: ExecutionContractV1,
        config: IntegratedRunConfigV1,
    ) -> Result<Self, IntegratedError> {
        let workspace_root = workspace_root
            .as_ref()
            .canonicalize()
            .map_err(|error| IntegratedError::Io(error.to_string()))?;
        let journal = DelegatedJournal::open(&config.journal_root)
            .map_err(|e| IntegratedError::Journal(format!("{e:?}")))?;
        let job_manager = JobManager::open(&config.job_root)
            .map_err(|e| IntegratedError::Tool(format!("{e:?}")))?;
        let mut provider_registry = ProviderRegistryV1::new();
        provider_registry.register(fake_provider_config(
            "ollama",
            ProviderType::LocalApi,
            ProviderAvailabilityV1::Available,
        ));
        provider_registry.register(ProviderConfigV1 {
            provider_id: "fake".into(),
            capabilities: crate::delegated::runtime::FakeProvider::capabilities(),
            availability: ProviderAvailabilityV1::Available,
            credential_env_var: None,
        });
        let run_id = RunId::new(contract.task_id.clone()).map_err(IntegratedError::Tool)?;
        Ok(Self {
            workspace_root,
            contract,
            config,
            journal,
            job_manager,
            supervisor: SupervisorSurface::new(),
            provider_registry,
            patch_proposals: BTreeMap::new(),
            last_verification: None,
            last_diff: None,
            completed_tool_call_ids: Vec::new(),
            provider_history: Vec::new(),
            completed_tool_calls: 0,
            consecutive_failed_verifications: 0,
            advice_consulted_for_current_failure: false,
            advisor_consultations_used: 0,
            recent_read_excerpts: Vec::new(),
            advisor_process: None,
            next_event_sequence: 1,
            worker_session_id: WorkerSessionId::new(format!("worker-session-{}", run_id.as_str()))
                .map_err(IntegratedError::Tool)?,
            current_turn_id: None,
            corrective_turns_used: 0,
            json_tool_recovery_turns_remaining: 0,
        })
    }

    pub fn tool_definitions(&self) -> Vec<ToolDefinitionV1> {
        catdesk_tool_definitions()
            .into_iter()
            .filter(|tool| !tool.name.starts_with("job."))
            .collect()
    }

    pub fn provider_policy(&self) -> ProviderRoutingPolicyV1 {
        ProviderRoutingPolicyV1 {
            primary_provider_id: "ollama".into(),
            fallback_provider_ids: vec!["fake".into()],
            disclosure_policy: ContextBudgetPolicyV1::local_default().disclosure_policy,
        }
    }

    pub fn run_id(&self) -> Result<RunId, IntegratedError> {
        RunId::new(self.contract.task_id.clone()).map_err(IntegratedError::Tool)
    }

    pub fn patch_proposal(&self, patch_id: &PatchId) -> Option<PatchProposalV1> {
        self.patch_proposals.get(patch_id.as_str()).cloned()
    }

    pub fn actual_diff(&self, diff_hash: &str) -> Option<ActualDiffArtifactV1> {
        self.last_diff
            .as_ref()
            .filter(|diff| diff.diff_hash == diff_hash)
            .cloned()
    }

    pub fn last_actual_diff(&self) -> Option<ActualDiffArtifactV1> {
        self.last_diff.clone()
    }

    pub async fn run_ollama_worker_loop(
        &mut self,
    ) -> Result<FinalReviewPackageV1, IntegratedError> {
        self.run_ollama_worker_loop_with_cancel(Arc::new(AtomicBool::new(false)))
            .await
    }

    pub async fn run_ollama_worker_loop_with_cancel(
        &mut self,
        cancel_requested: Arc<AtomicBool>,
    ) -> Result<FinalReviewPackageV1, IntegratedError> {
        self.start()?;
        let ollama = OllamaAdapter::new(&self.config.ollama_base_url, Some("5m".into()))?;
        self.provider_history.clear();
        self.provider_history.push(ProviderMessageV1 {
            role: "system".into(),
            content: worker_system_prompt(),
            tool_call_id: None,
            tool_name: None,
        });
        self.provider_history.push(ProviderMessageV1 {
            role: "user".into(),
            content: format!(
                "Execution contract:\n{}\nUse CatDesk tools until the acceptance criteria are verified. Do not claim completion until verify.run passes and diff.actual has captured the authoritative diff.",
                serde_json::to_string_pretty(&self.contract)?
            ),
            tool_call_id: None,
            tool_name: None,
        });
        let started = Instant::now();
        let allowed_tools = self.production_worker_tools();
        for turn in 1..=self.contract.max_turns {
            if cancel_requested.load(Ordering::SeqCst) {
                self.journal
                    .update_run_state(
                        &RunId::new(self.contract.task_id.clone())
                            .map_err(IntegratedError::Tool)?,
                        RunState::Cancelled,
                    )
                    .map_err(|e| IntegratedError::Journal(format!("{e:?}")))?;
                self.persist_durable_state()?;
                return Err(IntegratedError::Tool(
                    "run cancelled at turn boundary".into(),
                ));
            }
            if started.elapsed() > Duration::from_secs(self.contract.max_elapsed_seconds) {
                self.journal
                    .update_run_state(
                        &RunId::new(self.contract.task_id.clone())
                            .map_err(IntegratedError::Tool)?,
                        RunState::Failed,
                    )
                    .map_err(|e| IntegratedError::Journal(format!("{e:?}")))?;
                return Err(IntegratedError::Tool("elapsed budget exceeded".into()));
            }
            let turn_id = TurnId::new(format!("turn-{turn}")).map_err(IntegratedError::Tool)?;
            self.current_turn_id = Some(turn_id.clone());
            self.compact_provider_history_if_needed()?;
            let use_json_tool_recovery = self.json_tool_recovery_turns_remaining > 0;
            let result = match if use_json_tool_recovery {
                ollama
                    .chat_messages_once_accepting_json_envelope(
                        &self.config.model_id,
                        &self.provider_history,
                        turn_id,
                    )
                    .await
            } else {
                ollama
                    .chat_messages_once(
                        &self.config.model_id,
                        &self.provider_history,
                        &allowed_tools,
                        turn_id,
                    )
                    .await
            } {
                Ok(result) => {
                    if use_json_tool_recovery {
                        self.json_tool_recovery_turns_remaining = 0;
                    }
                    result
                }
                Err(error) => {
                    if use_json_tool_recovery {
                        self.json_tool_recovery_turns_remaining = 0;
                    }
                    match self.push_malformed_tool_syntax_correction(turn, &error) {
                        Ok(true) => {
                            self.persist_durable_state()?;
                            continue;
                        }
                        Ok(false) => return Err(IntegratedError::from(error)),
                        Err(correction_error) => return Err(correction_error),
                    }
                }
            };
            if result.output_bytes > 64 * 1024 {
                return Err(IntegratedError::Tool(
                    "provider output exceeded CatDesk bound".into(),
                ));
            }
            let mut saw_tool = false;
            for event in result.events {
                match event.kind {
                    NormalizedProviderEventKind::ToolCall => {
                        let mut call = event.tool_call.ok_or_else(|| {
                            IntegratedError::Tool("provider omitted tool call payload".into())
                        })?;
                        call.tool_call_id = ToolCallId::new(format!("tc-prod-{turn}"))
                            .map_err(IntegratedError::Tool)?;
                        call.arguments_hash =
                            stable_text_hash(&serde_json::to_string(&call.arguments)?);
                        self.provider_history.push(ProviderMessageV1 {
                            role: "assistant".into(),
                            content: serde_json::to_string(&json!({
                                "tool": call.tool_name,
                                "arguments": call.arguments.clone(),
                            }))?,
                            tool_call_id: Some(call.tool_call_id.as_str().to_string()),
                            tool_name: Some(call.tool_name.clone()),
                        });
                        let tool_result = self.execute_tool_call(&call).await;
                        match tool_result {
                            Ok(_tool_result) => {
                                if call.tool_name == "verify.run" {
                                    self.maybe_consult_configured_advisor_after_failed_repairs(
                                        cancel_requested.clone(),
                                    )
                                    .await?;
                                }
                            }
                            Err(error) => {
                                let message = format!(
                                    "Tool {} failed under CatDesk policy and was journaled. Error: {error:?}. Choose a permitted next CatDesk tool call; do not retry the same invalid request.",
                                    call.tool_name
                                );
                                if matches!(call.tool_name.as_str(), "patch.apply") {
                                    return Err(IntegratedError::Tool(message));
                                }
                                self.provider_history.push(ProviderMessageV1 {
                                    role: "user".into(),
                                    content: message,
                                    tool_call_id: Some(call.tool_call_id.as_str().to_string()),
                                    tool_name: Some(call.tool_name.clone()),
                                });
                            }
                        };
                        saw_tool = true;
                    }
                    NormalizedProviderEventKind::TextDelta => {
                        let text = event.text.unwrap_or_default();
                        self.provider_history.push(ProviderMessageV1 {
                            role: "assistant".into(),
                            content: text.clone(),
                            tool_call_id: None,
                            tool_name: None,
                        });
                    }
                    NormalizedProviderEventKind::CompletionClaim => {
                        let text = event.text.unwrap_or_default();
                        self.provider_history.push(ProviderMessageV1 {
                            role: "assistant".into(),
                            content: text.clone(),
                            tool_call_id: None,
                            tool_name: None,
                        });
                        if let Err(error) = self.verify_completion_gate(&text) {
                            if self.push_premature_completion_correction(
                                turn,
                                &error,
                                result.stop_reason.as_deref(),
                                result.prompt_eval_count,
                            )? {
                                saw_tool = true;
                                break;
                            }
                            return Err(error);
                        }
                        self.journal
                            .update_run_state(
                                &RunId::new(self.contract.task_id.clone())
                                    .map_err(IntegratedError::Tool)?,
                                RunState::CompletedVerified,
                            )
                            .map_err(|e| IntegratedError::Journal(format!("{e:?}")))?;
                        self.persist_durable_state()?;
                        return self.final_review();
                    }
                    NormalizedProviderEventKind::MalformedResponse => {
                        return Err(IntegratedError::Tool(
                            event
                                .text
                                .unwrap_or_else(|| "malformed provider response".into()),
                        ));
                    }
                    NormalizedProviderEventKind::CancelAck => {
                        return Err(IntegratedError::Tool("provider cancelled run".into()));
                    }
                    NormalizedProviderEventKind::TerminalError => {
                        return Err(IntegratedError::Tool(
                            event
                                .text
                                .unwrap_or_else(|| "provider terminal error".into()),
                        ));
                    }
                }
            }
            if !saw_tool && result.terminal {
                if self.push_premature_stop_correction(
                    turn,
                    result.stop_reason.as_deref(),
                    result.prompt_eval_count,
                )? {
                    self.persist_durable_state()?;
                    continue;
                }
                return Err(IntegratedError::Tool(
                    "provider stopped before CatDesk verification and diff completed".into(),
                ));
            }
            self.persist_durable_state()?;
        }
        Err(IntegratedError::Tool("turn budget exceeded".into()))
    }

    fn production_worker_tools(&self) -> Vec<ToolDefinitionV1> {
        self.tool_definitions()
    }

    fn push_malformed_tool_syntax_correction(
        &mut self,
        turn: u32,
        error: &RuntimeError,
    ) -> Result<bool, IntegratedError> {
        let message = format!("{error:?}");
        if !ollama_error_indicates_malformed_tool_syntax(&message) {
            return Ok(false);
        }
        let next_action = self
            .next_required_action_from_persisted_state()?
            .unwrap_or_else(|| "make a concise final completion claim".into());
        let instruction = format!(
            "The previous provider continuation failed because the model emitted malformed native tool-call syntax. The next continuation will use CatDesk JSON-envelope recovery without native Ollama tool definitions. Do not repeat prose, XML, markdown, or multiple calls. Return exactly one JSON object shaped as {{\"tool\":\"tool.name\",\"arguments\":{{...}}}}. CatDesk will validate the tool name and arguments before execution. Continue from the persisted run state; completed tool calls are authoritative and must not be replayed. Required next action: {next_action}"
        );
        self.json_tool_recovery_turns_remaining = 1;
        self.push_corrective_continuation("malformed-tool-call", turn, None, &instruction)
    }

    fn push_premature_completion_correction(
        &mut self,
        turn: u32,
        error: &IntegratedError,
        stop_reason: Option<&str>,
        prompt_eval_count: Option<u64>,
    ) -> Result<bool, IntegratedError> {
        let Some(next_action) = self.next_required_action_from_persisted_state()? else {
            return Ok(false);
        };
        let instruction = format!(
            "The previous response claimed completion before CatDesk accepted the final evidence. Gate error: {}. Required next action: {}",
            bounded_diagnostic_text(&format!("{error:?}"), 512),
            next_action
        );
        self.push_corrective_continuation(
            "premature-completion",
            turn,
            stop_reason.or_else(|| prompt_eval_count.map(|_| "prompt-eval-count-present")),
            &instruction,
        )
    }

    fn push_premature_stop_correction(
        &mut self,
        turn: u32,
        stop_reason: Option<&str>,
        prompt_eval_count: Option<u64>,
    ) -> Result<bool, IntegratedError> {
        let Some(next_action) = self.next_required_action_from_persisted_state()? else {
            return Ok(false);
        };
        let mut instruction = format!(
            "The provider stopped without a CatDesk tool call before the run was verified. Required next action: {next_action}"
        );
        if let Some(tokens) = prompt_eval_count {
            instruction.push_str(&format!(" PromptEvalCount={tokens}."));
        }
        self.push_corrective_continuation("premature-stop", turn, stop_reason, &instruction)
    }

    fn push_corrective_continuation(
        &mut self,
        kind: &str,
        turn: u32,
        stop_reason: Option<&str>,
        instruction: &str,
    ) -> Result<bool, IntegratedError> {
        if self.corrective_turns_used >= MAX_CORRECTIVE_TURNS_PER_RUN {
            let state = self.persisted_run_state_label()?;
            return Err(IntegratedError::Tool(format!(
                "corrective turn budget exhausted after {} corrections; model={}; runId={}; turn={turn}; persistedState={state}; lastCompletedToolCallId={}",
                self.corrective_turns_used,
                self.config.model_id,
                self.run_id()?.as_str(),
                self.last_completed_tool_call_id()
                    .unwrap_or_else(|| "none".into())
            )));
        }
        self.corrective_turns_used += 1;
        let state = self.persisted_run_state_label()?;
        let history_bytes = serde_json::to_vec(&self.provider_history)?.len();
        let diagnostic = json!({
            "kind": kind,
            "modelId": self.config.model_id,
            "runId": self.run_id()?.as_str(),
            "turn": turn,
            "historyBytes": history_bytes,
            "correctiveTurnNumber": self.corrective_turns_used,
            "maxCorrectiveTurns": MAX_CORRECTIVE_TURNS_PER_RUN,
            "lastCompletedToolCallId": self.last_completed_tool_call_id(),
            "persistedRunState": state,
            "stopReason": stop_reason.unwrap_or("unknown"),
        });
        self.provider_history.push(ProviderMessageV1 {
            role: "user".into(),
            content: format!(
                "<catdesk_corrective_continuation>\n{}\n{}\n</catdesk_corrective_continuation>",
                serde_json::to_string_pretty(&diagnostic)?,
                bounded_diagnostic_text(instruction, 2048)
            ),
            tool_call_id: None,
            tool_name: None,
        });
        self.append_event(
            LifecycleEvent::Delta,
            EventPayloadV1::Delta {
                text: format!(
                    "corrective continuation {kind} issued: {}",
                    bounded_diagnostic_text(instruction, 256)
                ),
            },
        )?;
        Ok(true)
    }

    fn next_required_action_from_persisted_state(&self) -> Result<Option<String>, IntegratedError> {
        if let Some(patch_id) = self.latest_unapplied_patch_id()? {
            return Ok(Some(format!(
                "call patch.apply with patchId {patch_id}, or make one explicit abandonment/escalation tool decision if applying it is no longer safe"
            )));
        }
        match self.last_verification.as_ref().map(|summary| &summary.status) {
            None => Ok(Some(
                "call verify.run next if no source mutation is required; otherwise inspect with read/search and use patch.preview before any patch.apply"
                    .into(),
            )),
            Some(VerificationStatusV1::Failed | VerificationStatusV1::NotConfigured) => Ok(Some(
                "diagnose the failed verifier output, inspect bounded source as needed, then call patch.preview for a corrected child patch within the repair budget"
                    .into(),
            )),
            Some(VerificationStatusV1::Passed) if self.last_diff.is_none() => Ok(Some(
                "call diff.actual for the contract allowed paths so CatDesk has authoritative diff evidence"
                    .into(),
            )),
            Some(VerificationStatusV1::Passed) => Ok(None),
        }
    }

    fn latest_unapplied_patch_id(&self) -> Result<Option<String>, IntegratedError> {
        let run_id = self.run_id()?;
        let applications = self
            .journal
            .load_patch_applications(&run_id)
            .map_err(|error| IntegratedError::Journal(format!("{error:?}")))?;
        let applied_patch_ids = applications
            .iter()
            .map(|application| application.patch_id.as_str().to_string())
            .collect::<BTreeSet<_>>();
        let proposals = self
            .journal
            .load_patch_proposals(&run_id)
            .map_err(|error| IntegratedError::Journal(format!("{error:?}")))?;
        Ok(proposals
            .iter()
            .rev()
            .map(|proposal| proposal.patch_id.as_str().to_string())
            .find(|patch_id| !applied_patch_ids.contains(patch_id)))
    }

    fn latest_patch_proposal_id(&self) -> Result<Option<String>, IntegratedError> {
        let run_id = self.run_id()?;
        let proposals = self
            .journal
            .load_patch_proposals(&run_id)
            .map_err(|error| IntegratedError::Journal(format!("{error:?}")))?;
        Ok(proposals
            .last()
            .map(|proposal| proposal.patch_id.as_str().to_string()))
    }

    fn persisted_run_state_label(&self) -> Result<String, IntegratedError> {
        let run_id = self.run_id()?;
        let applications = self
            .journal
            .load_patch_applications(&run_id)
            .map_err(|error| IntegratedError::Journal(format!("{error:?}")))?;
        Ok(format!(
            "patchProposals={}; patchApplications={}; verification={}; diff={}; completedToolCalls={}",
            self.patch_proposals.len(),
            applications.len(),
            self.last_verification
                .as_ref()
                .map(|summary| format!("{:?}", summary.status))
                .unwrap_or_else(|| "not-run".into()),
            self.last_diff
                .as_ref()
                .map(|diff| diff.diff_hash.clone())
                .unwrap_or_else(|| "none".into()),
            self.completed_tool_call_ids.len()
        ))
    }

    fn last_completed_tool_call_id(&self) -> Option<String> {
        self.completed_tool_call_ids.last().cloned()
    }

    #[cfg(test)]
    async fn run_fake_worker_loop_with_advisor(
        &mut self,
        mut provider: FakeProvider,
        cancel_requested: Arc<AtomicBool>,
    ) -> Result<FinalReviewPackageV1, IntegratedError> {
        self.start()?;
        self.provider_history.clear();
        self.provider_history.push(ProviderMessageV1 {
            role: "system".into(),
            content: worker_system_prompt(),
            tool_call_id: None,
            tool_name: None,
        });
        self.provider_history.push(ProviderMessageV1 {
            role: "user".into(),
            content: format!(
                "Execution contract:\n{}\nUse CatDesk tools until verify.run passes and diff.actual captures the authoritative diff.",
                serde_json::to_string_pretty(&self.contract)?
            ),
            tool_call_id: None,
            tool_name: None,
        });
        let allowed_tools = self.production_worker_tools();
        let allowed_names = allowed_tools
            .iter()
            .map(|tool| tool.name.clone())
            .collect::<BTreeSet<_>>();
        for turn in 1..=self.contract.max_turns {
            if cancel_requested.load(Ordering::SeqCst) {
                return Err(IntegratedError::Tool("run cancelled".into()));
            }
            self.compact_provider_history_if_needed()?;
            let turn_id = TurnId::new(format!("turn-{turn}")).map_err(IntegratedError::Tool)?;
            self.current_turn_id = Some(turn_id.clone());
            let request = ProviderTurnRequestV1 {
                run_id: self.run_id()?,
                worker_session_id: self.worker_session_id.clone(),
                turn_id: turn_id.clone(),
                model_id: self.config.model_id.clone(),
                context_json: json!({ "history": self.provider_history }),
                tool_definitions: allowed_tools.clone(),
                max_output_bytes: 16 * 1024,
            };
            let result = match provider.send_turn(&request, &allowed_names, &self.provider_history)
            {
                Ok(result) => result,
                Err(error) => match self.push_malformed_tool_syntax_correction(turn, &error) {
                    Ok(true) => {
                        self.persist_durable_state()?;
                        continue;
                    }
                    Ok(false) => return Err(IntegratedError::from(error)),
                    Err(correction_error) => return Err(correction_error),
                },
            };
            let mut saw_tool = false;
            for event in result.events {
                match event.kind {
                    NormalizedProviderEventKind::ToolCall => {
                        let mut call = event.tool_call.ok_or_else(|| {
                            IntegratedError::Tool("provider omitted tool call payload".into())
                        })?;
                        call.tool_call_id = ToolCallId::new(format!("tc-fake-{turn}"))
                            .map_err(IntegratedError::Tool)?;
                        call.arguments_hash =
                            stable_text_hash(&serde_json::to_string(&call.arguments)?);
                        self.provider_history.push(ProviderMessageV1 {
                            role: "assistant".into(),
                            content: serde_json::to_string(&json!({
                                "tool": call.tool_name,
                                "arguments": call.arguments.clone(),
                            }))?,
                            tool_call_id: Some(call.tool_call_id.as_str().to_string()),
                            tool_name: Some(call.tool_name.clone()),
                        });
                        self.execute_tool_call(&call).await?;
                        if call.tool_name == "verify.run" {
                            self.maybe_consult_configured_advisor_after_failed_repairs(
                                cancel_requested.clone(),
                            )
                            .await?;
                        }
                        saw_tool = true;
                    }
                    NormalizedProviderEventKind::CompletionClaim => {
                        let text = event.text.unwrap_or_default();
                        self.provider_history.push(ProviderMessageV1 {
                            role: "assistant".into(),
                            content: text.clone(),
                            tool_call_id: None,
                            tool_name: None,
                        });
                        if let Err(error) = self.verify_completion_gate(&text) {
                            if self.push_premature_completion_correction(
                                turn,
                                &error,
                                result.stop_reason.as_deref(),
                                result.prompt_eval_count,
                            )? {
                                saw_tool = true;
                                break;
                            }
                            return Err(error);
                        }
                        self.journal
                            .update_run_state(&self.run_id()?, RunState::CompletedVerified)
                            .map_err(|error| IntegratedError::Journal(format!("{error:?}")))?;
                        self.persist_durable_state()?;
                        return self.final_review();
                    }
                    NormalizedProviderEventKind::TextDelta => {
                        self.provider_history.push(ProviderMessageV1 {
                            role: "assistant".into(),
                            content: event.text.unwrap_or_default(),
                            tool_call_id: None,
                            tool_name: None,
                        });
                    }
                    NormalizedProviderEventKind::MalformedResponse
                    | NormalizedProviderEventKind::CancelAck
                    | NormalizedProviderEventKind::TerminalError => {
                        return Err(IntegratedError::Tool(
                            event.text.unwrap_or_else(|| "provider error".into()),
                        ));
                    }
                }
            }
            if result.terminal && !saw_tool {
                if self.push_premature_stop_correction(
                    turn,
                    result.stop_reason.as_deref(),
                    result.prompt_eval_count,
                )? {
                    self.persist_durable_state()?;
                    continue;
                }
                return Err(IntegratedError::Tool(
                    "provider stopped before verified completion".into(),
                ));
            }
            self.persist_durable_state()?;
        }
        Err(IntegratedError::Tool("turn budget exceeded".into()))
    }

    fn compact_provider_history_if_needed(&mut self) -> Result<(), IntegratedError> {
        const MAX_PROVIDER_INPUT_BYTES: usize = 48 * 1024;
        let serialized = serde_json::to_vec(&self.provider_history)?;
        if serialized.len() <= MAX_PROVIDER_INPUT_BYTES || self.provider_history.len() <= 10 {
            return Ok(());
        }

        let mut next = Vec::new();
        if let Some(system) = self.provider_history.first().cloned() {
            next.push(system);
        }
        if let Some(contract) = self.provider_history.get(1).cloned() {
            next.push(contract);
        }
        next.push(ProviderMessageV1 {
            role: "user".into(),
            content: self.context_checkpoint_summary()?,
            tool_call_id: None,
            tool_name: None,
        });
        let keep = self
            .provider_history
            .iter()
            .rev()
            .take(8)
            .cloned()
            .collect::<Vec<_>>();
        for message in keep.into_iter().rev() {
            next.push(message);
        }
        self.provider_history = next;
        let serialized = serde_json::to_vec(&self.provider_history)?;
        if serialized.len() > MAX_PROVIDER_INPUT_BYTES {
            return Err(IntegratedError::Tool(format!(
                "serialized provider input exceeded {MAX_PROVIDER_INPUT_BYTES} bytes after compaction"
            )));
        }
        Ok(())
    }

    fn context_checkpoint_summary(&self) -> Result<String, IntegratedError> {
        let latest_patch = self
            .latest_patch_proposal_id()?
            .unwrap_or_else(|| "none".into());
        let verification = self
            .last_verification
            .as_ref()
            .map(|summary| format!("{:?}", summary.status))
            .unwrap_or_else(|| "not-run".into());
        let latest_diff = self
            .last_diff
            .as_ref()
            .map(|diff| diff.diff_hash.clone())
            .unwrap_or_else(|| "none".into());
        Ok(format!(
            "Compacted checkpoint:\nobjective: {}\nallowed_paths: {}\nforbidden_paths: {}\nacceptance_criteria: {}\nlatest_patch: {}\nverification_status: {}\nlatest_diff: {}\nremaining_turn_budget: {}\nremaining_tool_budget: {}",
            self.contract.objective,
            self.contract.allowed_paths.join(", "),
            self.contract.forbidden_paths.join(", "),
            self.contract.acceptance_criteria.join("; "),
            latest_patch,
            verification,
            latest_diff,
            self.contract.max_turns,
            self.contract
                .max_tool_calls
                .saturating_sub(self.completed_tool_calls)
        ))
    }

    fn verify_completion_gate(&self, claim: &str) -> Result<(), IntegratedError> {
        let verification = self
            .last_verification
            .as_ref()
            .ok_or_else(|| IntegratedError::Verification("verification has not run".into()))?;
        if verification.status != VerificationStatusV1::Passed {
            return Err(IntegratedError::Verification(
                "verification has not passed".into(),
            ));
        }
        let diff = self.last_diff.as_ref().ok_or_else(|| {
            IntegratedError::Verification("actual diff has not been captured".into())
        })?;
        verification_passed_with_diff(claim, verification, diff)?;
        self.evaluate_acceptance_criteria_v1(verification, diff)?;
        self.ensure_diff_paths_allowed(diff)?;
        self.ensure_no_outcome_unknown()?;
        Ok(())
    }

    fn evaluate_acceptance_criteria_v1(
        &self,
        verification: &VerificationSummaryV1,
        diff: &ActualDiffArtifactV1,
    ) -> Result<(), IntegratedError> {
        for criterion in &self.contract.acceptance_criteria {
            match normalize_acceptance_criterion(criterion).as_str() {
                "cargo tests pass" | "cargo verification passes" | "verification passes" => {
                    if verification.status != VerificationStatusV1::Passed {
                        return Err(IntegratedError::Verification(format!(
                            "acceptance criterion is not satisfied: {criterion}"
                        )));
                    }
                }
                "authoritative diff is captured" => {
                    if diff.diff.trim().is_empty() {
                        return Err(IntegratedError::Verification(format!(
                            "acceptance criterion is not satisfied: {criterion}"
                        )));
                    }
                }
                _ => {
                    return Err(IntegratedError::Verification(format!(
                        "unsupported v1 acceptance criterion: {criterion}"
                    )));
                }
            }
        }
        Ok(())
    }

    fn ensure_diff_paths_allowed(
        &self,
        diff: &ActualDiffArtifactV1,
    ) -> Result<(), IntegratedError> {
        for path in &diff.paths {
            let normalized = path.replace('\\', "/");
            if self
                .contract
                .forbidden_paths
                .iter()
                .any(|forbidden| path_is_under_contract_path(&normalized, forbidden))
            {
                return Err(IntegratedError::Verification(format!(
                    "changed forbidden path {path}"
                )));
            }
            if !self
                .contract
                .allowed_paths
                .iter()
                .any(|allowed| path_is_under_contract_path(&normalized, allowed))
            {
                return Err(IntegratedError::Verification(format!(
                    "changed path outside allowed scope {path}"
                )));
            }
        }
        Ok(())
    }

    fn ensure_no_outcome_unknown(&self) -> Result<(), IntegratedError> {
        let run_id = RunId::new(self.contract.task_id.clone()).map_err(IntegratedError::Tool)?;
        let tool_calls = self
            .journal
            .load_tool_calls(&run_id)
            .map_err(|error| IntegratedError::Journal(format!("{error:?}")))?;
        if let Some(record) = tool_calls
            .values()
            .find(|record| record.status == ToolCallStatus::OutcomeUnknown)
        {
            return Err(IntegratedError::Verification(format!(
                "tool call {} has OUTCOME_UNKNOWN status",
                record.tool_call_id.as_str()
            )));
        }
        Ok(())
    }

    pub fn start(&mut self) -> Result<IntegratedRunSnapshotV1, IntegratedError> {
        let snapshot = match self.journal.create_run(&self.contract) {
            Ok(snapshot) => snapshot,
            Err(crate::delegated::journal::JournalError::DuplicateRun(_)) => {
                let run_id =
                    RunId::new(self.contract.task_id.clone()).map_err(IntegratedError::Tool)?;
                self.journal
                    .load_run(&run_id)
                    .map_err(|e| IntegratedError::Journal(format!("{e:?}")))?
            }
            Err(error) => return Err(IntegratedError::Journal(format!("{error:?}"))),
        };
        self.journal
            .update_run_state(&snapshot.run_id, RunState::Running)
            .map_err(|e| IntegratedError::Journal(format!("{e:?}")))?;
        self.supervisor.create_run(snapshot.run_id.clone());
        self.supervisor
            .start_run(&snapshot.run_id)
            .map_err(|e| IntegratedError::Supervisor(format!("{e:?}")))?;
        self.append_event(
            LifecycleEvent::Started,
            EventPayloadV1::RunStateChanged {
                state: RunState::Running,
            },
        )?;
        self.persist_durable_state()?;
        self.snapshot("run started")
    }

    pub fn recover(
        workspace_root: impl AsRef<Path>,
        contract: ExecutionContractV1,
        config: IntegratedRunConfigV1,
    ) -> Result<Self, IntegratedError> {
        let mut service = Self::new(workspace_root, contract, config)?;
        let run_id = RunId::new(service.contract.task_id.clone()).map_err(IntegratedError::Tool)?;
        if let Ok(run) = service.journal.load_run(&run_id) {
            service.next_event_sequence = run.last_event_sequence.saturating_add(1).max(1);
            service.supervisor.create_run(run.run_id.clone());
            service
                .supervisor
                .set_checkpoint(&run.run_id, "recovered from journal".into())
                .map_err(|e| IntegratedError::Supervisor(format!("{e:?}")))?;
            let tool_calls = service
                .journal
                .load_tool_calls(&run_id)
                .map_err(|e| IntegratedError::Journal(format!("{e:?}")))?;
            service.completed_tool_call_ids = tool_calls
                .values()
                .filter(|record| {
                    record.status == crate::delegated::journal::ToolCallStatus::Completed
                })
                .map(|record| record.tool_call_id.as_str().to_string())
                .collect();
            service.completed_tool_calls = service.completed_tool_call_ids.len() as u32;
            if let Some(state) = service.load_durable_state()? {
                service.patch_proposals = state
                    .patch_proposals
                    .into_iter()
                    .map(|proposal| (proposal.patch_id.as_str().to_string(), proposal))
                    .collect();
                service.last_verification = state.last_verification;
                service.last_diff = state.last_diff;
                service.completed_tool_call_ids = state.completed_tool_call_ids;
                service.provider_history = state.provider_history;
                service.completed_tool_calls = state.completed_tool_calls;
                service.consecutive_failed_verifications = state.consecutive_failed_verifications;
                service.advice_consulted_for_current_failure =
                    state.advice_consulted_for_current_failure;
                service.advisor_consultations_used = state.advisor_consultations_used;
                service.recent_read_excerpts = state.recent_read_excerpts;
                service.next_event_sequence =
                    service.next_event_sequence.max(state.next_event_sequence);
                service.corrective_turns_used = state.corrective_turns_used;
                service.json_tool_recovery_turns_remaining =
                    state.json_tool_recovery_turns_remaining;
            }
        }
        let _ = service
            .job_manager
            .recover_running_jobs()
            .map_err(|e| IntegratedError::Tool(format!("{e:?}")))?;
        Ok(service)
    }

    pub fn build_context(&self, turn_id: &str) -> Result<Value, IntegratedError> {
        let run_id = RunId::new(self.contract.task_id.clone()).map_err(IntegratedError::Tool)?;
        let mut builder = ContextBuilderV1::new(
            run_id,
            TurnId::new(turn_id).map_err(IntegratedError::Tool)?,
            ContextBudgetPolicyV1::local_default(),
        );
        builder
            .add_contract_summary(&self.contract)
            .map_err(|e| IntegratedError::Tool(format!("{e:?}")))?;
        Ok(serde_json::to_value(
            builder
                .build()
                .map_err(|e| IntegratedError::Tool(format!("{e:?}")))?,
        )?)
    }

    pub async fn execute_tool_call(
        &mut self,
        call: &NormalizedToolCallV1,
    ) -> Result<IntegratedToolResultV1, IntegratedError> {
        if self
            .completed_tool_call_ids
            .iter()
            .any(|id| id == call.tool_call_id.as_str())
        {
            return Err(IntegratedError::Tool(format!(
                "tool call {} already completed",
                call.tool_call_id.as_str()
            )));
        }
        let run_id = RunId::new(self.contract.task_id.clone()).map_err(IntegratedError::Tool)?;
        let mutation_kind = self
            .tool_definitions()
            .into_iter()
            .find(|tool| tool.name == call.tool_name)
            .map(|tool| tool.mutation_kind)
            .unwrap_or(ToolMutationKind::Mutating);
        self.enforce_tool_policy(call, &mutation_kind)?;
        self.journal
            .record_tool_call_requested(ToolCallRecordV1 {
                schema_version: EXECUTION_CONTRACT_SCHEMA_VERSION,
                run_id: run_id.clone(),
                tool_call_id: call.tool_call_id.clone(),
                tool_name: call.tool_name.clone(),
                request_hash: stable_text_hash(&serde_json::to_string(&call.arguments)?),
                arguments_hash: call.arguments_hash.clone(),
                mutation_kind: mutation_kind.clone(),
                status: ToolCallStatus::Requested,
                result_hash: None,
                outcome_summary: None,
            })
            .map_err(|e| IntegratedError::Journal(format!("{e:?}")))?;
        self.journal
            .transition_tool_call(
                &run_id,
                &call.tool_call_id,
                ToolCallStatus::PolicyAllowed,
                None,
                None,
            )
            .map_err(|e| IntegratedError::Journal(format!("{e:?}")))?;
        self.journal
            .transition_tool_call(
                &run_id,
                &call.tool_call_id,
                ToolCallStatus::Executing,
                None,
                None,
            )
            .map_err(|e| IntegratedError::Journal(format!("{e:?}")))?;
        let mut effective_args = call.arguments.clone();
        if call.tool_name == "patch.apply" {
            effective_args["toolCallId"] = json!(call.tool_call_id.as_str());
        }
        let result = match self.execute_tool(&call.tool_name, &effective_args).await {
            Ok(result) => result,
            Err(error) => {
                let status = if mutation_kind == ToolMutationKind::Mutating {
                    ToolCallStatus::OutcomeUnknown
                } else {
                    ToolCallStatus::Failed
                };
                let summary = format!("{error:?}");
                self.journal
                    .transition_tool_call(
                        &run_id,
                        &call.tool_call_id,
                        status,
                        None,
                        Some(summary.clone()),
                    )
                    .map_err(|e| IntegratedError::Journal(format!("{e:?}")))?;
                self.persist_durable_state()?;
                return Err(IntegratedError::Tool(summary));
            }
        };
        self.journal
            .transition_tool_call(
                &run_id,
                &call.tool_call_id,
                ToolCallStatus::Completed,
                Some(stable_text_hash(&result.bounded_text)),
                Some(result.summary.clone()),
            )
            .map_err(|e| IntegratedError::Journal(format!("{e:?}")))?;
        self.completed_tool_call_ids
            .push(call.tool_call_id.as_str().to_string());
        self.completed_tool_calls += 1;
        self.provider_history.push(ProviderMessageV1 {
            role: "tool".into(),
            content: result.bounded_text.clone(),
            tool_call_id: Some(call.tool_call_id.as_str().to_string()),
            tool_name: Some(call.tool_name.clone()),
        });
        self.push_final_reasoning_instruction_after_diff(call)?;
        self.append_event(
            LifecycleEvent::Delta,
            EventPayloadV1::Delta {
                text: format!("tool {} completed: {}", call.tool_name, result.summary),
            },
        )?;
        self.persist_durable_state()?;
        Ok(result)
    }

    fn push_final_reasoning_instruction_after_diff(
        &mut self,
        call: &NormalizedToolCallV1,
    ) -> Result<(), IntegratedError> {
        if call.tool_name != "diff.actual" {
            return Ok(());
        }
        if !matches!(
            self.last_verification
                .as_ref()
                .map(|summary| &summary.status),
            Some(VerificationStatusV1::Passed)
        ) || self.last_diff.is_none()
        {
            return Ok(());
        }
        self.provider_history.push(ProviderMessageV1 {
            role: "user".into(),
            content: "CatDesk has authoritative evidence: verify.run passed and diff.actual captured the final diff. Do not call another tool. Provide the final concise completion claim now.".into(),
            tool_call_id: None,
            tool_name: None,
        });
        Ok(())
    }

    pub async fn execute_tool(
        &mut self,
        tool_name: &str,
        args: &Value,
    ) -> Result<IntegratedToolResultV1, IntegratedError> {
        match tool_name {
            "read" => self.tool_read(args),
            "search" => self.tool_search(args),
            "patch.preview" => self.tool_patch_preview(args),
            "patch.apply" => self.tool_patch_apply(args),
            "patch.compare" => self.tool_patch_compare(args),
            "diff.actual" => self.tool_diff_actual(args),
            "verify.run" => self.tool_verify(args).await,
            "job.start" => self.tool_job_start(args).await,
            "job.status" => self.tool_job_status(args),
            "job.poll" => self.tool_job_poll(args),
            "job.cancel" => self.tool_job_cancel(args).await,
            _ => Err(IntegratedError::Tool(format!("unknown tool {tool_name}"))),
        }
    }

    pub fn final_review(&self) -> Result<FinalReviewPackageV1, IntegratedError> {
        let coordinator = RunCoordinator::new();
        let verification = self
            .last_verification
            .clone()
            .ok_or_else(|| IntegratedError::Verification("verification has not run".into()))?;
        let diff = self.last_diff.as_ref().ok_or_else(|| {
            IntegratedError::Verification("actual diff has not been captured".into())
        })?;
        let result = crate::delegated::contracts::FinalRunResultV1 {
            schema_version: EXECUTION_CONTRACT_SCHEMA_VERSION,
            status: RunState::CompletedVerified,
            objective: self.contract.objective.clone(),
            branch: self.contract.feature_branch.clone(),
            files_changed: diff.paths.clone(),
            verification: verification.summary.clone(),
            diff_artifacts: Vec::new(),
            provider_history: vec![self.config.model_id.clone()],
            unresolved_warnings: Vec::new(),
            final_recommendation: "ready for human review".into(),
        };
        coordinator
            .final_review_package(
                RunId::new(self.contract.task_id.clone()).map_err(IntegratedError::Tool)?,
                result,
                verification,
                format!("{} bytes of authoritative diff", diff.diff.len()),
            )
            .map_err(|e| IntegratedError::Verification(format!("{e:?}")))
    }

    pub fn events(
        &self,
        after_sequence: u64,
        limit: usize,
    ) -> Result<Vec<EventEnvelopeV1>, IntegratedError> {
        let run_id = RunId::new(self.contract.task_id.clone()).map_err(IntegratedError::Tool)?;
        self.journal
            .poll_events(
                &run_id,
                EventCursor {
                    after_sequence,
                    limit,
                },
            )
            .map_err(|e| IntegratedError::Journal(format!("{e:?}")))
    }

    pub fn maybe_consult_advisor_after_failed_repairs<A: AdvisorAdapter>(
        &mut self,
        advisor: &mut A,
        specific_question: impl Into<String>,
    ) -> Result<Option<AdviceResponseV1>, IntegratedError> {
        if self.consecutive_failed_verifications < 2 || self.advice_consulted_for_current_failure {
            return Ok(None);
        }
        let response = self.consult_advisor(
            advisor,
            AdviceTrigger::TwoFailedBoundedRepairAttempts {
                failed_attempts: self.consecutive_failed_verifications,
            },
            specific_question,
        )?;
        self.advice_consulted_for_current_failure = true;
        self.persist_durable_state()?;
        Ok(Some(response))
    }

    async fn maybe_consult_configured_advisor_after_failed_repairs(
        &mut self,
        cancel_requested: Arc<AtomicBool>,
    ) -> Result<(), IntegratedError> {
        let Some(advisor_config) = self.config.advisor.clone() else {
            return Ok(());
        };
        if !advisor_config.enabled
            || advisor_config.advisor_id != "deepseek-web"
            || advisor_config.disclosure_classification
                != AdviceDisclosureClassification::RemoteAllowed
            || self.consecutive_failed_verifications < 2
            || self.advice_consulted_for_current_failure
            || self.advisor_consultations_used >= advisor_config.maximum_consultations_per_run
        {
            return Ok(());
        }

        self.advice_consulted_for_current_failure = true;
        self.advisor_consultations_used = self.advisor_consultations_used.saturating_add(1);
        self.persist_durable_state()?;

        let Some(_runtime) = advisor_config.local_runtime.clone() else {
            self.record_advisor_unavailable_context(
                AdvisorStatus::Unavailable,
                "advisor local sidecar configuration is unavailable",
                advisor_config.advice_required,
            )?;
            return Ok(());
        };

        let mut advisor = self.configured_advisor_process()?;
        let contract = self.contract.clone();
        let journal = self.journal.clone();
        let worker_session_id = self.worker_session_id.clone();
        let draft = self.advice_draft(
            "Two consecutive bounded verification attempts failed. Provide advisory-only diagnosis and next-step suggestions based only on the bounded context supplied.".into(),
            advisor_config.maximum_response_length,
            advisor_config.disclosure_classification.clone(),
        );
        let broker_config = AdviceBrokerConfigV1 {
            advisor_enabled: true,
            allowed_advisor_id: Some(advisor_config.advisor_id.clone()),
            default_maximum_response_length: advisor_config.maximum_response_length,
            timeout: Duration::from_secs(180),
            ..AdviceBrokerConfigV1::remote_advisory_default()
        };
        let cancel_for_task = cancel_requested.clone();
        let consultation = tokio::task::spawn_blocking(move || {
            let mut broker = AdvisorBroker::new(&mut advisor, journal, broker_config)
                .with_worker_session_id(worker_session_id);
            let result = broker.consult_cancellable(
                &contract,
                draft,
                AdviceTrigger::TwoFailedBoundedRepairAttempts { failed_attempts: 2 },
                &cancel_for_task,
            );
            if cancel_for_task.load(Ordering::SeqCst) {
                let _ = advisor.shutdown(Duration::from_secs(5));
            }
            (advisor, result)
        });

        let (advisor, response) = consultation
            .await
            .map_err(|error| IntegratedError::Tool(format!("advisor task failed: {error}")))?;
        self.advisor_process = Some(advisor);
        if cancel_requested.load(Ordering::SeqCst) {
            self.record_advisor_unavailable_context_inner(
                AdvisorStatus::Cancelled,
                "advisor consultation was cancelled with the CatDesk run",
                advisor_config.advice_required,
                false,
            )?;
            return Ok(());
        }

        match response {
            Ok(response) => {
                if response.status == AdvisorStatus::Cancelled {
                    return self.record_advisor_unavailable_context_inner(
                        AdvisorStatus::Cancelled,
                        "advisor consultation was cancelled with the CatDesk run",
                        advisor_config.advice_required,
                        false,
                    );
                }
                self.refresh_next_event_sequence()?;
                self.deliver_advisor_response(response, advisor_config.advice_required)
            }
            Err(error) => self.record_advisor_unavailable_context(
                AdvisorStatus::Failed,
                &format!("advisor unavailable: {error:?}"),
                advisor_config.advice_required,
            ),
        }
    }

    pub fn consult_advisor<A: AdvisorAdapter>(
        &mut self,
        advisor: &mut A,
        trigger: AdviceTrigger,
        specific_question: impl Into<String>,
    ) -> Result<AdviceResponseV1, IntegratedError> {
        let advisor_config =
            self.config.advisor.clone().ok_or_else(|| {
                IntegratedError::Tool("advisor integration is not configured".into())
            })?;
        if !advisor_config.enabled {
            return Err(IntegratedError::Tool(
                "advisor integration is disabled".into(),
            ));
        }
        if advisor_config.advisor_id != advisor.advisor_id() {
            return Err(IntegratedError::Tool(format!(
                "advisor {} is not explicitly configured",
                advisor.advisor_id()
            )));
        }
        let draft = self.advice_draft(
            specific_question.into(),
            advisor_config.maximum_response_length,
            advisor_config.disclosure_classification.clone(),
        );
        let mut broker = AdvisorBroker::new(
            advisor,
            self.journal.clone(),
            AdviceBrokerConfigV1 {
                advisor_enabled: true,
                allowed_advisor_id: Some(advisor_config.advisor_id.clone()),
                timeout: Duration::from_secs(180),
                default_maximum_response_length: advisor_config.maximum_response_length,
                ..AdviceBrokerConfigV1::remote_advisory_default()
            },
        )
        .with_worker_session_id(self.worker_session_id.clone());
        let response = broker
            .consult(&self.contract, draft, trigger)
            .map_err(|error| IntegratedError::Tool(format!("{error:?}")))?;
        match response.status {
            AdvisorStatus::Completed => {
                let context = AdvisorBroker::<A>::untrusted_context_for_qwen(&response);
                self.provider_history.push(context);
                let run_id = self.run_id()?;
                broker
                    .record_advice_delivered_to_worker(&run_id, &response)
                    .map_err(|error| IntegratedError::Tool(format!("{error:?}")))?;
                self.refresh_next_event_sequence()?;
            }
            _ => {
                self.provider_history.push(ProviderMessageV1 {
                    role: "user".into(),
                    content: format!(
                        "<untrusted_advisor_context>\nAdvisor returned status {:?}; continue locally unless supervisor policy requires escalation.\n</untrusted_advisor_context>",
                        response.status
                    ),
                    tool_call_id: None,
                    tool_name: None,
                });
            }
        }
        self.persist_durable_state()?;
        Ok(response)
    }

    fn configured_advisor_process(&mut self) -> Result<DeepSeekProcessAdvisor, IntegratedError> {
        if let Some(advisor) = self.advisor_process.take() {
            return Ok(advisor);
        }
        let advisor_config =
            self.config.advisor.as_ref().ok_or_else(|| {
                IntegratedError::Tool("advisor integration is not configured".into())
            })?;
        let runtime = advisor_config.local_runtime.as_ref().ok_or_else(|| {
            IntegratedError::Tool("advisor local runtime is not configured".into())
        })?;
        let mut config = DeepSeekProcessAdvisorConfig::new(
            runtime.python_executable.clone(),
            runtime.adapter_script.clone(),
            runtime.profile_dir.clone(),
        );
        config.selectors_path = runtime.selectors_path.clone();
        config.headed = runtime.headed;
        config.allow_env_login = runtime.allow_env_login;
        Ok(DeepSeekProcessAdvisor::new(config))
    }

    fn deliver_advisor_response(
        &mut self,
        response: AdviceResponseV1,
        advice_required: bool,
    ) -> Result<(), IntegratedError> {
        match response.status {
            AdvisorStatus::Completed => {
                let context =
                    AdvisorBroker::<DeepSeekProcessAdvisor>::untrusted_context_for_qwen(&response);
                self.provider_history.push(context);
                let run_id = self.run_id()?;
                let mut fake = crate::delegated::advisor::FakeAdvisor::success();
                let mut broker = AdvisorBroker::new(
                    &mut fake,
                    self.journal.clone(),
                    AdviceBrokerConfigV1::remote_advisory_default(),
                )
                .with_worker_session_id(self.worker_session_id.clone());
                broker
                    .record_advice_delivered_to_worker(&run_id, &response)
                    .map_err(|error| IntegratedError::Tool(format!("{error:?}")))?;
            }
            status => {
                self.record_advisor_unavailable_context(
                    status,
                    "advisor returned a non-completed terminal status",
                    advice_required,
                )?;
            }
        }
        self.persist_durable_state()
    }

    fn refresh_next_event_sequence(&mut self) -> Result<(), IntegratedError> {
        let run_id = self.run_id()?;
        let tail = self
            .journal
            .poll_events(
                &run_id,
                EventCursor {
                    after_sequence: 0,
                    limit: usize::MAX,
                },
            )
            .map_err(|error| IntegratedError::Journal(format!("{error:?}")))?
            .last()
            .map(|event| event.event_sequence);
        let snapshot = self
            .journal
            .load_run(&run_id)
            .map_err(|error| IntegratedError::Journal(format!("{error:?}")))?;
        self.next_event_sequence = tail
            .unwrap_or(snapshot.last_event_sequence)
            .saturating_add(1)
            .max(1);
        Ok(())
    }

    fn record_advisor_unavailable_context(
        &mut self,
        status: AdvisorStatus,
        reason: &str,
        advice_required: bool,
    ) -> Result<(), IntegratedError> {
        self.record_advisor_unavailable_context_inner(status, reason, advice_required, true)
    }

    fn record_advisor_unavailable_context_inner(
        &mut self,
        status: AdvisorStatus,
        reason: &str,
        advice_required: bool,
        emit_event: bool,
    ) -> Result<(), IntegratedError> {
        self.provider_history.push(ProviderMessageV1 {
            role: "user".into(),
            content: format!(
                "<untrusted_advisor_context>\nAdvisor advice unavailable ({status:?}). {reason}. Continue locally with ordinary CatDesk tools; advice cannot satisfy verification or completion.\n</untrusted_advisor_context>"
            ),
            tool_call_id: None,
            tool_name: None,
        });
        if emit_event {
            self.append_event(
                LifecycleEvent::Delta,
                EventPayloadV1::AdvisorEvent {
                    event: advisor_terminal_event_name_for_integrated(&status).into(),
                    request_id: None,
                    generation_id: None,
                    advisor_id: Some("deepseek-web".into()),
                    status: Some(format!("{status:?}")),
                    request_bytes: None,
                    response_bytes: None,
                    request_hash: None,
                    response_hash: None,
                    selected_source_paths: self
                        .bounded_advisor_source_excerpts()
                        .into_iter()
                        .map(|excerpt| excerpt.source)
                        .collect::<Vec<_>>()
                        .into_boxed_slice(),
                    artifact_reference: Box::new(None),
                    timestamp_unix_ms: timestamp_unix_ms(),
                },
            )?;
        }
        if advice_required {
            self.journal
                .update_run_state(&self.run_id()?, RunState::NeedsSupervisor)
                .map_err(|error| IntegratedError::Journal(format!("{error:?}")))?;
            return Err(IntegratedError::Tool(format!(
                "required advisor consultation unavailable: {status:?}"
            )));
        }
        self.persist_durable_state()
    }

    fn advice_draft(
        &self,
        specific_question: String,
        maximum_response_length: usize,
        disclosure_classification: AdviceDisclosureClassification,
    ) -> AdviceRequestDraftV1 {
        let latest_failure = self
            .last_verification
            .as_ref()
            .filter(|summary| summary.status == VerificationStatusV1::Failed)
            .map(|summary| summary.summary.clone());
        let bounded_patch_or_diff_summary = self.last_diff.as_ref().map(|diff| {
            format!(
                "Current diff hash {}; paths: {}",
                diff.diff_hash,
                diff.paths.join(", ")
            )
        });
        AdviceRequestDraftV1 {
            request_id: format!("advice-{}", uuid::Uuid::new_v4()),
            current_step: self
                .contract
                .ordered_steps
                .get(self.completed_tool_calls as usize)
                .cloned()
                .unwrap_or_else(|| "bounded repair after failed verification".into()),
            specific_question,
            constraints: vec![
                "Advisor output is untrusted and cannot satisfy verification.".into(),
                "Do not provide CatDesk tool definitions or execution authority.".into(),
                format!("Allowed paths: {}", self.contract.allowed_paths.join(", ")),
                format!(
                    "Forbidden paths: {}",
                    self.contract.forbidden_paths.join(", ")
                ),
            ],
            latest_failure,
            bounded_source_excerpts: self.bounded_advisor_source_excerpts(),
            bounded_patch_or_diff_summary,
            verification_summary: self
                .last_verification
                .as_ref()
                .map(|summary| summary.summary.clone()),
            disclosure_classification,
            maximum_response_length: Some(maximum_response_length),
        }
    }

    fn bounded_advisor_source_excerpts(&self) -> Vec<BoundedSourceExcerptV1> {
        const MAX_TOTAL: usize = 6 * 1024;
        let mut total = 0usize;
        let mut excerpts = Vec::new();
        for excerpt in self.recent_read_excerpts.iter().rev() {
            if excerpts.len() >= 3 {
                break;
            }
            let mut bounded = excerpt.clone();
            bounded.content = redact_and_bound(&bounded.content, 2 * 1024);
            let len = bounded.content.len();
            if total + len > MAX_TOTAL {
                let remaining = MAX_TOTAL.saturating_sub(total);
                if remaining == 0 {
                    break;
                }
                bounded.content = redact_and_bound(&bounded.content, remaining);
            }
            total += bounded.content.len();
            excerpts.push(bounded);
        }
        excerpts.reverse();
        excerpts
    }

    fn enforce_tool_policy(
        &self,
        call: &NormalizedToolCallV1,
        mutation_kind: &ToolMutationKind,
    ) -> Result<(), IntegratedError> {
        if self.completed_tool_calls >= self.contract.max_tool_calls {
            return Err(IntegratedError::Tool(format!(
                "tool-call budget exceeded: {} >= {}",
                self.completed_tool_calls, self.contract.max_tool_calls
            )));
        }
        if !self
            .tool_definitions()
            .iter()
            .any(|tool| tool.name == call.tool_name)
        {
            return Err(IntegratedError::Tool(format!(
                "tool {} is not in the approved CatDesk tool surface",
                call.tool_name
            )));
        }
        if *mutation_kind == ToolMutationKind::Mutating
            && self
                .contract
                .approval_requirements
                .iter()
                .any(|requirement| {
                    requirement.required
                        && matches!(
                            requirement.kind,
                            ApprovalRequirementKind::ToolException
                                | ApprovalRequirementKind::UnrestrictedShell
                        )
                })
        {
            return Err(IntegratedError::Tool(
                "mutating tool requires explicit supervisor approval".into(),
            ));
        }
        Ok(())
    }

    fn persist_durable_state(&self) -> Result<(), IntegratedError> {
        let run_id = RunId::new(self.contract.task_id.clone()).map_err(IntegratedError::Tool)?;
        let state = IntegratedDurableStateV1 {
            schema_version: EXECUTION_CONTRACT_SCHEMA_VERSION,
            run_id,
            patch_proposals: self.patch_proposals.values().cloned().collect(),
            last_verification: self.last_verification.clone(),
            last_diff: self.last_diff.clone(),
            completed_tool_call_ids: self.completed_tool_call_ids.clone(),
            provider_history: self.provider_history.clone(),
            completed_tool_calls: self.completed_tool_calls,
            consecutive_failed_verifications: self.consecutive_failed_verifications,
            advice_consulted_for_current_failure: self.advice_consulted_for_current_failure,
            advisor_consultations_used: self.advisor_consultations_used,
            recent_read_excerpts: self.recent_read_excerpts.clone(),
            next_event_sequence: self.next_event_sequence,
            corrective_turns_used: self.corrective_turns_used,
            json_tool_recovery_turns_remaining: self.json_tool_recovery_turns_remaining,
        };
        let path = self.durable_state_path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension("tmp");
        fs::write(&tmp, serde_json::to_string_pretty(&state)? + "\n")?;
        fs::rename(tmp, path)?;
        Ok(())
    }

    fn load_durable_state(&self) -> Result<Option<IntegratedDurableStateV1>, IntegratedError> {
        let path = self.durable_state_path()?;
        if !path.exists() {
            return Ok(None);
        }
        Ok(Some(serde_json::from_str(&fs::read_to_string(path)?)?))
    }

    fn durable_state_path(&self) -> Result<PathBuf, IntegratedError> {
        let run_id = RunId::new(self.contract.task_id.clone()).map_err(IntegratedError::Tool)?;
        Ok(self
            .config
            .journal_root
            .join("integrated_state")
            .join(format!("{}.json", run_id.as_str().replace(':', "_"))))
    }

    fn snapshot(&mut self, checkpoint: &str) -> Result<IntegratedRunSnapshotV1, IntegratedError> {
        let run_id = RunId::new(self.contract.task_id.clone()).map_err(IntegratedError::Tool)?;
        self.supervisor
            .set_checkpoint(&run_id, checkpoint.into())
            .map_err(|e| IntegratedError::Supervisor(format!("{e:?}")))?;
        Ok(IntegratedRunSnapshotV1 {
            run_id,
            state: RunState::Running,
            checkpoint: checkpoint.into(),
            completed_tool_call_ids: self.completed_tool_call_ids.clone(),
        })
    }

    fn append_event(
        &mut self,
        lifecycle_event: LifecycleEvent,
        payload: EventPayloadV1,
    ) -> Result<(), IntegratedError> {
        self.refresh_next_event_sequence()?;
        let item_id = ItemId::new(format!("item-{}", self.next_event_sequence))
            .map_err(IntegratedError::Tool)?;
        let event = EventEnvelopeV1 {
            schema_version: EXECUTION_CONTRACT_SCHEMA_VERSION,
            event_sequence: self.next_event_sequence,
            run_id: RunId::new(self.contract.task_id.clone()).map_err(IntegratedError::Tool)?,
            worker_session_id: Some(self.worker_session_id.clone()),
            turn_id: self.current_turn_id.clone(),
            item_id: Some(item_id),
            lifecycle_event,
            request_hash: "fnv1a64:integrated".into(),
            result_hash: None,
            payload,
        }
        .with_result_hash()
        .map_err(IntegratedError::Tool)?;
        self.next_event_sequence += 1;
        self.journal
            .append_event(&event)
            .map_err(|e| IntegratedError::Journal(format!("{e:?}")))?;
        self.supervisor.append_event(event);
        Ok(())
    }

    fn tool_read(&mut self, args: &Value) -> Result<IntegratedToolResultV1, IntegratedError> {
        let path = string_arg(args, "path")?;
        let output = workspace_tools::read_file(&self.workspace_root.display().to_string(), &path)
            .map_err(IntegratedError::Tool)?;
        self.record_recent_read_excerpt(&output.path, &output.text)?;
        Ok(tool_result(
            "read",
            format!("read {} bytes from {}", output.bytes, output.path),
            serde_json::to_value(&output)?,
            output.render_text(),
        ))
    }

    fn record_recent_read_excerpt(
        &mut self,
        path: &str,
        content: &str,
    ) -> Result<(), IntegratedError> {
        let normalized = path.replace('\\', "/");
        if self
            .contract
            .forbidden_paths
            .iter()
            .any(|forbidden| path_is_under_contract_path(&normalized, forbidden))
        {
            return Ok(());
        }
        if !self
            .contract
            .allowed_paths
            .iter()
            .any(|allowed| path_is_under_contract_path(&normalized, allowed))
        {
            return Ok(());
        }
        let excerpt = BoundedSourceExcerptV1 {
            source: normalized.clone(),
            summary: format!("Recent successful file.read result for {normalized}"),
            content: redact_and_bound(content, 2 * 1024),
        };
        self.recent_read_excerpts
            .retain(|existing| existing.source != excerpt.source);
        self.recent_read_excerpts.push(excerpt);
        while self.recent_read_excerpts.len() > 3 {
            self.recent_read_excerpts.remove(0);
        }
        Ok(())
    }

    fn tool_search(&self, args: &Value) -> Result<IntegratedToolResultV1, IntegratedError> {
        let pattern = string_arg(args, "pattern")?;
        let path = args.get("path").and_then(Value::as_str);
        let output = workspace_tools::search_text(
            &self.workspace_root.display().to_string(),
            workspace_tools::SearchTextOptions {
                pattern: &pattern,
                path,
                glob: None,
                fixed_strings: args
                    .get("fixedStrings")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                case_insensitive: false,
                context: Some(1),
                before: None,
                after: None,
                max_matches: Some(50),
                max_matches_per_file: Some(10),
                include_hidden: false,
                no_ignore: false,
            },
        )
        .map_err(IntegratedError::Tool)?;
        Ok(tool_result(
            "search",
            format!("{} matches for {}", output.match_count, output.pattern),
            serde_json::to_value(&output)?,
            output.render_text(),
        ))
    }

    fn tool_patch_preview(
        &mut self,
        args: &Value,
    ) -> Result<IntegratedToolResultV1, IntegratedError> {
        let proposal = self.patch_from_args(args)?;
        let engine = PatchEngine::new(&self.workspace_root, &self.contract)?;
        let preview = engine.preview(&proposal)?;
        let run_id = RunId::new(self.contract.task_id.clone()).map_err(IntegratedError::Tool)?;
        self.journal
            .record_patch_proposal(PatchProposalRecordV1 {
                schema_version: EXECUTION_CONTRACT_SCHEMA_VERSION,
                patch_id: proposal.patch_id.clone(),
                parent_patch_id: proposal.parent_patch_id.clone(),
                run_id,
                turn_id: proposal.turn_id.clone(),
                base_snapshot_hash: proposal.base_snapshot_hash.clone(),
                target_paths: proposal.target_paths.clone(),
                expected_preimage_hashes: proposal
                    .expected_preimage_hashes
                    .iter()
                    .map(|hash| hash.hash.clone())
                    .collect(),
                proposed_diff_hash: stable_text_hash(&preview.preview_diff),
                model_rationale: proposal.model_rationale.clone(),
            })
            .map_err(|e| IntegratedError::Journal(format!("{e:?}")))?;
        self.patch_proposals
            .insert(proposal.patch_id.as_str().to_string(), proposal);
        self.persist_durable_state()?;
        Ok(tool_result(
            "patch.preview",
            format!("{:?}", preview.status),
            serde_json::to_value(&preview)?,
            preview.preview_diff.clone(),
        ))
    }

    fn tool_patch_apply(
        &mut self,
        args: &Value,
    ) -> Result<IntegratedToolResultV1, IntegratedError> {
        let patch_id = string_arg(args, "patchId")?;
        let proposal = self
            .patch_proposals
            .get(&patch_id)
            .cloned()
            .ok_or_else(|| IntegratedError::Tool(format!("unknown patch {patch_id}")))?;
        let engine = PatchEngine::new(&self.workspace_root, &self.contract)?;
        let result = engine.apply(&proposal)?;
        let tool_call_id = args
            .get("toolCallId")
            .and_then(Value::as_str)
            .unwrap_or("direct-patch-apply");
        let artifact_id = crate::delegated::contracts::ArtifactId::new(format!(
            "diff-{}",
            result.patch_id.as_str().replace(':', "_")
        ))
        .map_err(IntegratedError::Tool)?;
        self.journal
            .record_patch_application(
                &RunId::new(self.contract.task_id.clone()).map_err(IntegratedError::Tool)?,
                PatchApplicationRecordV1 {
                    schema_version: EXECUTION_CONTRACT_SCHEMA_VERSION,
                    patch_id: result.patch_id.clone(),
                    tool_call_id: ToolCallId::new(tool_call_id).map_err(IntegratedError::Tool)?,
                    status: PatchApplicationStatus::Applied,
                    actual_paths_changed: result.actual_paths_changed.clone(),
                    before_hashes: result
                        .before_hashes
                        .iter()
                        .map(|hash| hash.hash.clone())
                        .collect(),
                    after_hashes: result
                        .after_hashes
                        .iter()
                        .map(|hash| hash.hash.clone())
                        .collect(),
                    result_hash: stable_text_hash(&result.actual_diff),
                    diff_artifact_id: artifact_id,
                },
            )
            .map_err(|e| IntegratedError::Journal(format!("{e:?}")))?;
        self.persist_durable_state()?;
        Ok(tool_result(
            "patch.apply",
            format!("{:?}", result.status),
            serde_json::to_value(&result)?,
            result.actual_diff.clone(),
        ))
    }

    fn tool_patch_compare(&self, args: &Value) -> Result<IntegratedToolResultV1, IntegratedError> {
        let parent_id = string_arg(args, "parentPatchId")?;
        let candidate_id = string_arg(args, "candidatePatchId")?;
        let parent = self
            .patch_proposals
            .get(&parent_id)
            .ok_or_else(|| IntegratedError::Tool(format!("unknown patch {parent_id}")))?;
        let candidate = self
            .patch_proposals
            .get(&candidate_id)
            .ok_or_else(|| IntegratedError::Tool(format!("unknown patch {candidate_id}")))?;
        let comparison = compare_patches(parent, candidate);
        Ok(tool_result(
            "patch.compare",
            format!("{} modified files", comparison.files_modified.len()),
            serde_json::to_value(&comparison)?,
            serde_json::to_string_pretty(&comparison)?,
        ))
    }

    fn tool_diff_actual(
        &mut self,
        args: &Value,
    ) -> Result<IntegratedToolResultV1, IntegratedError> {
        let paths =
            string_array_arg(args, "paths").unwrap_or_else(|_| self.contract.allowed_paths.clone());
        let engine = PatchEngine::new(&self.workspace_root, &self.contract)?;
        let diff = engine.actual_git_diff(&paths)?;
        self.last_diff = Some(diff.clone());
        self.persist_durable_state()?;
        Ok(tool_result(
            "diff.actual",
            format!("captured diff hash {}", diff.diff_hash),
            serde_json::to_value(&diff)?,
            diff.diff.clone(),
        ))
    }

    async fn tool_verify(
        &mut self,
        args: &Value,
    ) -> Result<IntegratedToolResultV1, IntegratedError> {
        let timeout = args
            .get("timeout")
            .and_then(Value::as_u64)
            .unwrap_or(120_000)
            .clamp(30_000, 300_000);
        let output = verification::verify_project_with_timeout(
            &self.workspace_root.display().to_string(),
            timeout,
        )
        .await
        .map_err(IntegratedError::Verification)?;
        let summary = VerificationSummaryV1 {
            status: if output.success {
                VerificationStatusV1::Passed
            } else {
                VerificationStatusV1::Failed
            },
            command: "verify_project".into(),
            summary: output.render_text(),
        };
        match summary.status {
            VerificationStatusV1::Passed => {
                self.consecutive_failed_verifications = 0;
                self.advice_consulted_for_current_failure = false;
            }
            VerificationStatusV1::Failed | VerificationStatusV1::NotConfigured => {
                self.consecutive_failed_verifications =
                    self.consecutive_failed_verifications.saturating_add(1);
            }
        }
        self.last_verification = Some(summary.clone());
        self.persist_durable_state()?;
        Ok(tool_result(
            "verify.run",
            format!("{:?}", summary.status),
            serde_json::to_value(&output)?,
            output.render_text(),
        ))
    }

    async fn tool_job_start(
        &mut self,
        args: &Value,
    ) -> Result<IntegratedToolResultV1, IntegratedError> {
        let command = string_arg(args, "command")?;
        let cwd = self.contained_job_cwd(args.get("cwd").and_then(Value::as_str))?;
        let mut spec = JobSpecV1::new(command, cwd);
        spec.command_profile = args
            .get("commandProfile")
            .and_then(Value::as_str)
            .or_else(|| {
                self.contract
                    .allowed_command_profiles
                    .first()
                    .map(String::as_str)
            })
            .unwrap_or("default")
            .into();
        if let Some(max) = args.get("maxLogBytes").and_then(Value::as_u64) {
            spec.max_log_bytes = max;
        }
        let record = self
            .job_manager
            .start_job(spec)
            .await
            .map_err(|e| IntegratedError::Tool(format!("{e:?}")))?;
        Ok(tool_result(
            "job.start",
            format!("started {}", record.job_id.as_str()),
            serde_json::to_value(&record)?,
            format!("jobId: {}", record.job_id.as_str()),
        ))
    }

    fn contained_job_cwd(&self, cwd: Option<&str>) -> Result<PathBuf, IntegratedError> {
        let candidate = match cwd {
            Some(value) if !value.trim().is_empty() => {
                let path = Path::new(value);
                if path.is_absolute()
                    || value.contains(':')
                    || value
                        .split(['/', '\\'])
                        .any(|part| matches!(part, "" | "." | ".."))
                {
                    return Err(IntegratedError::Tool(format!(
                        "job cwd must be a contained relative path: {value}"
                    )));
                }
                self.workspace_root.join(path)
            }
            _ => self.workspace_root.clone(),
        };
        let canonical = candidate
            .canonicalize()
            .map_err(|error| IntegratedError::Tool(format!("job cwd must exist: {error}")))?;
        if !canonical.starts_with(&self.workspace_root) {
            return Err(IntegratedError::Tool(
                "job cwd escapes the contract workspace".into(),
            ));
        }
        Ok(canonical)
    }

    fn tool_job_status(&self, args: &Value) -> Result<IntegratedToolResultV1, IntegratedError> {
        let job_id = JobId::new(string_arg(args, "jobId")?)
            .map_err(|e| IntegratedError::Tool(format!("{e:?}")))?;
        let record = self
            .job_manager
            .get_job(&job_id)
            .map_err(|e| IntegratedError::Tool(format!("{e:?}")))?;
        Ok(tool_result(
            "job.status",
            format!("{:?}", record.status),
            serde_json::to_value(&record)?,
            serde_json::to_string_pretty(&record)?,
        ))
    }

    fn tool_job_poll(&self, args: &Value) -> Result<IntegratedToolResultV1, IntegratedError> {
        let job_id = JobId::new(string_arg(args, "jobId")?)
            .map_err(|e| IntegratedError::Tool(format!("{e:?}")))?;
        let stream = match args
            .get("stream")
            .and_then(Value::as_str)
            .unwrap_or("stdout")
        {
            "stderr" => LogStream::Stderr,
            _ => LogStream::Stdout,
        };
        let offset = args.get("offset").and_then(Value::as_u64).unwrap_or(0);
        let max_bytes = args
            .get("maxBytes")
            .and_then(Value::as_u64)
            .unwrap_or(16 * 1024) as usize;
        let poll = self
            .job_manager
            .poll_log(&job_id, stream, offset, max_bytes)
            .map_err(|e| IntegratedError::Tool(format!("{e:?}")))?;
        Ok(tool_result(
            "job.poll",
            format!("{} bytes", poll.text.len()),
            serde_json::to_value(json!({
                "text": poll.text,
                "offset": poll.offset,
                "nextOffset": poll.next_offset,
                "truncated": poll.truncated,
                "rotated": poll.rotated,
                "localPath": poll.local_path,
            }))?,
            "bounded job log returned in payload".into(),
        ))
    }

    async fn tool_job_cancel(
        &mut self,
        args: &Value,
    ) -> Result<IntegratedToolResultV1, IntegratedError> {
        let job_id = JobId::new(string_arg(args, "jobId")?)
            .map_err(|e| IntegratedError::Tool(format!("{e:?}")))?;
        let record = self
            .job_manager
            .cancel_job(&job_id)
            .await
            .map_err(|e| IntegratedError::Tool(format!("{e:?}")))?;
        Ok(tool_result(
            "job.cancel",
            format!("{:?}", record.status),
            serde_json::to_value(&record)?,
            format!("cancelled {}", record.job_id.as_str()),
        ))
    }

    fn patch_from_args(&self, args: &Value) -> Result<PatchProposalV1, IntegratedError> {
        let patch_id = string_arg(args, "patchId")?;
        let parent_patch_id = args
            .get("parentPatchId")
            .and_then(Value::as_str)
            .map(PatchId::new)
            .transpose()
            .map_err(IntegratedError::Tool)?;
        let operations = args
            .get("operations")
            .and_then(Value::as_array)
            .ok_or_else(|| IntegratedError::Tool("operations array is required".into()))?
            .iter()
            .map(|operation| {
                Ok(ReplaceOperationV1 {
                    path: string_arg(operation, "path")?,
                    old: string_arg(operation, "old")?,
                    new: string_arg(operation, "new")?,
                })
            })
            .collect::<Result<Vec<_>, IntegratedError>>()?;
        let target_paths = args
            .get("targetPaths")
            .and_then(Value::as_array)
            .map(|_| string_array_arg(args, "targetPaths"))
            .transpose()?
            .unwrap_or_else(|| {
                let mut paths = operations
                    .iter()
                    .map(|operation| operation.path.clone())
                    .collect::<Vec<_>>();
                paths.sort();
                paths.dedup();
                paths
            });
        let expected_preimage_hashes = target_paths
            .iter()
            .map(|path| {
                let text = fs::read_to_string(self.workspace_root.join(path))?;
                Ok(FileHashV1 {
                    path: path.clone(),
                    hash: stable_text_hash(&text),
                })
            })
            .collect::<Result<Vec<_>, IntegratedError>>()?;
        let mut base_snapshot = String::new();
        for path in &target_paths {
            let text = fs::read_to_string(self.workspace_root.join(path))?;
            base_snapshot.push_str(path);
            base_snapshot.push('\0');
            base_snapshot.push_str(&text);
            base_snapshot.push('\0');
        }
        Ok(PatchProposalV1 {
            schema_version: EXECUTION_CONTRACT_SCHEMA_VERSION,
            patch_id: PatchId::new(patch_id).map_err(IntegratedError::Tool)?,
            parent_patch_id,
            run_id: RunId::new(self.contract.task_id.clone()).map_err(IntegratedError::Tool)?,
            turn_id: TurnId::new(format!("turn-{}", self.next_event_sequence))
                .map_err(IntegratedError::Tool)?,
            base_snapshot_hash: stable_text_hash(&base_snapshot),
            target_paths,
            expected_preimage_hashes,
            operations,
            model_rationale: args
                .get("rationale")
                .and_then(Value::as_str)
                .unwrap_or("model proposed patch")
                .into(),
            claimed_acceptance_criteria: self.contract.acceptance_criteria.clone(),
        })
    }
}

fn tool_result(
    tool_name: &str,
    summary: String,
    payload: Value,
    bounded_text: String,
) -> IntegratedToolResultV1 {
    IntegratedToolResultV1 {
        tool_name: tool_name.into(),
        summary,
        payload,
        bounded_text: bound_text(bounded_text, 16 * 1024),
    }
}

fn bound_text(mut text: String, max_bytes: usize) -> String {
    if text.len() > max_bytes {
        text.truncate(max_bytes);
        text.push_str("\n[truncated]");
    }
    text
}

fn bounded_diagnostic_text(text: &str, max_bytes: usize) -> String {
    bound_text(redact_and_bound(text, max_bytes), max_bytes)
}

async fn wait_for_cancel(cancel_requested: Arc<AtomicBool>) {
    loop {
        if cancel_requested.load(Ordering::SeqCst) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

fn timestamp_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn advisor_terminal_event_name_for_integrated(status: &AdvisorStatus) -> &'static str {
    match status {
        AdvisorStatus::Completed => "advice_completed",
        AdvisorStatus::Cancelled => "advice_cancelled",
        AdvisorStatus::RateLimited => "advice_rate_limited",
        AdvisorStatus::TakeoverRequired | AdvisorStatus::LoginRequired => {
            "advice_takeover_required"
        }
        AdvisorStatus::TimedOut => "advice_timed_out",
        AdvisorStatus::Ready | AdvisorStatus::Unavailable | AdvisorStatus::Failed => {
            "advice_failed"
        }
    }
}

fn string_arg(args: &Value, name: &str) -> Result<String, IntegratedError> {
    args.get(name)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .ok_or_else(|| IntegratedError::Tool(format!("{name} string is required")))
}

fn string_array_arg(args: &Value, name: &str) -> Result<Vec<String>, IntegratedError> {
    args.get(name)
        .and_then(Value::as_array)
        .ok_or_else(|| IntegratedError::Tool(format!("{name} array is required")))?
        .iter()
        .map(|item| {
            item.as_str()
                .map(ToOwned::to_owned)
                .ok_or_else(|| IntegratedError::Tool(format!("{name} must contain strings")))
        })
        .collect()
}

fn worker_system_prompt() -> String {
    [
        "You are the delegated CatDesk worker.",
        "You have no direct filesystem, shell, Git, patch, process, browser, or network authority.",
        "Use only the CatDesk tool schemas provided by the host.",
        "Keep using tools until the execution contract is satisfied.",
        "When verification fails, inspect the bounded failure and propose a revised child patch.",
        "Verifier output is authoritative and overrides source comments or earlier assumptions.",
        "Do not add or edit tests unless the execution contract explicitly allows test paths.",
        "Do not claim completion until verify.run passes and diff.actual captures the authoritative diff.",
        "Return at most one tool call per turn when a tool is needed.",
    ]
    .join("\n")
}

fn path_is_under_contract_path(path: &str, scope: &str) -> bool {
    let path = path.replace('\\', "/");
    let scope = scope.trim_matches('/').replace('\\', "/");
    path == scope || path.starts_with(&format!("{scope}/"))
}

fn normalize_acceptance_criterion(criterion: &str) -> String {
    criterion
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

pub fn verification_passed_with_diff(
    claim: &str,
    verification: &VerificationSummaryV1,
    diff: &ActualDiffArtifactV1,
) -> Result<(), IntegratedError> {
    verify_model_completion_claim(
        claim,
        verification.status == VerificationStatusV1::Passed,
        diff,
    )
    .map_err(IntegratedError::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::delegated::advisor::DeepSeekProcessAdvisorConfig;
    use crate::delegated::contracts::{AdvisorDisclosureClassificationV1, AdvisorPolicyV1};
    use crate::delegated::runtime::{
        FakeProviderTurn, NormalizedProviderEventKind, OllamaAdapter, RuntimeError,
    };
    use std::process::Command;

    #[tokio::test]
    async fn fake_provider_integrated_run_recovers_without_repeating_completed_tool() {
        let root = temp_git_workspace("recover");
        fs::write(root.join("src/bug.txt"), "answer=41\n").expect("write");
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname=\"fixture\"\nversion=\"0.1.0\"\nedition=\"2021\"\n",
        )
        .expect("cargo");
        fs::create_dir_all(root.join("src")).expect("src");
        fs::write(
            root.join("src/lib.rs"),
            "pub fn answer() -> i32 {\n    41\n}\n\n#[test]\nfn test_answer() {\n    assert_eq!(answer(), 42);\n}\n",
        )
        .expect("lib");
        commit_all(&root);
        let contract = contract(&root, "run-t0023a-recover");
        let config = config(&root);
        let mut service = IntegratedDelegatedService::new(&root, contract.clone(), config.clone())
            .expect("service");
        service.start().expect("start");
        let read_call = NormalizedToolCallV1 {
            tool_call_id: crate::delegated::contracts::ToolCallId::new("tc-read").expect("id"),
            tool_name: "read".into(),
            arguments_hash: "fnv1a64:read".into(),
            arguments: json!({ "path": "src/lib.rs" }),
        };
        let read = service
            .execute_tool_call(&read_call)
            .await
            .expect("read tool");
        assert!(read.bounded_text.contains("answer"));
        println!(
            "CATDESK_RESTART_RECOVERY_BEFORE={}",
            serde_json::to_string(&json!({
                "completedTool": read_call.tool_name,
                "completedToolCallId": read_call.tool_call_id.as_str(),
                "boundedResultContainsAnswer": read.bounded_text.contains("answer"),
            }))
            .expect("recovery json")
        );

        let mut recovered =
            IntegratedDelegatedService::recover(&root, contract, config).expect("recover");
        let replay_result = recovered.execute_tool_call(&read_call).await;
        assert!(matches!(
            replay_result,
            Err(IntegratedError::Tool(ref message)) if message.contains("already completed")
        ));
        println!(
            "CATDESK_RESTART_RECOVERY_AFTER={}",
            serde_json::to_string(&json!({
                "recoveredFromJournal": true,
                "replayBlocked": matches!(
                    replay_result,
                    Err(IntegratedError::Tool(ref message)) if message.contains("already completed")
                ),
            }))
            .expect("recovery json")
        );
        let preview = recovered
            .execute_tool(
                "patch.preview",
                &json!({
                    "patchId": "patch-good",
                    "operations": [
                        { "path": "src/lib.rs", "old": "pub fn answer() -> i32 {\n    41\n}", "new": "pub fn answer() -> i32 {\n    42\n}" }
                    ]
                }),
            )
            .await
            .expect("preview");
        assert_eq!(preview.tool_name, "patch.preview");
        recovered
            .execute_tool("patch.apply", &json!({ "patchId": "patch-good" }))
            .await
            .expect("apply");
        let verification = recovered
            .execute_tool("verify.run", &json!({ "timeout": 120000 }))
            .await
            .expect("verify");
        assert_eq!(verification.summary, "Passed");
        recovered
            .execute_tool("diff.actual", &json!({ "paths": ["src/lib.rs"] }))
            .await
            .expect("diff");
        let review = recovered.final_review().expect("review");
        assert_eq!(review.final_result.status, RunState::CompletedVerified);
        assert!(!review.pushed);
        assert!(!review.merged);
        println!(
            "CATDESK_RESTART_RECOVERY_FINAL={}",
            serde_json::to_string(&json!({
                "status": format!("{:?}", review.final_result.status),
                "pushed": review.pushed,
                "merged": review.merged,
            }))
            .expect("recovery json")
        );
    }

    #[tokio::test]
    async fn integrated_job_tools_start_poll_and_cancel() {
        let root = temp_git_workspace("job-tools");
        let contract = contract(&root, "run-t0023a-jobs");
        let mut service =
            IntegratedDelegatedService::new(&root, contract, config(&root)).expect("service");
        service.start().expect("start");
        let started = service
            .execute_tool(
                "job.start",
                &json!({ "command": sleep_command(), "maxLogBytes": 4096 }),
            )
            .await
            .expect("start job");
        let job_id = started
            .payload
            .get("jobId")
            .and_then(Value::as_str)
            .expect("job id")
            .to_string();
        let status = service
            .execute_tool("job.status", &json!({ "jobId": job_id }))
            .await
            .expect("status");
        assert!(status.bounded_text.contains("RUNNING") || status.bounded_text.contains("Running"));
        let cancelled = service
            .execute_tool("job.cancel", &json!({ "jobId": job_id }))
            .await
            .expect("cancel");
        assert!(cancelled.bounded_text.contains("cancelled"));
    }

    #[tokio::test]
    async fn production_loop_acknowledges_cancel_before_provider_turn() {
        let root = temp_git_workspace("cancel-ack");
        fs::write(root.join("src/lib.rs"), "pub fn answer() -> i32 { 41 }\n").expect("lib");
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname=\"fixture\"\nversion=\"0.1.0\"\nedition=\"2021\"\n",
        )
        .expect("cargo");
        commit_all(&root);
        let contract = contract(&root, "run-cancel-ack");
        let config = config(&root);
        let cancel = Arc::new(AtomicBool::new(true));
        let mut service =
            IntegratedDelegatedService::new(&root, contract.clone(), config).expect("service");
        let result = service.run_ollama_worker_loop_with_cancel(cancel).await;
        assert!(
            matches!(result, Err(IntegratedError::Tool(message)) if message.contains("cancelled"))
        );
        let run_id = RunId::new(contract.task_id).expect("run");
        let snapshot = service.journal.load_run(&run_id).expect("snapshot");
        assert_eq!(snapshot.state, RunState::Cancelled);
    }

    #[tokio::test]
    async fn recovery_restores_patch_lineage_and_job_policy_contains_cwd() {
        let root = temp_git_workspace("recover-patches");
        fs::write(root.join("src/lib.rs"), "pub fn answer() -> i32 { 41 }\n").expect("lib");
        commit_all(&root);
        let contract = contract(&root, "run-t0023b-patch-recovery");
        let config = config(&root);
        let mut service = IntegratedDelegatedService::new(&root, contract.clone(), config.clone())
            .expect("service");
        service.start().expect("start");
        let preview_call = NormalizedToolCallV1 {
            tool_call_id: ToolCallId::new("tc-preview").expect("tool id"),
            tool_name: "patch.preview".into(),
            arguments_hash: "fnv1a64:preview".into(),
            arguments: json!({
                "patchId": "patch-restored",
                "operations": [{
                    "path": "src/lib.rs",
                    "old": "pub fn answer() -> i32 { 41 }",
                    "new": "pub fn answer() -> i32 { 42 }"
                }]
            }),
        };
        service
            .execute_tool_call(&preview_call)
            .await
            .expect("preview");

        let mut recovered =
            IntegratedDelegatedService::recover(&root, contract, config).expect("recover");
        assert!(recovered.patch_proposals.contains_key("patch-restored"));
        recovered
            .execute_tool_call(&NormalizedToolCallV1 {
                tool_call_id: ToolCallId::new("tc-apply").expect("tool id"),
                tool_name: "patch.apply".into(),
                arguments_hash: "fnv1a64:apply".into(),
                arguments: json!({ "patchId": "patch-restored" }),
            })
            .await
            .expect("apply restored patch");
        assert!(
            recovered
                .execute_tool_call(&NormalizedToolCallV1 {
                    tool_call_id: ToolCallId::new("tc-job-escape").expect("tool id"),
                    tool_name: "job.start".into(),
                    arguments_hash: "fnv1a64:job".into(),
                    arguments: json!({ "command": sleep_command(), "cwd": ".." }),
                })
                .await
                .is_err()
        );
        assert!(
            recovered
                .execute_tool_call(&NormalizedToolCallV1 {
                    tool_call_id: ToolCallId::new("tc-job-profile").expect("tool id"),
                    tool_name: "job.start".into(),
                    arguments_hash: "fnv1a64:job-profile".into(),
                    arguments: json!({
                        "command": sleep_command(),
                        "commandProfile": "unapproved"
                    }),
                })
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn tool_failures_transition_to_failed_or_outcome_unknown() {
        let root = temp_git_workspace("failure-transitions");
        fs::write(root.join("src/lib.rs"), "pub fn answer() -> i32 { 41 }\n").expect("lib");
        commit_all(&root);
        let contract = contract(&root, "run-t0023b-failures");
        let config = config(&root);
        let mut service = IntegratedDelegatedService::new(&root, contract.clone(), config.clone())
            .expect("service");
        service.start().expect("start");
        let read_call = NormalizedToolCallV1 {
            tool_call_id: ToolCallId::new("tc-read-missing").expect("tool id"),
            tool_name: "read".into(),
            arguments_hash: "fnv1a64:read-missing".into(),
            arguments: json!({ "path": "src/missing.rs" }),
        };
        assert!(service.execute_tool_call(&read_call).await.is_err());
        let apply_call = NormalizedToolCallV1 {
            tool_call_id: ToolCallId::new("tc-apply-missing").expect("tool id"),
            tool_name: "patch.apply".into(),
            arguments_hash: "fnv1a64:apply-missing".into(),
            arguments: json!({ "patchId": "missing-patch" }),
        };
        assert!(service.execute_tool_call(&apply_call).await.is_err());
        let run_id = RunId::new(contract.task_id).expect("run id");
        let calls = DelegatedJournal::open(config.journal_root)
            .expect("journal")
            .load_tool_calls(&run_id)
            .expect("tool calls");
        assert_eq!(
            calls.get("tc-read-missing").expect("read call").status,
            ToolCallStatus::Failed
        );
        assert_eq!(
            calls.get("tc-apply-missing").expect("apply call").status,
            ToolCallStatus::OutcomeUnknown
        );
    }

    #[tokio::test]
    async fn fake_worker_two_failed_repairs_gets_untrusted_advice_then_completes() {
        let root = temp_git_workspace("advisor-integration");
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname=\"fixture\"\nversion=\"0.1.0\"\nedition=\"2021\"\n",
        )
        .expect("cargo");
        fs::write(
            root.join("src/lib.rs"),
            "pub fn answer() -> i32 {\n    41\n}\n",
        )
        .expect("lib");
        fs::create_dir_all(root.join("tests")).expect("tests");
        fs::write(
            root.join("tests/answer_test.rs"),
            "#[test]\nfn answer_is_expected_value() {\n    assert_eq!(fixture::answer(), 42);\n}\n",
        )
        .expect("test");
        commit_all(&root);
        let mut cfg = config(&root);
        cfg.advisor = Some(IntegratedAdvisorConfigV1 {
            enabled: true,
            advisor_id: "fake-advisor".into(),
            disclosure_classification: AdviceDisclosureClassification::RemoteAllowed,
            maximum_response_length: 2048,
            maximum_consultations_per_run: 1,
            advice_required: false,
            local_runtime: None,
        });
        let contract = contract(&root, "run-t0024c-fake-advisor");
        let mut service =
            IntegratedDelegatedService::new(&root, contract.clone(), cfg).expect("service");
        service.start().expect("start");

        for (index, value) in [(1, 40), (2, 43)] {
            let current = fs::read_to_string(root.join("src/lib.rs")).expect("source");
            service
                .execute_tool(
                    "patch.preview",
                    &json!({
                        "patchId": format!("patch-wrong-{index}"),
                        "operations": [{
                            "path": "src/lib.rs",
                            "old": current,
                            "new": format!("pub fn answer() -> i32 {{\n    {value}\n}}\n")
                        }],
                        "modelRationale": "bounded wrong repair for advisor trigger test"
                    }),
                )
                .await
                .expect("preview");
            service
                .execute_tool(
                    "patch.apply",
                    &json!({"patchId": format!("patch-wrong-{index}")}),
                )
                .await
                .expect("apply");
            let verification = service
                .execute_tool("verify.run", &json!({"timeout": 30000}))
                .await
                .expect("verify");
            assert_eq!(verification.summary, "Failed");
        }

        let mut advisor = crate::delegated::advisor::FakeAdvisor::success();
        let advice = service
            .maybe_consult_advisor_after_failed_repairs(
                &mut advisor,
                "What should the next bounded repair inspect before patching?",
            )
            .expect("advisor consult")
            .expect("advice response");

        assert_eq!(advice.status, AdvisorStatus::Completed);
        assert!(
            service
                .provider_history
                .iter()
                .any(|message| message.role == "user"
                    && message.content.contains("<untrusted_advisor_context>")
                    && message.content.contains("ordinary CatDesk tools"))
        );

        let current = fs::read_to_string(root.join("src/lib.rs")).expect("source");
        service
            .execute_tool(
                "patch.preview",
                &json!({
                    "patchId": "patch-correct-after-advice",
                    "parentPatchId": "patch-wrong-2",
                    "operations": [{
                        "path": "src/lib.rs",
                        "old": current,
                        "new": "pub fn answer() -> i32 {\n    42\n}\n"
                    }],
                    "modelRationale": "worker independently inspected and corrected after untrusted advice"
                }),
            )
            .await
            .expect("preview correct");
        service
            .execute_tool(
                "patch.apply",
                &json!({"patchId": "patch-correct-after-advice"}),
            )
            .await
            .expect("apply correct");
        let passed = service
            .execute_tool("verify.run", &json!({"timeout": 30000}))
            .await
            .expect("verify passed");
        assert_eq!(passed.summary, "Passed");
        service
            .execute_tool("diff.actual", &json!({"paths":["src/lib.rs"]}))
            .await
            .expect("diff");
        let review = service.final_review().expect("review");
        assert_eq!(review.final_result.status, RunState::CompletedVerified);
        let second_advice = service
            .maybe_consult_advisor_after_failed_repairs(&mut advisor, "another request")
            .expect("second consult check");
        assert!(second_advice.is_none());
    }

    #[tokio::test]
    async fn autonomous_loop_triggers_one_advisor_consultation_after_two_failed_repairs() {
        let root = temp_git_workspace("advisor-autonomous-loop");
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname=\"fixture\"\nversion=\"0.1.0\"\nedition=\"2021\"\n",
        )
        .expect("cargo");
        fs::write(
            root.join("src/lib.rs"),
            "pub fn answer() -> i32 {\n    41\n}\n",
        )
        .expect("lib");
        fs::create_dir_all(root.join("tests")).expect("tests");
        fs::write(
            root.join("tests/answer_test.rs"),
            "#[test]\nfn answer_is_expected_value() {\n    assert_eq!(fixture::answer(), 42);\n}\n",
        )
        .expect("test");
        commit_all(&root);

        let mut contract = contract(&root, "run-t0024c1-autonomous-advisor");
        contract.max_turns = 20;
        contract.max_tool_calls = 40;
        contract.advisor_policy = Some(AdvisorPolicyV1 {
            enabled: true,
            advisor_id: "deepseek-web".into(),
            disclosure_classification: AdvisorDisclosureClassificationV1::RemoteAllowed,
            maximum_response_length: 2048,
            maximum_consultations_per_run: 1,
            advice_required: false,
        });
        let mut cfg = config(&root);
        cfg.advisor = Some(IntegratedAdvisorConfigV1 {
            enabled: true,
            advisor_id: "deepseek-web".into(),
            disclosure_classification: AdviceDisclosureClassification::RemoteAllowed,
            maximum_response_length: 2048,
            maximum_consultations_per_run: 1,
            advice_required: false,
            local_runtime: Some(fake_deepseek_sidecar(&root)),
        });
        let turns = vec![
            FakeProviderTurn::ToolCall(tool_envelope(
                "tc-read-1",
                "read",
                json!({"path":"src/lib.rs"}),
            )),
            FakeProviderTurn::ToolCall(tool_envelope(
                "tc-preview-1",
                "patch.preview",
                json!({
                    "patchId": "patch-wrong-1",
                    "operations": [{
                        "path": "src/lib.rs",
                        "old": "pub fn answer() -> i32 {\n    41\n}\n",
                        "new": "pub fn answer() -> i32 {\n    40\n}\n"
                    }]
                }),
            )),
            FakeProviderTurn::ToolCall(tool_envelope(
                "tc-apply-1",
                "patch.apply",
                json!({"patchId":"patch-wrong-1"}),
            )),
            FakeProviderTurn::ToolCall(tool_envelope(
                "tc-verify-1",
                "verify.run",
                json!({"timeout": 30000}),
            )),
            FakeProviderTurn::ToolCall(tool_envelope(
                "tc-preview-2",
                "patch.preview",
                json!({
                    "patchId": "patch-wrong-2",
                    "parentPatchId": "patch-wrong-1",
                    "operations": [{
                        "path": "src/lib.rs",
                        "old": "pub fn answer() -> i32 {\n    40\n}\n",
                        "new": "pub fn answer() -> i32 {\n    43\n}\n"
                    }]
                }),
            )),
            FakeProviderTurn::ToolCall(tool_envelope(
                "tc-apply-2",
                "patch.apply",
                json!({"patchId":"patch-wrong-2"}),
            )),
            FakeProviderTurn::ToolCall(tool_envelope(
                "tc-verify-2",
                "verify.run",
                json!({"timeout": 30000}),
            )),
            FakeProviderTurn::ToolCall(tool_envelope(
                "tc-read-after-advice",
                "read",
                json!({"path":"src/lib.rs"}),
            )),
            FakeProviderTurn::ToolCall(tool_envelope(
                "tc-preview-3",
                "patch.preview",
                json!({
                    "patchId": "patch-correct",
                    "parentPatchId": "patch-wrong-2",
                    "operations": [{
                        "path": "src/lib.rs",
                        "old": "pub fn answer() -> i32 {\n    43\n}\n",
                        "new": "pub fn answer() -> i32 {\n    42\n}\n"
                    }]
                }),
            )),
            FakeProviderTurn::ToolCall(tool_envelope(
                "tc-apply-3",
                "patch.apply",
                json!({"patchId":"patch-correct"}),
            )),
            FakeProviderTurn::ToolCall(tool_envelope(
                "tc-verify-3",
                "verify.run",
                json!({"timeout": 30000}),
            )),
            FakeProviderTurn::ToolCall(tool_envelope(
                "tc-diff",
                "diff.actual",
                json!({"paths":["src/lib.rs"]}),
            )),
            FakeProviderTurn::Complete(
                "Verification passed and authoritative diff is captured.".into(),
            ),
        ];
        let mut service =
            IntegratedDelegatedService::new(&root, contract.clone(), cfg).expect("service");
        let review = service
            .run_fake_worker_loop_with_advisor(
                FakeProvider::new(turns),
                Arc::new(AtomicBool::new(false)),
            )
            .await
            .expect("fake loop completes");

        assert_eq!(review.final_result.status, RunState::CompletedVerified);
        assert_eq!(service.advisor_consultations_used, 1);
        assert!(service.provider_history.iter().any(|message| {
            message.role == "user" && message.content.contains("<untrusted_advisor_context>")
        }));
        assert!(service.tool_definitions().iter().all(|tool| {
            !tool.name.starts_with("advisor.") && !tool.name.starts_with("advice.")
        }));
        let advice_index = service
            .provider_history
            .iter()
            .position(|message| message.content.contains("<untrusted_advisor_context>"))
            .expect("advice context");
        let later_read = service
            .provider_history
            .iter()
            .enumerate()
            .any(|(index, message)| {
                index > advice_index && message.tool_name.as_deref() == Some("read")
            });
        assert!(later_read, "worker must inspect source after advice");
        assert_eq!(service.consecutive_failed_verifications, 0);
        assert!(!service.advice_consulted_for_current_failure);
    }

    #[tokio::test]
    async fn optional_configured_advisor_failure_continues_locally() {
        let root = temp_git_workspace("advisor-optional-unavailable");
        fs::write(root.join("src/lib.rs"), "pub fn answer() -> i32 { 41 }\n").expect("lib");
        commit_all(&root);
        let mut cfg = config(&root);
        cfg.advisor = Some(IntegratedAdvisorConfigV1 {
            enabled: true,
            advisor_id: "deepseek-web".into(),
            disclosure_classification: AdviceDisclosureClassification::RemoteAllowed,
            maximum_response_length: 1024,
            maximum_consultations_per_run: 1,
            advice_required: false,
            local_runtime: None,
        });
        let contract = contract(&root, "run-t0024c2-optional-advisor-unavailable");
        let mut service =
            IntegratedDelegatedService::new(&root, contract.clone(), cfg).expect("service");
        service.start().expect("start");
        service.consecutive_failed_verifications = 2;

        service
            .maybe_consult_configured_advisor_after_failed_repairs(Arc::new(AtomicBool::new(false)))
            .await
            .expect("optional advisor unavailable continues");

        assert!(service.provider_history.iter().any(|message| {
            message.content.contains("Advisor advice unavailable")
                && message.content.contains("Continue locally")
        }));
        assert_ne!(
            service
                .journal
                .load_run(&RunId::new(contract.task_id).expect("run"))
                .expect("snapshot")
                .state,
            RunState::NeedsSupervisor
        );
    }

    #[tokio::test]
    async fn fake_loop_recovers_malformed_tool_syntax_with_bounded_correction() {
        let root = temp_git_workspace("malformed-tool-correction");
        write_answer_fixture(&root, 41, 42);
        commit_all(&root);
        let mut contract = contract(&root, "run-malformed-tool-correction");
        contract.max_turns = 10;
        contract.max_tool_calls = 10;
        let mut service =
            IntegratedDelegatedService::new(&root, contract, config(&root)).expect("service");
        let turns = vec![
            FakeProviderTurn::Malformed(
                "qwen tool call parsing failed: XML syntax error on line 14".into(),
            ),
            FakeProviderTurn::ToolCall(tool_envelope(
                "tc-read",
                "read",
                json!({"path":"src/lib.rs"}),
            )),
            FakeProviderTurn::ToolCall(tool_envelope(
                "tc-preview",
                "patch.preview",
                json!({
                    "patchId": "patch-good",
                    "operations": [{
                        "path": "src/lib.rs",
                        "old": "pub fn answer() -> i32 {\n    41\n}\n",
                        "new": "pub fn answer() -> i32 {\n    42\n}\n"
                    }]
                }),
            )),
            FakeProviderTurn::ToolCall(tool_envelope(
                "tc-apply",
                "patch.apply",
                json!({"patchId":"patch-good"}),
            )),
            FakeProviderTurn::ToolCall(tool_envelope(
                "tc-verify",
                "verify.run",
                json!({"timeout": 30000}),
            )),
            FakeProviderTurn::ToolCall(tool_envelope(
                "tc-diff",
                "diff.actual",
                json!({"paths":["src/lib.rs"]}),
            )),
            FakeProviderTurn::Complete(
                "verification passed and authoritative diff captured".into(),
            ),
        ];

        let review = service
            .run_fake_worker_loop_with_advisor(
                FakeProvider::new(turns),
                Arc::new(AtomicBool::new(false)),
            )
            .await
            .expect("corrected malformed response");

        assert_eq!(review.final_result.status, RunState::CompletedVerified);
        assert_eq!(service.corrective_turns_used, 1);
        assert!(service.provider_history.iter().any(|message| {
            message.content.contains("malformed-tool-call")
                && message.content.contains("exactly one JSON object")
        }));
    }

    #[tokio::test]
    async fn premature_stop_after_patch_preview_requests_patch_apply_once() {
        let root = temp_git_workspace("premature-after-preview");
        write_answer_fixture(&root, 41, 42);
        commit_all(&root);
        let mut contract = contract(&root, "run-premature-after-preview");
        contract.max_turns = 10;
        contract.max_tool_calls = 10;
        let turns = vec![
            FakeProviderTurn::ToolCall(tool_envelope(
                "tc-preview",
                "patch.preview",
                json!({
                    "patchId": "patch-good",
                    "operations": [{
                        "path": "src/lib.rs",
                        "old": "pub fn answer() -> i32 {\n    41\n}\n",
                        "new": "pub fn answer() -> i32 {\n    42\n}\n"
                    }]
                }),
            )),
            FakeProviderTurn::Complete("done early".into()),
            FakeProviderTurn::ToolCall(tool_envelope(
                "tc-apply",
                "patch.apply",
                json!({"patchId":"patch-good"}),
            )),
            FakeProviderTurn::ToolCall(tool_envelope(
                "tc-verify",
                "verify.run",
                json!({"timeout": 30000}),
            )),
            FakeProviderTurn::ToolCall(tool_envelope(
                "tc-diff",
                "diff.actual",
                json!({"paths":["src/lib.rs"]}),
            )),
            FakeProviderTurn::Complete(
                "verification passed and authoritative diff captured".into(),
            ),
        ];
        let mut service = IntegratedDelegatedService::new(&root, contract.clone(), config(&root))
            .expect("service");

        let review = service
            .run_fake_worker_loop_with_advisor(
                FakeProvider::new(turns),
                Arc::new(AtomicBool::new(false)),
            )
            .await
            .expect("completed");

        assert_eq!(review.final_result.status, RunState::CompletedVerified);
        assert!(service.provider_history.iter().any(|message| {
            message
                .content
                .contains("patch.apply with patchId patch-good")
        }));
        let applications = service
            .journal
            .load_patch_applications(&RunId::new(contract.task_id).expect("run"))
            .expect("applications");
        assert_eq!(applications.len(), 1, "patch.apply must not duplicate");
    }

    #[tokio::test]
    async fn premature_stop_after_patch_apply_requests_verification() {
        let root = temp_git_workspace("premature-after-apply");
        write_answer_fixture(&root, 41, 42);
        commit_all(&root);
        let mut contract = contract(&root, "run-premature-after-apply");
        contract.max_turns = 10;
        contract.max_tool_calls = 10;
        let turns = vec![
            FakeProviderTurn::ToolCall(tool_envelope(
                "tc-preview",
                "patch.preview",
                json!({
                    "patchId": "patch-good",
                    "operations": [{
                        "path": "src/lib.rs",
                        "old": "pub fn answer() -> i32 {\n    41\n}\n",
                        "new": "pub fn answer() -> i32 {\n    42\n}\n"
                    }]
                }),
            )),
            FakeProviderTurn::ToolCall(tool_envelope(
                "tc-apply",
                "patch.apply",
                json!({"patchId":"patch-good"}),
            )),
            FakeProviderTurn::Complete("done early".into()),
            FakeProviderTurn::ToolCall(tool_envelope(
                "tc-verify",
                "verify.run",
                json!({"timeout": 30000}),
            )),
            FakeProviderTurn::ToolCall(tool_envelope(
                "tc-diff",
                "diff.actual",
                json!({"paths":["src/lib.rs"]}),
            )),
            FakeProviderTurn::Complete(
                "verification passed and authoritative diff captured".into(),
            ),
        ];
        let mut service =
            IntegratedDelegatedService::new(&root, contract, config(&root)).expect("service");

        service
            .run_fake_worker_loop_with_advisor(
                FakeProvider::new(turns),
                Arc::new(AtomicBool::new(false)),
            )
            .await
            .expect("completed");

        assert!(service.provider_history.iter().any(|message| {
            message
                .content
                .contains("Required next action: call verify.run")
        }));
    }

    #[tokio::test]
    async fn failed_verification_requests_repair_not_completion() {
        let root = temp_git_workspace("failed-verify-repair");
        write_answer_fixture(&root, 41, 42);
        commit_all(&root);
        let mut contract = contract(&root, "run-failed-verify-repair");
        contract.max_turns = 12;
        contract.max_tool_calls = 12;
        let turns = vec![
            FakeProviderTurn::ToolCall(tool_envelope(
                "tc-preview-1",
                "patch.preview",
                json!({
                    "patchId": "patch-wrong",
                    "operations": [{
                        "path": "src/lib.rs",
                        "old": "pub fn answer() -> i32 {\n    41\n}\n",
                        "new": "pub fn answer() -> i32 {\n    40\n}\n"
                    }]
                }),
            )),
            FakeProviderTurn::ToolCall(tool_envelope(
                "tc-apply-1",
                "patch.apply",
                json!({"patchId":"patch-wrong"}),
            )),
            FakeProviderTurn::ToolCall(tool_envelope(
                "tc-verify-1",
                "verify.run",
                json!({"timeout": 30000}),
            )),
            FakeProviderTurn::Complete("done early".into()),
            FakeProviderTurn::ToolCall(tool_envelope(
                "tc-preview-2",
                "patch.preview",
                json!({
                    "patchId": "patch-good",
                    "parentPatchId": "patch-wrong",
                    "operations": [{
                        "path": "src/lib.rs",
                        "old": "pub fn answer() -> i32 {\n    40\n}\n",
                        "new": "pub fn answer() -> i32 {\n    42\n}\n"
                    }]
                }),
            )),
            FakeProviderTurn::ToolCall(tool_envelope(
                "tc-apply-2",
                "patch.apply",
                json!({"patchId":"patch-good"}),
            )),
            FakeProviderTurn::ToolCall(tool_envelope(
                "tc-verify-2",
                "verify.run",
                json!({"timeout": 30000}),
            )),
            FakeProviderTurn::ToolCall(tool_envelope(
                "tc-diff",
                "diff.actual",
                json!({"paths":["src/lib.rs"]}),
            )),
            FakeProviderTurn::Complete(
                "verification passed and authoritative diff captured".into(),
            ),
        ];
        let mut service =
            IntegratedDelegatedService::new(&root, contract, config(&root)).expect("service");

        service
            .run_fake_worker_loop_with_advisor(
                FakeProvider::new(turns),
                Arc::new(AtomicBool::new(false)),
            )
            .await
            .expect("completed");

        assert!(service.provider_history.iter().any(|message| {
            message
                .content
                .contains("diagnose the failed verifier output")
                && message.content.contains("corrected child patch")
        }));
    }

    #[tokio::test]
    async fn passed_verification_without_diff_requests_diff_actual() {
        let root = temp_git_workspace("passed-verify-no-diff");
        write_answer_fixture(&root, 41, 42);
        commit_all(&root);
        let mut contract = contract(&root, "run-passed-verify-no-diff");
        contract.max_turns = 10;
        contract.max_tool_calls = 10;
        let turns = vec![
            FakeProviderTurn::ToolCall(tool_envelope(
                "tc-preview",
                "patch.preview",
                json!({
                    "patchId": "patch-good",
                    "operations": [{
                        "path": "src/lib.rs",
                        "old": "pub fn answer() -> i32 {\n    41\n}\n",
                        "new": "pub fn answer() -> i32 {\n    42\n}\n"
                    }]
                }),
            )),
            FakeProviderTurn::ToolCall(tool_envelope(
                "tc-apply",
                "patch.apply",
                json!({"patchId":"patch-good"}),
            )),
            FakeProviderTurn::ToolCall(tool_envelope(
                "tc-verify",
                "verify.run",
                json!({"timeout": 30000}),
            )),
            FakeProviderTurn::Complete("done early".into()),
            FakeProviderTurn::ToolCall(tool_envelope(
                "tc-diff",
                "diff.actual",
                json!({"paths":["src/lib.rs"]}),
            )),
            FakeProviderTurn::Complete(
                "verification passed and authoritative diff captured".into(),
            ),
        ];
        let mut service =
            IntegratedDelegatedService::new(&root, contract, config(&root)).expect("service");

        service
            .run_fake_worker_loop_with_advisor(
                FakeProvider::new(turns),
                Arc::new(AtomicBool::new(false)),
            )
            .await
            .expect("completed");

        assert!(service.provider_history.iter().any(|message| {
            message.content.contains("call diff.actual")
                && message.content.contains("authoritative diff evidence")
        }));
        assert!(service.provider_history.iter().any(|message| {
            message
                .content
                .contains("Do not call another tool. Provide the final concise completion claim")
        }));
    }

    #[tokio::test]
    async fn repeated_premature_completion_exhausts_correction_budget() {
        let root = temp_git_workspace("premature-budget");
        write_answer_fixture(&root, 41, 42);
        commit_all(&root);
        let mut contract = contract(&root, "run-premature-budget");
        contract.max_turns = 6;
        let mut service =
            IntegratedDelegatedService::new(&root, contract, config(&root)).expect("service");

        let result = service
            .run_fake_worker_loop_with_advisor(
                FakeProvider::new(vec![
                    FakeProviderTurn::Complete("done early 1".into()),
                    FakeProviderTurn::Complete("done early 2".into()),
                    FakeProviderTurn::Complete("done early 3".into()),
                ]),
                Arc::new(AtomicBool::new(false)),
            )
            .await;

        assert!(matches!(
            result,
            Err(IntegratedError::Tool(message))
                if message.contains("corrective turn budget exhausted")
        ));
        assert_eq!(service.corrective_turns_used, MAX_CORRECTIVE_TURNS_PER_RUN);
    }

    #[tokio::test]
    async fn latest_unapplied_patch_follows_journal_proposal_chronology() {
        let root = temp_git_workspace("latest-patch-chronology");
        write_answer_fixture(&root, 41, 42);
        commit_all(&root);
        let contract = contract(&root, "run-latest-patch-chronology");
        let mut service =
            IntegratedDelegatedService::new(&root, contract, config(&root)).expect("service");
        service.start().expect("start");

        for (patch_id, value) in [("patch-9", 40), ("patch-10", 42)] {
            service
                .execute_tool(
                    "patch.preview",
                    &json!({
                        "patchId": patch_id,
                        "operations": [{
                            "path": "src/lib.rs",
                            "old": "pub fn answer() -> i32 {\n    41\n}\n",
                            "new": format!("pub fn answer() -> i32 {{\n    {value}\n}}\n")
                        }]
                    }),
                )
                .await
                .expect("preview");
        }

        let next_action = service
            .next_required_action_from_persisted_state()
            .expect("next action")
            .expect("patch action");

        assert!(next_action.contains("patch.apply with patchId patch-10"));
        assert!(!next_action.contains("patch.apply with patchId patch-9"));
    }

    #[tokio::test]
    async fn malformed_tool_correction_budget_failure_is_propagated() {
        let root = temp_git_workspace("malformed-budget-propagated");
        write_answer_fixture(&root, 41, 42);
        commit_all(&root);
        let mut contract = contract(&root, "run-malformed-budget-propagated");
        contract.max_turns = 6;
        let mut service =
            IntegratedDelegatedService::new(&root, contract, config(&root)).expect("service");

        let malformed = "Ollama qwen tool call parsing failed: XML syntax error on line 14";
        let result = service
            .run_fake_worker_loop_with_advisor(
                FakeProvider::new(vec![
                    FakeProviderTurn::Malformed(malformed.into()),
                    FakeProviderTurn::Malformed(malformed.into()),
                    FakeProviderTurn::Malformed(malformed.into()),
                ]),
                Arc::new(AtomicBool::new(false)),
            )
            .await;

        assert!(matches!(
            result,
            Err(IntegratedError::Tool(message))
                if message.contains("corrective turn budget exhausted")
                    && message.contains("model=qwen3.5:9b")
        ));
        assert_eq!(service.corrective_turns_used, MAX_CORRECTIVE_TURNS_PER_RUN);
    }

    #[tokio::test]
    async fn no_patch_task_is_not_forced_into_patch_apply() {
        let root = temp_git_workspace("no-patch-next-action");
        write_answer_fixture(&root, 42, 42);
        commit_all(&root);
        let mut contract = contract(&root, "run-no-patch-next-action");
        contract.acceptance_criteria = vec!["cargo tests pass".into()];
        let mut service =
            IntegratedDelegatedService::new(&root, contract, config(&root)).expect("service");
        service.start().expect("start");

        let corrected = service
            .push_premature_completion_correction(
                1,
                &IntegratedError::Verification("verification has not run".into()),
                Some("stop"),
                Some(256),
            )
            .expect("correction");

        assert!(corrected);
        let correction = service.provider_history.last().expect("correction");
        assert!(correction.content.contains("call verify.run next"));
        assert!(!correction.content.contains("patch.apply with patchId"));
    }

    #[tokio::test]
    async fn recovered_patch_application_remains_authoritative_after_restart() {
        let root = temp_git_workspace("persisted-tool-authority");
        write_answer_fixture(&root, 41, 42);
        commit_all(&root);
        let contract = contract(&root, "run-persisted-tool-authority");
        let config = config(&root);
        let mut service = IntegratedDelegatedService::new(&root, contract.clone(), config.clone())
            .expect("service");
        service.start().expect("start");
        service
            .execute_tool(
                "patch.preview",
                &json!({
                    "patchId": "patch-good",
                    "operations": [{
                        "path": "src/lib.rs",
                        "old": "pub fn answer() -> i32 {\n    41\n}\n",
                        "new": "pub fn answer() -> i32 {\n    42\n}\n"
                    }]
                }),
            )
            .await
            .expect("preview");
        service
            .execute_tool("patch.apply", &json!({"patchId":"patch-good"}))
            .await
            .expect("apply");

        let recovered =
            IntegratedDelegatedService::recover(&root, contract, config).expect("recover");
        let next_action = recovered
            .next_required_action_from_persisted_state()
            .expect("next action")
            .expect("next action exists");

        assert!(next_action.contains("verify.run"));
        assert!(!next_action.contains("patch.apply with patchId patch-good"));
    }

    #[tokio::test]
    async fn required_configured_advisor_failure_persists_needs_supervisor() {
        let root = temp_git_workspace("advisor-required-unavailable");
        fs::write(root.join("src/lib.rs"), "pub fn answer() -> i32 { 41 }\n").expect("lib");
        commit_all(&root);
        let mut cfg = config(&root);
        cfg.advisor = Some(IntegratedAdvisorConfigV1 {
            enabled: true,
            advisor_id: "deepseek-web".into(),
            disclosure_classification: AdviceDisclosureClassification::RemoteAllowed,
            maximum_response_length: 1024,
            maximum_consultations_per_run: 1,
            advice_required: true,
            local_runtime: None,
        });
        let contract = contract(&root, "run-t0024c2-required-advisor-unavailable");
        let mut service =
            IntegratedDelegatedService::new(&root, contract.clone(), cfg).expect("service");
        service.start().expect("start");
        service.consecutive_failed_verifications = 2;

        let result = service
            .maybe_consult_configured_advisor_after_failed_repairs(Arc::new(AtomicBool::new(false)))
            .await;

        assert!(
            matches!(result, Err(IntegratedError::Tool(message)) if message.contains("required advisor consultation unavailable"))
        );
        assert_eq!(
            service
                .journal
                .load_run(&RunId::new(contract.task_id).expect("run"))
                .expect("snapshot")
                .state,
            RunState::NeedsSupervisor
        );
    }

    #[tokio::test]
    async fn configured_advisor_cancellation_is_prompt_and_suppresses_delivery() {
        let root = temp_git_workspace("advisor-cancel-prompt");
        fs::write(root.join("src/lib.rs"), "pub fn answer() -> i32 { 41 }\n").expect("lib");
        commit_all(&root);
        let mut cfg = config(&root);
        cfg.advisor = Some(IntegratedAdvisorConfigV1 {
            enabled: true,
            advisor_id: "deepseek-web".into(),
            disclosure_classification: AdviceDisclosureClassification::RemoteAllowed,
            maximum_response_length: 1024,
            maximum_consultations_per_run: 1,
            advice_required: false,
            local_runtime: Some(blocking_until_cancel_deepseek_sidecar(&root)),
        });
        let contract = contract(&root, "run-t0024c2-advisor-cancel");
        let mut service =
            IntegratedDelegatedService::new(&root, contract.clone(), cfg).expect("service");
        service.start().expect("start");
        service.consecutive_failed_verifications = 2;
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_after_start = cancel.clone();
        let cancel_thread = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            cancel_after_start.store(true, Ordering::SeqCst);
        });
        let started = Instant::now();

        service
            .maybe_consult_configured_advisor_after_failed_repairs(cancel)
            .await
            .expect("cancelled optional advisor does not fail run");
        cancel_thread.join().expect("cancel thread");

        assert!(
            started.elapsed() < Duration::from_secs(5),
            "advisor cancellation should be bounded and prompt"
        );
        assert!(service.provider_history.iter().any(|message| {
            message.content.contains("Advisor advice unavailable")
                && message.content.contains("Cancelled")
        }));
        assert!(
            !service.provider_history.iter().any(|message| {
                message.content.contains("<untrusted_advisor_context>")
                    && message.content.contains("Recommendations:")
            }),
            "cancelled advice must not be delivered as completed context"
        );
        let events = service
            .journal
            .poll_events(
                &RunId::new(contract.task_id).expect("run"),
                EventCursor {
                    after_sequence: 0,
                    limit: usize::MAX,
                },
            )
            .expect("events");
        let advisor_terminal_events = events
            .iter()
            .filter_map(|event| match &event.payload {
                EventPayloadV1::AdvisorEvent { event, .. }
                    if matches!(
                        event.as_str(),
                        "advice_completed"
                            | "advice_cancelled"
                            | "advice_failed"
                            | "advice_rate_limited"
                            | "advice_takeover_required"
                            | "advice_timed_out"
                    ) =>
                {
                    Some(event.as_str())
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(advisor_terminal_events, vec!["advice_cancelled"]);
    }

    #[tokio::test]
    #[ignore = "opt-in headed DeepSeek browser advisor integration smoke"]
    async fn live_deepseek_process_advisor_explicit_request_returns_untrusted_context() {
        let root = temp_git_workspace("deepseek-live-advisor-integration");
        fs::write(root.join("src/lib.rs"), "pub fn answer() -> i32 { 42 }\n").expect("lib");
        commit_all(&root);
        let mut cfg = config(&root);
        cfg.advisor = Some(IntegratedAdvisorConfigV1 {
            enabled: true,
            advisor_id: "deepseek-web".into(),
            disclosure_classification: AdviceDisclosureClassification::RemoteAllowed,
            maximum_response_length: 512,
            maximum_consultations_per_run: 1,
            advice_required: false,
            local_runtime: None,
        });
        let contract = contract(&root, "run-t0024c-live-deepseek");
        let mut service = IntegratedDelegatedService::new(&root, contract, cfg).expect("service");
        service.start().expect("start");
        let python = std::env::var("CATDESK_DEEPSEEK_ADVISOR_PYTHON")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(".tmp/deepseek-advisor-venv/Scripts/python.exe"));
        let script = std::env::var("CATDESK_DEEPSEEK_ADVISOR_SCRIPT")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("experimental/advisors/deepseek_web_advisor.py"));
        let profile = std::env::var("CATDESK_DEEPSEEK_ADVISOR_PROFILE")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(".tmp/deepseek-advisor-profile"));
        let selectors = std::env::var("CATDESK_DEEPSEEK_ADVISOR_SELECTORS")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("experimental/advisors/deepseek_selectors.json"));
        let mut process_config = DeepSeekProcessAdvisorConfig::new(python, script, profile);
        process_config.selectors_path = Some(selectors);
        process_config.headed = true;
        let mut advisor = DeepSeekProcessAdvisor::new(process_config);

        let response = service
            .consult_advisor(
                &mut advisor,
                AdviceTrigger::ExplicitQwenRequest,
                "In one short sentence, provide advisory-only guidance for a synthetic CatDesk test.",
            )
            .expect("live advisor response");

        assert_eq!(response.status, AdvisorStatus::Completed);
        assert!(service.provider_history.iter().any(|message| {
            message.role == "user" && message.content.contains("<untrusted_advisor_context>")
        }));
        assert!(service.tool_definitions().iter().all(|tool| {
            !tool.name.starts_with("advisor.") && !tool.name.starts_with("advice.")
        }));
        advisor.shutdown(Duration::from_secs(5)).expect("shutdown");
    }

    #[tokio::test]
    #[ignore = "opt-in headed DeepSeek browser advisor plus deterministic worker-loop smoke"]
    async fn live_deepseek_combined_autonomous_advisor_worker_proof() {
        let root = temp_git_workspace("deepseek-live-combined-advisor");
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname=\"fixture\"\nversion=\"0.1.0\"\nedition=\"2021\"\n",
        )
        .expect("cargo");
        fs::write(
            root.join("src/lib.rs"),
            "pub fn answer() -> i32 {\n    41\n}\n",
        )
        .expect("lib");
        fs::create_dir_all(root.join("tests")).expect("tests");
        fs::write(
            root.join("tests/answer_test.rs"),
            "#[test]\nfn answer_is_expected_value() {\n    assert_eq!(fixture::answer(), 42);\n}\n",
        )
        .expect("test");
        commit_all(&root);

        let mut contract = contract(&root, "run-t0024c2-live-combined-advisor");
        contract.max_turns = 20;
        contract.max_tool_calls = 40;
        contract.advisor_policy = Some(AdvisorPolicyV1 {
            enabled: true,
            advisor_id: "deepseek-web".into(),
            disclosure_classification: AdvisorDisclosureClassificationV1::RemoteAllowed,
            maximum_response_length: 1024,
            maximum_consultations_per_run: 1,
            advice_required: false,
        });
        let mut cfg = config(&root);
        cfg.advisor = Some(IntegratedAdvisorConfigV1 {
            enabled: true,
            advisor_id: "deepseek-web".into(),
            disclosure_classification: AdviceDisclosureClassification::RemoteAllowed,
            maximum_response_length: 1024,
            maximum_consultations_per_run: 1,
            advice_required: false,
            local_runtime: Some(live_deepseek_runtime_config()),
        });
        let turns = vec![
            FakeProviderTurn::ToolCall(tool_envelope(
                "tc-read-1",
                "read",
                json!({"path":"src/lib.rs"}),
            )),
            FakeProviderTurn::ToolCall(tool_envelope(
                "tc-preview-1",
                "patch.preview",
                json!({
                    "patchId": "patch-wrong-1",
                    "operations": [{
                        "path": "src/lib.rs",
                        "old": "pub fn answer() -> i32 {\n    41\n}\n",
                        "new": "pub fn answer() -> i32 {\n    40\n}\n"
                    }]
                }),
            )),
            FakeProviderTurn::ToolCall(tool_envelope(
                "tc-apply-1",
                "patch.apply",
                json!({"patchId":"patch-wrong-1"}),
            )),
            FakeProviderTurn::ToolCall(tool_envelope(
                "tc-verify-1",
                "verify.run",
                json!({"timeout": 30000}),
            )),
            FakeProviderTurn::ToolCall(tool_envelope(
                "tc-preview-2",
                "patch.preview",
                json!({
                    "patchId": "patch-wrong-2",
                    "parentPatchId": "patch-wrong-1",
                    "operations": [{
                        "path": "src/lib.rs",
                        "old": "pub fn answer() -> i32 {\n    40\n}\n",
                        "new": "pub fn answer() -> i32 {\n    43\n}\n"
                    }]
                }),
            )),
            FakeProviderTurn::ToolCall(tool_envelope(
                "tc-apply-2",
                "patch.apply",
                json!({"patchId":"patch-wrong-2"}),
            )),
            FakeProviderTurn::ToolCall(tool_envelope(
                "tc-verify-2",
                "verify.run",
                json!({"timeout": 30000}),
            )),
            FakeProviderTurn::ToolCall(tool_envelope(
                "tc-read-after-advice",
                "read",
                json!({"path":"src/lib.rs"}),
            )),
            FakeProviderTurn::ToolCall(tool_envelope(
                "tc-preview-3",
                "patch.preview",
                json!({
                    "patchId": "patch-correct",
                    "parentPatchId": "patch-wrong-2",
                    "operations": [{
                        "path": "src/lib.rs",
                        "old": "pub fn answer() -> i32 {\n    43\n}\n",
                        "new": "pub fn answer() -> i32 {\n    42\n}\n"
                    }]
                }),
            )),
            FakeProviderTurn::ToolCall(tool_envelope(
                "tc-apply-3",
                "patch.apply",
                json!({"patchId":"patch-correct"}),
            )),
            FakeProviderTurn::ToolCall(tool_envelope(
                "tc-verify-3",
                "verify.run",
                json!({"timeout": 30000}),
            )),
            FakeProviderTurn::ToolCall(tool_envelope(
                "tc-diff",
                "diff.actual",
                json!({"paths":["src/lib.rs"]}),
            )),
            FakeProviderTurn::Complete(
                "Verification passed and authoritative diff is captured.".into(),
            ),
        ];
        let mut service = IntegratedDelegatedService::new(&root, contract, cfg).expect("service");
        let review = service
            .run_fake_worker_loop_with_advisor(
                FakeProvider::new(turns),
                Arc::new(AtomicBool::new(false)),
            )
            .await
            .expect("combined live advisor proof");

        assert_eq!(review.final_result.status, RunState::CompletedVerified);
        assert_eq!(service.advisor_consultations_used, 1);
        let advice_index = service
            .provider_history
            .iter()
            .position(|message| message.content.contains("<untrusted_advisor_context>"))
            .expect("advice delivered");
        assert!(
            service
                .provider_history
                .iter()
                .enumerate()
                .any(|(index, message)| {
                    index > advice_index && message.tool_name.as_deref() == Some("read")
                })
        );
        assert!(service.tool_definitions().iter().all(|tool| {
            !tool.name.starts_with("advisor.") && !tool.name.starts_with("advice.")
        }));
    }

    #[tokio::test]
    #[ignore = "requires local Ollama and runs a full disposable model-tool-model flow"]
    async fn ollama_qwen_live_production_worker_loop_closure() {
        let root = temp_git_workspace("qwen-production-loop");
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname=\"fixture\"\nversion=\"0.1.0\"\nedition=\"2021\"\n",
        )
        .expect("cargo");
        fs::write(
            root.join("src/lib.rs"),
            "/// Return the answer. Hint: the answer should be the next prime after forty-one.\npub fn answer() -> i32 {\n    41\n}\n",
        )
        .expect("lib");
        fs::create_dir_all(root.join("tests")).expect("tests");
        fs::write(
            root.join("tests/answer_test.rs"),
            "#[test]\nfn answer_is_expected_value() {\n    assert_eq!(fixture::answer(), 42);\n}\n",
        )
        .expect("test");
        commit_all(&root);

        let model = live_ollama_model_id();
        let mut contract = contract(&root, "run-t0023b-qwen-production");
        contract.provider_policy.primary_model_id = model.clone();
        contract.objective = "Make the disposable Rust fixture pass cargo test. You must inspect src/lib.rs, propose and apply patches through CatDesk, run verification, revise after any failure, capture diff.actual, and only then complete.".into();
        contract.ordered_steps = vec![
            "read src/lib.rs".into(),
            "preview a source patch".into(),
            "apply the patch".into(),
            "run verification".into(),
            "revise the source patch if verification fails".into(),
            "capture diff.actual after verification passes".into(),
        ];
        contract.acceptance_criteria = vec![
            "cargo tests pass".into(),
            "authoritative diff is captured".into(),
        ];
        contract.max_turns = 18;
        contract.max_tool_calls = 18;
        contract.max_elapsed_seconds = 240;
        let mut cfg = config(&root);
        cfg.model_id = model;
        let mut service = IntegratedDelegatedService::new(&root, contract, cfg).expect("service");
        let result = service.run_ollama_worker_loop().await;
        if result.is_err() {
            println!(
                "CATDESK_QWEN_PRODUCTION_LOOP_FAILED={}",
                serde_json::to_string_pretty(&service.provider_history).expect("history json")
            );
        }
        let review = result.expect("production loop completes");
        assert_eq!(review.final_result.status, RunState::CompletedVerified);
        assert!(
            service.provider_history.iter().any(|message| {
                message.tool_name.as_deref() == Some("verify.run")
                    && message.content.contains("FAILED")
            }),
            "live production loop must feed a failed verification back to Qwen"
        );
        assert!(
            service
                .provider_history
                .iter()
                .any(|message| message.tool_name.as_deref() == Some("diff.actual")),
            "live production loop must capture authoritative diff through a model-requested tool"
        );
        println!(
            "CATDESK_QWEN_PRODUCTION_LOOP={}",
            serde_json::to_string_pretty(&service.provider_history).expect("history json")
        );
    }

    #[tokio::test]
    #[ignore = "requires local Ollama and runs a scripted disposable model-tool-model flow"]
    async fn ollama_qwen_live_model_tool_model_closure() {
        let root = temp_git_workspace("qwen-live");
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname=\"fixture\"\nversion=\"0.1.0\"\nedition=\"2021\"\n",
        )
        .expect("cargo");
        fs::write(
            root.join("src/lib.rs"),
            "/// Return the answer. Hint: the answer should be the next prime after forty-one.\npub fn answer() -> i32 {\n    41\n}\n",
        )
        .expect("lib");
        fs::create_dir_all(root.join("tests")).expect("tests");
        fs::write(
            root.join("tests/answer_test.rs"),
            "#[test]\nfn answer_is_expected_value() {\n    assert_eq!(fixture::answer(), 42);\n}\n",
        )
        .expect("test");
        commit_all(&root);

        let model = live_ollama_model_id();
        let mut contract = contract(&root, "run-t0023a-qwen-live");
        contract.provider_policy.primary_model_id = model.clone();
        let mut cfg = config(&root);
        cfg.model_id = model.clone();
        let mut service =
            IntegratedDelegatedService::new(&root, contract.clone(), cfg).expect("service");
        service.start().expect("start");
        let ollama =
            OllamaAdapter::new("http://127.0.0.1:11434", Some("5m".into())).expect("ollama");
        let tools = service.tool_definitions();
        let mut transcript = Vec::new();

        let read_call = qwen_tool_call(
            &ollama,
            &model,
            "You are the delegated CatDesk worker. Call read for src/lib.rs. Return only a tool call.",
            &only_tools(&tools, &["read"]),
            "tc-live-read",
        )
        .await;
        println!(
            "CATDESK_QWEN_STEP read_call={}",
            serde_json::to_string(&read_call).expect("json")
        );
        assert_eq!(read_call.tool_name, "read");
        let read_result = service.execute_tool_call(&read_call).await.expect("read");
        transcript.push(json!({"step":"read","toolCall":read_call,"result":read_result.summary}));

        let preview_prompt = format!(
            "Objective: make cargo test pass. You have only read src/lib.rs, not tests. The bounded read result is:\n{}\nCall patch.preview with patchId patch-live-1 and operations to fix the source. Each operation.old must be an exact unique multi-line block from the file, not a single repeated token. Return only a tool call.",
            read_result.bounded_text
        );
        let mut preview_call = qwen_tool_call(
            &ollama,
            &model,
            &preview_prompt,
            &only_tools(&tools, &["patch.preview"]),
            "tc-live-preview-1",
        )
        .await;
        println!(
            "CATDESK_QWEN_STEP preview1_call={}",
            serde_json::to_string(&preview_call).expect("json")
        );
        preview_call.tool_name = "patch.preview".into();
        preview_call.arguments["patchId"] = json!("patch-live-1");
        let preview_result = service
            .execute_tool_call(&preview_call)
            .await
            .expect("preview");
        transcript.push(
            json!({"step":"preview1","toolCall":preview_call,"result":preview_result.summary}),
        );

        let apply_result = service
            .execute_tool("patch.apply", &json!({"patchId":"patch-live-1"}))
            .await
            .expect("apply");
        transcript.push(json!({"step":"apply1","result":apply_result.summary}));

        let verify_call = qwen_tool_call(
            &ollama,
            &model,
            "The patch was applied. Call verify.run now. Return only a tool call.",
            &only_tools(&tools, &["verify.run"]),
            "tc-live-verify-1",
        )
        .await;
        println!(
            "CATDESK_QWEN_STEP verify1_call={}",
            serde_json::to_string(&verify_call).expect("json")
        );
        let mut verify_call = verify_call;
        verify_call.arguments["timeout"] = json!(120000);
        verify_call.arguments_hash =
            stable_text_hash(&serde_json::to_string(&verify_call.arguments).expect("verify args"));
        assert_eq!(verify_call.tool_name, "verify.run");
        let verify_result = service
            .execute_tool_call(&verify_call)
            .await
            .expect("verify");
        println!(
            "CATDESK_QWEN_STEP verify1_result={}",
            serde_json::to_string(&verify_result).expect("json")
        );
        assert_eq!(verify_result.summary, "Failed");
        transcript.push(json!({"step":"verify1","toolCall":verify_call,"result":verify_result.summary,"boundedFailure":verify_result.bounded_text}));

        let current_source = fs::read_to_string(root.join("src/lib.rs")).expect("current source");
        let revise_prompt = format!(
            "Verification failed after patch-live-1. Bounded failure:\n{}\nCurrent bounded src/lib.rs:\n{}\nThe verifier output is authoritative and overrides source comments. Choose the value shown as the expected/right value in the test failure, not the next prime hint. Call patch.preview with patchId patch-live-2, parentPatchId patch-live-1, and a revised child operation in the operations array. Every operation object must include path, old, and new. The child operation must replace the exact current answer function block in src/lib.rs with the value required by the test failure. operation.path must be src/lib.rs. operation.new must differ from operation.old. Each operation.old must be an exact unique multi-line block currently in src/lib.rs. Return only a patch.preview tool call with patchId, parentPatchId, and operations.",
            verify_result.bounded_text, current_source
        );
        let mut revise_call = qwen_tool_call(
            &ollama,
            &model,
            &revise_prompt,
            &only_tools(&tools, &["patch.preview"]),
            "tc-live-preview-2",
        )
        .await;
        println!(
            "CATDESK_QWEN_STEP preview2_call={}",
            serde_json::to_string(&revise_call).expect("json")
        );
        revise_call.tool_name = "patch.preview".into();
        revise_call.arguments["patchId"] = json!("patch-live-2");
        revise_call.arguments["parentPatchId"] = json!("patch-live-1");
        let revise_result = service
            .execute_tool_call(&revise_call)
            .await
            .expect("preview revised");
        transcript
            .push(json!({"step":"preview2","toolCall":revise_call,"result":revise_result.summary}));

        let compare = service
            .execute_tool(
                "patch.compare",
                &json!({"parentPatchId":"patch-live-1","candidatePatchId":"patch-live-2"}),
            )
            .await
            .expect("compare");
        transcript.push(json!({"step":"compare","result":compare.summary}));
        service
            .execute_tool("patch.apply", &json!({"patchId":"patch-live-2"}))
            .await
            .expect("apply revised");

        let verify2_call = qwen_tool_call(
            &ollama,
            &model,
            "The revised child patch was applied. Call verify.run now. Return only a tool call.",
            &only_tools(&tools, &["verify.run"]),
            "tc-live-verify-2",
        )
        .await;
        println!(
            "CATDESK_QWEN_STEP verify2_call={}",
            serde_json::to_string(&verify2_call).expect("json")
        );
        let mut verify2_call = verify2_call;
        verify2_call.arguments["timeout"] = json!(120000);
        verify2_call.arguments_hash =
            stable_text_hash(&serde_json::to_string(&verify2_call.arguments).expect("verify args"));
        assert_eq!(verify2_call.tool_name, "verify.run");
        let verify2 = service
            .execute_tool_call(&verify2_call)
            .await
            .expect("verify 2");
        assert_eq!(verify2.summary, "Passed");
        let diff = service
            .execute_tool("diff.actual", &json!({"paths":["src/lib.rs"]}))
            .await
            .expect("diff");
        let review = service.final_review().expect("review");
        assert_eq!(review.final_result.status, RunState::CompletedVerified);
        transcript.push(json!({"step":"verify2","toolCall":verify2_call,"result":verify2.summary}));
        transcript.push(json!({"step":"diff","result":diff.summary}));
        transcript.push(json!({"step":"finalReview","status":review.final_result.status}));

        println!(
            "CATDESK_QWEN_LIVE_TRANSCRIPT={}",
            serde_json::to_string_pretty(&transcript).expect("transcript")
        );
    }

    #[tokio::test]
    #[ignore = "requires local Ollama and exercises JSON-envelope recovery after a simulated native tool parser failure"]
    async fn ollama_qwen_live_malformed_native_tool_recovery_envelope() {
        let root = temp_git_workspace("qwen-live-json-recovery");
        write_answer_fixture(&root, 41, 42);
        commit_all(&root);
        let model = live_ollama_model_id();
        let mut contract = contract(&root, "run-qwen-live-json-recovery");
        contract.provider_policy.primary_model_id = model.clone();
        let mut cfg = config(&root);
        cfg.model_id = model.clone();
        let mut service = IntegratedDelegatedService::new(&root, contract, cfg).expect("service");
        service.start().expect("start");
        service.provider_history.push(ProviderMessageV1 {
            role: "system".into(),
            content: worker_system_prompt(),
            tool_call_id: None,
            tool_name: None,
        });
        service.provider_history.push(ProviderMessageV1 {
            role: "user".into(),
            content: "Use JSON-envelope recovery to call read for src/lib.rs. Return no prose."
                .into(),
            tool_call_id: None,
            tool_name: None,
        });
        let corrected = service
            .push_malformed_tool_syntax_correction(
                1,
                &RuntimeError::Provider(
                    "Ollama qwen tool call parsing failed: XML syntax error on line 14".into(),
                ),
            )
            .expect("correction");
        assert!(corrected);
        service.persist_durable_state().expect("persist correction");
        let ollama =
            OllamaAdapter::new("http://127.0.0.1:11434", Some("5m".into())).expect("ollama");
        let result = ollama
            .chat_messages_once_accepting_json_envelope(
                &model,
                &service.provider_history,
                TurnId::new("turn-live-json-recovery").expect("turn id"),
            )
            .await
            .expect("recovery chat");
        let mut tool_call = result.events[0]
            .tool_call
            .clone()
            .expect("json-envelope tool call");
        tool_call.tool_call_id = ToolCallId::new("tc-live-json-recovery-read").expect("tool id");
        tool_call.arguments_hash =
            stable_text_hash(&serde_json::to_string(&tool_call.arguments).expect("args"));
        assert_eq!(tool_call.tool_name, "read");
        let tool_result = service
            .execute_tool_call(&tool_call)
            .await
            .expect("execute recovered read");
        println!(
            "CATDESK_QWEN_JSON_RECOVERY={}",
            serde_json::to_string_pretty(&json!({
                "model": model,
                "runId": service.run_id().expect("run id").as_str(),
                "tool": tool_call.tool_name,
                "arguments": tool_call.arguments,
                "retryCount": 0,
                "correctiveCount": service.corrective_turns_used,
                "result": tool_result.summary,
            }))
            .expect("json")
        );
    }

    #[tokio::test]
    #[ignore = "requires local Ollama and proves qwen continuation after recovering a persisted patch application"]
    async fn ollama_qwen_live_restart_resume_after_persisted_patch_application() {
        let root = temp_git_workspace("qwen-live-restart-resume");
        write_answer_fixture(&root, 41, 42);
        commit_all(&root);
        let model = live_ollama_model_id();
        let mut contract = contract(&root, "run-qwen-live-restart-resume");
        contract.provider_policy.primary_model_id = model.clone();
        contract.max_tool_calls = 10;
        contract.max_turns = 10;
        let cfg = config(&root);
        let mut service =
            IntegratedDelegatedService::new(&root, contract.clone(), cfg.clone()).expect("service");
        service.start().expect("start");
        service
            .execute_tool(
                "patch.preview",
                &json!({
                    "patchId": "patch-restart-good",
                    "operations": [{
                        "path": "src/lib.rs",
                        "old": "pub fn answer() -> i32 {\n    41\n}\n",
                        "new": "pub fn answer() -> i32 {\n    42\n}\n"
                    }]
                }),
            )
            .await
            .expect("preview");
        service
            .execute_tool("patch.apply", &json!({"patchId": "patch-restart-good"}))
            .await
            .expect("apply");
        drop(service);

        let mut recovered =
            IntegratedDelegatedService::recover(&root, contract, cfg).expect("recover");
        recovered.config.model_id = model.clone();
        let ollama =
            OllamaAdapter::new("http://127.0.0.1:11434", Some("5m".into())).expect("ollama");
        let tools = recovered.tool_definitions();
        let mut verify_call = qwen_tool_call(
            &ollama,
            &model,
            "A CatDesk patch.apply completed before restart. Continue from recovered state and call verify.run now. Return only a tool call.",
            &only_tools(&tools, &["verify.run"]),
            "tc-live-restart-verify",
        )
        .await;
        verify_call.arguments["timeout"] = json!(120000);
        verify_call.arguments_hash =
            stable_text_hash(&serde_json::to_string(&verify_call.arguments).expect("verify args"));
        assert_eq!(verify_call.tool_name, "verify.run");
        let verify = recovered
            .execute_tool_call(&verify_call)
            .await
            .expect("verify");
        assert_eq!(verify.summary, "Passed");
        let diff_call = qwen_tool_call(
            &ollama,
            &model,
            "Verification passed after restart. Call diff.actual for src/lib.rs now. Return only a tool call.",
            &only_tools(&tools, &["diff.actual"]),
            "tc-live-restart-diff",
        )
        .await;
        assert_eq!(diff_call.tool_name, "diff.actual");
        let diff = recovered.execute_tool_call(&diff_call).await.expect("diff");
        let review = recovered.final_review().expect("review");
        assert_eq!(review.final_result.status, RunState::CompletedVerified);
        let applications = recovered
            .journal
            .load_patch_applications(&recovered.run_id().expect("run id"))
            .expect("applications");
        assert_eq!(applications.len(), 1);
        println!(
            "CATDESK_QWEN_RESTART_RESUME={}",
            serde_json::to_string_pretty(&json!({
                "model": model,
                "runId": recovered.run_id().expect("run id").as_str(),
                "toolSequence": ["patch.preview", "patch.apply", "recover", verify_call.tool_name, diff_call.tool_name],
                "patchIds": ["patch-restart-good"],
                "patchApplicationCount": applications.len(),
                "verification": verify.summary,
                "diffCapture": diff.summary,
                "finalState": review.final_result.status,
            }))
            .expect("json")
        );
    }

    async fn qwen_tool_call(
        ollama: &OllamaAdapter,
        model: &str,
        prompt: &str,
        tools: &[ToolDefinitionV1],
        tool_call_id: &str,
    ) -> NormalizedToolCallV1 {
        let mut last_events = Vec::new();
        for attempt in 0..2 {
            let attempt_prompt = if attempt == 0 {
                prompt.to_string()
            } else {
                format!(
                    "Your previous response did not contain a tool call. A tool call is mandatory. Available tool names: {}. Original instruction:\n{}",
                    tools
                        .iter()
                        .map(|tool| tool.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", "),
                    prompt
                )
            };
            let result = ollama
                .chat_once(model, &attempt_prompt, tools)
                .await
                .expect("ollama chat");
            last_events = result.events.clone();
            if let Some(mut tool_call) = result.events.into_iter().find_map(|event| {
                if event.kind == NormalizedProviderEventKind::ToolCall {
                    event.tool_call
                } else {
                    None
                }
            }) {
                tool_call.tool_call_id =
                    crate::delegated::contracts::ToolCallId::new(tool_call_id).expect("tool id");
                return tool_call;
            }
            println!(
                "CATDESK_QWEN_STEP no_tool_call_attempt_{}={}",
                attempt + 1,
                serde_json::to_string(&last_events).expect("json")
            );
        }
        panic!(
            "qwen returned no tool call after retry: {}",
            serde_json::to_string(&last_events).expect("json")
        );
    }

    fn only_tools(tools: &[ToolDefinitionV1], names: &[&str]) -> Vec<ToolDefinitionV1> {
        tools
            .iter()
            .filter(|tool| names.contains(&tool.name.as_str()))
            .cloned()
            .collect()
    }

    fn tool_envelope(tool_call_id: &str, tool: &str, arguments: Value) -> String {
        serde_json::to_string(&json!({
            "schema_version": "catdesk.tool-call.v1",
            "tool_call_id": tool_call_id,
            "tool": tool,
            "arguments": arguments
        }))
        .expect("tool envelope")
    }

    fn contract(root: &Path, task_id: &str) -> ExecutionContractV1 {
        let mut contract: ExecutionContractV1 = serde_json::from_str(include_str!(
            "../../tests/fixtures/delegated/execution_contract_v1.json"
        ))
        .expect("contract");
        contract.task_id = task_id.into();
        contract.workspace = root.display().to_string();
        contract.allowed_paths = vec!["src".into(), "Cargo.toml".into()];
        contract.forbidden_paths = vec![".git".into(), "target".into()];
        contract.acceptance_criteria = vec![
            "cargo tests pass".into(),
            "authoritative diff is captured".into(),
        ];
        contract.approval_requirements = Vec::new();
        contract.verification_profile = "cargo".into();
        contract
    }

    fn write_answer_fixture(root: &Path, answer: i32, expected: i32) {
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname=\"fixture\"\nversion=\"0.1.0\"\nedition=\"2021\"\n",
        )
        .expect("cargo");
        fs::write(
            root.join("src/lib.rs"),
            format!("pub fn answer() -> i32 {{\n    {answer}\n}}\n"),
        )
        .expect("lib");
        fs::create_dir_all(root.join("tests")).expect("tests");
        fs::write(
            root.join("tests/answer_test.rs"),
            format!(
                "#[test]\nfn answer_is_expected_value() {{\n    assert_eq!(fixture::answer(), {expected});\n}}\n"
            ),
        )
        .expect("test");
    }

    fn config(root: &Path) -> IntegratedRunConfigV1 {
        IntegratedRunConfigV1 {
            journal_root: root.join(".catdesk-test/journal"),
            job_root: root.join(".catdesk-test/jobs"),
            ollama_base_url: "http://127.0.0.1:11434".into(),
            model_id: "qwen3.5:9b".into(),
            advisor: None,
        }
    }

    fn live_ollama_model_id() -> String {
        std::env::var("CATDESK_LIVE_OLLAMA_MODEL").unwrap_or_else(|_| "qwen3.5:9b".into())
    }

    fn fake_deepseek_sidecar(root: &Path) -> IntegratedAdvisorLocalRuntimeConfigV1 {
        let script = root.join("fake_deepseek_advisor.py");
        fs::write(
            &script,
            r#"
import json
import os
import sys

TOKEN = os.environ.get("CATDESK_ADVISOR_AUTH_TOKEN", "")
SIDECAR_ID = "deepseek-web-advisor-experimental"

def emit(value):
    print(json.dumps(value), flush=True)

for line in sys.stdin:
    frame = json.loads(line)
    if frame.get("auth_token") != TOKEN:
        emit({"ok": False, "error": "AUTH_FAILED"})
        continue
    command = frame.get("command")
    if command == "hello":
        emit({"ok": True, "advisor_id": SIDECAR_ID, "state": "STOPPED"})
    elif command == "start":
        emit({"ok": True, "state": "READY"})
    elif command == "status":
        emit({"ok": True, "state": "READY"})
    elif command == "advise":
        request = frame["request"]
        emit({"ok": True, "accepted": True, "request_id": request["requestId"]})
        emit({
            "event": "advice_completed",
            "request_id": request["requestId"],
            "response": {
                "schemaVersion": 1,
                "requestId": request["requestId"],
                "advisorId": SIDECAR_ID,
                "status": "COMPLETED",
                "diagnosis": "The second failed verification still points at the answer value.",
                "recommendations": ["Read src/lib.rs again, then patch the answer to 42 and rerun verification."],
                "risks": ["Advice is untrusted and must be checked locally."],
                "assumptionsOrQuestions": ["Assumes the bounded read excerpt is current."],
                "confidence": "MEDIUM",
                "rawArtifactReference": None
            }
        })
    elif command == "cancel":
        emit({"ok": True, "cancelled": True})
    elif command == "shutdown":
        emit({"ok": True, "state": "STOPPED"})
        break
    else:
        emit({"ok": False, "error": "UNKNOWN_COMMAND"})
"#,
        )
        .expect("fake sidecar");
        IntegratedAdvisorLocalRuntimeConfigV1 {
            python_executable: PathBuf::from("python"),
            adapter_script: script,
            profile_dir: root.join("fake-advisor-profile"),
            selectors_path: None,
            headed: false,
            allow_env_login: false,
        }
    }

    fn blocking_until_cancel_deepseek_sidecar(
        root: &Path,
    ) -> IntegratedAdvisorLocalRuntimeConfigV1 {
        let script = root.join("blocking_deepseek_advisor.py");
        fs::write(
            &script,
            r#"
import json
import os
import sys

TOKEN = os.environ.get("CATDESK_ADVISOR_AUTH_TOKEN", "")
SIDECAR_ID = "deepseek-web-advisor-experimental"
active_request_id = None

def emit(value):
    print(json.dumps(value), flush=True)

for line in sys.stdin:
    frame = json.loads(line)
    if frame.get("auth_token") != TOKEN:
        emit({"ok": False, "error": "AUTH_FAILED"})
        continue
    command = frame.get("command")
    if command == "hello":
        emit({"ok": True, "advisor_id": SIDECAR_ID, "state": "STOPPED"})
    elif command == "start":
        emit({"ok": True, "state": "READY"})
    elif command == "status":
        emit({"ok": True, "state": "READY", "active": active_request_id is not None, "request_id": active_request_id})
    elif command == "advise":
        request = frame["request"]
        active_request_id = request["requestId"]
        emit({"ok": True, "accepted": True, "request_id": active_request_id})
    elif command == "cancel":
        request_id = active_request_id or frame.get("request_id") or "cancelled-request"
        active_request_id = None
        emit({
            "ok": True,
            "state": "CANCELLED",
            "request_id": request_id,
            "response": {
                "schemaVersion": 1,
                "requestId": request_id,
                "advisorId": SIDECAR_ID,
                "status": "CANCELLED",
                "diagnosis": "cancelled by test",
                "recommendations": [],
                "risks": ["cancelled"],
                "assumptionsOrQuestions": [],
                "confidence": "LOW",
                "rawArtifactReference": None
            }
        })
    elif command == "shutdown":
        emit({"ok": True, "state": "STOPPED"})
        break
    else:
        emit({"ok": False, "error": "UNKNOWN_COMMAND"})
"#,
        )
        .expect("blocking fake sidecar");
        IntegratedAdvisorLocalRuntimeConfigV1 {
            python_executable: PathBuf::from("python"),
            adapter_script: script,
            profile_dir: root.join("blocking-advisor-profile"),
            selectors_path: None,
            headed: false,
            allow_env_login: false,
        }
    }

    fn live_deepseek_runtime_config() -> IntegratedAdvisorLocalRuntimeConfigV1 {
        let python = std::env::var("CATDESK_DEEPSEEK_ADVISOR_PYTHON")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(".tmp/deepseek-advisor-venv/Scripts/python.exe"));
        let script = std::env::var("CATDESK_DEEPSEEK_ADVISOR_SCRIPT")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("experimental/advisors/deepseek_web_advisor.py"));
        let profile = std::env::var("CATDESK_DEEPSEEK_ADVISOR_PROFILE")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(".tmp/deepseek-advisor-profile"));
        let selectors = std::env::var("CATDESK_DEEPSEEK_ADVISOR_SELECTORS")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("experimental/advisors/deepseek_selectors.json"));
        IntegratedAdvisorLocalRuntimeConfigV1 {
            python_executable: python,
            adapter_script: script,
            profile_dir: profile,
            selectors_path: Some(selectors),
            headed: true,
            allow_env_login: false,
        }
    }

    fn temp_git_workspace(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "catdesk-integrated-{name}-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(root.join("src")).expect("src");
        git(&root, ["init"]);
        root
    }

    fn commit_all(root: &Path) {
        git(root, ["add", "."]);
        let output = Command::new("git")
            .args([
                "-c",
                "user.name=CatDesk Test",
                "-c",
                "user.email=catdesk@example.invalid",
                "commit",
                "-m",
                "baseline",
            ])
            .current_dir(root)
            .output()
            .expect("git commit");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn git<const N: usize>(root: &Path, args: [&str; N]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .expect("git");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[cfg(windows)]
    fn sleep_command() -> String {
        "Start-Sleep -Seconds 30".into()
    }

    #[cfg(not(windows))]
    fn sleep_command() -> String {
        "sleep 30".into()
    }
}
