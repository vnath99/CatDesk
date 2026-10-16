//! Explicit current-user operator queue for registered Codex projects.
//!
//! Task payloads and receipts live outside every Codex workspace. The
//! existing control registry owns the fair cursor and exact workspace lease;
//! this is same-user operator coordination, not hostile-worker isolation.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use uuid::Uuid;

use crate::delegated::autonomy_projects::{
    AutonomousProjectRegistryStoreV1, AutonomousProjectV1, AutonomousWorkspaceLeaseV1,
    GlobalScheduleDecisionV1, ScheduledProviderV1, resolve_registered_project_from_control_root,
};
use crate::delegated::autonomy_runtime::{
    acquire_project_writer_lock, fixed_current_user_catdesk_state_root,
    fixed_current_user_catdesk_state_root_path,
};

use super::{
    CodexScope, OperatorCodexAction, bounded_probe, codex_command,
    ensure_no_interrupted_managed_writer, ensure_same_project_binding, exact_thread,
    invocation_args, registered_project, resolve_native_cli, valid_effort, valid_model,
};

const QUEUE_SCHEMA_VERSION: u32 = 1;
const MAX_TASK_BYTES: usize = 24 * 1024;
const MAX_JOB_BYTES: u64 = 64 * 1024;
const MAX_JOBS: usize = 512;
const MAX_DIR_ENTRIES: usize = 1024;
const MAX_RECEIPT_BYTES: u64 = 16 * 1024;
const MAX_NATIVE_EXE_BYTES: u64 = 512 * 1024 * 1024;
// Expiry is an operator recovery signal only; the scheduler never reclaims
// this lease based on time. The window exceeds the maximum native turn bound.
const LEASE_SECONDS: u64 = 24 * 60 * 60;
const PROMPT_TIMEOUT: Duration = Duration::from_secs(30);
const KILL_TIMEOUT: Duration = Duration::from_secs(20);
const DEFAULT_TURN_TIMEOUT: Duration = Duration::from_secs(2 * 60 * 60);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum QueueAction {
    Add {
        scope: CodexScope,
        task_id: String,
        task: String,
        model: Option<String>,
        effort: Option<String>,
    },
    Status,
    Receipt {
        receipt_id: String,
    },
    Dispatch,
    Pause {
        scope: CodexScope,
        task_id: String,
    },
    Resume {
        scope: CodexScope,
        task_id: String,
    },
}

pub(crate) fn parse_action(args: &[String]) -> Result<QueueAction, String> {
    let verb = args.get(3).map(String::as_str).ok_or_else(|| {
        "CODEX_QUEUE_USAGE: expected add, status, receipt, dispatch, pause, or resume".to_string()
    })?;
    if !matches!(
        verb,
        "add" | "status" | "receipt" | "dispatch" | "pause" | "resume"
    ) {
        return Err("CODEX_QUEUE_USAGE: unsupported queue action".into());
    }
    let mut values = BTreeMap::new();
    let mut index = 4;
    while index < args.len() {
        let flag = args[index].as_str();
        if !matches!(
            flag,
            "--project-id"
                | "--workspace"
                | "--task-id"
                | "--task"
                | "--model"
                | "--effort"
                | "--receipt-id"
        ) {
            return Err("CODEX_QUEUE_USAGE: unsupported option".into());
        }
        let value = args
            .get(index + 1)
            .ok_or_else(|| "CODEX_QUEUE_USAGE: an option requires a value".to_string())?;
        if value.is_empty()
            || value.len() > MAX_TASK_BYTES
            || values.insert(flag, value.clone()).is_some()
        {
            return Err("CODEX_QUEUE_USAGE: an option is empty, oversized, or repeated".into());
        }
        index += 2;
    }
    if matches!(verb, "status" | "dispatch") {
        if !values.is_empty() {
            return Err("CODEX_QUEUE_USAGE: status and dispatch take no options".into());
        }
        return Ok(if verb == "status" {
            QueueAction::Status
        } else {
            QueueAction::Dispatch
        });
    }
    if verb == "receipt" {
        let receipt_id = values
            .remove("--receipt-id")
            .ok_or("CODEX_QUEUE_USAGE: receipt requires --receipt-id")?;
        if !values.is_empty()
            || !Uuid::parse_str(&receipt_id).is_ok_and(|id| id.to_string() == receipt_id)
        {
            return Err("CODEX_QUEUE_USAGE: receipt requires one canonical receipt UUID".into());
        }
        return Ok(QueueAction::Receipt { receipt_id });
    }
    let project_id = values
        .remove("--project-id")
        .ok_or("CODEX_QUEUE_USAGE: --project-id is required")?;
    valid_slug(&project_id)?;
    let workspace = PathBuf::from(
        values
            .remove("--workspace")
            .ok_or("CODEX_QUEUE_USAGE: --workspace is required")?,
    );
    if !workspace.is_absolute() {
        return Err("CODEX_SCOPE: --workspace must be absolute".into());
    }
    let task_id = values
        .remove("--task-id")
        .ok_or("CODEX_QUEUE_USAGE: --task-id is required")?;
    valid_slug(&task_id)?;
    let scope = CodexScope {
        project_id,
        workspace,
    };
    if verb == "pause" || verb == "resume" {
        if !values.is_empty() {
            return Err("CODEX_QUEUE_USAGE: pause and resume only accept scope and task ID".into());
        }
        return Ok(if verb == "pause" {
            QueueAction::Pause { scope, task_id }
        } else {
            QueueAction::Resume { scope, task_id }
        });
    }
    let task = values
        .remove("--task")
        .ok_or("CODEX_QUEUE_USAGE: add requires --task")?;
    if task.trim().is_empty() || task.contains('\0') || task.len() > MAX_TASK_BYTES {
        return Err("CODEX_QUEUE_USAGE: task is empty or invalid".into());
    }
    let model = values.remove("--model");
    let effort = values.remove("--effort");
    if model.as_deref().is_some_and(|value| !valid_model(value))
        || effort.as_deref().is_some_and(|value| !valid_effort(value))
        || !values.is_empty()
    {
        return Err("CODEX_QUEUE_USAGE: invalid model, effort, or option".into());
    }
    Ok(QueueAction::Add {
        scope,
        task_id,
        task,
        model,
        effort,
    })
}

