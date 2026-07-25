use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::delegated::context::{ContextBudgetPolicyV1, ContextBuilderV1};
use crate::delegated::contracts::{
    ApprovalRequirementKind, ExecutionContractV1, PatchId, RunId, RunState, ToolCallId, TurnId,
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
use crate::delegated::runtime::{
    NormalizedProviderEventKind, NormalizedToolCallV1, OllamaAdapter, ProviderMessageV1,
    ProviderType, RuntimeError, ToolDefinitionV1, catdesk_tool_definitions,
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
    next_event_sequence: u64,
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
    next_event_sequence: u64,
}

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
            next_event_sequence: 1,
        })
    }

    pub fn tool_definitions(&self) -> Vec<ToolDefinitionV1> {
        catdesk_tool_definitions()
    }

    pub fn provider_policy(&self) -> ProviderRoutingPolicyV1 {
        ProviderRoutingPolicyV1 {
            primary_provider_id: "ollama".into(),
            fallback_provider_ids: vec!["fake".into()],
            disclosure_policy: ContextBudgetPolicyV1::local_default().disclosure_policy,
        }
    }

    pub async fn run_ollama_worker_loop(
        &mut self,
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
            let result = ollama
                .chat_messages_once(
                    &self.config.model_id,
                    &self.provider_history,
                    &allowed_tools,
                    turn_id,
                )
                .await?;
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
                        let content = match tool_result {
                            Ok(tool_result) => format!(
                                "Tool result for {}:\n{}",
                                call.tool_name, tool_result.bounded_text
                            ),
                            Err(error) => {
                                let message = format!(
                                    "Tool {} failed under CatDesk policy and was journaled. Error: {error:?}. Choose a permitted next CatDesk tool call; do not retry the same invalid request.",
                                    call.tool_name
                                );
                                if matches!(
                                    call.tool_name.as_str(),
                                    "patch.apply" | "job.start" | "job.cancel"
                                ) {
                                    return Err(IntegratedError::Tool(message));
                                }
                                message
                            }
                        };
                        self.provider_history.push(ProviderMessageV1 {
                            role: "user".into(),
                            content,
                            tool_call_id: Some(call.tool_call_id.as_str().to_string()),
                            tool_name: Some(call.tool_name),
                        });
                        saw_tool = true;
                    }
                    NormalizedProviderEventKind::TextDelta
                    | NormalizedProviderEventKind::CompletionClaim => {
                        let text = event.text.unwrap_or_default();
                        self.provider_history.push(ProviderMessageV1 {
                            role: "assistant".into(),
                            content: text.clone(),
                            tool_call_id: None,
                            tool_name: None,
                        });
                        if self.last_verification.as_ref().is_some_and(|verification| {
                            verification.status == VerificationStatusV1::Passed
                        }) && self.last_diff.is_some()
                        {
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
                return Err(IntegratedError::Tool(
                    "provider stopped before CatDesk verification and diff completed".into(),
                ));
            }
            self.persist_durable_state()?;
        }
        Err(IntegratedError::Tool("turn budget exceeded".into()))
    }

    fn production_worker_tools(&self) -> Vec<ToolDefinitionV1> {
        let needs_jobs = self.contract.objective.to_ascii_lowercase().contains("job")
            || self
                .contract
                .ordered_steps
                .iter()
                .any(|step| step.to_ascii_lowercase().contains("job"));
        self.tool_definitions()
            .into_iter()
            .filter(|tool| needs_jobs || !tool.name.starts_with("job."))
            .collect()
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
                service.next_event_sequence =
                    service.next_event_sequence.max(state.next_event_sequence);
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
        self.append_event(
            LifecycleEvent::Delta,
            EventPayloadV1::Delta {
                text: format!("tool {} completed: {}", call.tool_name, result.summary),
            },
        )?;
        self.persist_durable_state()?;
        Ok(result)
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
        if call.tool_name == "job.start" {
            let profile = call
                .arguments
                .get("commandProfile")
                .and_then(Value::as_str)
                .or_else(|| {
                    self.contract
                        .allowed_command_profiles
                        .first()
                        .map(String::as_str)
                })
                .unwrap_or("default");
            if !self
                .contract
                .allowed_command_profiles
                .iter()
                .any(|allowed| allowed == profile)
            {
                return Err(IntegratedError::Tool(format!(
                    "command profile {profile} is not allowed by the execution contract"
                )));
            }
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
            next_event_sequence: self.next_event_sequence,
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
        let event = EventEnvelopeV1 {
            schema_version: EXECUTION_CONTRACT_SCHEMA_VERSION,
            event_sequence: self.next_event_sequence,
            run_id: RunId::new(self.contract.task_id.clone()).map_err(IntegratedError::Tool)?,
            worker_session_id: None,
            turn_id: None,
            item_id: None,
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

    fn tool_read(&self, args: &Value) -> Result<IntegratedToolResultV1, IntegratedError> {
        let path = string_arg(args, "path")?;
        let output = workspace_tools::read_file(&self.workspace_root.display().to_string(), &path)
            .map_err(IntegratedError::Tool)?;
        Ok(tool_result(
            "read",
            format!("read {} bytes from {}", output.bytes, output.path),
            serde_json::to_value(&output)?,
            output.render_text(),
        ))
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
        Ok(PatchProposalV1 {
            schema_version: EXECUTION_CONTRACT_SCHEMA_VERSION,
            patch_id: PatchId::new(patch_id).map_err(IntegratedError::Tool)?,
            parent_patch_id,
            run_id: RunId::new(self.contract.task_id.clone()).map_err(IntegratedError::Tool)?,
            turn_id: TurnId::new(format!("turn-{}", self.next_event_sequence))
                .map_err(IntegratedError::Tool)?,
            base_snapshot_hash: stable_text_hash(&serde_json::to_string(&target_paths)?),
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
    use crate::delegated::runtime::{NormalizedProviderEventKind, OllamaAdapter};
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
    #[ignore = "requires local Ollama with qwen3.5:9b and runs a full disposable model-tool-model flow"]
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

        let mut contract = contract(&root, "run-t0023b-qwen-production");
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
            "cargo verification passes".into(),
            "src/lib.rs contains the implementation fix".into(),
            "authoritative diff is captured".into(),
            "do not add or edit tests".into(),
        ];
        contract.max_turns = 18;
        contract.max_tool_calls = 18;
        contract.max_elapsed_seconds = 240;
        let mut service =
            IntegratedDelegatedService::new(&root, contract, config(&root)).expect("service");
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
    #[ignore = "requires local Ollama with qwen3.5:9b and runs a scripted disposable model-tool-model flow"]
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

        let contract = contract(&root, "run-t0023a-qwen-live");
        let mut service = IntegratedDelegatedService::new(&root, contract.clone(), config(&root))
            .expect("service");
        service.start().expect("start");
        let ollama =
            OllamaAdapter::new("http://127.0.0.1:11434", Some("5m".into())).expect("ollama");
        let tools = service.tool_definitions();
        let mut transcript = Vec::new();

        let read_call = qwen_tool_call(
            &ollama,
            "qwen3.5:9b",
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
            "qwen3.5:9b",
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
            "qwen3.5:9b",
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
            "qwen3.5:9b",
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
            "qwen3.5:9b",
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

    fn contract(root: &Path, task_id: &str) -> ExecutionContractV1 {
        let mut contract: ExecutionContractV1 = serde_json::from_str(include_str!(
            "../../tests/fixtures/delegated/execution_contract_v1.json"
        ))
        .expect("contract");
        contract.task_id = task_id.into();
        contract.workspace = root.display().to_string();
        contract.allowed_paths = vec!["src".into(), "Cargo.toml".into()];
        contract.forbidden_paths = vec![".git".into(), "target".into()];
        contract.approval_requirements = Vec::new();
        contract.verification_profile = "cargo".into();
        contract
    }

    fn config(root: &Path) -> IntegratedRunConfigV1 {
        IntegratedRunConfigV1 {
            journal_root: root.join(".catdesk-test/journal"),
            job_root: root.join(".catdesk-test/jobs"),
            ollama_base_url: "http://127.0.0.1:11434".into(),
            model_id: "qwen3.5:9b".into(),
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
