//! Operator-assigned isolated worktree editing for a fixed project connection.
//! Reuses delegated contracts, patch preimages/journal and project writer locks.
//! Native queue launch requires an additional exact local capability; ordinary
//! contract tools retain their isolated worktree and command restrictions.

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
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
#[cfg(windows)]
#[path = "project_task_recovery.rs"]
mod recovery;

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

fn effective_tools(profiles: &[CommandProfile]) -> Vec<&'static str> {
    TOOLS
        .iter()
        .copied()
        .filter(|tool| *tool != "verify.command" || !profiles.is_empty())
        .collect()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Assignment {
    schema_version: u32,
    project_id: String,
    base_commit: String,
    contract: ExecutionContractV1,
    #[serde(default)]
    command_profiles: Vec<CommandProfile>,
    #[serde(default)]
    web_work: Option<AssignmentWork>,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AssignmentWork {
    request_id: String,
    event_id: String,
}

#[derive(Clone)]
struct AssignmentInspection {
    bytes: Vec<u8>,
    contract: ExecutionContractV1,
    command_profiles: Vec<CommandProfile>,
    root: PathBuf,
    witness: (PathBuf, PathBuf, String, String),
    web_work: Option<AssignmentWork>,
}

fn continuation_contract(inspected: &AssignmentInspection) -> Value {
    let contract = &inspected.contract;
    json!({"objective":contract.objective,"allowedPaths":contract.allowed_paths,
        "forbiddenPaths":contract.forbidden_paths,"allowedTools":effective_tools(&inspected.command_profiles),
        "orderedSteps":contract.ordered_steps,"acceptanceCriteria":contract.acceptance_criteria,
        "expectedArtifacts":contract.expected_artifacts,"escalationConditions":contract.escalation_conditions,
        "verificationProfile":contract.verification_profile,
        "commandProfiles":inspected.command_profiles.iter().map(|profile|json!({"id":profile.id,
            "timeoutSeconds":profile.timeout_seconds})).collect::<Vec<_>>(),
        "contractLimits":{"maxToolCalls":contract.max_tool_calls,"maxElapsedSeconds":contract.max_elapsed_seconds,
            "maxTurns":contract.max_turns,"retryBudget":contract.retry_budget},
        "limitsRestartedByRollover":false})
}

/// Conversation URLs and generations are deliberately excluded: a proved
/// rollover may change those, but cannot grant a different native owner/root.
pub(crate) fn assignment_scope_sha256(project: &ResolvedProjectScopeV1) -> String {
    catdesk_wake::protocol::digest(
        &serde_json::to_string(&(
            &project.control_workspace,
            &project.project.project_id,
            &project.project.workspace,
            &project.project.git_identity,
            &project.project.verification_profile,
            &project.project.codex_thread_id,
        ))
        .expect("serializable project identity"),
    )
}

struct TaskState {
    service: IntegratedDelegatedService,
    writer: Option<ProjectWriterGuard>,
    project_lease: Option<crate::operator_facade::WebWorkLease>,
    closed: bool,
    uncertain: bool,
}

pub(crate) struct ProjectTask {
    project: ResolvedProjectScopeV1,
    root: PathBuf,
    recovery_claim: Option<String>,
    witness: (PathBuf, PathBuf, String, String),
    contract: ExecutionContractV1,
    journal: DelegatedJournal,
    started: std::time::Instant,
    connection_budget: std::time::Duration,
    command_profiles: Vec<CommandProfile>,
    linked_assignment: Option<AssignmentInspection>,
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

fn require_registered_scope(project: &ResolvedProjectScopeV1) -> Result<(), String> {
    let registry = AutonomousProjectRegistryStoreV1::open_read_only(
        project.control_workspace.join(".catdesk/projects"),
    )
    .and_then(|store| store.load_registry())
    .map_err(|_| "PROJECT_TASK_REGISTRY_UNAVAILABLE")?;
    if !registry
        .projects
        .iter()
        .any(|current| current == &project.project)
    {
        return Err("PROJECT_TASK_REGISTRATION_CHANGED".into());
    }
    Ok(())
}

fn inspect_assignment(
    project: &ResolvedProjectScopeV1,
    assignment: &Path,
    connection_seconds: u64,
) -> Result<AssignmentInspection, String> {
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
    let web_work = assignment.web_work;
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
        if !ids.insert(&profile.id) || !contract.allowed_command_profiles.contains(&profile.id) {
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
    if let Some(work) = &web_work {
        if work.request_id.len() > 128
            || !catdesk_wake::protocol::valid_id(&work.request_id)
            || work.event_id
                != format!(
                    "web-work-{}",
                    catdesk_wake::protocol::digest(&format!(
                        "{}\0{}",
                        project.project.project_id, work.request_id
                    ))
                )
        {
            return Err("PROJECT_TASK_WORK_SCOPE_CHANGED".into());
        }
    }
    Ok(AssignmentInspection {
        bytes,
        contract,
        command_profiles,
        root,
        witness,
        web_work,
    })
}

fn state_root(project: &ResolvedProjectScopeV1, task_id: &str) -> PathBuf {
    project
        .control_workspace
        .join(".catdesk/project-mcp-tasks")
        .join(&project.project.project_id)
        .join(task_id)
}

fn plain_directory(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(|_| "PROJECT_TASK_ARCHIVE_UNAVAILABLE")?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err("PROJECT_TASK_ARCHIVE_UNSAFE".into());
        }
    }
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("PROJECT_TASK_ARCHIVE_UNSAFE".into());
    }
    Ok(())
}

fn task_directory(
    project: &ResolvedProjectScopeV1,
    task_id: &str,
    create: bool,
) -> Result<PathBuf, String> {
    if task_id.len() > 128 || !catdesk_wake::protocol::valid_id(task_id) {
        return Err("PROJECT_TASK_ASSIGNMENT_INVALID".into());
    }
    let mut path = project.control_workspace.join(".catdesk");
    plain_directory(&path)?;
    for component in ["project-mcp-tasks", &project.project.project_id, task_id] {
        path.push(component);
        if create
            && !path
                .try_exists()
                .map_err(|_| "PROJECT_TASK_ARCHIVE_UNAVAILABLE")?
        {
            fs::create_dir(&path).map_err(|_| "PROJECT_TASK_ARCHIVE_UNAVAILABLE")?;
        }
        plain_directory(&path)?;
    }
    Ok(path)
}

pub(crate) fn read_assignment_bytes(path: &Path) -> Result<Vec<u8>, String> {
    if !regular(path) {
        return Err("PROJECT_TASK_ASSIGNMENT_UNAVAILABLE".into());
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|_| "PROJECT_TASK_ASSIGNMENT_UNAVAILABLE")?
        .take(128 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "PROJECT_TASK_ASSIGNMENT_UNAVAILABLE")?;
    if bytes.len() > 128 * 1024 {
        return Err("PROJECT_TASK_ASSIGNMENT_TOO_LARGE".into());
    }
    Ok(bytes)
}

pub(crate) fn exact_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    if path
        .try_exists()
        .map_err(|_| "PROJECT_TASK_ARCHIVE_UNAVAILABLE")?
    {
        if read_assignment_bytes(path)? != bytes {
            return Err("PROJECT_TASK_ARCHIVE_CONFLICT".into());
        }
        return Ok(());
    }
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| "PROJECT_TASK_ARCHIVE_UNAVAILABLE")?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| "PROJECT_TASK_ARCHIVE_UNAVAILABLE")?;
    drop(file);
    if read_assignment_bytes(path)? != bytes {
        return Err("PROJECT_TASK_ARCHIVE_READBACK_CHANGED".into());
    }
    Ok(())
}

fn assignment_link(
    project: &ResolvedProjectScopeV1,
    inspected: &AssignmentInspection,
) -> crate::web_work_state::WorkTaskAssignment {
    crate::web_work_state::WorkTaskAssignment {
        task_id: inspected.contract.task_id.clone(),
        assignment_sha256: catdesk_wake::protocol::digest(
            std::str::from_utf8(&inspected.bytes).expect("parsed JSON is UTF-8"),
        ),
        scope_sha256: assignment_scope_sha256(project),
    }
}

fn check_source(
    inspected: &AssignmentInspection,
    request: &str,
    event: &str,
) -> Result<(), String> {
    if !inspected
        .web_work
        .as_ref()
        .is_some_and(|source| source.request_id == request && source.event_id == event)
    {
        return Err("PROJECT_TASK_EXPLICIT_WORK_ASSOCIATION_REQUIRED".into());
    }
    Ok(())
}