fn valid_slug(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        Err("CODEX_QUEUE_USAGE: project and task IDs must be conservative slugs".into())
    } else {
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct QueueBinding {
    project_id: String,
    workspace: PathBuf,
    git_identity: String,
    verification_profile: String,
    thread_id: String,
    target_sha256: Option<String>,
}

impl QueueBinding {
    fn from_project(project: &AutonomousProjectV1) -> Result<Self, String> {
        Ok(Self {
            project_id: project.project_id.clone(),
            workspace: project.workspace.clone(),
            git_identity: project.git_identity.clone(),
            verification_profile: project.verification_profile.clone(),
            thread_id: exact_thread(project)?.into(),
            target_sha256: project.chatgpt_target_sha256.clone(),
        })
    }
    fn matches(&self, project: &AutonomousProjectV1) -> bool {
        Self::from_project(project).as_ref() == Ok(self)
    }
    fn scope(&self) -> CodexScope {
        CodexScope {
            project_id: self.project_id.clone(),
            workspace: self.workspace.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum QueueState {
    Ready,
    Paused,
    Claimed,
    Unknown,
    NativeExitedZero,
    Failed,
    Attention,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct QueueAttempt {
    attempt_id: String,
    lease: AutonomousWorkspaceLeaseV1,
    started_at_unix: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct QueueJob {
    schema_version: u32,
    task_id: String,
    binding: QueueBinding,
    prompt: String,
    prompt_sha256: String,
    model: Option<String>,
    effort: Option<String>,
    state: QueueState,
    created_at_unix: u64,
    attempt: Option<QueueAttempt>,
    receipt_id: Option<String>,
}

impl QueueJob {
    fn new(
        task_id: String,
        project: &AutonomousProjectV1,
        prompt: String,
        model: Option<String>,
        effort: Option<String>,
    ) -> Result<Self, String> {
        let prompt_sha256 = format!("{:x}", Sha256::digest(prompt.as_bytes()));
        Ok(Self {
            schema_version: QUEUE_SCHEMA_VERSION,
            task_id,
            binding: QueueBinding::from_project(project)?,
            prompt,
            prompt_sha256,
            model,
            effort,
            state: QueueState::Ready,
            created_at_unix: unix_now()?,
            attempt: None,
            receipt_id: None,
        })
    }
    fn same_request(&self, other: &Self) -> bool {
        self.task_id == other.task_id
            && self.binding == other.binding
            && self.prompt == other.prompt
            && self.model == other.model
            && self.effort == other.effort
    }
    fn summary(&self) -> Value {
        json!({
            "projectId": self.binding.project_id, "taskId": self.task_id,
            "workspace": self.binding.workspace, "threadId": self.binding.thread_id,
            "targetSha256": self.binding.target_sha256,
            "promptSha256": self.prompt_sha256, "state": self.state,
            "createdAtUnix": self.created_at_unix,
            "attemptId": self.attempt.as_ref().map(|attempt| &attempt.attempt_id),
            "receiptId": self.receipt_id,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct QueueReceipt {
    schema_version: u32,
    receipt_id: String,
    attempt_id: String,
    project_id: String,
    task_id: String,
    workspace: PathBuf,
    thread_id: String,
    target_sha256: Option<String>,
    prompt_sha256: String,
    requested_model: Option<String>,
    requested_effort: Option<String>,
    native_executable: Option<PathBuf>,
    native_version: Option<String>,
    native_executable_sha256: Option<String>,
    evidence_level: String,
    exit_code: Option<i32>,
    child_exit_observed: bool,
    binding_still_matches: bool,
    outcome: String,
    recorded_at_unix: u64,
}

struct QueueStore {
    root: PathBuf,
}

impl QueueStore {
    fn namespaced_path(
        control: &AutonomousProjectV1,
        project_workspace: &Path,
    ) -> Result<PathBuf, String> {
        let workspace = project_workspace
            .canonicalize()
            .map_err(|_| "CODEX_QUEUE: project workspace is unavailable".to_string())?;
        let planned_root = fixed_current_user_catdesk_state_root_path()
            .map_err(|_| "CODEX_QUEUE: fixed current-user state root is unavailable".to_string())?;
        reject_overlap(&planned_root, &workspace)?;
        reject_overlap(&planned_root, &control.workspace)?;
        let identity = format!("{}\0{}", control.workspace.display(), control.git_identity);
        let namespace = format!("{:x}", Sha256::digest(identity.as_bytes()));
        Ok(planned_root.join("OperatorCodexQueue").join(namespace))
    }
    fn open_for_operator(
        control: &AutonomousProjectV1,
        project_workspace: &Path,
    ) -> Result<Self, String> {
        let planned = Self::namespaced_path(control, project_workspace)?;
        let workspace = project_workspace
            .canonicalize()
            .map_err(|_| "CODEX_QUEUE: project workspace is unavailable".to_string())?;
        let registry = scheduler(&control.workspace)?
            .load_registry()
            .map_err(|_| "CODEX_QUEUE: control registry is invalid".to_string())?;
        let state_root = planned
            .parent()
            .and_then(Path::parent)
            .ok_or("CODEX_QUEUE: queue namespace is invalid")?;
        for registered in &registry.projects {
            reject_overlap(state_root, &registered.workspace)?;
        }
        let user_root = fixed_current_user_catdesk_state_root()
            .map_err(|_| "CODEX_QUEUE: fixed current-user state root is unavailable".to_string())?;
        reject_overlap(&user_root, &workspace)?;
        reject_overlap(&user_root, &control.workspace)?;
        let queue_parent = user_root.join("OperatorCodexQueue");
        checked_dir(&queue_parent)?;
        let namespace = planned
            .file_name()
            .ok_or("CODEX_QUEUE: queue namespace is invalid")?;
        Self::open_at(queue_parent.join(namespace))
    }
    fn open_read_only(control: &AutonomousProjectV1) -> Result<Option<Self>, String> {
        let planned = Self::namespaced_path(control, &control.workspace)?;
        let user_root = planned
            .parent()
            .and_then(Path::parent)
            .ok_or("CODEX_QUEUE: queue namespace is invalid")?;
        let queue_parent = planned
            .parent()
            .ok_or("CODEX_QUEUE: queue namespace is invalid")?;
        for path in [user_root, queue_parent, planned.as_path()] {
            let metadata = match fs::symlink_metadata(path) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(_) => return Err("CODEX_QUEUE: queue state is unreadable".into()),
            };
            if !metadata.is_dir() || redirected(&metadata) {
                return Err("CODEX_QUEUE: queue state is redirected or unsafe".into());
            }
        }
        for child in ["jobs", "receipts"] {
            let metadata = fs::symlink_metadata(planned.join(child))
                .map_err(|_| "CODEX_QUEUE: queue state is incomplete".to_string())?;
            if !metadata.is_dir() || redirected(&metadata) {
                return Err("CODEX_QUEUE: queue state is redirected or unsafe".into());
            }
        }
        Ok(Some(Self { root: planned }))
    }
    fn open_at(root: PathBuf) -> Result<Self, String> {
        checked_dir(&root)?;
        checked_dir(&root.join("jobs"))?;
        checked_dir(&root.join("receipts"))?;
        Ok(Self { root })
    }
    fn lock(&self) -> Result<File, String> {
        let path = self.root.join("queue.lock");
        checked_file_if_present(&path, 0)?;
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        nofollow(&mut options);
        let file = options
            .open(&path)
            .map_err(|_| "CODEX_QUEUE: queue lock is unavailable")?;
        let opened = file
            .metadata()
            .map_err(|_| "CODEX_QUEUE: queue lock is unreadable")?;
        let observed =
            fs::symlink_metadata(&path).map_err(|_| "CODEX_QUEUE: queue lock is unreadable")?;
        if !opened.is_file()
            || opened.len() != 0
            || redirected(&opened)
            || !observed.is_file()
            || redirected(&observed)
            || !same_open_file_identity(&file, &path, &opened, &observed)
        {
            return Err("CODEX_QUEUE: queue lock is redirected or changed".into());
        }
        file.try_lock()
            .map_err(|_| "CODEX_QUEUE: another queue action is changing state".to_string())?;
        Ok(file)
    }
    fn job_path(&self, project_id: &str, task_id: &str) -> PathBuf {
        let digest = Sha256::digest(format!("{project_id}\0{task_id}").as_bytes());
        self.root.join("jobs").join(format!("{digest:x}.json"))
    }
    fn load_job(&self, project_id: &str, task_id: &str) -> Result<Option<QueueJob>, String> {
        let path = self.job_path(project_id, task_id);
        let Some(bytes) = read_regular_bounded(&path, MAX_JOB_BYTES)? else {
            return Ok(None);
        };
        let job: QueueJob = serde_json::from_slice(&bytes)
            .map_err(|_| "CODEX_QUEUE: a job record is corrupt".to_string())?;
        validate_job(&job)?;
        if job.binding.project_id != project_id || job.task_id != task_id {
            return Err("CODEX_QUEUE: job path and identity disagree".into());
        }
        Ok(Some(job))
    }
    fn jobs(&self) -> Result<Vec<QueueJob>, String> {
        let mut jobs = Vec::new();
        let mut identities = BTreeSet::new();
        let mut entries = 0;
        for entry in fs::read_dir(self.root.join("jobs"))
            .map_err(|_| "CODEX_QUEUE: jobs directory is unavailable")?
        {
            entries += 1;
            if entries > MAX_DIR_ENTRIES {
                return Err("CODEX_QUEUE: jobs directory entry limit reached".into());
            }
            let entry = entry.map_err(|_| "CODEX_QUEUE: jobs directory is unreadable")?;
            let path = entry.path();
            if path.extension().is_some_and(|extension| extension == "tmp") {
                continue;
            }
            if jobs.len() >= MAX_JOBS {
                return Err("CODEX_QUEUE: job limit reached".into());
            }
            let Some(bytes) = read_regular_bounded(&path, MAX_JOB_BYTES)? else {
                continue;
            };
            let job: QueueJob = serde_json::from_slice(&bytes)
                .map_err(|_| "CODEX_QUEUE: a job record is corrupt".to_string())?;
            validate_job(&job)?;
            if path != self.job_path(&job.binding.project_id, &job.task_id)
                || !identities.insert((job.binding.project_id.clone(), job.task_id.clone()))
            {
                return Err("CODEX_QUEUE: job filename or identity is inconsistent".into());
            }
            jobs.push(job);
        }
        jobs.sort_by(|left, right| {
            (
                left.created_at_unix,
                &left.binding.project_id,
                &left.task_id,
            )
                .cmp(&(
                    right.created_at_unix,
                    &right.binding.project_id,
                    &right.task_id,
                ))
        });
        Ok(jobs)
    }
    fn save_job(&self, job: &QueueJob) -> Result<(), String> {
        validate_job(job)?;
        let path = self.job_path(&job.binding.project_id, &job.task_id);
        checked_file_if_present(&path, MAX_JOB_BYTES)?;
        let bytes =
            serde_json::to_vec_pretty(job).map_err(|_| "CODEX_QUEUE: job could not be encoded")?;
        if bytes.len().saturating_add(1) as u64 > MAX_JOB_BYTES {
            return Err("CODEX_QUEUE: job record is oversized".into());
        }
        atomic_write(&path, &bytes)
    }
    fn write_receipt(&self, receipt: &QueueReceipt) -> Result<(), String> {
        let path = self
            .root
            .join("receipts")
            .join(format!("{}.json", receipt.receipt_id));
        let bytes = serde_json::to_vec_pretty(receipt)
            .map_err(|_| "CODEX_QUEUE: receipt could not be encoded")?;
        if bytes.len().saturating_add(1) as u64 > MAX_RECEIPT_BYTES {
            return Err("CODEX_QUEUE: receipt record is oversized".into());
        }
        create_new_record(&path, &bytes)
    }
    fn read_receipt(&self, receipt_id: &str) -> Result<QueueReceipt, String> {
        let path = self
            .root
            .join("receipts")
            .join(format!("{receipt_id}.json"));
        let bytes = read_regular_bounded(&path, MAX_RECEIPT_BYTES)?
            .ok_or("CODEX_QUEUE: receipt is unavailable")?;
        let receipt: QueueReceipt = serde_json::from_slice(&bytes)
            .map_err(|_| "CODEX_QUEUE: receipt is corrupt".to_string())?;
        if receipt.schema_version != QUEUE_SCHEMA_VERSION
            || receipt.receipt_id != receipt_id
            || !Uuid::parse_str(&receipt.attempt_id)
                .is_ok_and(|id| id.to_string() == receipt.attempt_id)
        {
            return Err("CODEX_QUEUE: receipt identity is invalid".into());
        }
        let job = self
            .load_job(&receipt.project_id, &receipt.task_id)?
            .ok_or("CODEX_QUEUE: receipt task is unavailable")?;
        if job.binding.workspace != receipt.workspace
            || job.binding.thread_id != receipt.thread_id
            || job.binding.target_sha256 != receipt.target_sha256
            || job.prompt_sha256 != receipt.prompt_sha256
        {
            return Err("CODEX_QUEUE: receipt task binding changed".into());
        }
        Ok(receipt)
    }
}

fn reject_overlap(state_root: &Path, project_workspace: &Path) -> Result<(), String> {
    if state_root.starts_with(project_workspace) || project_workspace.starts_with(state_root) {
        Err("CODEX_QUEUE: user state overlaps a project workspace".into())
    } else {
        Ok(())
    }
}

fn unix_now() -> Result<u64, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|_| "CODEX_QUEUE: system clock is invalid".into())
}

fn redirected(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        false
    }
}

fn nofollow(options: &mut OpenOptions) {
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x0020_0000); // FILE_FLAG_OPEN_REPARSE_POINT
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
}

fn same_open_file_identity(
    file: &File,
    path: &Path,
    opened: &fs::Metadata,
    observed: &fs::Metadata,
) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        let mut options = OpenOptions::new();
        options.read(true);
        nofollow(&mut options);
        let Ok(second) = options.open(path) else {
            return false;
        };
        let Ok(second_metadata) = second.metadata() else {
            return false;
        };
        if !second_metadata.is_file() || redirected(&second_metadata) {
            return false;
        }
        let Ok(first_info) = crate::windows_protected_fs::handle_information(
            file.as_raw_handle(),
            "operator queue record",
        ) else {
            return false;
        };
        let Ok(second_info) = crate::windows_protected_fs::handle_information(
            second.as_raw_handle(),
            "operator queue record",
        ) else {
            return false;
        };
        let _ = (opened, observed);
        first_info.file_attributes & 0x400 == 0
            && second_info.file_attributes & 0x400 == 0
            && first_info.volume_serial_number == second_info.volume_serial_number
            && first_info.file_index_high == second_info.file_index_high
            && first_info.file_index_low == second_info.file_index_low
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let _ = (file, path);
        opened.dev() == observed.dev() && opened.ino() == observed.ino()
    }
    #[cfg(not(any(windows, unix)))]
    {
        let _ = (file, path, opened, observed);
        false
    }
}

fn checked_dir(path: &Path) -> Result<(), String> {
    match fs::create_dir(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(_) => return Err("CODEX_QUEUE: state directory is unavailable".into()),
    }
    let metadata =
        fs::symlink_metadata(path).map_err(|_| "CODEX_QUEUE: state directory is unavailable")?;
    if !metadata.is_dir() || redirected(&metadata) {
        return Err("CODEX_QUEUE: state directory is redirected or unsafe".into());
    }
    Ok(())
}

fn checked_file_if_present(path: &Path, max_bytes: u64) -> Result<(), String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err("CODEX_QUEUE: state file is unreadable".into()),
    };
    if !metadata.is_file() || redirected(&metadata) || metadata.len() > max_bytes {
        return Err("CODEX_QUEUE: state file is redirected or oversized".into());
    }
    Ok(())
}

fn read_regular_bounded(path: &Path, max_bytes: u64) -> Result<Option<Vec<u8>>, String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("CODEX_QUEUE: state file is unreadable".into()),
    };
    if !metadata.is_file() || redirected(&metadata) || metadata.len() > max_bytes {
        return Err("CODEX_QUEUE: state file is redirected or oversized".into());
    }
    let mut options = OpenOptions::new();
    options.read(true);
    nofollow(&mut options);
    let file = options
        .open(path)
        .map_err(|_| "CODEX_QUEUE: state file is unreadable")?;
    let opened = file
        .metadata()
        .map_err(|_| "CODEX_QUEUE: state file is unreadable")?;
    let observed =
        fs::symlink_metadata(path).map_err(|_| "CODEX_QUEUE: state file is unreadable")?;
    if !opened.is_file()
        || redirected(&opened)
        || opened.len() > max_bytes
        || !observed.is_file()
        || redirected(&observed)
        || !same_open_file_identity(&file, path, &opened, &observed)
    {
        return Err("CODEX_QUEUE: state file changed or is redirected".into());
    }
    let mut bytes = Vec::new();
    (&file)
        .take(max_bytes + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "CODEX_QUEUE: state file is unreadable")?;
    if bytes.len() as u64 > max_bytes {
        return Err("CODEX_QUEUE: state file is oversized".into());
    }
    let observed_after = fs::symlink_metadata(path)
        .map_err(|_| "CODEX_QUEUE: state file changed during read".to_string())?;
    if !same_open_file_identity(&file, path, &opened, &observed_after)
        || redirected(&observed_after)
    {
        return Err("CODEX_QUEUE: state file changed during read".into());
    }
    Ok(Some(bytes))
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or("CODEX_QUEUE: state path has no parent")?;
    let draft = parent.join(format!(".queue-{}.tmp", Uuid::new_v4()));
    create_new_record(&draft, bytes)?;
    fs::rename(&draft, path).map_err(|_| "CODEX_QUEUE: atomic state publication failed".to_string())
}

