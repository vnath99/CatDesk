use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::delegated::context::{ContextBudgetPolicyV1, ContextBuilderV1};
use crate::delegated::contracts::{ExecutionContractV1, PatchId, RunId, RunState, TurnId};
use crate::delegated::coordinator::{
    FinalReviewPackageV1, RunCoordinator, VerificationStatusV1, VerificationSummaryV1,
};
use crate::delegated::events::{EventCursor, EventEnvelopeV1, EventPayloadV1, LifecycleEvent};
use crate::delegated::job_manager::{JobId, JobManager, JobSpecV1, LogStream};
use crate::delegated::journal::{
    DelegatedJournal, ToolCallRecordV1, ToolCallStatus, ToolMutationKind,
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
    NormalizedToolCallV1, ProviderType, RuntimeError, ToolDefinitionV1, catdesk_tool_definitions,
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
        self.journal
            .record_tool_call_requested(ToolCallRecordV1 {
                schema_version: EXECUTION_CONTRACT_SCHEMA_VERSION,
                run_id: run_id.clone(),
                tool_call_id: call.tool_call_id.clone(),
                tool_name: call.tool_name.clone(),
                request_hash: stable_text_hash(&serde_json::to_string(&call.arguments)?),
                arguments_hash: call.arguments_hash.clone(),
                mutation_kind,
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
        let result = self.execute_tool(&call.tool_name, &call.arguments).await?;
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
        self.append_event(
            LifecycleEvent::Delta,
            EventPayloadV1::Delta {
                text: format!("tool {} completed: {}", call.tool_name, result.summary),
            },
        )?;
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
        self.patch_proposals
            .insert(proposal.patch_id.as_str().to_string(), proposal);
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
            .unwrap_or(120_000);
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
        let cwd = args
            .get("cwd")
            .and_then(Value::as_str)
            .map(|path| self.workspace_root.join(path))
            .unwrap_or_else(|| self.workspace_root.clone());
        let mut spec = JobSpecV1::new(command, cwd);
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
    #[ignore = "requires local Ollama with qwen3.5:9b and runs a full disposable model-tool-model flow"]
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