/// Archive first; a crash before the ledger link leaves an inert exact archive.
pub(crate) fn archive_work_assignment(
    project: &ResolvedProjectScopeV1,
    path: &Path,
    expected_sha256: &str,
    request: &str,
    event: &str,
) -> Result<crate::web_work_state::WorkTaskAssignment, String> {
    let inspected = inspect_assignment(project, path, 180)?;
    check_source(&inspected, request, event)?;
    let link = assignment_link(project, &inspected);
    if link.assignment_sha256 != expected_sha256 {
        return Err("PROJECT_TASK_ASSIGNMENT_DIGEST_CHANGED".into());
    }
    let _writer = acquire_project_writer_lock(&inspected.root)
        .map_err(|_| "PROJECT_TASK_WRITER_BUSY_OR_UNRESOLVED")?;
    let directory = task_directory(project, &link.task_id, true)?;
    let archive = directory.join("assignment.json");
    if !archive.exists() && directory.join("journal").exists() {
        return Err("PROJECT_TASK_LEGACY_JOURNAL_NOT_ASSOCIATED".into());
    }
    exact_file(&archive, &inspected.bytes)?;
    Ok(link)
}

fn check_record_assignment(
    project: &ResolvedProjectScopeV1,
    inspected: &AssignmentInspection,
    record: &crate::web_work_state::WorkRecord,
) -> Result<(), String> {
    check_source(inspected, &record.request_id, &record.event_id)?;
    if record.phase != "OPEN"
        || record.task_assignment.as_ref() != Some(&assignment_link(project, inspected))
    {
        return Err("PROJECT_TASK_WORK_ASSOCIATION_CHANGED".into());
    }
    let archive =
        task_directory(project, &inspected.contract.task_id, false)?.join("assignment.json");
    if read_assignment_bytes(&archive)? != inspected.bytes {
        return Err("PROJECT_TASK_ARCHIVE_CONFLICT".into());
    }
    Ok(())
}

#[derive(Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WebExecutionGrant {
    schema_version: u32,
    project_id: String,
    request_id: String,
    source_event_id: String,
    assignment: crate::web_work_state::WorkTaskAssignment,
    key_sha256: String,
    #[serde(default, skip_serializing_if = "manual_wake_disabled")]
    allow_manual_wake: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    native_queue: Option<crate::operator_codex_cli::NativeQueueSelection>,
}

fn manual_wake_disabled(value: &bool) -> bool {
    !value
}

/// Explicit local opt-in. This permission follows only C46-proved source
/// continuations; it never replaces a contract, workspace or native owner.
pub(crate) fn grant_web_execution(
    project: &ResolvedProjectScopeV1,
    record: &crate::web_work_state::WorkRecord,
    current: &catdesk_wake::protocol::Target,
    revoke: bool,
) -> Result<Value, String> {
    grant_web_execution_with_manual_wake(project, record, current, revoke, false)
}

pub(crate) fn grant_web_execution_with_manual_wake(
    project: &ResolvedProjectScopeV1,
    record: &crate::web_work_state::WorkRecord,
    current: &catdesk_wake::protocol::Target,
    revoke: bool,
    allow_manual_wake: bool,
) -> Result<Value, String> {
    grant_web_execution_with_capabilities(project, record, current, revoke, allow_manual_wake, None)
}

pub(crate) fn grant_web_execution_with_capabilities(
    project: &ResolvedProjectScopeV1,
    record: &crate::web_work_state::WorkRecord,
    current: &catdesk_wake::protocol::Target,
    revoke: bool,
    allow_manual_wake: bool,
    native_queue_task_id: Option<&str>,
) -> Result<Value, String> {
    if revoke && (allow_manual_wake || native_queue_task_id.is_some()) {
        return Err("WEB_TASK_GRANT_ARGUMENTS_INVALID".into());
    }
    let prepared = prepare_work_resume(project, record, current)?;
    let link = record
        .task_assignment
        .as_ref()
        .ok_or("WEB_TASK_GRANT_REQUIRED")?;
    let directory = task_directory(project, &link.task_id, false)?;
    let revoked = directory.join("execution-revoked.json");
    if revoke {
        exact_file(&revoked, b"{\"revoked\":true}")?;
        return Ok(
            json!({"status":"EXECUTION_REVOKED","projectId":project.project.project_id,
            "taskId":link.task_id,"launchAuthorized":false,"nativeSessionAction":"NONE"}),
        );
    }
    if revoked
        .try_exists()
        .map_err(|_| "WEB_TASK_GRANT_UNAVAILABLE")?
    {
        return Err("WEB_TASK_GRANT_REVOKED".into());
    }
    let native_queue = native_queue_task_id
        .map(|id| {
            let grant_path = directory.join("execution-grant.json");
            if grant_path
                .try_exists()
                .map_err(|_| "WEB_TASK_GRANT_UNAVAILABLE")?
            {
                let retained: WebExecutionGrant =
                    serde_json::from_slice(&read_assignment_bytes(&grant_path)?)
                        .map_err(|_| "WEB_TASK_GRANT_INVALID")?;
                if let Some(selection) = retained
                    .native_queue
                    .filter(|selection| selection.task_id == id)
                {
                    crate::operator_codex_cli::native_queue_selection_status(
                        &project.control_workspace,
                        &project.project.project_id,
                        &selection,
                    )?;
                    return Ok(selection);
                }
            }
            crate::operator_codex_cli::freeze_native_queue_selection(
                &project.control_workspace,
                &project.project.project_id,
                id,
            )
        })
        .transpose()?;
    let key_path = execution_key_path(project, link, true)?;
    let key = if key_path
        .try_exists()
        .map_err(|_| "WEB_TASK_GRANT_UNAVAILABLE")?
    {
        let key: String = serde_json::from_slice(&read_assignment_bytes(&key_path)?)
            .map_err(|_| "WEB_TASK_GRANT_INVALID")?;
        if !valid_execution_key(&key) {
            return Err("WEB_TASK_GRANT_INVALID".into());
        }
        key
    } else {
        let key = format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        exact_file(
            &key_path,
            &serde_json::to_vec(&key).map_err(|_| "WEB_TASK_GRANT_INVALID")?,
        )?;
        key
    };
    let grant = WebExecutionGrant {
        schema_version: 1,
        project_id: project.project.project_id.clone(),
        request_id: record.request_id.clone(),
        source_event_id: record.event_id.clone(),
        assignment: link.clone(),
        key_sha256: catdesk_wake::protocol::digest(&key),
        allow_manual_wake,
        native_queue: native_queue.clone(),
    };
    exact_file(
        &directory.join("execution-grant.json"),
        &serde_json::to_vec(&grant).map_err(|_| "WEB_TASK_GRANT_INVALID")?,
    )?;
    let mut result = json!({"status":"EXECUTION_GRANTED","projectId":project.project.project_id,
        "requestId":record.request_id,"sourceEventId":record.event_id,"taskId":link.task_id,
        "executionKeyFile":key_path,"executionKeyDisclosed":false,
        "assignmentSha256":link.assignment_sha256,"scopeSha256":link.scope_sha256,
        "targetGeneration":prepared["targetGeneration"],"targetSha256":prepared["targetSha256"],
        "executionBudgetSeconds":180,"executionAuthorized":true,"workerStarted":false,"nativeSessionAction":"NONE"});
    if allow_manual_wake {
        result["manualWakeAllowed"] = json!(true);
    }
    if let Some(selection) = native_queue {
        result["nativeQueue"] = json!({"selection":selection,"registeredWorkspace":project.project.workspace,
            "normalNativeCli":true,"workerStarted":false,"separateFromIsolatedContractTools":true});
    }
    Ok(result)
}

fn execution_key_path(
    project: &ResolvedProjectScopeV1,
    link: &crate::web_work_state::WorkTaskAssignment,
    create: bool,
) -> Result<PathBuf, String> {
    // A shared central read tool must not disclose a sibling task's key.
    // Reuse the fixed OS user-state root, outside every registered workspace.
    let key_directory = crate::delegated::autonomy_runtime::fixed_current_user_catdesk_state_root()
        .map_err(|_| "WEB_TASK_KEY_STORE_UNAVAILABLE")?
        .join("WebTaskKeys");
    if create
        && !key_directory
            .try_exists()
            .map_err(|_| "WEB_TASK_KEY_STORE_UNAVAILABLE")?
    {
        fs::create_dir(&key_directory).map_err(|_| "WEB_TASK_KEY_STORE_UNAVAILABLE")?;
    }
    plain_directory(&key_directory)?;
    let key_identity = catdesk_wake::protocol::digest(
        &serde_json::to_string(&(
            &project.control_workspace,
            &project.project.project_id,
            link,
        ))
        .map_err(|_| "WEB_TASK_GRANT_INVALID")?,
    );
    Ok(key_directory.join(format!("{key_identity}.json")))
}