fn create_new_record(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|_| "CODEX_QUEUE: record already exists or cannot be created")?;
    file.write_all(bytes)
        .and_then(|()| file.write_all(b"\n"))
        .and_then(|()| file.sync_all())
        .map_err(|_| "CODEX_QUEUE: record could not be persisted".to_string())
}

fn validate_job(job: &QueueJob) -> Result<(), String> {
    valid_slug(&job.task_id)?;
    valid_slug(&job.binding.project_id)?;
    if job.schema_version != QUEUE_SCHEMA_VERSION
        || job.prompt.is_empty()
        || job.prompt.len() > MAX_TASK_BYTES
        || job.prompt.contains('\0')
        || job.prompt_sha256 != format!("{:x}", Sha256::digest(job.prompt.as_bytes()))
        || !Uuid::parse_str(&job.binding.thread_id)
            .is_ok_and(|id| id.to_string() == job.binding.thread_id)
        || job
            .model
            .as_deref()
            .is_some_and(|value| !valid_model(value))
        || job
            .effort
            .as_deref()
            .is_some_and(|value| !valid_effort(value))
        || job.binding.workspace.as_os_str().is_empty()
    {
        return Err("CODEX_QUEUE: job record is invalid".into());
    }
    if job.state == QueueState::Claimed && job.attempt.is_none() {
        return Err("CODEX_QUEUE: claimed job has no attempt".into());
    }
    Ok(())
}

pub(crate) async fn execute_action(
    action: QueueAction,
    control_workspace: &Path,
) -> Result<Value, String> {
    // A queue verb never accepts a caller-selected control plane. All state
    // below is anchored to the registered CatDesk Git root.
    let control = resolve_registered_project_from_control_root(control_workspace, "catdesk")
        .map_err(|_| {
            "CODEX_QUEUE_SCOPE: CatDesk control Git identity is unavailable".to_string()
        })?;
    let pause_action = matches!(&action, QueueAction::Pause { .. });
    match action {
        QueueAction::Add {
            scope,
            task_id,
            task,
            model,
            effort,
        } => {
            let project = registered_project(&control.control_workspace, &scope)?;
            let store = QueueStore::open_for_operator(&control.project, &project.workspace)?;
            let _lock = store.lock()?;
            let proposed = QueueJob::new(task_id, &project, task, model, effort)?;
            if let Some(existing) =
                store.load_job(&proposed.binding.project_id, &proposed.task_id)?
            {
                if !existing.same_request(&proposed) {
                    return Err("CODEX_QUEUE_IDEMPOTENCY: task ID already has a different scope, prompt, or profile".into());
                }
                return Ok(
                    json!({ "action":"codex_queue_add", "idempotent":true, "job":existing.summary() }),
                );
            }
            if store.jobs()?.len() >= MAX_JOBS {
                return Err("CODEX_QUEUE: job limit reached".into());
            }
            store.save_job(&proposed)?;
            Ok(json!({ "action":"codex_queue_add", "idempotent":false, "job":proposed.summary() }))
        }
        QueueAction::Status => {
            let jobs = match QueueStore::open_read_only(&control.project)? {
                Some(store) => store
                    .jobs()?
                    .iter()
                    .map(QueueJob::summary)
                    .collect::<Vec<_>>(),
                None => Vec::new(),
            };
            Ok(json!({
                "action":"codex_queue_status", "jobs":jobs,
                "claimRecoveryPolicy":"CLAIMED and UNKNOWN jobs are never replayed automatically",
                "execution":"explicit_dispatch_only", "authority":"same_user_operator_coordination",
            }))
        }
        QueueAction::Receipt { receipt_id } => {
            let store = QueueStore::open_read_only(&control.project)?
                .ok_or("CODEX_QUEUE: queue state is unavailable")?;
            Ok(
                json!({ "action":"codex_queue_receipt", "receipt":store.read_receipt(&receipt_id)? }),
            )
        }
        QueueAction::Pause { scope, task_id } | QueueAction::Resume { scope, task_id } => {
            let pause = pause_action;
            let project = registered_project(&control.control_workspace, &scope)?;
            let store = QueueStore::open_for_operator(&control.project, &project.workspace)?;
            let _lock = store.lock()?;
            let mut job = store
                .load_job(&project.project_id, &task_id)?
                .ok_or("CODEX_QUEUE: task ID is not queued for this project")?;
            if !job.binding.matches(&project) {
                return Err("CODEX_QUEUE_SCOPE: queued task binding drifted".into());
            }
            let expected = if pause {
                QueueState::Ready
            } else {
                QueueState::Paused
            };
            if job.state != expected {
                return Err("CODEX_QUEUE_STATE: only a quiescent ready task may pause, or a paused task resume".into());
            }
            job.state = if pause {
                QueueState::Paused
            } else {
                QueueState::Ready
            };
            store.save_job(&job)?;
            Ok(
                json!({ "action":if pause { "codex_queue_pause" } else { "codex_queue_resume" }, "job":job.summary() }),
            )
        }
        QueueAction::Dispatch => dispatch_one(&control.project).await,
    }
}

