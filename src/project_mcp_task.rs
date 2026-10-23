//! Operator-assigned isolated worktree editing for a fixed project connection.
//! Reuses delegated contracts, patch preimages/journal and project writer locks.
//! There is no provider launch, arbitrary command or ownership of the native root.

use crate::delegated::{
    autonomy_projects::{AutonomousProjectRegistryStoreV1, ResolvedProjectScopeV1},
    autonomy_runtime::{ProjectWriterGuard, acquire_project_writer_lock},
    autonomy_supervisor::web_task_worktree_witness,
    contracts::{ExecutionContractV1, RunId, ToolCallId, validate_contract},
    integrated::{IntegratedDelegatedService, IntegratedRunConfigV1},
    journal::{DelegatedJournal, ToolCallStatus},
    patch_engine::stable_text_hash,
    runtime::NormalizedToolCallV1,
};
use crate::project_task_command::{CommandProfile, CommandResult};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

pub(crate) const TOOL: &str = "catdesk_project_task_tool";
const TOOLS: &[&str] = &[
    "read",
    "search",
    "patch.preview",
    "patch.apply",
    "patch.compare",
    "diff.actual",
    "verify.command",
];

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Assignment {
    schema_version: u32,
    project_id: String,
    base_commit: String,
    contract: ExecutionContractV1,
    #[serde(default)]
    command_profiles: Vec<CommandProfile>,
}

struct TaskState {
    service: IntegratedDelegatedService,
    writer: Option<ProjectWriterGuard>,
    closed: bool,
    uncertain: bool,
}

pub(crate) struct ProjectTask {
    project: ResolvedProjectScopeV1,
    root: PathBuf,
    witness: (PathBuf, PathBuf, String, String),
    contract: ExecutionContractV1,
    journal: DelegatedJournal,
    started: std::time::Instant,
    connection_budget: std::time::Duration,
    command_profiles: Vec<CommandProfile>,
    state: Mutex<TaskState>,
}

fn regular(path: &Path) -> bool {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return false;
    };
    #[cfg(windows)]
    let redirected = {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    };
    #[cfg(not(windows))]
    let redirected = metadata.file_type().is_symlink();
    metadata.is_file() && !redirected
}

fn unregistered_task_root(project: &ResolvedProjectScopeV1, root: &Path) -> Result<(), String> {
    let registry = AutonomousProjectRegistryStoreV1::open_read_only(
        project.control_workspace.join(".catdesk/projects"),
    )
    .and_then(|store| store.load_registry())
    .map_err(|_| "PROJECT_TASK_REGISTRY_UNAVAILABLE")?;
    if registry
        .projects
        .iter()
        .any(|p| root.starts_with(&p.workspace) || p.workspace.starts_with(root))
    {
        return Err("PROJECT_TASK_WORKTREE_ALREADY_REGISTERED".into());
    }
    Ok(())
}

impl ProjectTask {
    #[cfg(test)]
    pub(crate) fn open(
        project: &ResolvedProjectScopeV1,
        assignment: &Path,
    ) -> Result<Arc<Self>, String> {
        Self::open_with_budget(project, assignment, 180)
    }