/// Called only by the local rollover owner while its existing project fence is
/// held. Reads an existing opt-in grant; it can never create new authority.
pub(crate) fn granted_handoff_context(
    project: &ResolvedProjectScopeV1,
    record: &crate::web_work_state::WorkRecord,
    current: &catdesk_wake::protocol::Target,
) -> Result<Option<Value>, String> {
    let Some(link) = &record.task_assignment else {
        return Ok(None);
    };
    let directory = task_directory(project, &link.task_id, false)?;
    if !directory
        .join("execution-grant.json")
        .try_exists()
        .map_err(|_| "WEB_TASK_GRANT_UNAVAILABLE")?
        || directory
            .join("execution-revoked.json")
            .try_exists()
            .map_err(|_| "WEB_TASK_GRANT_UNAVAILABLE")?
    {
        return Ok(None);
    }
    let key: String = serde_json::from_slice(&read_assignment_bytes(&execution_key_path(
        project, link, false,
    )?)?)
    .map_err(|_| "WEB_TASK_GRANT_INVALID")?;
    let (_, allow_manual_wake, native_queue) = authorized_web_grant(project, record, &key)?;
    let prepared = prepare_work_resume(project, record, current)?;
    let journal_root = directory.join("journal");
    let (calls, proposals, applications) = if journal_root
        .try_exists()
        .map_err(|_| "PROJECT_TASK_RECOVERY_REQUIRED")?
    {
        let journal =
            DelegatedJournal::open(&journal_root).map_err(|_| "PROJECT_TASK_RECOVERY_REQUIRED")?;
        let run = RunId::new(&link.task_id).map_err(|_| "PROJECT_TASK_CONTRACT_INVALID")?;
        let calls = journal
            .load_tool_calls(&run)
            .map_err(|_| "PROJECT_TASK_RECOVERY_REQUIRED")?;
        let proposals = journal
            .load_patch_proposals(&run)
            .map_err(|_| "PROJECT_TASK_RECOVERY_REQUIRED")?;
        let applications = journal
            .load_patch_applications(&run)
            .map_err(|_| "PROJECT_TASK_RECOVERY_REQUIRED")?;
        if applications.iter().any(|application| {
            matches!(
                application.status,
                crate::delegated::journal::PatchApplicationStatus::OutcomeUnknown
            )
        }) {
            return Err("PROJECT_TASK_RECOVERY_REQUIRED".into());
        }
        (calls, proposals, applications)
    } else {
        (Default::default(), Vec::new(), Vec::new())
    };
    let journaled: Vec<_> = calls
        .values()
        .map(|call| {
            json!({"callId":call.tool_call_id,"tool":call.tool_name,
        "status":call.status,"argumentsSha256":call.arguments_hash,"outcome":call.outcome_summary})
        })
        .collect();
    let mut context = json!({"schemaVersion":1,"mcpTool":"catdesk_web_task_tool","projectId":project.project.project_id,
        "requestId":record.request_id,"sourceEventId":record.event_id,"taskId":link.task_id,
        "assignmentSha256":link.assignment_sha256,"scopeSha256":link.scope_sha256,"executionKey":key,
        "executionWorkspace":prepared["executionWorkspace"],"registeredWorkspace":project.project.workspace,
        "baseCommit":prepared["baseCommit"],"branch":prepared["branch"],
        "taskContract":prepared["taskContract"],
        "targetSelection":"USE_VERIFIED_CONTINUATION_GENERATION_AND_DIGEST","journaledCalls":journaled,
        "patchProposals":proposals.iter().map(|proposal|json!({"patchId":proposal.patch_id,
            "targetPaths":proposal.target_paths,"proposedDiffSha256":proposal.proposed_diff_hash})).collect::<Vec<_>>(),
        "patchApplications":applications.iter().map(|application|json!({"patchId":application.patch_id,
            "callId":application.tool_call_id,"status":application.status,"pathsChanged":application.actual_paths_changed})).collect::<Vec<_>>(),
        "completedCallReplayAllowed":false,"unknownCallReplayAllowed":false,"nativeSessionAction":"NONE"});
    context["cachedSchemaTransport"] = crate::web_task_dispatch::cached_transport_metadata();
    if allow_manual_wake {
        context["manualWake"] = json!({"tool":"wake.manual_end_turn", "allowed":true,
            "arguments":["sourceTurnId","priority","ttlSeconds"], "maximumTtlSeconds":catdesk_wake::protocol::MAX_WAKE_TTL_SECONDS,
            "budget":"SHARED_ASSIGNMENT_MAX_TOOL_CALLS", "fixedServerMessage":true, "queueOnly":true});
    }
    if let Some(selection) = native_queue {
        context["nativeQueue"] = json!({"tool":"native.codex.dispatch","statusTool":"native.codex.status","selection":selection.clone(),
            "arguments":{},"registeredWorkspace":project.project.workspace,
            "normalNativeCli":true,"separateFromIsolatedContractTools":true,
            "scope":"EXACT_FROZEN_LOCAL_QUEUE_TASK_ONLY","retryWithNewCallIdAllowed":false});
        context["nativeQueue"]["snapshot"] = crate::operator_codex_cli::native_queue_selection_status(
            &project.control_workspace, &project.project.project_id, &selection)
            .unwrap_or_else(|reason| json!({"status":"UNAVAILABLE_OR_CHANGED","reason":reason,"taskCompletionVerified":false}));
    }
    if let Some(report) = &record.reported_status {
        // Snapshot metadata only; it cannot replace execution evidence, scope or
        // grant checks. Omit the field for legacy records to retain their shape.
        context["coordinatorReport"] = json!({
            "basis":"COORDINATOR_REPORTED_NOT_VERIFIED",
            "snapshot":report,"taskCompletionVerified":false,"ownershipReleased":false,
            "refreshPolicy":"FROZEN_AT_HANDOFF_PREPARATION_CHECK_CURRENT_DEPENDENCIES_BEFORE_WORK"
        });
    }
    Ok(Some(context))
}