fn scheduler(control_workspace: &Path) -> Result<AutonomousProjectRegistryStoreV1, String> {
    AutonomousProjectRegistryStoreV1::open_read_only(control_workspace.join(".catdesk/projects"))
        .map_err(|_| "CODEX_QUEUE: control registry is unavailable".to_string())
}

fn claim_one(control_workspace: &Path, queue: &QueueStore) -> Result<Option<QueueJob>, String> {
    let _lock = queue.lock()?;
    let read_only_scheduler = scheduler(control_workspace)?;
    let registry = read_only_scheduler
        .load_registry()
        .map_err(|_| "CODEX_QUEUE: control registry is invalid".to_string())?;
    // The fixed user-state root must stay outside every registered project.
    for project in &registry.projects {
        if queue.root.starts_with(&project.workspace) || project.workspace.starts_with(&queue.root)
        {
            return Err("CODEX_QUEUE: queue state overlaps a registered workspace".into());
        }
    }
    let mut ready = BTreeMap::<String, QueueJob>::new();
    for mut job in queue.jobs()? {
        if job.state != QueueState::Ready {
            continue;
        }
        let observed = registered_project(control_workspace, &job.binding.scope());
        if !observed
            .as_ref()
            .is_ok_and(|project| job.binding.matches(project))
        {
            // No launch occurred. Leave the original authority snapshot and
            // prompt intact for a human to reconcile; do not migrate it to a
            // new root, target, or thread.
            job.state = QueueState::Attention;
            queue.save_job(&job)?;
            continue;
        }
        ready.entry(job.binding.project_id.clone()).or_insert(job);
    }
    let ready_ids = registry
        .projects
        .iter()
        .filter(|project| ready.contains_key(&project.project_id))
        .map(|project| project.project_id.as_str())
        .collect::<Vec<_>>();
    if ready_ids.is_empty() {
        return Ok(None);
    }
    // Dispatch is the first queue operation allowed to initialize scheduler
    // children. Validate the existing registry and every ready binding before
    // opening the mutating store, then reject any registry change observed
    // across that transition. `open` checks existing children before use and
    // creates the absent leases directory required by scheduler claims.
    let scheduler =
        AutonomousProjectRegistryStoreV1::open(control_workspace.join(".catdesk/projects"))
            .map_err(|_| "CODEX_QUEUE: scheduler state is unavailable or unsafe".to_string())?;
    if scheduler
        .load_registry()
        .map_err(|_| "CODEX_QUEUE: control registry is invalid".to_string())?
        != registry
    {
        return Err("CODEX_QUEUE_SCOPE: registry changed before scheduler claim".into());
    }
    let now = unix_now()?;
    let owner = format!("queue-{}", Uuid::new_v4());
    let decision = scheduler
        .schedule_next_ready_worker(
            &ready_ids,
            &owner,
            ScheduledProviderV1::Codex,
            now,
            now.saturating_add(LEASE_SECONDS),
        )
        .map_err(|_| "CODEX_QUEUE: scheduler state is unavailable or unsafe".to_string())?;
    let lease = match decision {
        GlobalScheduleDecisionV1::Granted(lease) => lease,
        GlobalScheduleDecisionV1::Rejected { reason } => {
            return Err(format!("CODEX_QUEUE_CAPACITY: {reason}"));
        }
    };
    let mut job = ready
        .remove(&lease.project_id)
        .ok_or("CODEX_QUEUE: scheduler selected a project without a ready job")?;
    if lease.project_id != job.binding.project_id
        || lease.workspace != job.binding.workspace
        || lease.owner_id != owner
        || lease.provider != ScheduledProviderV1::Codex
        || !registered_project(control_workspace, &job.binding.scope())
            .as_ref()
            .is_ok_and(|project| job.binding.matches(project))
    {
        scheduler.release_workspace_lease(&lease).map_err(|_| {
            "CODEX_QUEUE: drifted scheduler grant could not be released".to_string()
        })?;
        job.state = QueueState::Attention;
        queue.save_job(&job)?;
        return Err("CODEX_QUEUE_SCOPE: selected registry binding changed during claim".into());
    }
    job.state = QueueState::Claimed;
    job.receipt_id = None;
    job.attempt = Some(QueueAttempt {
        attempt_id: Uuid::new_v4().to_string(),
        lease: lease.clone(),
        started_at_unix: now,
    });
    if let Err(error) = queue.save_job(&job) {
        scheduler.release_workspace_lease(&lease).map_err(|_| {
            "CODEX_QUEUE: claim failed and scheduler lease could not be released".to_string()
        })?;
        return Err(error);
    }
    Ok(Some(job))
}

#[derive(Debug)]
struct TurnResult {
    outcome: &'static str,
    exit_code: Option<i32>,
    child_exit_observed: bool,
    child_absent_proven: bool,
    marker_retired: bool,
    native_executable: Option<PathBuf>,
    native_version: Option<String>,
    native_executable_sha256: Option<String>,
}

impl TurnResult {
    fn before_spawn(outcome: &'static str) -> Self {
        Self {
            outcome,
            exit_code: None,
            child_exit_observed: false,
            child_absent_proven: true,
            marker_retired: true,
            native_executable: None,
            native_version: None,
            native_executable_sha256: None,
        }
    }
    fn unknown(outcome: &'static str) -> Self {
        Self {
            outcome,
            exit_code: None,
            child_exit_observed: false,
            child_absent_proven: false,
            marker_retired: false,
            native_executable: None,
            native_version: None,
            native_executable_sha256: None,
        }
    }
    fn with_cli(mut self, cli: &NativeCliPreflight) -> Self {
        self.native_executable = Some(cli.executable.clone());
        self.native_version = Some(cli.version.clone());
        self.native_executable_sha256 = Some(cli.sha256.clone());
        self
    }
}

struct NativeCliPreflight {
    executable: PathBuf,
    codex_home: Option<PathBuf>,
    version: String,
    sha256: String,
}

fn native_executable_sha256(path: &Path) -> Result<String, String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|_| "CODEX_QUEUE_CLI: native executable is unavailable".to_string())?;
    if !metadata.is_file() || redirected(&metadata) || metadata.len() > MAX_NATIVE_EXE_BYTES {
        return Err("CODEX_QUEUE_CLI: native executable is redirected or oversized".into());
    }
    let mut options = OpenOptions::new();
    options.read(true);
    nofollow(&mut options);
    let mut file = options
        .open(path)
        .map_err(|_| "CODEX_QUEUE_CLI: native executable cannot be opened".to_string())?;
    let opened = file
        .metadata()
        .map_err(|_| "CODEX_QUEUE_CLI: native executable cannot be inspected")?;
    let observed = fs::symlink_metadata(path)
        .map_err(|_| "CODEX_QUEUE_CLI: native executable changed".to_string())?;
    if !opened.is_file()
        || redirected(&opened)
        || opened.len() > MAX_NATIVE_EXE_BYTES
        || !same_open_file_identity(&file, path, &opened, &observed)
    {
        return Err("CODEX_QUEUE_CLI: native executable changed".into());
    }
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut total = 0_u64;
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|_| "CODEX_QUEUE_CLI: native executable cannot be hashed".to_string())?;
        if count == 0 {
            break;
        }
        total = total.saturating_add(count as u64);
        if total > MAX_NATIVE_EXE_BYTES {
            return Err("CODEX_QUEUE_CLI: native executable is oversized".into());
        }
        digest.update(&buffer[..count]);
    }
    let observed_after = fs::symlink_metadata(path)
        .map_err(|_| "CODEX_QUEUE_CLI: native executable changed".to_string())?;
    if !same_open_file_identity(&file, path, &opened, &observed_after) {
        return Err("CODEX_QUEUE_CLI: native executable changed".into());
    }
    Ok(format!("{:x}", digest.finalize()))
}

async fn preflight_native_cli(workspace: &Path) -> Result<NativeCliPreflight, String> {
    let (selected, codex_home) = resolve_native_cli()?;
    preflight_selected_cli(selected, codex_home, workspace).await
}

async fn preflight_selected_cli(
    selected: PathBuf,
    codex_home: Option<PathBuf>,
    workspace: &Path,
) -> Result<NativeCliPreflight, String> {
    let selected_metadata = fs::symlink_metadata(&selected)
        .map_err(|_| "CODEX_QUEUE_CLI: selected executable is unavailable".to_string())?;
    if !selected_metadata.is_file() || redirected(&selected_metadata) {
        return Err("CODEX_QUEUE_CLI: selected executable is redirected or unsafe".into());
    }
    // Pin an absolute executable before switching the child's current_dir to
    // a different registered project. A relative override would otherwise
    // resolve against that project's files at spawn time.
    let executable = selected
        .canonicalize()
        .map_err(|_| "CODEX_QUEUE_CLI: selected executable cannot be resolved".to_string())?;
    let sha256 = native_executable_sha256(&executable)?;
    let (version_ok, version) = bounded_probe(
        codex_command(&executable, workspace, codex_home.as_deref()),
        &["--version"],
    )
    .await?;
    if !version_ok || !version.starts_with("codex-cli ") {
        return Err("CODEX_QUEUE_CLI: executable did not report Codex CLI version".into());
    }
    let (authenticated, _) = bounded_probe(
        codex_command(&executable, workspace, codex_home.as_deref()),
        &["login", "status"],
    )
    .await?;
    if !authenticated {
        return Err(
            "CODEX_QUEUE_AUTH: native Codex login is unavailable; queued task remains ready".into(),
        );
    }
    if native_executable_sha256(&executable)? != sha256 {
        return Err("CODEX_QUEUE_CLI: native executable changed during preflight".into());
    }
    Ok(NativeCliPreflight {
        executable,
        codex_home,
        version,
        sha256,
    })
}