    pub(crate) fn open_with_budget(
        project: &ResolvedProjectScopeV1,
        assignment: &Path,
        connection_seconds: u64,
    ) -> Result<Arc<Self>, String> {
        if !regular(assignment) {
            return Err("PROJECT_TASK_ASSIGNMENT_UNAVAILABLE".into());
        }
        let mut bytes = Vec::new();
        fs::File::open(assignment)
            .map_err(|_| "PROJECT_TASK_ASSIGNMENT_UNAVAILABLE")?
            .take(128 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "PROJECT_TASK_ASSIGNMENT_UNAVAILABLE")?;
        if bytes.len() > 128 * 1024 {
            return Err("PROJECT_TASK_ASSIGNMENT_TOO_LARGE".into());
        }
        let assignment: Assignment =
            serde_json::from_slice(&bytes).map_err(|_| "PROJECT_TASK_ASSIGNMENT_INVALID")?;
        let contract = assignment.contract;
        let command_profiles = assignment.command_profiles;
        validate_contract(&contract).map_err(|_| "PROJECT_TASK_CONTRACT_INVALID")?;
        if assignment.schema_version != 1
            || assignment.project_id != project.project.project_id
            || !catdesk_wake::protocol::valid_id(&contract.task_id)
            || contract.task_id.len() > 128
            || contract.max_elapsed_seconds > 180
            || contract.max_tool_calls > 256
            || contract.advisor_policy.is_some()
            || contract.approval_requirements.iter().any(|r| r.required)
        {
            return Err("PROJECT_TASK_ASSIGNMENT_INVALID".into());
        }
        if command_profiles.len() > 8 || !(1..=180).contains(&connection_seconds) {
            return Err("PROJECT_TASK_COMMAND_AUTHORITY_INVALID".into());
        }
        let mut ids = std::collections::BTreeSet::new();
        for profile in &command_profiles {
            profile.validate()?;
            if !ids.insert(&profile.id) || !contract.allowed_command_profiles.contains(&profile.id)
            {
                return Err("PROJECT_TASK_COMMAND_PROFILE_NOT_ASSIGNED".into());
            }
        }
        let root = Path::new(&contract.workspace)
            .canonicalize()
            .map_err(|_| "PROJECT_TASK_WORKTREE_UNAVAILABLE")?;
        // Writes always go to a separately assigned linked worktree. The
        // existing native project's registered checkout is never claimed.
        if root == project.project.workspace
            || root.starts_with(&project.control_workspace)
            || project.control_workspace.starts_with(&root)
            || !regular(&root.join(".git"))
        {
            return Err("PROJECT_TASK_REQUIRES_SEPARATE_LINKED_WORKTREE".into());
        }
        unregistered_task_root(project, &root)?;
        let witness = web_task_worktree_witness(&root)?;
        let source = web_task_worktree_witness(&project.project.workspace)?;
        if witness.0 != source.0
            || witness.1 == witness.0
            || witness.1 == source.1
            || witness.2 != assignment.base_commit
            || witness.3 != contract.feature_branch
            || witness.3 == source.3
        {
            return Err("PROJECT_TASK_WORKTREE_IDENTITY_MISMATCH".into());
        }
        let mut writer = acquire_project_writer_lock(&root)
            .map_err(|_| "PROJECT_TASK_WRITER_BUSY_OR_UNRESOLVED")?;
        let state_root = project
            .control_workspace
            .join(".catdesk/project-mcp-tasks")
            .join(&project.project.project_id)
            .join(&contract.task_id);
        let journal_root = state_root.join("journal");
        let journal = DelegatedJournal::open(&journal_root)
            .map_err(|_| "PROJECT_TASK_JOURNAL_UNAVAILABLE")?;
        let run = RunId::new(&contract.task_id).map_err(|_| "PROJECT_TASK_CONTRACT_INVALID")?;
        // A known task may resume its exact contract; malformed/partial state
        // is never treated as an empty new run or permission to replay.
        // DelegatedJournal owns its run-directory encoding. Any prior journal
        // bytes require loading that exact contract; do not guess its path or
        // mistake an incomplete run for a fresh assignment.
        let existing_journal = fs::read_dir(&journal_root)
            .map_err(|_| "PROJECT_TASK_RECOVERY_REQUIRED")?
            .next()
            .is_some();
        if existing_journal {
            let stored = journal
                .load_contract(&run)
                .map_err(|_| "PROJECT_TASK_RECOVERY_REQUIRED")?;
            if stored != contract {
                return Err("PROJECT_TASK_CONTRACT_CHANGED".into());
            }
            if journal
                .load_tool_calls(&run)
                .map_err(|_| "PROJECT_TASK_RECOVERY_REQUIRED")?
                .values()
                .any(|r| !matches!(r.status, ToolCallStatus::Completed | ToolCallStatus::Failed))
            {
                return Err("PROJECT_TASK_RECOVERY_REQUIRED".into());
            }
        }
        let profiles_path = state_root.join("command-profiles.json");
        if profiles_path.exists() {
            if !regular(&profiles_path)
                || fs::metadata(&profiles_path)
                    .map_err(|_| "PROJECT_TASK_COMMAND_AUTHORITY_INVALID")?
                    .len()
                    > 128 * 1024
            {
                return Err("PROJECT_TASK_COMMAND_AUTHORITY_INVALID".into());
            }
            let saved: Vec<CommandProfile> = serde_json::from_slice(
                &fs::read(&profiles_path).map_err(|_| "PROJECT_TASK_COMMAND_AUTHORITY_INVALID")?,
            )
            .map_err(|_| "PROJECT_TASK_COMMAND_AUTHORITY_INVALID")?;
            if saved != command_profiles {
                return Err("PROJECT_TASK_COMMAND_AUTHORITY_CHANGED".into());
            }
        } else if !command_profiles.is_empty() {
            if existing_journal {
                return Err("PROJECT_TASK_COMMAND_AUTHORITY_CHANGED".into());
            }
            use std::io::Write;
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&profiles_path)
                .map_err(|_| "PROJECT_TASK_COMMAND_AUTHORITY_INVALID")?;
            file.write_all(
                &serde_json::to_vec(&command_profiles)
                    .map_err(|_| "PROJECT_TASK_COMMAND_AUTHORITY_INVALID")?,
            )
            .and_then(|_| file.sync_all())
            .map_err(|_| "PROJECT_TASK_COMMAND_AUTHORITY_INVALID")?;
        }
        let mut service = IntegratedDelegatedService::recover(
            &root,
            contract.clone(),
            IntegratedRunConfigV1 {
                journal_root,
                job_root: state_root.join("jobs"),
                ollama_base_url: "http://127.0.0.1:11434".into(),
                model_id: "operator-assigned-chatgpt-web".into(),
                advisor: None,
            },
        )
        .map_err(|_| "PROJECT_TASK_RECOVERY_REQUIRED")?;
        writer
            .begin_writer_turn()
            .map_err(|_| "PROJECT_TASK_WRITER_MARKER_UNAVAILABLE")?;
        service
            .start()
            .map_err(|_| "PROJECT_TASK_START_UNCERTAIN_MARKER_RETAINED")?;
        let connection_budget =
            std::time::Duration::from_secs(connection_seconds.min(contract.max_elapsed_seconds));
        Ok(Arc::new(Self {
            project: project.clone(),
            root,
            witness,
            contract,
            journal,
            started: std::time::Instant::now(),
            connection_budget,
            command_profiles,
            state: Mutex::new(TaskState {
                service,
                writer: Some(writer),
                closed: false,
                uncertain: false,
            }),
        }))
    }

    pub(crate) fn metadata(&self) -> Value {
        json!({"taskId":self.contract.task_id,"executionWorkspace":self.root,
            "registeredWorkspace":self.project.project.workspace,"access":if self.command_profiles.is_empty() {"CONTRACT_PATCH_TOOLS"} else {"CONTRACT_PATCH_AND_COMMAND_PROFILES"},
            "nativeWriterClaimed":false,"isolatedWorktreeWriterClaimed":true,
            "allowedPaths":self.contract.allowed_paths,"allowedTools":self.allowed_tools(),
            "commandsEnabled":!self.command_profiles.is_empty(),
            "commandProfiles":self.command_profiles.iter().map(|p|json!({"id":p.id,"timeoutSeconds":p.timeout_seconds})).collect::<Vec<_>>(),
            "baseCommit":self.witness.2,"branch":self.witness.3})
    }

    pub(crate) fn commands_enabled(&self) -> bool {
        !self.command_profiles.is_empty()
    }

    fn allowed_tools(&self) -> Vec<&'static str> {
        TOOLS
            .iter()
            .copied()
            .filter(|tool| *tool != "verify.command" || !self.command_profiles.is_empty())
            .collect()
    }

    pub(crate) fn schema(&self) -> Value {
        schema(self.allowed_tools())
    }

    fn validate_paths(&self, name: &str, args: &Value) -> Result<(), String> {
        let check = |value: &Value| -> Result<(), String> {
            let path = value
                .as_str()
                .ok_or("PROJECT_TASK_EXPLICIT_PATH_REQUIRED")?;
            // Let the existing patch engine enforce contract path policy; read
            // and diff also need this explicit check (the generic read service
            // otherwise permits the entire workspace).
            if !self.contract.allowed_paths.iter().any(|allowed| {
                path == allowed
                    || (allowed == "." && !path.starts_with('.'))
                    || path
                        .strip_prefix(allowed)
                        .is_some_and(|s| s.starts_with('/'))
            }) || path.split(['/', '\\']).any(|c| {
                c.is_empty()
                    || c == "."
                    || c == ".."
                    || c == ".git"
                    || c == ".catdesk"
                    || c.contains(':')
            }) || self.contract.forbidden_paths.iter().any(|forbidden| {
                path == forbidden
                    || path
                        .strip_prefix(forbidden)
                        .is_some_and(|s| s.starts_with('/'))
            }) {
                return Err("PROJECT_TASK_PATH_OUTSIDE_ASSIGNMENT".into());
            }
            let target = self.root.join(path);
            let existing = if target.exists() {
                target.as_path()
            } else {
                target
                    .parent()
                    .ok_or("PROJECT_TASK_PATH_OUTSIDE_ASSIGNMENT")?
            };
            if !existing
                .canonicalize()
                .map_err(|_| "PROJECT_TASK_PATH_UNAVAILABLE")?
                .starts_with(&self.root)
            {
                return Err("PROJECT_TASK_PATH_OUTSIDE_ASSIGNMENT".into());
            }
            Ok(())
        };
        match name {
            "read" | "search" => check(
                args.get("path")
                    .ok_or("PROJECT_TASK_EXPLICIT_PATH_REQUIRED")?,
            ),
            "patch.preview" => {
                let operations = args
                    .get("operations")
                    .and_then(Value::as_array)
                    .filter(|v| !v.is_empty() && v.len() <= 16)
                    .ok_or("PROJECT_TASK_OPERATIONS_REQUIRED")?;
                for op in operations {
                    check(&op["path"])?;
                }
                if let Some(paths) = args.get("targetPaths") {
                    for path in paths
                        .as_array()
                        .ok_or("PROJECT_TASK_EXPLICIT_PATH_REQUIRED")?
                    {
                        check(path)?;
                    }
                }
                Ok(())
            }
            "diff.actual" => {
                for path in args
                    .get("paths")
                    .and_then(Value::as_array)
                    .filter(|v| !v.is_empty())
                    .ok_or("PROJECT_TASK_EXPLICIT_PATH_REQUIRED")?
                {
                    check(path)?;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    pub(crate) async fn execute(self: &Arc<Self>, args: Value) -> Result<Value, String> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct Call {
            task_id: String,
            call_id: String,
            tool: String,
            arguments: Value,
        }
        let call: Call = serde_json::from_value(args).map_err(|_| "PROJECT_TASK_CALL_INVALID")?;
        if call.task_id != self.contract.task_id || !TOOLS.contains(&call.tool.as_str()) {
            return Err("PROJECT_TASK_CALL_NOT_ASSIGNED".into());
        }
        self.validate_paths(&call.tool, &call.arguments)?;
        let tool_call_id =
            ToolCallId::new(call.call_id).map_err(|_| "PROJECT_TASK_CALL_INVALID")?;
        let task = self.clone();
        let runtime = tokio::runtime::Handle::current();
        tokio::task::spawn_blocking(move || {
            // Only this task is serialized. Cancellation of an HTTP awaiter
            // cannot drop the writer while a synchronous patch is still live.
            let mut state = task
                .state
                .lock()
                .map_err(|_| "PROJECT_TASK_RECOVERY_REQUIRED")?;
            if state.closed || state.uncertain {
                return Err("PROJECT_TASK_RECOVERY_REQUIRED".into());
            }
            unregistered_task_root(&task.project, &task.root)?;
            if task.started.elapsed() >= task.connection_budget {
                return Err("PROJECT_TASK_CONNECTION_BUDGET_EXPIRED".into());
            }
            if web_task_worktree_witness(&task.root)? != task.witness {
                return Err("PROJECT_TASK_WORKTREE_CHANGED".into());
            }
            let run =
                RunId::new(&task.contract.task_id).map_err(|_| "PROJECT_TASK_CALL_INVALID")?;
            let prior = task
                .journal
                .load_tool_calls(&run)
                .map_err(|_| "PROJECT_TASK_RECOVERY_REQUIRED")?;
            let arguments_hash = stable_text_hash(
                &serde_json::to_string(&call.arguments).map_err(|_| "PROJECT_TASK_CALL_INVALID")?,
            );
            if let Some(record) = prior.get(tool_call_id.as_str()) {
                if record.tool_name != call.tool || record.arguments_hash != arguments_hash {
                    return Err("PROJECT_TASK_CALL_ID_CONFLICT".into());
                }
                return Err(if record.status == ToolCallStatus::Completed {
                    "PROJECT_TASK_CALL_ALREADY_COMPLETED"
                } else {
                    "PROJECT_TASK_CALL_RECORDED_NO_REPLAY"
                }
                .into());
            }
            if prior.len() >= task.contract.max_tool_calls as usize {
                return Err("PROJECT_TASK_TOOL_BUDGET_EXCEEDED".into());
            }
            let request = NormalizedToolCallV1 {
                tool_call_id,
                tool_name: call.tool,
                arguments: call.arguments,
                arguments_hash,
            };
            if request.tool_name == "verify.command" {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Select {
                    profile: String,
                }
                let selected: Select = serde_json::from_value(request.arguments.clone())
                    .map_err(|_| "PROJECT_TASK_COMMAND_SELECTION_INVALID")?;
                let profile = task
                    .command_profiles
                    .iter()
                    .find(|p| p.id == selected.profile)
                    .ok_or("PROJECT_TASK_COMMAND_PROFILE_NOT_ASSIGNED")?;
                let budget = task
                    .connection_budget
                    .saturating_sub(task.started.elapsed())
                    .saturating_sub(std::time::Duration::from_secs(20));
                if budget < std::time::Duration::from_secs(1) {
                    return Err("PROJECT_COMMAND_BUDGET_EXPIRED".into());
                }
                // From the first durable request until confirmed result/exit,
                // any unexpected failure must retain the existing writer marker.
                state.uncertain = true;
                let result = task.execute_command(&request, &run, profile, budget);
                state.uncertain = task
                    .journal
                    .load_tool_calls(&run)
                    .map(|calls| {
                        calls.values().any(|r| {
                            !matches!(r.status, ToolCallStatus::Completed | ToolCallStatus::Failed)
                        })
                    })
                    .unwrap_or(true);
                return result;
            }
            match runtime.block_on(state.service.execute_tool_call(&request)) {
                Ok(result) => serde_json::to_value(result)
                    .map_err(|_| "PROJECT_TASK_RESULT_UNAVAILABLE".into()),
                Err(_) => {
                    state.uncertain = task
                        .journal
                        .load_tool_calls(&run)
                        .map(|calls| {
                            calls.values().any(|r| {
                                !matches!(
                                    r.status,
                                    ToolCallStatus::Completed | ToolCallStatus::Failed
                                )
                            })
                        })
                        .unwrap_or(true);
                    Err("PROJECT_TASK_TOOL_REJECTED_SEE_DURABLE_JOURNAL_NO_BLIND_RETRY".into())
                }
            }
        })
        .await
        .map_err(|_| "PROJECT_TASK_RECOVERY_REQUIRED")?
    }

    fn execute_command(
        &self,
        request: &NormalizedToolCallV1,
        run: &RunId,
        profile: &CommandProfile,
        budget: std::time::Duration,
    ) -> Result<Value, String> {
        use crate::delegated::journal::{ToolCallRecordV1, ToolMutationKind};
        self.journal
            .record_tool_call_requested(ToolCallRecordV1 {
                schema_version: 1,
                run_id: run.clone(),
                tool_call_id: request.tool_call_id.clone(),
                tool_name: request.tool_name.clone(),
                request_hash: request.arguments_hash.clone(),
                arguments_hash: request.arguments_hash.clone(),
                mutation_kind: ToolMutationKind::Mutating,
                status: ToolCallStatus::Requested,
                result_hash: None,
                outcome_summary: None,
            })
            .map_err(|_| "PROJECT_TASK_COMMAND_JOURNAL_UNAVAILABLE")?;
        for phase in [ToolCallStatus::PolicyAllowed, ToolCallStatus::Executing] {
            self.journal
                .transition_tool_call(run, &request.tool_call_id, phase, None, None)
                .map_err(|_| "PROJECT_TASK_COMMAND_JOURNAL_UNAVAILABLE")?;
        }
        let registry = AutonomousProjectRegistryStoreV1::open_read_only(
            self.project.control_workspace.join(".catdesk/projects"),
        )
        .and_then(|s| s.load_registry())
        .map_err(|_| "PROJECT_TASK_REGISTRY_UNAVAILABLE")?;
        let mut deny: Vec<PathBuf> = registry.projects.into_iter().map(|p| p.workspace).collect();
        deny.push(self.project.control_workspace.clone());
        let started_utc = time::OffsetDateTime::now_utc().to_string();
        let result: CommandResult =
            match crate::project_task_command::run(&self.root, &deny, profile, budget) {
                Ok(result) => result,
                Err(reason) => {
                    let _ = self.journal.transition_tool_call(
                        run,
                        &request.tool_call_id,
                        if crate::project_task_command::failed_before_launch(&reason) {
                            ToolCallStatus::Failed
                        } else {
                            ToolCallStatus::OutcomeUnknown
                        },
                        None,
                        Some(reason.clone()),
                    );
                    return Err(reason);
                }
            };
        // Persist enough identity to audit a result independently of its path.
        // These local receipts may contain private paths/output; never publish
        // them wholesale as acceptance evidence.
        let mut receipt =
            serde_json::to_value(&result).map_err(|_| "PROJECT_TASK_COMMAND_RESULT_UNAVAILABLE")?;
        receipt["provenance"] = json!({
            "schemaVersion":1,"projectId":self.project.project.project_id,
            "taskId":self.contract.task_id,"callId":request.tool_call_id,
            "requestSha256":request.arguments_hash,"executionWorkspace":self.root,
            "baseCommit":self.witness.2,"branch":self.witness.3,
            "startedUtc":started_utc,"finishedUtc":time::OffsetDateTime::now_utc().to_string()
        });
        let encoded =
            serde_json::to_vec(&receipt).map_err(|_| "PROJECT_TASK_COMMAND_RESULT_UNAVAILABLE")?;
        let digest = catdesk_wake::protocol::digest(&String::from_utf8_lossy(&encoded));
        let results = self
            .project
            .control_workspace
            .join(".catdesk/project-mcp-tasks")
            .join(&self.project.project.project_id)
            .join(&self.contract.task_id)
            .join("command-results");
        fs::create_dir_all(&results).map_err(|_| "PROJECT_TASK_COMMAND_RESULT_UNAVAILABLE")?;
        use std::io::Write;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(results.join(format!(
                "{}.json",
                catdesk_wake::protocol::digest(request.tool_call_id.as_str())
            )))
            .map_err(|_| "PROJECT_TASK_COMMAND_RESULT_UNAVAILABLE")?;
        file.write_all(&encoded)
            .and_then(|_| file.sync_all())
            .map_err(|_| "PROJECT_TASK_COMMAND_RESULT_UNAVAILABLE")?;
        let summary = format!(
            "profile {}: exit {:?}, timedOut {}, cleanup proved",
            profile.id, result.exit_code, result.timed_out
        );
        self.journal
            .transition_tool_call(
                run,
                &request.tool_call_id,
                ToolCallStatus::Completed,
                Some(digest.clone()),
                Some(summary.clone()),
            )
            .map_err(|_| "PROJECT_TASK_COMMAND_JOURNAL_UNAVAILABLE")?;
        Ok(
            json!({"toolName":"verify.command","summary":summary,"payload":receipt,"resultSha256":digest,"boundedText":format!("{}\n{}", result.stdout, result.stderr)}),
        )
    }

    pub(crate) async fn close(self: &Arc<Self>) -> Result<(), String> {
        let task = self.clone();
        tokio::task::spawn_blocking(move || {
            let mut state = task
                .state
                .lock()
                .map_err(|_| "PROJECT_TASK_RECOVERY_REQUIRED")?;
            state.closed = true;
            if state.uncertain {
                return Err("PROJECT_TASK_UNCERTAIN_MARKER_RETAINED".into());
            }
            state
                .writer
                .as_mut()
                .ok_or("PROJECT_TASK_WRITER_RETIRE_FAILED")?
                .finish_writer_turn()
                .map_err(|_| "PROJECT_TASK_WRITER_RETIRE_FAILED")?;
            // Confirmed drain/marker retirement releases this exact lock even
            // if a closed router/context Arc remains alive temporarily.
            state.writer.take();
            Ok(())
        })
        .await
        .map_err(|_| "PROJECT_TASK_RECOVERY_REQUIRED")?
    }
}

fn schema(tools: Vec<&str>) -> Value {
    json!({"name":TOOL,"description":"Use this connection's operator-assigned isolated task worktree and existing durable patch tools. Stable callId required; recorded calls are never replayed. No arbitrary shell or native session action.",
        "inputSchema":{"type":"object","properties":{"taskId":{"type":"string"},"callId":{"type":"string"},"tool":{"type":"string","enum":tools},"arguments":{"type":"object"}},"required":["taskId","callId","tool","arguments"],"additionalProperties":false}})
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use crate::delegated::autonomy_projects::AutonomousProjectV1;

    fn git(root: &Path, args: &[&str]) {
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "Git fixture failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    pub(super) fn fixture(id: &str) -> (ResolvedProjectScopeV1, PathBuf, PathBuf) {
        let parent = std::env::temp_dir().join(format!("project-task-{}", uuid::Uuid::new_v4()));
        let source = parent.join("source");
        let control = parent.join("control");
        fs::create_dir_all(source.join("src")).unwrap();
        fs::create_dir(&control).unwrap();
        fs::write(source.join("src/lib.rs"), "pub fn value() -> u32 { 1 }\n").unwrap();
        git(&source, &["init", "-q", "-b", "native"]);
        git(&source, &["add", "src/lib.rs"]);
        git(
            &source,
            &[
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.test",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-qm",
                "fixture",
            ],
        );
        let isolated = parent.join("isolated");
        git(
            &source,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "codex/web-task",
                isolated.to_str().unwrap(),
            ],
        );
        let isolated = isolated.canonicalize().unwrap();
        let scope = ResolvedProjectScopeV1 {
            control_workspace: control.canonicalize().unwrap(),
            project: AutonomousProjectV1 {
                project_id: id.into(),
                workspace: source.canonicalize().unwrap(),
                git_identity: "https://example.test/project.git".into(),
                verification_profile: "rust_full".into(),
                codex_thread_id: None,
                chatgpt_target_url: None,
                chatgpt_target_sha256: None,
                chatgpt_target_history: vec![],
            },
        };
        fs::create_dir_all(scope.control_workspace.join(".catdesk/projects")).unwrap();
        fs::write(
            scope
                .control_workspace
                .join(".catdesk/projects/projects.json"),
            serde_json::to_vec(
                &crate::delegated::autonomy_projects::AutonomousProjectRegistryV1 {
                    schema_version: 1,
                    maximum_codex_workers: 2,
                    maximum_qwen_workers: 1,
                    projects: vec![scope.project.clone()],
                },
            )
            .unwrap(),
        )
        .unwrap();
        let mut contract: ExecutionContractV1 = serde_json::from_str(include_str!(
            "../tests/fixtures/delegated/execution_contract_v1.json"
        ))
        .unwrap();
        contract.task_id = "task-fixture".into();
        contract.workspace = isolated.to_str().unwrap().into();
        contract.feature_branch = "codex/web-task".into();
        contract.allowed_paths = vec!["src".into()];
        contract.forbidden_paths = vec![".git".into(), ".catdesk".into(), "private".into()];
        contract.max_elapsed_seconds = 180;
        contract.approval_requirements.clear();
        let assignment = parent.join("assignment.json");
        fs::write(&assignment, serde_json::to_vec(&json!({"schemaVersion":1,"projectId":id,"baseCommit":web_task_worktree_witness(&isolated).unwrap().2,"contract":contract})).unwrap()).unwrap();
        (scope, isolated, assignment)
    }

    fn call(id: &str, tool: &str, arguments: Value) -> Value {
        json!({"taskId":"task-fixture","callId":id,"tool":tool,"arguments":arguments})
    }

    fn preview(id: &str) -> Value {
        call(
            id,
            "patch.preview",
            json!({"patchId":"patch-fixture","operations":[{"path":"src/lib.rs","old":"{ 1 }","new":"{ 2 }"}]}),
        )
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn project_task_two_worktrees_patch_independently_and_resume_without_duplicate() {
        let (a_scope, a_root, a_assignment) = fixture("alpha");
        let (b_scope, b_root, b_assignment) = fixture("beta");
        let a = ProjectTask::open(&a_scope, &a_assignment).unwrap();
        let b = ProjectTask::open(&b_scope, &b_assignment).unwrap();
        assert!(acquire_project_writer_lock(&a_root).is_err());
        // Native source checkout remains available while the Web task owns
        // only its separate worktree.
        drop(acquire_project_writer_lock(&a_scope.project.workspace).unwrap());
        let (pa, pb) = tokio::join!(
            a.execute(preview("preview-a")),
            b.execute(preview("preview-b"))
        );
        pa.unwrap();
        pb.unwrap();
        let apply = call("apply-a", "patch.apply", json!({"patchId":"patch-fixture"}));
        let (aa, ab) = tokio::join!(
            a.execute(apply.clone()),
            b.execute(call(
                "apply-b",
                "patch.apply",
                json!({"patchId":"patch-fixture"})
            ))
        );
        aa.unwrap();
        ab.unwrap();
        assert!(
            fs::read_to_string(a_root.join("src/lib.rs"))
                .unwrap()
                .contains("{ 2 }")
        );
        assert!(
            fs::read_to_string(b_root.join("src/lib.rs"))
                .unwrap()
                .contains("{ 2 }")
        );
        assert!(
            fs::read_to_string(a_scope.project.workspace.join("src/lib.rs"))
                .unwrap()
                .contains("{ 1 }")
        );
        assert!(
            fs::read_to_string(b_scope.project.workspace.join("src/lib.rs"))
                .unwrap()
                .contains("{ 1 }")
        );
        assert_eq!(
            a.execute(apply.clone()).await.unwrap_err(),
            "PROJECT_TASK_CALL_ALREADY_COMPLETED"
        );
        assert_eq!(
            a.execute(call(
                "apply-a",
                "diff.actual",
                json!({"paths":["src/lib.rs"]})
            ))
            .await
            .unwrap_err(),
            "PROJECT_TASK_CALL_ID_CONFLICT"
        );
        a.close().await.unwrap();
        b.close().await.unwrap();
        drop(a);
        drop(b);
        let resumed = ProjectTask::open(&a_scope, &a_assignment).unwrap();
        assert_eq!(
            resumed.execute(apply).await.unwrap_err(),
            "PROJECT_TASK_CALL_ALREADY_COMPLETED"
        );
        resumed.close().await.unwrap();
        drop(acquire_project_writer_lock(&a_root).unwrap());
    }

    #[tokio::test]
    async fn project_task_refuses_wrong_task_paths_shell_and_original_checkout() {
        let (scope, root, assignment) = fixture("alpha");
        let task = ProjectTask::open(&scope, &assignment).unwrap();
        for args in [
            call(
                "wrong-shell",
                "run_command",
                json!({"command":"echo denied"}),
            ),
            json!({"taskId":"other-task","callId":"wrong-task","tool":"read","arguments":{"path":"src/lib.rs"}}),
            call("escape", "read", json!({"path":"../source/src/lib.rs"})),
            call("private", "read", json!({"path":".git"})),
            call(
                "absolute",
                "read",
                json!({"path":scope.project.workspace.join("src/lib.rs")}),
            ),
        ] {
            assert!(task.execute(args).await.is_err());
        }
        task.close().await.unwrap();
        drop(task);
        let mut data: Value = serde_json::from_slice(&fs::read(&assignment).unwrap()).unwrap();
        data["contract"]["workspace"] = json!(scope.project.workspace);
        fs::write(&assignment, serde_json::to_vec(&data).unwrap()).unwrap();
        assert_eq!(
            ProjectTask::open(&scope, &assignment).err().unwrap(),
            "PROJECT_TASK_REQUIRES_SEPARATE_LINKED_WORKTREE"
        );
        assert!(
            fs::read_to_string(root.join("src/lib.rs"))
                .unwrap()
                .contains("{ 1 }")
        );
    }

    #[tokio::test]
    async fn project_task_unknown_outcome_quarantines_only_its_writer() {
        let (scope, root, assignment) = fixture("alpha");
        let task = ProjectTask::open(&scope, &assignment).unwrap();
        // Mutation enters the durable service but names no prepared patch.
        // Existing journal marks OUTCOME_UNKNOWN. Never reinterpret as retry.
        assert!(
            task.execute(call(
                "ambiguous",
                "patch.apply",
                json!({"patchId":"missing"})
            ))
            .await
            .is_err()
        );
        assert_eq!(
            task.close().await.unwrap_err(),
            "PROJECT_TASK_UNCERTAIN_MARKER_RETAINED"
        );
        drop(task);
        assert!(acquire_project_writer_lock(&root).is_err());
        assert_eq!(
            ProjectTask::open(&scope, &assignment).err().unwrap(),
            "PROJECT_TASK_WRITER_BUSY_OR_UNRESOLVED"
        );
        drop(acquire_project_writer_lock(&scope.project.workspace).unwrap());
        let (sibling, sibling_root, sibling_assignment) = fixture("beta");
        let continuing = ProjectTask::open(&sibling, &sibling_assignment).unwrap();
        continuing
            .execute(preview("sibling-preview"))
            .await
            .unwrap();
        continuing
            .execute(call(
                "sibling-apply",
                "patch.apply",
                json!({"patchId":"patch-fixture"}),
            ))
            .await
            .unwrap();
        assert!(
            fs::read_to_string(sibling_root.join("src/lib.rs"))
                .unwrap()
                .contains("{ 2 }")
        );
        continuing.close().await.unwrap();
        // Fixture pending marker is deliberately retained as uncertainty proof;
        // no production claim or historical owner is reconciled by this test.
    }

    #[tokio::test]
    async fn project_task_refuses_contract_replacement_and_registered_worktree_takeover() {
        let (scope, root, assignment) = fixture("alpha");
        let task = ProjectTask::open(&scope, &assignment).unwrap();
        task.close().await.unwrap();
        let original = fs::read(&assignment).unwrap();
        let mut changed: Value = serde_json::from_slice(&original).unwrap();
        changed["contract"]["objective"] = json!("different task under same identity");
        fs::write(&assignment, serde_json::to_vec(&changed).unwrap()).unwrap();
        assert_eq!(
            ProjectTask::open(&scope, &assignment).err().unwrap(),
            "PROJECT_TASK_CONTRACT_CHANGED"
        );
        fs::write(&assignment, original).unwrap();
        let registry_path = scope
            .control_workspace
            .join(".catdesk/projects/projects.json");
        let mut registry: crate::delegated::autonomy_projects::AutonomousProjectRegistryV1 =
            serde_json::from_slice(&fs::read(&registry_path).unwrap()).unwrap();
        let mut other = scope.project.clone();
        other.project_id = "another-native-project".into();
        other.workspace = root;
        registry.projects.push(other);
        fs::write(registry_path, serde_json::to_vec(&registry).unwrap()).unwrap();
        assert_eq!(
            ProjectTask::open(&scope, &assignment).err().unwrap(),
            "PROJECT_TASK_WORKTREE_ALREADY_REGISTERED"
        );
    }

    fn command_assignment(
        scope: &ResolvedProjectScopeV1,
        root: &Path,
        assignment: &Path,
        peer: &Path,
        script: &str,
        seconds: u64,
    ) {
        fs::write(root.join("verify.ps1"), script).unwrap();
        fs::write(
            root.join("paths.json"),
            serde_json::to_vec(&json!({"source":crate::command::normalize_windows_verbatim_path(scope.project.workspace.clone()),"peer":crate::command::normalize_windows_verbatim_path(peer.to_path_buf())})).unwrap(),
        )
        .unwrap();
        let program = PathBuf::from(std::env::var_os("SystemRoot").unwrap())
            .join("System32/WindowsPowerShell/v1.0/powershell.exe");
        let mut data: Value = serde_json::from_slice(&fs::read(assignment).unwrap()).unwrap();
        data["contract"]["allowedCommandProfiles"] = json!(["verify-fixture"]);
        data["commandProfiles"] = json!([{"id":"verify-fixture","argv":[program,"-NoLogo","-NoProfile","-NonInteractive","-File",root.join("verify.ps1")],"timeoutSeconds":seconds}]);
        fs::write(assignment, serde_json::to_vec(&data).unwrap()).unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    #[ignore = "requires installed native Windows command sandbox; explicit bounded qualification only"]
    async fn project_task_commands_parallel_sandboxed_verification_and_restart() {
        let (a_scope, a_root, a_assignment) = fixture("alpha");
        let (b_scope, b_root, b_assignment) = fixture("beta");
        let script = r#"
$ErrorActionPreference='Stop'
$paths=Get-Content (Join-Path $PSScriptRoot 'paths.json') -Raw|ConvertFrom-Json
if($env:CATDESK_MCP_AUTH_TOKEN -or $env:OPENAI_API_KEY){throw 'Credential inherited'}
$content=[IO.File]::ReadAllText((Join-Path $PSScriptRoot 'src/lib.rs'))
if(-not $content.Contains('{ 2 }')){throw 'Patch absent'}
foreach($target in @((Join-Path $paths.source 'src/lib.rs'),(Join-Path $paths.peer 'cross-write.txt'),(Join-Path $PSScriptRoot '.git'))){
    try {[IO.File]::AppendAllText($target,'UNAUTHORIZED');exit 91}catch{}
}
[IO.File]::WriteAllText((Join-Path $PSScriptRoot 'command-ready.txt'),'ready')
$deadline=[DateTime]::UtcNow.AddSeconds(6)
while(-not (Test-Path -LiteralPath (Join-Path $paths.peer 'command-ready.txt'))){if([DateTime]::UtcNow -gt $deadline){exit 93};Start-Sleep -Milliseconds 25}
Write-Output 'PATCH_VERIFIED_PARALLEL_PEER_READY_OUTSIDE_WRITES_REFUSED'
"#;
        command_assignment(&a_scope, &a_root, &a_assignment, &b_root, script, 30);
        command_assignment(&b_scope, &b_root, &b_assignment, &a_root, script, 30);
        let a = ProjectTask::open(&a_scope, &a_assignment).unwrap();
        let b = ProjectTask::open(&b_scope, &b_assignment).unwrap();
        // A native-equivalent sandbox command keeps reading the original
        // checkout while Web tasks verify patches and refuse outside writes.
        // Shared ACL changes must not block this native-equivalent command.
        fs::write(
            a_scope.project.workspace.join("reader-paths.json"),
            serde_json::to_vec(&json!([
                crate::command::normalize_windows_verbatim_path(a_root.clone()),
                crate::command::normalize_windows_verbatim_path(b_root.clone())
            ]))
            .unwrap(),
        )
        .unwrap();
        fs::write(a_scope.project.workspace.join("reader.ps1"), r#"
$ErrorActionPreference='Stop'
$roots=Get-Content (Join-Path $PSScriptRoot 'reader-paths.json') -Raw|ConvertFrom-Json
[IO.File]::WriteAllText((Join-Path $PSScriptRoot 'reader-ready.txt'),'ready')
$deadline=[DateTime]::UtcNow.AddSeconds(20)
do {
    $content=[IO.File]::ReadAllText((Join-Path $PSScriptRoot 'src/lib.rs'))
    if(-not $content.Contains('{ 1 }')){exit 94}
    if([DateTime]::UtcNow -gt $deadline){exit 95}
    Start-Sleep -Milliseconds 30
} while(-not ((Test-Path (Join-Path $roots[0] 'command-ready.txt')) -and (Test-Path (Join-Path $roots[1] 'command-ready.txt'))))
Write-Output 'NATIVE_EQUIVALENT_READER_CONTINUED_DURING_TASK_COMMANDS'
"#).unwrap();
        let native_root = a_scope.project.workspace.clone();
        let mut native_writer = acquire_project_writer_lock(&native_root).unwrap();
        native_writer.begin_writer_turn().unwrap();
        let reader_root = native_root.clone();
        let assignment_data: Value =
            serde_json::from_slice(&fs::read(&a_assignment).unwrap()).unwrap();
        let mut reader_profile: CommandProfile =
            serde_json::from_value(assignment_data["commandProfiles"][0].clone()).unwrap();
        reader_profile.id = "native-equivalent-reader".into();
        *reader_profile.argv.last_mut().unwrap() = reader_root
            .join("reader.ps1")
            .to_string_lossy()
            .into_owned();
        reader_profile.timeout_seconds = 45;
        let reader = tokio::task::spawn_blocking(move || {
            crate::project_task_command::run(
                &reader_root,
                &[],
                &reader_profile,
                std::time::Duration::from_secs(60),
            )
        });
        let readiness = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while !native_root.join("reader-ready.txt").exists()
            && std::time::Instant::now() < readiness
        {
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
        a.execute(preview("a-preview")).await.unwrap();
        b.execute(preview("b-preview")).await.unwrap();
        a.execute(call(
            "a-apply",
            "patch.apply",
            json!({"patchId":"patch-fixture"}),
        ))
        .await
        .unwrap();
        b.execute(call(
            "b-apply",
            "patch.apply",
            json!({"patchId":"patch-fixture"}),
        ))
        .await
        .unwrap();
        let verify = call(
            "a-verify",
            "verify.command",
            json!({"profile":"verify-fixture"}),
        );
        let (ar, br) = tokio::join!(
            a.execute(verify.clone()),
            b.execute(call(
                "b-verify",
                "verify.command",
                json!({"profile":"verify-fixture"})
            ))
        );
        // Drain both exact owners before assertions, including a failed command.
        let ac = a.close().await;
        let bc = b.close().await;
        let native_result = reader.await.unwrap().unwrap();
        native_writer.finish_writer_turn().unwrap();
        let ar = ar.unwrap();
        let br = br.unwrap();
        ac.unwrap();
        bc.unwrap();
        assert!(
            native_result.success,
            "native-equivalent command interrupted: {} {}",
            native_result.stdout, native_result.stderr
        );
        for result in [&ar, &br] {
            assert_eq!(result["payload"]["success"], true, "{result}");
            assert_eq!(result["payload"]["cleanupProved"], true);
            assert_eq!(result["payload"]["modelCalls"], 0);
        }
        assert_eq!(ar["payload"]["provenance"]["projectId"], "alpha");
        assert_eq!(br["payload"]["provenance"]["projectId"], "beta");
        assert_eq!(ar["payload"]["provenance"]["callId"], "a-verify");
        let receipt_path = a_scope
            .control_workspace
            .join(".catdesk/project-mcp-tasks")
            .join("alpha")
            .join(&a.contract.task_id)
            .join("command-results")
            .join(format!(
                "{}.json",
                catdesk_wake::protocol::digest("a-verify")
            ));
        let recorded = fs::read_to_string(receipt_path).unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&recorded).unwrap(),
            ar["payload"]
        );
        assert_eq!(
            catdesk_wake::protocol::digest(&recorded),
            ar["resultSha256"]
        );
        assert!(
            ar["payload"]["provenance"]["startedUtc"]
                .as_str()
                .unwrap()
                .contains("+00:00")
        );
        assert!(
            !a_root.join("cross-write.txt").exists() && !b_root.join("cross-write.txt").exists()
        );
        assert!(
            fs::read_to_string(a_scope.project.workspace.join("src/lib.rs"))
                .unwrap()
                .contains("{ 1 }")
        );
        let resumed = ProjectTask::open(&a_scope, &a_assignment).unwrap();
        assert_eq!(
            resumed.execute(verify).await.unwrap_err(),
            "PROJECT_TASK_CALL_ALREADY_COMPLETED"
        );
        assert!(
            resumed
                .execute(call(
                    "bad-command",
                    "verify.command",
                    json!({"profile":"verify-fixture","argv":["injected"]})
                ))
                .await
                .is_err()
        );
        resumed.close().await.unwrap();
        let mut changed: Value = serde_json::from_slice(&fs::read(&a_assignment).unwrap()).unwrap();
        changed["commandProfiles"][0]["timeoutSeconds"] = json!(31);
        fs::write(&a_assignment, serde_json::to_vec(&changed).unwrap()).unwrap();
        assert_eq!(
            ProjectTask::open(&a_scope, &a_assignment).err().unwrap(),
            "PROJECT_TASK_COMMAND_AUTHORITY_CHANGED"
        );
    }

    #[tokio::test]
    #[ignore = "requires installed native Windows command sandbox; explicit bounded qualification only"]
    async fn project_task_commands_timeout_drains_owned_tree_and_preserves_receipt() {
        let (scope, root, assignment) = fixture("alpha");
        command_assignment(
            &scope,
            &root,
            &assignment,
            &root,
            "Write-Output 'COMMAND_STARTED'; Start-Sleep -Seconds 30",
            20,
        );
        let task = ProjectTask::open(&scope, &assignment).unwrap();
        let request = call(
            "timeout-verify",
            "verify.command",
            json!({"profile":"verify-fixture"}),
        );
        let result = task.execute(request.clone()).await;
        let close = task.close().await;
        let result = result.unwrap();
        close.unwrap();
        assert_eq!(result["payload"]["timedOut"], true, "{result}");
        assert_eq!(result["payload"]["cleanupProved"], true);
        assert_eq!(result["payload"]["success"], false);
        assert!(
            result["payload"]["stdout"]
                .as_str()
                .unwrap()
                .contains("COMMAND_STARTED"),
            "{result}"
        );
        assert!(result["payload"]["elapsedMs"].as_u64().unwrap() < 35000);
        let resumed = ProjectTask::open(&scope, &assignment).unwrap();
        assert_eq!(
            resumed.execute(request).await.unwrap_err(),
            "PROJECT_TASK_CALL_ALREADY_COMPLETED"
        );
        resumed.close().await.unwrap();
    }
}