fn valid_execution_key(key: &str) -> bool {
    key.len() == 64
        && key
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

pub(crate) fn authorized_web_assignment(
    project: &ResolvedProjectScopeV1,
    record: &crate::web_work_state::WorkRecord,
    key: &str,
) -> Result<PathBuf, String> {
    authorized_web_grant(project, record, key).map(|(archive, _, _)| archive)
}

pub(crate) fn authorized_manual_wake_assignment(
    project: &ResolvedProjectScopeV1,
    record: &crate::web_work_state::WorkRecord,
    key: &str,
) -> Result<PathBuf, String> {
    let (archive, allowed, _) = authorized_web_grant(project, record, key)?;
    if !allowed {
        return Err("WEB_TASK_MANUAL_WAKE_NOT_GRANTED".into());
    }
    Ok(archive)
}

fn authorized_web_grant(
    project: &ResolvedProjectScopeV1,
    record: &crate::web_work_state::WorkRecord,
    key: &str,
) -> Result<
    (
        PathBuf,
        bool,
        Option<crate::operator_codex_cli::NativeQueueSelection>,
    ),
    String,
> {
    if !valid_execution_key(key) {
        return Err("WEB_TASK_GRANT_REQUIRED".into());
    }
    let link = record
        .task_assignment
        .as_ref()
        .ok_or("WEB_TASK_GRANT_REQUIRED")?;
    let directory = task_directory(project, &link.task_id, false)?;
    if directory
        .join("execution-revoked.json")
        .try_exists()
        .map_err(|_| "WEB_TASK_GRANT_UNAVAILABLE")?
    {
        return Err("WEB_TASK_GRANT_REVOKED".into());
    }
    let grant: WebExecutionGrant = serde_json::from_slice(&read_assignment_bytes(
        &directory.join("execution-grant.json"),
    )?)
    .map_err(|_| "WEB_TASK_GRANT_REQUIRED")?;
    let expected = WebExecutionGrant {
        schema_version: 1,
        project_id: project.project.project_id.clone(),
        request_id: record.request_id.clone(),
        source_event_id: record.event_id.clone(),
        assignment: link.clone(),
        key_sha256: catdesk_wake::protocol::digest(key),
        allow_manual_wake: grant.allow_manual_wake,
        native_queue: grant.native_queue.clone(),
    };
    // Compare fixed-size digests without exposing the key in errors or status.
    let key_matches = grant
        .key_sha256
        .as_bytes()
        .iter()
        .zip(expected.key_sha256.as_bytes())
        .fold(
            grant.key_sha256.len() ^ expected.key_sha256.len(),
            |diff, (a, b)| diff | usize::from(a ^ b),
        )
        == 0;
    if !key_matches || grant != expected || link.scope_sha256 != assignment_scope_sha256(project) {
        return Err("WEB_TASK_GRANT_SCOPE_MISMATCH".into());
    }
    let archive = directory.join("assignment.json");
    // Only bounded metadata under the paired/registry transaction. The exact
    // archive was checked at grant time. ProjectTask::open and execute check
    // Git/worktree state again under their own project lease before action.
    let bytes = read_assignment_bytes(&archive)?;
    let digest = catdesk_wake::protocol::digest(
        std::str::from_utf8(&bytes).map_err(|_| "WEB_TASK_GRANT_INVALID")?,
    );
    if record.phase != "OPEN" || digest != link.assignment_sha256 {
        return Err("WEB_TASK_GRANT_SCOPE_MISMATCH".into());
    }
    if let Some(selection) = &grant.native_queue {
        selection.validate()?;
    }
    Ok((archive, grant.allow_manual_wake, grant.native_queue))
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ManualWakeRequest {
    pub(crate) source_turn_id: String,
    pub(crate) priority: u8,
    pub(crate) ttl_seconds: u64,
}

impl ManualWakeRequest {
    pub(crate) fn parse(arguments: Value) -> Result<Self, String> {
        let request: Self = serde_json::from_value(arguments)
            .map_err(|_| "WEB_TASK_MANUAL_WAKE_ARGUMENTS_INVALID")?;
        if request.source_turn_id.len() > 128
            || !catdesk_wake::protocol::valid_id(&request.source_turn_id)
            || request.ttl_seconds == 0
            || request.ttl_seconds > catdesk_wake::protocol::MAX_WAKE_TTL_SECONDS
        {
            return Err("WEB_TASK_MANUAL_WAKE_ARGUMENTS_INVALID".into());
        }
        Ok(request)
    }
}

/// Shared durable call admission for explicitly granted extra capabilities.
struct JournaledTaskCall {
    journal: DelegatedJournal,
    run: RunId,
    tool_call_id: ToolCallId,
}

fn prepare_journaled_task_call(
    project: &ResolvedProjectScopeV1,
    inspected: &AssignmentInspection,
    call_id: &str,
    tool_name: &str,
    arguments_hash: String,
) -> Result<JournaledTaskCall, String> {
    prepare_journaled_task_call_checked(
        project,
        inspected,
        call_id,
        tool_name,
        arguments_hash,
        true,
    )
}

fn prepare_journaled_task_call_checked(
    project: &ResolvedProjectScopeV1,
    inspected: &AssignmentInspection,
    call_id: &str,
    tool_name: &str,
    arguments_hash: String,
    record: bool,
) -> Result<JournaledTaskCall, String> {
    use crate::delegated::journal::{ToolCallRecordV1, ToolMutationKind};
    let tool_call_id = ToolCallId::new(call_id).map_err(|_| "PROJECT_TASK_CALL_INVALID")?;
    let directory = task_directory(project, &inspected.contract.task_id, false)?;
    let journal_root = directory.join("journal");
    if !journal_root
        .try_exists()
        .map_err(|_| "PROJECT_TASK_RECOVERY_REQUIRED")?
    {
        fs::create_dir(&journal_root).map_err(|_| "PROJECT_TASK_RECOVERY_REQUIRED")?;
    }
    plain_directory(&journal_root)?;
    let existing = fs::read_dir(&journal_root)
        .map_err(|_| "PROJECT_TASK_RECOVERY_REQUIRED")?
        .next()
        .is_some();
    let journal =
        DelegatedJournal::open(&journal_root).map_err(|_| "PROJECT_TASK_RECOVERY_REQUIRED")?;
    let run =
        RunId::new(&inspected.contract.task_id).map_err(|_| "PROJECT_TASK_CONTRACT_INVALID")?;
    if existing {
        if journal
            .load_contract(&run)
            .map_err(|_| "PROJECT_TASK_RECOVERY_REQUIRED")?
            != inspected.contract
        {
            return Err("PROJECT_TASK_CONTRACT_CHANGED".into());
        }
    } else {
        journal
            .create_run(&inspected.contract)
            .map_err(|_| "PROJECT_TASK_RECOVERY_REQUIRED")?;
    }
    let calls = journal
        .load_tool_calls(&run)
        .map_err(|_| "PROJECT_TASK_RECOVERY_REQUIRED")?;
    if let Some(prior) = calls.get(tool_call_id.as_str()) {
        if prior.tool_name != tool_name || prior.arguments_hash != arguments_hash {
            return Err("PROJECT_TASK_CALL_ID_CONFLICT".into());
        }
        return Err(if prior.status == ToolCallStatus::Completed {
            "PROJECT_TASK_CALL_ALREADY_COMPLETED"
        } else {
            "PROJECT_TASK_CALL_RECORDED_NO_REPLAY"
        }
        .into());
    }
    // A new call/turn identity cannot route around an ambiguous earlier effect.
    if calls.values().any(|call| {
        !matches!(
            call.status,
            ToolCallStatus::Completed | ToolCallStatus::Failed
        )
    }) {
        return Err("PROJECT_TASK_RECOVERY_REQUIRED".into());
    }
    if calls.len() >= inspected.contract.max_tool_calls as usize {
        return Err("PROJECT_TASK_TOOL_BUDGET_EXCEEDED".into());
    }
    if !record {
        return Ok(JournaledTaskCall {
            journal,
            run,
            tool_call_id,
        });
    }
    journal
        .record_tool_call_requested(ToolCallRecordV1 {
            schema_version: 1,
            run_id: run.clone(),
            tool_call_id: tool_call_id.clone(),
            tool_name: tool_name.into(),
            request_hash: arguments_hash.clone(),
            arguments_hash,
            mutation_kind: ToolMutationKind::Mutating,
            status: ToolCallStatus::Requested,
            result_hash: None,
            outcome_summary: None,
        })
        .map_err(|_| "PROJECT_TASK_RECOVERY_REQUIRED")?;
    for phase in [ToolCallStatus::PolicyAllowed, ToolCallStatus::Executing] {
        journal
            .transition_tool_call(&run, &tool_call_id, phase, None, None)
            .map_err(|_| "PROJECT_TASK_RECOVERY_REQUIRED")?;
    }
    Ok(JournaledTaskCall {
        journal,
        run,
        tool_call_id,
    })
}

pub(crate) fn journal_manual_wake(
    project: &ResolvedProjectScopeV1,
    record: &crate::web_work_state::WorkRecord,
    key: &str,
    call_id: &str,
    request: &ManualWakeRequest,
    expected: &catdesk_wake::protocol::Target,
    publish: impl FnOnce() -> Result<catdesk_wake::protocol::Event, String>,
) -> Result<Value, String> {
    let archive = authorized_manual_wake_assignment(project, record, key)?;
    let inspected = inspect_assignment(project, &archive, 180)?;
    check_record_assignment(project, &inspected, record)?;
    let arguments_hash =
        stable_text_hash(&serde_json::to_string(request).map_err(|_| "PROJECT_TASK_CALL_INVALID")?);
    let JournaledTaskCall {
        journal,
        run,
        tool_call_id,
    } = prepare_journaled_task_call(
        project,
        &inspected,
        call_id,
        "wake.manual_end_turn",
        arguments_hash,
    )?;
    let event = match publish() {
        Ok(event) => event,
        Err(reason) => {
            // These failures are proved before a queue write. Any unclassified
            // I/O/atomic-write outcome keeps the task's existing unknown fence.
            let phase = if matches!(
                reason.as_str(),
                "TARGET_EXPECTATION_MISMATCH"
                    | "CONFIG_PROJECT_RETIRED"
                    | "QUEUE_WAKE_METADATA_INVALID"
                    | "QUEUE_EVENT_INVALID"
                    | "QUEUE_EVENT_ID_CONFLICT"
                    | "QUEUE_CAPACITY_EXCEEDED"
                    | "WAKE_PROJECT_TARGET_CHANGED"
                    | "WAKE_PROJECT_TARGET_MISMATCH"
                    | "WAKE_HISTORICAL_EVENT_RETAINED"
                    | "WAKE_ACTIVATION_INVALID"
            ) {
                ToolCallStatus::Failed
            } else {
                ToolCallStatus::OutcomeUnknown
            };
            journal
                .transition_tool_call(&run, &tool_call_id, phase, None, Some(reason.clone()))
                .map_err(|_| "PROJECT_TASK_RECOVERY_REQUIRED")?;
            return Err(reason);
        }
    };
    event.validate()?;
    let metadata = event
        .wake_metadata()?
        .ok_or("QUEUE_WAKE_METADATA_INVALID")?;
    if event.project_id != project.project.project_id
        || event.target_generation != expected.generation
        || metadata.purpose != catdesk_wake::protocol::WakePurpose::ManualEndTurn
        || metadata.source_event_id != record.event_id
        || metadata.source_turn_id != request.source_turn_id
        || metadata.priority != request.priority
        || metadata.expires_utc - event.created_utc != request.ttl_seconds
    {
        return Err("WEB_TASK_MANUAL_WAKE_EVENT_MISMATCH".into());
    }
    let result = json!({"status":"WAKE_INTENT_RECORDED", "projectId":event.project_id,
        "eventId":event.event_id, "createdUtc":event.created_utc,
        "targetGeneration":event.target_generation, "sourceTurnId":request.source_turn_id,
        "expiresUtc":metadata.expires_utc,
        "deliveryProved":false, "taskCompletionVerified":false,
        "nativeSessionAction":"NONE", "browserStarted":false, "wakeHostStarted":false});
    let result_hash = catdesk_wake::protocol::digest(&result.to_string());
    journal
        .transition_tool_call(
            &run,
            &tool_call_id,
            ToolCallStatus::Completed,
            Some(result_hash),
            Some(format!(
                "manual Wake intent {} recorded; delivery unverified",
                event.event_id
            )),
        )
        .map_err(|_| "PROJECT_TASK_RECOVERY_REQUIRED")?;
    Ok(result)
}

pub(crate) fn native_status_selection(
    project: &ResolvedProjectScopeV1,
    record: &crate::web_work_state::WorkRecord,
    key: &str,
) -> Result<crate::operator_codex_cli::NativeQueueSelection, String> {
    let (_, _, selection) = authorized_web_grant(project, record, key)?;
    selection.ok_or_else(|| "WEB_TASK_NATIVE_NOT_GRANTED".into())
}

pub(crate) struct NativeTaskCall {
    pub(crate) selection: crate::operator_codex_cli::NativeQueueSelection,
    journal: JournaledTaskCall,
}

pub(crate) fn prepare_native_task_call(
    project: &ResolvedProjectScopeV1,
    record: &crate::web_work_state::WorkRecord,
    key: &str,
    task_id: &str,
    call_id: &str,
    lease: &crate::operator_facade::WebWorkLease,
    current: &catdesk_wake::protocol::Target,
) -> Result<NativeTaskCall, String> {
    let (archive, _, selection) = authorized_web_grant(project, record, key)?;
    let selection = selection.ok_or("WEB_TASK_NATIVE_NOT_GRANTED")?;
    let inspected = inspect_assignment(project, &archive, 180)?;
    check_record_assignment(project, &inspected, record)?;
    if inspected.contract.task_id != task_id {
        return Err("WEB_TASK_GRANT_SCOPE_MISMATCH".into());
    }
    // Keep call identity tied to the immutable grant, even after a proved
    // continuation selects a linked native job. Check replay/unknown fences
    // before changing queue metadata; record execution only after migration.
    let hash = stable_text_hash(
        &serde_json::to_string(&selection).map_err(|_| "WEB_TASK_NATIVE_SELECTION_INVALID")?,
    );
    prepare_journaled_task_call_checked(
        project,
        &inspected,
        call_id,
        "native.codex.dispatch",
        hash.clone(),
        false,
    )?;
    let selection = crate::operator_codex_cli::continue_native_queue_selection(
        &project.control_workspace,
        &project.project.project_id,
        &selection,
        lease,
        &record.target,
        current,
        &record.event_id,
    )?;
    crate::operator_codex_cli::check_native_queue_selection(
        &project.control_workspace,
        &project.project.project_id,
        &selection,
    )?;
    let journal =
        prepare_journaled_task_call(project, &inspected, call_id, "native.codex.dispatch", hash)?;
    Ok(NativeTaskCall { selection, journal })
}

impl NativeTaskCall {
    pub(crate) fn finish(self, result: &Result<Value, String>) -> Result<(), String> {
        let (phase, hash, summary) = match result {
            Ok(value) if value["status"] == "prelaunch_refused" => (
                ToolCallStatus::Failed, Some(catdesk_wake::protocol::digest(&value.to_string())),
                "Native preflight refused before task claim/launch; retry only after prerequisite changes".into()),
            Ok(value)
                if value["status"] == "no_ready_tasks"
                    || matches!(
                        value["job"]["state"].as_str(),
                        Some("NATIVE_EXITED_ZERO" | "FAILED" | "READY")
                    ) =>
            {
                (
                    ToolCallStatus::Completed,
                    Some(catdesk_wake::protocol::digest(&value.to_string())),
                    "Native dispatch receipt recorded; task verification remains separate".into(),
                )
            }
            Ok(_) => (
                ToolCallStatus::OutcomeUnknown,
                None,
                "Native dispatch needs recovery; no replay".into(),
            ),
            Err(reason) => (ToolCallStatus::OutcomeUnknown, None, reason.clone()),
        };
        self.journal
            .journal
            .transition_tool_call(
                &self.journal.run,
                &self.journal.tool_call_id,
                phase,
                hash,
                Some(summary),
            )
            .map(|_| ())
            .map_err(|_| "PROJECT_TASK_RECOVERY_REQUIRED".to_owned())
    }
}

fn check_linked_assignment(
    project: &ResolvedProjectScopeV1,
    inspected: &AssignmentInspection,
    lease: &crate::operator_facade::WebWorkLease,
) -> Result<(), String> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        let result = check_linked_assignment_once(project, inspected, lease);
        if matches!(
            result.as_ref().map_err(String::as_str),
            Err("WEB_CONTEXT_REGISTRATION_BUSY" | "WEB_TARGET_WAKE_CONFIG_BUSY")
        ) && std::time::Instant::now() < deadline
        {
            std::thread::sleep(std::time::Duration::from_millis(10));
        } else {
            return result;
        }
    }
}