fn turn_timeout() -> Result<Duration, String> {
    match std::env::var("CATDESK_CODEX_QUEUE_TURN_TIMEOUT_SECS") {
        Ok(value) => {
            let seconds: u64 = value
                .parse()
                .map_err(|_| "CODEX_QUEUE_TIMEOUT: invalid timeout")?;
            if !(60..=43_200).contains(&seconds) {
                return Err("CODEX_QUEUE_TIMEOUT: timeout must be 60 to 43200 seconds".into());
            }
            Ok(Duration::from_secs(seconds))
        }
        Err(std::env::VarError::NotPresent) => Ok(DEFAULT_TURN_TIMEOUT),
        Err(_) => Err("CODEX_QUEUE_TIMEOUT: invalid timeout".into()),
    }
}

async fn run_queued_turn(
    job: &QueueJob,
    control_workspace: &Path,
    cli: &NativeCliPreflight,
    turn_limit: Duration,
) -> TurnResult {
    let scope = job.binding.scope();
    let project = match registered_project(control_workspace, &scope) {
        Ok(project) if job.binding.matches(&project) => project,
        _ => return TurnResult::before_spawn("SCOPE_DRIFT_BEFORE_SPAWN"),
    };
    let executable = &cli.executable;
    let codex_home = cli.codex_home.as_deref();
    if native_executable_sha256(executable).ok().as_deref() != Some(&cli.sha256) {
        return TurnResult::before_spawn("NATIVE_BINARY_DRIFT_BEFORE_SPAWN");
    }
    let mut writer = match acquire_project_writer_lock(&project.workspace) {
        Ok(writer) => writer,
        Err(_) => return TurnResult::before_spawn("PROJECT_WRITER_UNAVAILABLE"),
    };
    let observed = registered_project(control_workspace, &scope);
    if !observed
        .as_ref()
        .is_ok_and(|selected| job.binding.matches(selected))
        || ensure_same_project_binding(&project, observed.as_ref().unwrap_or(&project)).is_err()
    {
        return TurnResult::before_spawn("SCOPE_DRIFT_BEFORE_SPAWN");
    }
    if ensure_no_interrupted_managed_writer(&project).is_err() {
        return TurnResult::before_spawn("MANAGED_WRITER_UNRESOLVED");
    }
    let action = OperatorCodexAction::Run {
        scope,
        task: job.prompt.clone(),
        model: job.model.clone(),
        effort: job.effort.clone(),
    };
    let args = match invocation_args(&action, &project) {
        Ok(args) => args,
        Err(_) => return TurnResult::before_spawn("NATIVE_INVOCATION_INVALID"),
    };
    let mut command: Command = codex_command(executable, &project.workspace, codex_home);
    command
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .kill_on_drop(true);
    if writer.begin_writer_turn().is_err() {
        return TurnResult {
            marker_retired: false,
            native_executable: Some(executable.clone()),
            ..TurnResult::before_spawn("WRITER_MARKER_UNAVAILABLE")
        };
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(_) => {
            let retired = writer.finish_writer_turn().is_ok();
            return TurnResult {
                marker_retired: retired,
                native_executable: Some(executable.clone()),
                ..TurnResult::before_spawn("NATIVE_SPAWN_FAILED")
            };
        }
    };
    let prompt_accepted = match child.stdin.take() {
        Some(mut stdin) => tokio::time::timeout(PROMPT_TIMEOUT, async {
            stdin.write_all(job.prompt.as_bytes()).await?;
            stdin.shutdown().await
        })
        .await
        .is_ok_and(|result| result.is_ok()),
        None => false,
    };
    if !prompt_accepted {
        let _ = tokio::time::timeout(KILL_TIMEOUT, child.kill()).await;
        // The native parent may have descendants. Keep the pending writer
        // marker and scheduler lease until an explicit proof-based recovery.
        return TurnResult::unknown("PROMPT_PIPE_DESCENDANTS_UNPROVEN");
    }
    let status = match tokio::time::timeout(turn_limit, child.wait()).await {
        Ok(Ok(status)) => status,
        Ok(Err(_)) => {
            let _ = tokio::time::timeout(KILL_TIMEOUT, child.kill()).await;
            return TurnResult::unknown("NATIVE_EXIT_UNOBSERVED");
        }
        Err(_) => {
            let _ = tokio::time::timeout(KILL_TIMEOUT, child.kill()).await;
            return TurnResult::unknown("NATIVE_TIMEOUT_DESCENDANTS_UNPROVEN");
        }
    };
    let retired = writer.finish_writer_turn().is_ok();
    TurnResult {
        outcome: if status.success() {
            "NATIVE_EXIT_ZERO"
        } else {
            "NATIVE_EXIT_NONZERO"
        },
        exit_code: status.code(),
        child_exit_observed: true,
        child_absent_proven: false,
        marker_retired: retired,
        native_executable: Some(executable.clone()),
        native_version: None,
        native_executable_sha256: None,
    }
}

fn complete_attempt(
    control_workspace: &Path,
    queue: &QueueStore,
    claimed: &QueueJob,
    turn: TurnResult,
) -> Result<Value, String> {
    let _lock = queue.lock()?;
    let mut job = queue
        .load_job(&claimed.binding.project_id, &claimed.task_id)?
        .ok_or("CODEX_QUEUE: claimed job disappeared before receipt")?;
    if job.state != QueueState::Claimed || job.attempt != claimed.attempt {
        return Err("CODEX_QUEUE: claimed job ownership changed before receipt".into());
    }
    let attempt = job
        .attempt
        .as_ref()
        .ok_or("CODEX_QUEUE: claimed attempt is absent")?;
    let binding_still_matches = registered_project(control_workspace, &job.binding.scope())
        .as_ref()
        .is_ok_and(|project| job.binding.matches(project));
    // The attempt ID is printed at CLAIMED publication. A crash between
    // receipt publication and job update therefore leaves a discoverable
    // receipt at the same ID, without replaying the native turn.
    let receipt_id = attempt.attempt_id.clone();
    let receipt = QueueReceipt {
        schema_version: QUEUE_SCHEMA_VERSION,
        receipt_id: receipt_id.clone(),
        attempt_id: attempt.attempt_id.clone(),
        project_id: job.binding.project_id.clone(),
        task_id: job.task_id.clone(),
        workspace: job.binding.workspace.clone(),
        thread_id: job.binding.thread_id.clone(),
        target_sha256: job.binding.target_sha256.clone(),
        prompt_sha256: job.prompt_sha256.clone(),
        requested_model: job.model.clone(),
        requested_effort: job.effort.clone(),
        native_executable: turn.native_executable.clone(),
        native_version: turn.native_version.clone(),
        native_executable_sha256: turn.native_executable_sha256.clone(),
        evidence_level: if turn.child_exit_observed {
            "NATIVE_EXIT_OBSERVED"
        } else if turn.child_absent_proven {
            "CHILD_ABSENT_BEFORE_SPAWN"
        } else {
            "PROCESS_STATE_UNKNOWN"
        }
        .into(),
        exit_code: turn.exit_code,
        child_exit_observed: turn.child_exit_observed,
        binding_still_matches,
        outcome: turn.outcome.into(),
        recorded_at_unix: unix_now()?,
    };
    queue.write_receipt(&receipt)?;
    job.receipt_id = Some(receipt_id);
    let final_state = if !turn.child_exit_observed && !turn.child_absent_proven {
        QueueState::Unknown
    } else if !binding_still_matches || !turn.marker_retired || turn.outcome.contains("DRIFT") {
        QueueState::Attention
    } else if turn.outcome == "NATIVE_EXIT_ZERO" {
        QueueState::NativeExitedZero
    } else if turn.child_absent_proven {
        QueueState::Ready
    } else {
        QueueState::Failed
    };
    let release_safe = turn.child_exit_observed || turn.child_absent_proven;
    // Stage attention while the lease is still held. A failed release or a
    // crash between receipt and release cannot masquerade as a completed job.
    job.state = if release_safe {
        QueueState::Attention
    } else {
        QueueState::Unknown
    };
    queue.save_job(&job)?;
    let lease_released = if release_safe {
        scheduler(control_workspace)?
            .release_workspace_lease(&attempt.lease)
            .map_err(|_| {
                "CODEX_QUEUE: observed attempt receipt saved; exact lease release failed and task needs attention"
                    .to_string()
            })?;
        job.state = final_state;
        queue.save_job(&job)?;
        true
    } else {
        false
    };
    Ok(json!({
        "action":"codex_queue_dispatch", "job":job.summary(), "receipt":receipt,
        "leaseReleased":lease_released,
        "authority":"same_user_operator_coordination",
    }))
}