fn check_linked_assignment_once(
    project: &ResolvedProjectScopeV1,
    inspected: &AssignmentInspection,
    lease: &crate::operator_facade::WebWorkLease,
) -> Result<(), String> {
    let source = inspected
        .web_work
        .as_ref()
        .ok_or("PROJECT_TASK_EXPLICIT_WORK_ASSOCIATION_REQUIRED")?;
    let paired = crate::mcp::operator_read_project_web_target_diagnosed(
        &project.control_workspace,
        &project.project.project_id,
    )
    .map_err(|failure| failure.code)?
    .ok_or("PROJECT_TASK_PAIRED_TARGET_REQUIRED")?;
    crate::mcp::with_current_project_web_target(
        &project.control_workspace,
        &project.project.project_id,
        paired.generation,
        &paired.sha256,
        |_, paired| {
            let current =
                catdesk_wake::protocol::Target::new(paired.generation, paired.url.clone())?;
            require_registered_scope(project)?;
            let record = crate::web_work_state::WorkLedger::open(
                &project.control_workspace,
                &project.project.project_id,
            )?
            .record(&source.request_id)?
            .ok_or("WEB_WORK_REQUEST_MISSING")?;
            lease.check_source_task(&record.target, &current, &record.event_id)?;
            crate::web_work::require_assignment_source(
                &project.control_workspace,
                &project.project.project_id,
                &record,
                record.target == current,
            )?;
            check_record_assignment(project, inspected, &record)
        },
    )
}

/// Admission is durable metadata only. It never starts a service, writer turn,
/// endpoint or provider; a later launcher must reacquire and revalidate pins.
pub(crate) fn prepare_work_resume(
    project: &ResolvedProjectScopeV1,
    record: &crate::web_work_state::WorkRecord,
    current: &catdesk_wake::protocol::Target,
) -> Result<Value, String> {
    let link = record
        .task_assignment
        .as_ref()
        .ok_or("PROJECT_TASK_EXPLICIT_WORK_ASSOCIATION_REQUIRED")?;
    let directory = task_directory(project, &link.task_id, false)?;
    let inspected = inspect_assignment(project, &directory.join("assignment.json"), 180)?;
    check_record_assignment(project, &inspected, record)?;
    let _writer = acquire_project_writer_lock(&inspected.root)
        .map_err(|_| "PROJECT_TASK_WRITER_BUSY_OR_UNRESOLVED")?;
    let journal_root = directory.join("journal");
    let mut call_count = 0;
    if journal_root
        .try_exists()
        .map_err(|_| "PROJECT_TASK_RECOVERY_REQUIRED")?
    {
        plain_directory(&journal_root)?;
        let journal =
            DelegatedJournal::open(&journal_root).map_err(|_| "PROJECT_TASK_RECOVERY_REQUIRED")?;
        let run = RunId::new(&link.task_id).map_err(|_| "PROJECT_TASK_CONTRACT_INVALID")?;
        if journal
            .load_contract(&run)
            .map_err(|_| "PROJECT_TASK_RECOVERY_REQUIRED")?
            != inspected.contract
        {
            return Err("PROJECT_TASK_CONTRACT_CHANGED".into());
        }
        let calls = journal
            .load_tool_calls(&run)
            .map_err(|_| "PROJECT_TASK_RECOVERY_REQUIRED")?;
        if calls.values().any(|call| {
            !matches!(
                call.status,
                ToolCallStatus::Completed | ToolCallStatus::Failed
            )
        }) {
            return Err("PROJECT_TASK_RECOVERY_REQUIRED".into());
        }
        call_count = calls.len();
        let profiles = directory.join("command-profiles.json");
        if profiles.exists() {
            let saved: Vec<CommandProfile> =
                serde_json::from_slice(&read_assignment_bytes(&profiles)?)
                    .map_err(|_| "PROJECT_TASK_COMMAND_AUTHORITY_INVALID")?;
            if saved != inspected.command_profiles {
                return Err("PROJECT_TASK_COMMAND_AUTHORITY_CHANGED".into());
            }
        } else if !inspected.command_profiles.is_empty() {
            return Err("PROJECT_TASK_COMMAND_AUTHORITY_CHANGED".into());
        }
    }
    let admission = json!({"schemaVersion":1,"status":"PREPARED_ONLY","launchAuthorized":false,
        "projectId":project.project.project_id,"requestId":record.request_id,"sourceEventId":record.event_id,
        "taskId":link.task_id,"assignmentSha256":link.assignment_sha256,"scopeSha256":link.scope_sha256,
        "originalTargetGeneration":record.target.generation,"originalTargetSha256":record.target.digest,
        "targetGeneration":current.generation,"targetSha256":current.digest,
        "executionWorkspace":inspected.root,"baseCommit":inspected.witness.2,"branch":inspected.witness.3});
    exact_file(
        &directory.join(format!(
            "resume-prepared-{}-{}.json",
            current.generation, current.digest
        )),
        &serde_json::to_vec(&admission).map_err(|_| "PROJECT_TASK_ARCHIVE_UNAVAILABLE")?,
    )?;
    let mut output = admission;
    output["journaledCallCount"] = json!(call_count);
    output["completedCallReplayAllowed"] = json!(false);
    output["nativeSessionAction"] = json!("NONE");
    // Add display context after preserving the original immutable admission
    // bytes. Older prepared records must remain exact on a cold retry.
    output["taskContract"] = continuation_contract(&inspected);
    Ok(output)
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
        require_registered_scope(project)?;
        // Share retirement's per-project fence before creating task state.
        // Recheck after acquiring it so a previously resolved connection
        // cannot cross a concurrent registration retirement.
        let project_lease = crate::operator_facade::acquire_web_work_lease(
            &project.control_workspace,
            &project.project.project_id,
        )?;
        require_registered_scope(project)?;
        let inspected = inspect_assignment(project, assignment, connection_seconds)?;
        if inspected.web_work.is_some() {
            check_linked_assignment(project, &inspected, &project_lease)?;
        }
        let linked_assignment = inspected.web_work.as_ref().map(|_| inspected.clone());
        #[cfg(windows)]
        let lifetime =
            crate::reviewed_candidate_owner::OwnedCandidateChild::current_project_task_lifetime()?;
        #[cfg(windows)]
        let recovery_inspection = inspected.clone();
        #[cfg(windows)]
        let mut writer = if lifetime.is_some() {
            recovery::lock_and_recover(project, &inspected)?
        } else {
            acquire_project_writer_lock(&inspected.root)
                .map_err(|_| "PROJECT_TASK_WRITER_BUSY_OR_UNRESOLVED")?
        };
        #[cfg(not(windows))]
        let mut writer = acquire_project_writer_lock(&inspected.root)
            .map_err(|_| "PROJECT_TASK_WRITER_BUSY_OR_UNRESOLVED")?;
        let AssignmentInspection {
            contract,
            command_profiles,
            root,
            witness,
            ..
        } = inspected;
        let state_root = state_root(project, &contract.task_id);
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
        #[cfg(windows)]
        let recovery_claim = if let Some(lifetime) = lifetime {
            recovery::begin(&mut writer, project, &recovery_inspection, &lifetime)?
        } else {
            writer
                .begin_writer_turn()
                .map_err(|_| "PROJECT_TASK_WRITER_MARKER_UNAVAILABLE")?;
            None
        };
        #[cfg(not(windows))]
        let recovery_claim = {
            writer
                .begin_writer_turn()
                .map_err(|_| "PROJECT_TASK_WRITER_MARKER_UNAVAILABLE")?;
            None
        };
        service
            .start()
            .map_err(|_| "PROJECT_TASK_START_UNCERTAIN_MARKER_RETAINED")?;
        let connection_budget =
            std::time::Duration::from_secs(connection_seconds.min(contract.max_elapsed_seconds));
        Ok(Arc::new(Self {
            project: project.clone(),
            root,
            recovery_claim,
            witness,
            contract,
            journal,
            started: std::time::Instant::now(),
            connection_budget,
            command_profiles,
            linked_assignment,
            state: Mutex::new(TaskState {
                service,
                writer: Some(writer),
                project_lease: Some(project_lease),
                closed: false,
                uncertain: false,
            }),
        }))
    }

    pub(crate) fn metadata(&self) -> Value {
        json!({"taskId":self.contract.task_id,"executionWorkspace":self.root,
            "zeroCallRecoveryOwnerClaim":self.recovery_claim,
            "coldRecoveryProfile":if self.recovery_claim.is_some(){"DEDICATED_NAMED_JOB_ZERO_CALLS"}else{"LEGACY_NO_AUTORECOVERY"},
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
        effective_tools(&self.command_profiles)
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
            let mut existing = target.as_path();
            loop {
                match fs::symlink_metadata(existing) {
                    Ok(_) => break,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        // New assigned files may need several parent directories.
                        // Stop at any existing entry, including a dangling link;
                        // only true absence permits walking to another ancestor.
                        existing = existing.parent().ok_or("PROJECT_TASK_PATH_UNAVAILABLE")?;
                    }
                    Err(_) => return Err("PROJECT_TASK_PATH_UNAVAILABLE".into()),
                }
            }
            let canonical = existing
                .canonicalize()
                .map_err(|_| "PROJECT_TASK_PATH_UNAVAILABLE")?;
            if !canonical.starts_with(&self.root) {
                return Err("PROJECT_TASK_PATH_OUTSIDE_ASSIGNMENT".into());
            }
            if existing != target && !canonical.is_dir() {
                return Err("PROJECT_TASK_PATH_UNAVAILABLE".into());
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
            if let Some(assignment) = &task.linked_assignment {
                check_linked_assignment(
                    &task.project,
                    assignment,
                    state
                        .project_lease
                        .as_ref()
                        .ok_or("PROJECT_TASK_RECOVERY_REQUIRED")?,
                )?;
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
            #[cfg(windows)]
            if let Some(claim) = &task.recovery_claim {
                recovery::closed(&task.project, &task.contract.task_id, claim)?;
            }
            // Confirmed drain/marker retirement releases this exact lock even
            // if a closed router/context Arc remains alive temporarily.
            state.writer.take();
            state.project_lease.take();
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
pub(crate) mod tests {
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

    pub(crate) fn fixture(id: &str) -> (ResolvedProjectScopeV1, PathBuf, PathBuf) {
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
                    retired_projects: Vec::new(),
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

    fn linked_fixture() -> (
        ResolvedProjectScopeV1,
        PathBuf,
        PathBuf,
        crate::web_work_state::WorkRecord,
    ) {
        let (scope, root, path) = fixture("alpha");
        let event = format!(
            "web-work-{}",
            catdesk_wake::protocol::digest("alpha\0request-a")
        );
        let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        value["webWork"] = json!({"requestId":"request-a","eventId":event});
        fs::write(&path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
        let ledger =
            crate::web_work_state::WorkLedger::open(&scope.control_workspace, "alpha").unwrap();
        ledger
            .prepare(
                "request-a",
                &event,
                &"a".repeat(64),
                &catdesk_wake::protocol::Target::new(1, "https://chatgpt.com/c/task-source".into())
                    .unwrap(),
            )
            .unwrap();
        let sha = catdesk_wake::protocol::digest(&fs::read_to_string(&path).unwrap());
        let link = archive_work_assignment(&scope, &path, &sha, "request-a", &event).unwrap();
        let orphan = ledger.record("request-a").unwrap().unwrap();
        assert!(orphan.task_assignment.is_none());
        assert_eq!(
            prepare_work_resume(&scope, &orphan, &orphan.target).unwrap_err(),
            "PROJECT_TASK_EXPLICIT_WORK_ASSOCIATION_REQUIRED"
        );
        let archived =
            fs::read(state_root(&scope, "task-fixture").join("assignment.json")).unwrap();
        assert_eq!(
            archive_work_assignment(&scope, &path, &sha, "request-a", &event).unwrap(),
            link
        );
        assert_eq!(
            fs::read(state_root(&scope, "task-fixture").join("assignment.json")).unwrap(),
            archived
        );
        ledger.associate("request-a", &event, &link).unwrap();
        (
            scope,
            root,
            path,
            ledger.record("request-a").unwrap().unwrap(),
        )
    }

    #[test]
    fn assignment_archive_and_prepare_are_exact_inert_and_scope_stable_across_only_web_rollover() {
        let (scope, root, path, record) = linked_fixture();
        let directory = state_root(&scope, "task-fixture");
        assert_eq!(
            fs::read(directory.join("assignment.json")).unwrap(),
            fs::read(&path).unwrap()
        );
        assert!(!directory.join("journal").exists());
        let output = prepare_work_resume(&scope, &record, &record.target).unwrap();
        assert_eq!(output["status"], "PREPARED_ONLY");
        assert_eq!(output["launchAuthorized"], false);
        let assignment: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        let context = &output["taskContract"];
        for field in [
            "objective",
            "allowedPaths",
            "forbiddenPaths",
            "orderedSteps",
            "acceptanceCriteria",
            "expectedArtifacts",
            "escalationConditions",
            "verificationProfile",
        ] {
            assert_eq!(context[field], assignment["contract"][field]);
        }
        assert_eq!(context["commandProfiles"], json!([]));
        assert!(
            !context["allowedTools"]
                .as_array()
                .unwrap()
                .contains(&json!("verify.command"))
        );
        assert_eq!(
            context["contractLimits"]["maxToolCalls"],
            assignment["contract"]["maxToolCalls"]
        );
        assert_eq!(context["limitsRestartedByRollover"], false);
        let admission_path = directory.join(format!(
            "resume-prepared-{}-{}.json",
            record.target.generation, record.target.digest
        ));
        let admission_before = fs::read(&admission_path).unwrap();
        let persisted: Value = serde_json::from_slice(&admission_before).unwrap();
        assert!(persisted.get("taskContract").is_none());
        assert_eq!(
            prepare_work_resume(&scope, &record, &record.target).unwrap(),
            output
        );
        assert_eq!(fs::read(&admission_path).unwrap(), admission_before);
        assert!(!directory.join("journal").exists());
        assert!(!directory.join("jobs").exists());
        drop(acquire_project_writer_lock(&root).unwrap());
        drop(acquire_project_writer_lock(&scope.project.workspace).unwrap());
        let mut after_rollover = scope.clone();
        after_rollover.project.chatgpt_target_url =
            Some("https://chatgpt.com/c/replacement".into());
        after_rollover.project.chatgpt_target_sha256 = Some("b".repeat(64));
        after_rollover
            .project
            .chatgpt_target_history
            .push("https://chatgpt.com/c/task-source".into());
        assert_eq!(
            assignment_scope_sha256(&scope),
            assignment_scope_sha256(&after_rollover)
        );
        let replacement =
            catdesk_wake::protocol::Target::new(2, "https://chatgpt.com/c/replacement".into())
                .unwrap();
        prepare_work_resume(&after_rollover, &record, &replacement).unwrap();
        let mut changed_owner = after_rollover;
        changed_owner.project.codex_thread_id = Some("replacement-native-owner".into());
        assert_eq!(
            prepare_work_resume(&changed_owner, &record, &replacement).unwrap_err(),
            "PROJECT_TASK_WORK_ASSOCIATION_CHANGED"
        );
        assert_eq!(
            fs::read(directory.join("assignment.json")).unwrap(),
            fs::read(path).unwrap()
        );
    }

    #[test]
    fn linked_assignment_rejects_unarchived_legacy_tampering_closed_source_and_busy_writer() {
        let (scope, root, path, record) = linked_fixture();
        let bytes = fs::read(&path).unwrap();
        let sha = catdesk_wake::protocol::digest(std::str::from_utf8(&bytes).unwrap());
        assert_eq!(
            archive_work_assignment(
                &scope,
                &path,
                &"0".repeat(64),
                "request-a",
                &record.event_id
            )
            .unwrap_err(),
            "PROJECT_TASK_ASSIGNMENT_DIGEST_CHANGED"
        );
        let held = acquire_project_writer_lock(&root).unwrap();
        assert_eq!(
            prepare_work_resume(&scope, &record, &record.target).unwrap_err(),
            "PROJECT_TASK_WRITER_BUSY_OR_UNRESOLVED"
        );
        drop(held);
        let mut closed = record.clone();
        closed.phase = "CLOSED".into();
        assert_eq!(
            prepare_work_resume(&scope, &closed, &record.target).unwrap_err(),
            "PROJECT_TASK_WORK_ASSOCIATION_CHANGED"
        );
        let directory = state_root(&scope, "task-fixture");
        fs::write(directory.join("assignment.json"), b"{}").unwrap();
        assert_eq!(
            archive_work_assignment(&scope, &path, &sha, "request-a", &record.event_id)
                .unwrap_err(),
            "PROJECT_TASK_ARCHIVE_CONFLICT"
        );
        fs::remove_file(directory.join("assignment.json")).unwrap();
        let inspected = inspect_assignment(&scope, &path, 180).unwrap();
        DelegatedJournal::open(directory.join("journal"))
            .unwrap()
            .create_run(&inspected.contract)
            .unwrap();
        assert_eq!(
            archive_work_assignment(&scope, &path, &sha, "request-a", &record.event_id)
                .unwrap_err(),
            "PROJECT_TASK_LEGACY_JOURNAL_NOT_ASSOCIATED"
        );
    }

    #[test]
    fn prepared_admission_never_clears_unresolved_journal_call_or_changes_contract() {
        use crate::delegated::journal::{ToolCallRecordV1, ToolMutationKind};
        let (scope, _, path, record) = linked_fixture();
        let inspected = inspect_assignment(&scope, &path, 180).unwrap();
        let journal =
            DelegatedJournal::open(state_root(&scope, "task-fixture").join("journal")).unwrap();
        journal.create_run(&inspected.contract).unwrap();
        let run = RunId::new("task-fixture").unwrap();
        let call = ToolCallRecordV1 {
            schema_version: 1,
            run_id: run.clone(),
            tool_call_id: ToolCallId::new("uncertain-call").unwrap(),
            tool_name: "patch.apply".into(),
            request_hash: "a".repeat(64),
            arguments_hash: "b".repeat(64),
            mutation_kind: ToolMutationKind::Mutating,
            status: ToolCallStatus::Requested,
            result_hash: None,
            outcome_summary: None,
        };
        journal.record_tool_call_requested(call).unwrap();
        for phase in [
            ToolCallStatus::PolicyAllowed,
            ToolCallStatus::Executing,
            ToolCallStatus::OutcomeUnknown,
        ] {
            journal
                .transition_tool_call(
                    &run,
                    &ToolCallId::new("uncertain-call").unwrap(),
                    phase,
                    None,
                    None,
                )
                .unwrap();
        }
        let before = journal.load_tool_calls(&run).unwrap();
        assert_eq!(
            prepare_work_resume(&scope, &record, &record.target).unwrap_err(),
            "PROJECT_TASK_RECOVERY_REQUIRED"
        );
        assert_eq!(journal.load_tool_calls(&run).unwrap(), before);
        let mut changed = inspected.contract;
        changed.objective.push_str(" changed");
        let journal_root = state_root(&scope, "task-fixture").join("journal");
        let run_directory = fs::read_dir(journal_root)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        fs::write(
            run_directory.join("contract.json"),
            serde_json::to_vec(&changed).unwrap(),
        )
        .unwrap();
        assert_eq!(
            prepare_work_resume(&scope, &record, &record.target).unwrap_err(),
            "PROJECT_TASK_CONTRACT_CHANGED"
        );
    }

    fn preview(id: &str) -> Value {
        call(
            id,
            "patch.preview",
            json!({"patchId":"patch-fixture","operations":[{"path":"src/lib.rs","old":"{ 1 }","new":"{ 2 }"}]}),
        )
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn project_task_creates_assigned_nested_files_independently_without_replay() {
        let (a_scope, a_root, a_assignment) = fixture("alpha");
        let (b_scope, b_root, b_assignment) = fixture("beta");
        let path = "src/components/nested/feature.rs";
        for assignment in [&a_assignment, &b_assignment] {
            let mut value: Value = serde_json::from_slice(&fs::read(assignment).unwrap()).unwrap();
            value["contract"]["allowedPaths"] = json!([path]);
            fs::write(assignment, serde_json::to_vec(&value).unwrap()).unwrap();
        }
        let a = ProjectTask::open(&a_scope, &a_assignment).unwrap();
        let b = ProjectTask::open(&b_scope, &b_assignment).unwrap();
        let create = |id: &str, text: &str| {
            call(
                id,
                "patch.preview",
                json!({"patchId":"nested-create","operations":[{"path":path,"old":"","new":text}]}),
            )
        };
        assert!(!a_root.join("src/components").exists());
        assert!(!b_root.join("src/components").exists());
        let (pa, pb) = tokio::join!(
            a.execute(create(
                "preview-a",
                "pub const PROJECT: &str = \"alpha\";\n"
            )),
            b.execute(create("preview-b", "pub const PROJECT: &str = \"beta\";\n")),
        );
        if pa.is_err() || pb.is_err() {
            a.close().await.unwrap();
            b.close().await.unwrap();
        }
        pa.expect("assigned nested file must reach patch preview");
        pb.expect("sibling assigned nested file must reach patch preview");
        assert!(!a_root.join(path).exists());
        assert!(!b_root.join(path).exists());
        let apply = call("apply-a", "patch.apply", json!({"patchId":"nested-create"}));
        let (aa, ab) = tokio::join!(
            a.execute(apply.clone()),
            b.execute(call(
                "apply-b",
                "patch.apply",
                json!({"patchId":"nested-create"})
            )),
        );
        aa.unwrap();
        ab.unwrap();
        for (task, root, scope, name) in [
            (&a, &a_root, &a_scope, "alpha"),
            (&b, &b_root, &b_scope, "beta"),
        ] {
            let expected = format!("pub const PROJECT: &str = \"{name}\";\n");
            assert_eq!(fs::read_to_string(root.join(path)).unwrap(), expected);
            let diff = task
                .execute(call("diff", "diff.actual", json!({"paths":[path]})))
                .await
                .unwrap();
            assert!(
                diff.to_string()
                    .contains("+++ b/src/components/nested/feature.rs")
            );
            assert!(!scope.project.workspace.join("src/components").exists());
            drop(acquire_project_writer_lock(&scope.project.workspace).unwrap());
            assert_eq!(task.execute(call("other-file", "patch.preview", json!({
                "patchId":"unassigned","operations":[{"path":"src/components/nested/other.rs","old":"","new":"denied"}]
            }))).await.unwrap_err(), "PROJECT_TASK_PATH_OUTSIDE_ASSIGNMENT");
            assert!(!root.join("src/components/nested/other.rs").exists());
        }
        a.close().await.unwrap();
        b.close().await.unwrap();
        drop(a);
        drop(b);
        let cold = ProjectTask::open(&a_scope, &a_assignment).unwrap();
        let run = RunId::new("task-fixture").unwrap();
        let calls_before = cold.journal.load_tool_calls(&run).unwrap();
        assert_eq!(
            cold.execute(apply).await.unwrap_err(),
            "PROJECT_TASK_CALL_ALREADY_COMPLETED"
        );
        assert_eq!(calls_before, cold.journal.load_tool_calls(&run).unwrap());
        assert_eq!(
            fs::read_to_string(a_root.join(path)).unwrap(),
            "pub const PROJECT: &str = \"alpha\";\n"
        );
        cold.close().await.unwrap();
        drop(cold);
        for scope in [a_scope, b_scope] {
            let parent = scope
                .control_workspace
                .parent()
                .unwrap()
                .canonicalize()
                .unwrap();
            assert_eq!(
                parent.parent(),
                Some(std::env::temp_dir().canonicalize().unwrap().as_path())
            );
            fs::remove_dir_all(parent).unwrap();
        }
    }

    #[tokio::test]
    async fn project_task_nested_paths_refuse_redirected_and_invalid_ancestors_before_writes() {
        let (scope, root, assignment) = fixture("alpha");
        let isolated_before = fs::read(root.join("src/lib.rs")).unwrap();
        let native_before = fs::read(scope.project.workspace.join("src/lib.rs")).unwrap();
        let task = ProjectTask::open(&scope, &assignment).unwrap();
        let parent = root.parent().unwrap();
        let outside = parent.join("outside");
        fs::create_dir(&outside).unwrap();
        let link = root.join("src/redirect");
        let output = std::process::Command::new("powershell.exe")
            .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command",
                "New-Item -ItemType Junction -Path $env:CATDESK_TEST_LINK -Target $env:CATDESK_TEST_TARGET -ErrorAction Stop | Out-Null"])
            .env("CATDESK_TEST_LINK", crate::command::normalize_windows_verbatim_path(link.clone()))
            .env("CATDESK_TEST_TARGET", crate::command::normalize_windows_verbatim_path(outside.clone()))
            .output().unwrap();
        assert!(
            output.status.success(),
            "isolated directory junction fixture required"
        );
        assert_eq!(
            link.canonicalize().unwrap(),
            outside.canonicalize().unwrap()
        );
        let invalid = |id: &str, path: &str| {
            call(
                id,
                "patch.preview",
                json!({
                    "patchId":"denied-nested","operations":[{"path":path,"old":"","new":"denied"}]
                }),
            )
        };
        assert_eq!(
            task.execute(invalid("outside", "src/redirect/new/deep/file.rs"))
                .await
                .unwrap_err(),
            "PROJECT_TASK_PATH_OUTSIDE_ASSIGNMENT"
        );
        assert!(!outside.join("new").exists());
        fs::remove_dir(&outside).unwrap();
        assert_eq!(
            task.execute(invalid("dangling", "src/redirect/new/deep/file.rs"))
                .await
                .unwrap_err(),
            "PROJECT_TASK_PATH_UNAVAILABLE"
        );
        assert!(
            !outside.exists(),
            "dangling redirect must not recreate its target"
        );
        assert_eq!(
            task.execute(invalid("not-directory", "src/lib.rs/new/file.rs"))
                .await
                .unwrap_err(),
            "PROJECT_TASK_PATH_UNAVAILABLE"
        );
        for path in [
            "../source/new/file.rs",
            "src/.git/new/file.rs",
            "src/.catdesk/new/file.rs",
            "src/new/../file.rs",
        ] {
            assert_eq!(
                task.execute(invalid("invalid", path)).await.unwrap_err(),
                "PROJECT_TASK_PATH_OUTSIDE_ASSIGNMENT"
            );
        }
        assert!(
            task.journal
                .load_tool_calls(&RunId::new("task-fixture").unwrap())
                .unwrap()
                .is_empty()
        );
        assert_eq!(fs::read(root.join("src/lib.rs")).unwrap(), isolated_before);
        assert_eq!(
            fs::read(scope.project.workspace.join("src/lib.rs")).unwrap(),
            native_before
        );
        task.close().await.unwrap();
        drop(task);
        fs::remove_dir(link).unwrap();
        let cleanup = parent.canonicalize().unwrap();
        assert_eq!(
            cleanup.parent(),
            Some(std::env::temp_dir().canonicalize().unwrap().as_path())
        );
        fs::remove_dir_all(cleanup).unwrap();
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
    async fn project_task_retirement_fence_is_project_scoped_and_released_after_clean_close() {
        let (a_scope, _, a_assignment) = fixture("alpha");
        let (mut b_scope, b_root, b_assignment) = fixture("beta");
        let registry_path = a_scope
            .control_workspace
            .join(".catdesk/projects/projects.json");
        let mut registry: crate::delegated::autonomy_projects::AutonomousProjectRegistryV1 =
            serde_json::from_slice(&fs::read(&registry_path).unwrap()).unwrap();
        registry.projects.push(b_scope.project.clone());
        fs::write(&registry_path, serde_json::to_vec(&registry).unwrap()).unwrap();
        b_scope.control_workspace = a_scope.control_workspace.clone();

        let a = ProjectTask::open(&a_scope, &a_assignment).unwrap();
        assert_eq!(
            crate::operator_facade::acquire_web_work_lease(&a_scope.control_workspace, "alpha")
                .err()
                .unwrap(),
            "WEB_ROLLOVER_PROJECT_BUSY"
        );
        let b = ProjectTask::open(&b_scope, &b_assignment).unwrap();
        b.execute(preview("sibling-fence-preview")).await.unwrap();
        b.execute(call(
            "sibling-fence-apply",
            "patch.apply",
            json!({"patchId":"patch-fixture"}),
        ))
        .await
        .unwrap();
        assert!(
            fs::read_to_string(b_root.join("src/lib.rs"))
                .unwrap()
                .contains("{ 2 }")
        );
        a.close().await.unwrap();
        // A retained closed connection no longer prevents retirement.
        drop(
            crate::operator_facade::acquire_web_work_lease(&a_scope.control_workspace, "alpha")
                .unwrap(),
        );
        assert_eq!(
            crate::operator_facade::acquire_web_work_lease(&b_scope.control_workspace, "beta")
                .err()
                .unwrap(),
            "WEB_ROLLOVER_PROJECT_BUSY"
        );
        b.close().await.unwrap();
        drop(
            crate::operator_facade::acquire_web_work_lease(&b_scope.control_workspace, "beta")
                .unwrap(),
        );
    }

    #[test]
    fn project_task_stale_registration_refuses_before_creating_task_state() {
        let (scope, _, assignment) = fixture("alpha");
        let registry_path = scope
            .control_workspace
            .join(".catdesk/projects/projects.json");
        let mut registry: crate::delegated::autonomy_projects::AutonomousProjectRegistryV1 =
            serde_json::from_slice(&fs::read(&registry_path).unwrap()).unwrap();
        registry.projects.clear();
        fs::write(&registry_path, serde_json::to_vec(&registry).unwrap()).unwrap();
        assert_eq!(
            ProjectTask::open(&scope, &assignment).err().unwrap(),
            "PROJECT_TASK_REGISTRATION_CHANGED"
        );
        assert!(
            !scope
                .control_workspace
                .join(".catdesk/project-mcp-tasks")
                .exists()
        );
        assert!(
            !scope
                .control_workspace
                .join(".catdesk/web-rollovers")
                .exists()
        );
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
        assert_eq!(
            crate::operator_facade::acquire_web_work_lease(&scope.control_workspace, "alpha")
                .err()
                .unwrap(),
            "WEB_ROLLOVER_PROJECT_BUSY"
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