async fn dispatch_one(control: &AutonomousProjectV1) -> Result<Value, String> {
    let control_workspace = &control.workspace;
    let queue = QueueStore::open_for_operator(control, control_workspace)?;
    if !queue
        .jobs()?
        .iter()
        .any(|job| job.state == QueueState::Ready)
    {
        return Ok(json!({ "action":"codex_queue_dispatch", "status":"no_ready_tasks" }));
    }
    let turn_limit = turn_timeout()?;
    let cli = preflight_native_cli(control_workspace).await?;
    let Some(job) = claim_one(control_workspace, &queue)? else {
        return Ok(json!({ "action":"codex_queue_dispatch", "status":"no_ready_tasks" }));
    };
    let turn = run_queued_turn(&job, control_workspace, &cli, turn_limit)
        .await
        .with_cli(&cli);
    complete_attempt(control_workspace, &queue, &job, turn)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::delegated::autonomy_projects::project_chat_target_digest;
    use std::process::Command as StdCommand;

    fn parse(items: &[&str]) -> Result<QueueAction, String> {
        parse_action(
            &items
                .iter()
                .map(|item| (*item).to_string())
                .collect::<Vec<_>>(),
        )
    }

    fn fixture_root() -> PathBuf {
        let root = std::env::temp_dir().join(format!("catdesk-operator-queue-{}", Uuid::new_v4()));
        fs::create_dir(&root).expect("fixture root");
        root
    }

    fn cleanup(root: PathBuf) {
        let root = root.canonicalize().expect("fixture root canonical");
        let temp = std::env::temp_dir().canonicalize().expect("temp canonical");
        assert!(root != temp && root.starts_with(&temp));
        fs::remove_dir_all(root).expect("fixture cleanup");
    }

    fn fixture_git(root: &Path, origin: &str) {
        fs::create_dir(root).expect("Git root");
        for args in [vec!["init", "-q"], vec!["remote", "add", "origin", origin]] {
            assert!(
                StdCommand::new("git")
                    .arg("-C")
                    .arg(root)
                    .args(args)
                    .status()
                    .expect("Git available")
                    .success()
            );
        }
    }

    fn fake_native_cli(root: &Path) -> PathBuf {
        let source = root.join("fake-codex.rs");
        let executable = root.join(if cfg!(windows) {
            "fake-codex.exe"
        } else {
            "fake-codex"
        });
        fs::write(&source, r#"
            use std::io::Read;
            fn main() {
                let args = std::env::args().skip(1).collect::<Vec<_>>();
                if args.iter().any(|arg| arg == "--version") { println!("codex-cli fake-fixture"); return; }
                if args.windows(2).any(|pair| pair == ["login", "status"]) { println!("logged in"); return; }
                let mut prompt = String::new();
                std::io::stdin().read_to_string(&mut prompt).expect("prompt");
                let thread = args.get(args.len().saturating_sub(2)).expect("thread");
                std::fs::write(format!("queue-proof-{thread}.txt"), prompt).expect("proof");
                println!("fixture native turn complete");
            }
        "#).unwrap();
        assert!(
            StdCommand::new("rustc")
                .arg(&source)
                .arg("-o")
                .arg(&executable)
                .status()
                .expect("rustc available")
                .success()
        );
        executable
    }

    fn fixture_project(id: &str, root: &Path) -> AutonomousProjectV1 {
        let target = format!("https://chatgpt.com/c/{}-fixture", id);
        AutonomousProjectV1 {
            project_id: id.into(),
            workspace: root.canonicalize().expect("project canonical"),
            git_identity: format!("https://example.test/{id}.git"),
            verification_profile: "rust_full".into(),
            codex_thread_id: Some(Uuid::new_v4().to_string()),
            chatgpt_target_sha256: Some(project_chat_target_digest(&target)),
            chatgpt_target_url: Some(target),
        }
    }

    fn three_project_fixture(
        max_codex_workers: u32,
    ) -> (PathBuf, Vec<AutonomousProjectV1>, QueueStore) {
        let fixture = fixture_root();
        let control = fixture.join("catdesk");
        let alpha = fixture.join("alpha");
        let beta = fixture.join("beta");
        fixture_git(&control, "https://example.test/catdesk.git");
        fixture_git(&alpha, "https://example.test/alpha.git");
        fixture_git(&beta, "https://example.test/beta.git");
        let registry = AutonomousProjectRegistryStoreV1::open(control.join(".catdesk/projects"))
            .expect("registry store");
        registry
            .initialize(max_codex_workers, 1)
            .expect("registry initialized");
        let projects = [(&"catdesk", &control), (&"alpha", &alpha), (&"beta", &beta)]
            .into_iter()
            .map(|(id, root)| fixture_project(id, root))
            .collect::<Vec<_>>();
        for project in &projects {
            registry
                .register_project(project.clone())
                .expect("registered");
        }
        let queue = QueueStore::open_at(fixture.join("user-state/OperatorCodexQueue"))
            .or_else(|_| {
                fs::create_dir(fixture.join("user-state")).expect("user state parent");
                QueueStore::open_at(fixture.join("user-state/OperatorCodexQueue"))
            })
            .expect("queue");
        (fixture, projects, queue)
    }

    #[test]
    fn parser_rejects_scope_and_profile_bypasses() {
        assert!(matches!(
            parse(&["operator", "codex", "queue", "status"]),
            Ok(QueueAction::Status)
        ));
        assert!(matches!(
            parse(&["operator", "codex", "queue", "dispatch"]),
            Ok(QueueAction::Dispatch)
        ));
        let receipt_id = Uuid::new_v4().to_string();
        assert!(matches!(
            parse(&[
                "operator",
                "codex",
                "queue",
                "receipt",
                "--receipt-id",
                &receipt_id
            ]),
            Ok(QueueAction::Receipt { .. })
        ));
        assert!(
            parse(&[
                "operator",
                "codex",
                "queue",
                "receipt",
                "--receipt-id",
                "../../other"
            ])
            .is_err()
        );
        let add = [
            "operator",
            "codex",
            "queue",
            "add",
            "--project-id",
            "alpha",
            "--workspace",
            "C:/alpha",
            "--task-id",
            "safe_one",
            "--task",
            "edit a test",
        ];
        assert!(matches!(parse(&add), Ok(QueueAction::Add { .. })));
        assert!(parse(&[&add[..], &["--sandbox", "danger-full-access"]].concat()).is_err());
        assert!(parse(&[&add[..], &["--task", "second"]].concat()).is_err());
        assert!(parse(&[&add[..], &["--model", "gpt;evil"]].concat()).is_err());
        assert!(
            parse(&[
                "operator",
                "codex",
                "queue",
                "add",
                "--project-id",
                "alpha",
                "--workspace",
                "relative",
                "--task-id",
                "safe",
                "--task",
                "edit"
            ])
            .is_err()
        );
        assert!(
            parse(&[
                "operator",
                "codex",
                "queue",
                "resume",
                "--project-id",
                "alpha",
                "--workspace",
                "C:/alpha",
                "--task-id",
                "safe",
                "--task",
                "replay"
            ])
            .is_err()
        );
    }

    #[test]
    fn records_are_idempotent_and_status_omits_prompt() {
        let (fixture, projects, queue) = three_project_fixture(1);
        let job = QueueJob::new(
            "task_one".into(),
            &projects[1],
            "private prompt text".into(),
            Some("gpt-6.1-sol".into()),
            Some("medium".into()),
        )
        .expect("job");
        queue.save_job(&job).expect("persist job");
        let same = QueueJob::new(
            "task_one".into(),
            &projects[1],
            "private prompt text".into(),
            Some("gpt-6.1-sol".into()),
            Some("medium".into()),
        )
        .expect("same job");
        assert!(
            queue
                .load_job("alpha", "task_one")
                .expect("load")
                .unwrap()
                .same_request(&same)
        );
        let changed = QueueJob::new(
            "task_one".into(),
            &projects[1],
            "different prompt".into(),
            None,
            None,
        )
        .expect("changed");
        assert!(!job.same_request(&changed));
        let summary = job.summary().to_string();
        assert!(!summary.contains("private prompt text"));
        let stored = fs::read_to_string(queue.job_path("alpha", "task_one")).unwrap();
        assert!(!stored.contains("https://chatgpt.com/c/"));
        assert_eq!(queue.jobs().expect("enumerate").len(), 1);
        cleanup(fixture);
    }

    #[test]
    fn fair_claim_and_observed_failure_release_exact_lease() {
        let (fixture, projects, queue) = three_project_fixture(1);
        for project in &projects {
            let job = QueueJob::new(
                format!("job_{}", project.project_id),
                project,
                "fixture edit".into(),
                None,
                None,
            )
            .expect("job");
            queue.save_job(&job).expect("persist job");
        }
        let control = &projects[0].workspace;
        let mut selected = Vec::new();
        for iteration in 0..3 {
            let claimed = claim_one(control, &queue).expect("claim").expect("job");
            selected.push(claimed.binding.project_id.clone());
            let result = TurnResult {
                outcome: if iteration == 0 {
                    "NATIVE_EXIT_NONZERO"
                } else {
                    "NATIVE_EXIT_ZERO"
                },
                exit_code: Some(if iteration == 0 { 1 } else { 0 }),
                child_exit_observed: true,
                child_absent_proven: false,
                marker_retired: true,
                native_executable: None,
                native_version: None,
                native_executable_sha256: None,
            };
            let receipt = complete_attempt(control, &queue, &claimed, result).expect("receipt");
            assert_eq!(receipt["leaseReleased"], true);
            assert_eq!(receipt["receipt"]["threadId"], claimed.binding.thread_id);
            assert_eq!(receipt["receipt"]["evidenceLevel"], "NATIVE_EXIT_OBSERVED");
            assert_eq!(
                receipt["receipt"]["receiptId"],
                claimed.attempt.as_ref().unwrap().attempt_id
            );
            let reread = queue
                .read_receipt(&claimed.attempt.as_ref().unwrap().attempt_id)
                .unwrap();
            assert_eq!(reread.thread_id, claimed.binding.thread_id);
        }
        assert_eq!(selected, ["alpha", "beta", "catdesk"]);
        assert_eq!(
            queue
                .load_job("catdesk", "job_catdesk")
                .unwrap()
                .unwrap()
                .state,
            QueueState::NativeExitedZero
        );
        assert_eq!(
            queue.load_job("alpha", "job_alpha").unwrap().unwrap().state,
            QueueState::Failed
        );
        assert_eq!(
            fs::read_dir(control.join(".catdesk/projects/leases"))
                .unwrap()
                .count(),
            0
        );
        assert!(!projects[1].workspace.join(".catdesk").exists());
        assert!(!projects[2].workspace.join(".catdesk").exists());
        cleanup(fixture);
    }

    #[test]
    fn valid_registry_without_scheduler_children_initializes_on_claim() {
        let (fixture, projects, queue) = three_project_fixture(1);
        let control = &projects[0].workspace;
        let registry_root = control.join(".catdesk/projects");
        for child in ["leases", "registration-confirmations", "thread-adoptions"] {
            fs::remove_dir(registry_root.join(child)).expect("remove empty fixture child");
        }
        queue
            .save_job(
                &QueueJob::new(
                    "missing_leases".into(),
                    &projects[1],
                    "fixture task".into(),
                    None,
                    None,
                )
                .unwrap(),
            )
            .unwrap();
        let registered = scheduler(control)
            .unwrap()
            .load_registry()
            .unwrap()
            .projects;
        assert_eq!(registered.len(), projects.len());
        for project in &projects {
            assert!(registered.contains(project));
        }
        assert!(!registry_root.join("leases").exists());
        assert!(!registry_root.join("scheduler.lock").exists());

        let claimed = claim_one(control, &queue)
            .expect("claim")
            .expect("ready job");
        assert_eq!(claimed.binding.project_id, "alpha");
        assert_eq!(claimed.state, QueueState::Claimed);
        assert!(registry_root.join("leases").is_dir());
        assert!(registry_root.join("scheduler.lock").is_file());
        scheduler(control)
            .unwrap()
            .release_workspace_lease(&claimed.attempt.unwrap().lease)
            .expect("release exact fixture lease");
        cleanup(fixture);
    }

    #[test]
    fn corrupt_scheduler_child_refuses_claim_without_consuming_job() {
        let (fixture, projects, queue) = three_project_fixture(1);
        let control = &projects[0].workspace;
        let registry_root = control.join(".catdesk/projects");
        fs::remove_dir(registry_root.join("leases")).expect("remove empty fixture leases");
        fs::write(registry_root.join("leases"), b"not a directory")
            .expect("corrupt fixture scheduler child");
        queue
            .save_job(
                &QueueJob::new(
                    "corrupt_leases".into(),
                    &projects[1],
                    "fixture task".into(),
                    None,
                    None,
                )
                .unwrap(),
            )
            .unwrap();

        assert!(
            claim_one(control, &queue)
                .expect_err("unsafe scheduler child")
                .contains("scheduler state is unavailable or unsafe")
        );
        let job = queue.load_job("alpha", "corrupt_leases").unwrap().unwrap();
        assert_eq!(job.state, QueueState::Ready);
        assert!(job.attempt.is_none());
        assert!(!registry_root.join("scheduler.lock").exists());
        cleanup(fixture);
    }

    #[test]
    fn origin_drift_never_reroutes_ready_job() {
        let (fixture, projects, queue) = three_project_fixture(1);
        for project in projects.iter().skip(1) {
            queue
                .save_job(
                    &QueueJob::new(
                        format!("job_{}", project.project_id),
                        project,
                        "fixture edit".into(),
                        None,
                        None,
                    )
                    .unwrap(),
                )
                .unwrap();
        }
        assert!(
            StdCommand::new("git")
                .arg("-C")
                .arg(&projects[1].workspace)
                .args([
                    "remote",
                    "set-url",
                    "origin",
                    "https://example.test/changed.git"
                ])
                .status()
                .expect("Git drift")
                .success()
        );
        let claimed = claim_one(&projects[0].workspace, &queue)
            .expect("claim")
            .expect("other project");
        assert_eq!(claimed.binding.project_id, "beta");
        assert_eq!(
            queue.load_job("alpha", "job_alpha").unwrap().unwrap().state,
            QueueState::Attention
        );
        assert!(!projects[1].workspace.join(".catdesk").exists());
        // The fixture owns exactly this lease; no child was launched.
        scheduler(&projects[0].workspace)
            .unwrap()
            .release_workspace_lease(&claimed.attempt.unwrap().lease)
            .unwrap();
        cleanup(fixture);
    }

    #[test]
    fn claimed_attempt_remains_durable_and_is_never_reselected() {
        let (fixture, projects, queue) = three_project_fixture(2);
        for project in projects.iter().skip(1) {
            queue
                .save_job(
                    &QueueJob::new(
                        format!("job_{}", project.project_id),
                        project,
                        "fixture edit".into(),
                        None,
                        None,
                    )
                    .unwrap(),
                )
                .unwrap();
        }
        let first = claim_one(&projects[0].workspace, &queue).unwrap().unwrap();
        assert_eq!(first.binding.project_id, "alpha");
        let second = claim_one(&projects[0].workspace, &queue).unwrap().unwrap();
        assert_eq!(second.binding.project_id, "beta");
        assert_eq!(
            queue.load_job("alpha", "job_alpha").unwrap().unwrap().state,
            QueueState::Claimed
        );
        assert!(claim_one(&projects[0].workspace, &queue).unwrap().is_none());
        for claimed in [first, second] {
            scheduler(&projects[0].workspace)
                .unwrap()
                .release_workspace_lease(&claimed.attempt.unwrap().lease)
                .unwrap();
        }
        cleanup(fixture);
    }

    #[test]
    fn prelaunch_writer_refusal_keeps_task_ready_and_releases_lease() {
        let (fixture, projects, queue) = three_project_fixture(1);
        for project in projects.iter().skip(1) {
            queue
                .save_job(
                    &QueueJob::new(
                        format!("job_{}", project.project_id),
                        project,
                        "fixture edit".into(),
                        None,
                        None,
                    )
                    .unwrap(),
                )
                .unwrap();
        }
        let first = claim_one(&projects[0].workspace, &queue).unwrap().unwrap();
        assert_eq!(first.binding.project_id, "alpha");
        let outcome = complete_attempt(
            &projects[0].workspace,
            &queue,
            &first,
            TurnResult::before_spawn("PROJECT_WRITER_UNAVAILABLE"),
        )
        .unwrap();
        assert_eq!(outcome["leaseReleased"], true);
        assert_eq!(outcome["job"]["state"], "READY");
        assert_eq!(outcome["receipt"]["childExitObserved"], false);
        assert_eq!(
            outcome["receipt"]["evidenceLevel"],
            "CHILD_ABSENT_BEFORE_SPAWN"
        );
        assert_eq!(
            queue
                .read_receipt(&first.attempt.as_ref().unwrap().attempt_id)
                .unwrap()
                .outcome,
            "PROJECT_WRITER_UNAVAILABLE"
        );
        let next = claim_one(&projects[0].workspace, &queue).unwrap().unwrap();
        assert_eq!(next.binding.project_id, "beta");
        scheduler(&projects[0].workspace)
            .unwrap()
            .release_workspace_lease(&next.attempt.unwrap().lease)
            .unwrap();
        cleanup(fixture);
    }

    #[test]
    fn unobserved_child_blocks_replay_and_retains_exact_lease() {
        let (fixture, projects, queue) = three_project_fixture(1);
        for project in projects.iter().skip(1) {
            queue
                .save_job(
                    &QueueJob::new(
                        format!("job_{}", project.project_id),
                        project,
                        "fixture edit".into(),
                        None,
                        None,
                    )
                    .unwrap(),
                )
                .unwrap();
        }
        let first = claim_one(&projects[0].workspace, &queue).unwrap().unwrap();
        let outcome = complete_attempt(
            &projects[0].workspace,
            &queue,
            &first,
            TurnResult::unknown("NATIVE_EXIT_UNOBSERVED"),
        )
        .unwrap();
        assert_eq!(outcome["leaseReleased"], false);
        assert_eq!(outcome["job"]["state"], "UNKNOWN");
        assert_eq!(outcome["receipt"]["evidenceLevel"], "PROCESS_STATE_UNKNOWN");
        assert_eq!(
            queue.load_job("alpha", "job_alpha").unwrap().unwrap().state,
            QueueState::Unknown
        );
        let error = claim_one(&projects[0].workspace, &queue).expect_err("provider capacity held");
        assert!(error.contains("CAPACITY"), "{error}");
        scheduler(&projects[0].workspace)
            .unwrap()
            .release_workspace_lease(&first.attempt.unwrap().lease)
            .unwrap();
        cleanup(fixture);
    }

    #[test]
    fn unsafe_or_oversized_queue_entries_fail_closed() {
        let fixture = fixture_root();
        let parent = fixture.join("user-state");
        fs::create_dir(&parent).unwrap();
        let workspace = fixture.join("project");
        fs::create_dir(&workspace).unwrap();
        assert!(reject_overlap(&workspace.join("CatDesk"), &workspace).is_err());
        assert!(!workspace.join("CatDesk").exists());
        let queue = QueueStore::open_at(parent.join("OperatorCodexQueue")).unwrap();
        fs::write(
            queue.root.join("jobs/invalid.json"),
            vec![b'x'; MAX_JOB_BYTES as usize + 1],
        )
        .unwrap();
        assert!(queue.jobs().is_err());
        fs::remove_file(queue.root.join("jobs/invalid.json")).unwrap();
        for index in 0..=MAX_DIR_ENTRIES {
            fs::write(
                queue.root.join("jobs").join(format!(".draft-{index}.tmp")),
                b"",
            )
            .unwrap();
        }
        assert!(queue.jobs().is_err());
        cleanup(fixture);
    }

    #[test]
    fn separate_verified_controls_use_distinct_queue_namespaces() {
        let (fixture_a, projects_a, _) = three_project_fixture(1);
        let (fixture_b, projects_b, _) = three_project_fixture(1);
        let path_a = QueueStore::namespaced_path(&projects_a[0], &projects_a[0].workspace).unwrap();
        let path_b = QueueStore::namespaced_path(&projects_b[0], &projects_b[0].workspace).unwrap();
        assert_ne!(path_a, path_b);
        assert!(
            QueueStore::open_read_only(&projects_a[0])
                .unwrap()
                .is_none()
        );
        assert!(!path_a.exists());
        let root = fixture_root();
        let parent = root.join("state");
        fs::create_dir(&parent).unwrap();
        let queue_a = QueueStore::open_at(parent.join(path_a.file_name().unwrap())).unwrap();
        let queue_b = QueueStore::open_at(parent.join(path_b.file_name().unwrap())).unwrap();
        queue_a
            .save_job(
                &QueueJob::new(
                    "only_a".into(),
                    &projects_a[1],
                    "fixture".into(),
                    None,
                    None,
                )
                .unwrap(),
            )
            .unwrap();
        assert_eq!(queue_a.jobs().unwrap().len(), 1);
        assert!(queue_b.jobs().unwrap().is_empty());
        assert!(
            claim_one(&projects_b[0].workspace, &queue_b)
                .unwrap()
                .is_none()
        );
        assert_eq!(queue_a.jobs().unwrap().len(), 1);
        cleanup(root);
        cleanup(fixture_a);
        cleanup(fixture_b);
    }

    #[test]
    fn attempt_id_finds_receipt_after_publication_before_job_update() {
        let (fixture, projects, queue) = three_project_fixture(1);
        queue
            .save_job(
                &QueueJob::new(
                    "crash_window".into(),
                    &projects[1],
                    "fixture".into(),
                    None,
                    None,
                )
                .unwrap(),
            )
            .unwrap();
        let claimed = claim_one(&projects[0].workspace, &queue).unwrap().unwrap();
        let attempt = claimed.attempt.as_ref().unwrap();
        let receipt = QueueReceipt {
            schema_version: QUEUE_SCHEMA_VERSION,
            receipt_id: attempt.attempt_id.clone(),
            attempt_id: attempt.attempt_id.clone(),
            project_id: claimed.binding.project_id.clone(),
            task_id: claimed.task_id.clone(),
            workspace: claimed.binding.workspace.clone(),
            thread_id: claimed.binding.thread_id.clone(),
            target_sha256: claimed.binding.target_sha256.clone(),
            prompt_sha256: claimed.prompt_sha256.clone(),
            requested_model: None,
            requested_effort: None,
            native_executable: None,
            native_version: None,
            native_executable_sha256: None,
            evidence_level: "NATIVE_EXIT_OBSERVED".into(),
            exit_code: Some(0),
            child_exit_observed: true,
            binding_still_matches: true,
            outcome: "NATIVE_EXIT_ZERO".into(),
            recorded_at_unix: unix_now().unwrap(),
        };
        queue.write_receipt(&receipt).unwrap();
        assert_eq!(
            queue
                .load_job("alpha", "crash_window")
                .unwrap()
                .unwrap()
                .state,
            QueueState::Claimed
        );
        assert_eq!(queue.read_receipt(&attempt.attempt_id).unwrap(), receipt);
        scheduler(&projects[0].workspace)
            .unwrap()
            .release_workspace_lease(&attempt.lease)
            .unwrap();
        cleanup(fixture);
    }

    #[test]
    fn exact_lease_release_failure_leaves_attention_and_readable_receipt() {
        let (fixture, projects, queue) = three_project_fixture(1);
        queue
            .save_job(
                &QueueJob::new(
                    "release_fault".into(),
                    &projects[1],
                    "fixture".into(),
                    None,
                    None,
                )
                .unwrap(),
            )
            .unwrap();
        let claimed = claim_one(&projects[0].workspace, &queue).unwrap().unwrap();
        let attempt = claimed.attempt.as_ref().unwrap();
        assert!(attempt.lease.expires_at_unix - attempt.started_at_unix >= 12 * 60 * 60);
        // Inject an exact lease disappearance before completion; the receipt
        // remains discoverable, and no job is reported as completed.
        scheduler(&projects[0].workspace)
            .unwrap()
            .release_workspace_lease(&attempt.lease)
            .unwrap();
        let error = complete_attempt(
            &projects[0].workspace,
            &queue,
            &claimed,
            TurnResult {
                outcome: "NATIVE_EXIT_ZERO",
                exit_code: Some(0),
                child_exit_observed: true,
                child_absent_proven: false,
                marker_retired: true,
                native_executable: None,
                native_version: None,
                native_executable_sha256: None,
            },
        )
        .expect_err("exact release missing");
        assert!(error.contains("needs attention"), "{error}");
        assert_eq!(
            queue
                .load_job("alpha", "release_fault")
                .unwrap()
                .unwrap()
                .state,
            QueueState::Attention
        );
        assert_eq!(
            queue
                .read_receipt(&attempt.attempt_id)
                .unwrap()
                .evidence_level,
            "NATIVE_EXIT_OBSERVED"
        );
        cleanup(fixture);
    }

    #[test]
    fn oversized_receipt_is_rejected_before_publication() {
        let fixture = fixture_root();
        let parent = fixture.join("state");
        fs::create_dir(&parent).unwrap();
        let queue = QueueStore::open_at(parent.join("queue")).unwrap();
        let receipt_id = Uuid::new_v4().to_string();
        let receipt = QueueReceipt {
            schema_version: QUEUE_SCHEMA_VERSION,
            receipt_id: receipt_id.clone(),
            attempt_id: receipt_id.clone(),
            project_id: "fixture".into(),
            task_id: "large".into(),
            workspace: PathBuf::from("x".repeat(MAX_RECEIPT_BYTES as usize)),
            thread_id: Uuid::new_v4().to_string(),
            target_sha256: None,
            prompt_sha256: "0".repeat(64),
            requested_model: None,
            requested_effort: None,
            native_executable: None,
            native_version: None,
            native_executable_sha256: None,
            evidence_level: "CHILD_ABSENT_BEFORE_SPAWN".into(),
            exit_code: None,
            child_exit_observed: false,
            binding_still_matches: false,
            outcome: "FIXTURE".into(),
            recorded_at_unix: unix_now().unwrap(),
        };
        assert!(
            queue
                .write_receipt(&receipt)
                .unwrap_err()
                .contains("oversized")
        );
        assert!(
            !queue
                .root
                .join("receipts")
                .join(format!("{receipt_id}.json"))
                .exists()
        );
        cleanup(fixture);
    }

    #[tokio::test]
    async fn fake_native_child_produces_project_scoped_receipts_and_proofs() {
        let (fixture, projects, queue) = three_project_fixture(1);
        let executable = fake_native_cli(&fixture);
        let cli = preflight_selected_cli(executable, None, &projects[0].workspace)
            .await
            .expect("fake direct executable preflight");
        assert_eq!(cli.version, "codex-cli fake-fixture");
        for project in projects.iter().skip(1) {
            queue
                .save_job(
                    &QueueJob::new(
                        format!("job_{}", project.project_id),
                        project,
                        format!("edit only {}", project.project_id),
                        None,
                        None,
                    )
                    .unwrap(),
                )
                .unwrap();
        }
        for expected in ["alpha", "beta"] {
            let claimed = claim_one(&projects[0].workspace, &queue).unwrap().unwrap();
            assert_eq!(claimed.binding.project_id, expected);
            let turn = run_queued_turn(
                &claimed,
                &projects[0].workspace,
                &cli,
                Duration::from_secs(30),
            )
            .await
            .with_cli(&cli);
            assert!(turn.child_exit_observed && turn.marker_retired, "{turn:?}");
            let result = complete_attempt(&projects[0].workspace, &queue, &claimed, turn).unwrap();
            assert_eq!(result["job"]["state"], "NATIVE_EXITED_ZERO");
            assert_eq!(result["receipt"]["nativeVersion"], "codex-cli fake-fixture");
            assert_eq!(result["receipt"]["nativeExecutableSha256"], cli.sha256);
            assert_eq!(result["receipt"]["evidenceLevel"], "NATIVE_EXIT_OBSERVED");
            let project = projects
                .iter()
                .find(|project| project.project_id == expected)
                .unwrap();
            let proof = project
                .workspace
                .join(format!("queue-proof-{}.txt", claimed.binding.thread_id));
            assert_eq!(
                fs::read_to_string(proof).unwrap(),
                format!("edit only {expected}")
            );
            for other in projects
                .iter()
                .filter(|project| project.project_id != expected)
            {
                assert!(
                    !other
                        .workspace
                        .join(format!("queue-proof-{}.txt", claimed.binding.thread_id))
                        .exists()
                );
            }
        }
        cleanup(fixture);
    }
}
