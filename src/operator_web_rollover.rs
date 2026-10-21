//! Durable, one-project Web rollover. Browser proof and paired Wake authority
//! are separate checkpoints; a possibly submitted bootstrap is never replayed.

use crate::{
    delegated::autonomy_projects::{AutonomousProjectRegistryStoreV1, AutonomousProjectV1},
    mcp,
    web_conversation_context::{
        ConversationKey, Observation, PressureStatus, VisibleSourceKind, WebContextStore,
    },
    web_work_state::WorkLedger,
};
use catdesk_wake::{
    protocol::{Target, digest, valid_id},
    runtime,
    store::{Delivery, Phase as DeliveryPhase, ResponseState, Store, TurnTimer, atomic, now, read},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Handoff {
    pub schema_version: u32,
    pub project_id: String,
    pub source_task_id: String,
    pub objective: String,
    pub completed: Vec<String>,
    pub remaining: Vec<String>,
    pub next_action: String,
    pub owned_paths: Vec<String>,
    pub verification: Vec<String>,
    pub blockers: Vec<String>,
}

impl Handoff {
    fn validate(&self, project: &str) -> Result<(), String> {
        if self.schema_version != 1
            || self.project_id != project
            || !valid_id(&self.source_task_id)
            || self.objective.trim().is_empty()
            || self.next_action.trim().is_empty()
            || self.remaining.is_empty()
            || [
                &self.completed,
                &self.remaining,
                &self.owned_paths,
                &self.verification,
                &self.blockers,
            ]
            .iter()
            .any(|items| items.len() > 32 || items.iter().any(|s| s.trim().is_empty()))
        {
            return Err("WEB_HANDOFF_INVALID".into());
        }
        let bytes = serde_json::to_vec(self).map_err(|_| "WEB_HANDOFF_INVALID")?;
        if bytes.len() > 10 * 1024 || bytes.contains(&0) {
            return Err("WEB_HANDOFF_TOO_LARGE".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum Phase {
    Prepared,
    Submitting,
    Quarantined,
    ResponsePending,
    Created,
    Bound,
    ContinuationQueued,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum ContextMeterStatus {
    #[default]
    Pending,
    Recorded,
    RetryRequired,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Record {
    schema_version: u32,
    intent_id: String,
    project_id: String,
    source_task_id: String,
    source: Target,
    phase: Phase,
    #[serde(default)]
    bootstrap_progress: Option<runtime::BootstrapProgress>,
    handoff: String,
    message_digest: String,
    created_utc: u64,
    receipt: Option<runtime::BootstrapReceipt>,
    continuation_event_id: String,
    #[serde(default)]
    bound_target: Option<Target>,
    #[serde(default)]
    reconciliation_reason: Option<String>,
    #[serde(default)]
    context_meter_status: ContextMeterStatus,
    #[serde(default)]
    handoff_event_id: Option<String>,
    #[serde(default)]
    source_fingerprint: Option<String>,
    #[serde(default)]
    source_snapshot_verified: bool,
    #[serde(default)]
    quarantine: Option<Quarantine>,
}

/// Operator-observed incident evidence, never a bootstrap or binding receipt.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Quarantine {
    observed_conversation_url: String,
    recorded_utc: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DraftArchive {
    schema_version: u32,
    project_id: String,
    intent_id: String,
    source: Target,
    message_digest: String,
    expected_text: String,
    actual_text: String,
    observed_bytes: usize,
    recorded_utc: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DraftArchiveReceipt {
    schema_version: u32,
    project_id: String,
    intent_id: String,
    source_digest: String,
    message_digest: String,
    archived_utc: u64,
    evidence: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct HandoffRequest {
    schema_version: u32,
    project_id: String,
    intent_id: String,
    source_task_id: String,
    source: Target,
    native_workspace: PathBuf,
    native_thread_id: Option<String>,
    event_id: String,
    message: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AutoStage {
    Observe,
    CollectHandoff,
    Execute,
    SourceWitnessRequired,
    SubmissionUnknown,
    Quarantined,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AutoStatus {
    schema_version: u32,
    project_id: String,
    tick_utc: u64,
    stage: String,
    target_generation: Option<u64>,
    reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_failure: Option<AutoFailure>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    retry: Option<AutoRetry>,
}

const AUTO_STATUS_SCHEMA_VERSION: u32 = 2;
const AUTO_RETRY_BASE_SECONDS: u64 = 300;
const AUTO_RETRY_MAX_SECONDS: u64 = 1800;
const AUTO_RETRY_MAX_FAILURES: u32 = 16;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AutoRetry {
    consecutive_failures: u32,
    next_attempt_utc: u64,
    target_generation: Option<u64>,
}

fn auto_retry_delay(failures: u32) -> u64 {
    AUTO_RETRY_BASE_SECONDS
        .saturating_mul(1u64 << failures.saturating_sub(1).min(3))
        .min(AUTO_RETRY_MAX_SECONDS)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AutoFailure {
    reason: String,
    tick_utc: u64,
    target_generation: Option<u64>,
}

impl HandoffRequest {
    fn validate(&self, project: &str) -> Result<(), String> {
        if self.schema_version != 1
            || self.project_id != project
            || !valid_id(&self.intent_id)
            || !valid_id(&self.source_task_id)
            || self.event_id != format!("web-handoff-{}", self.intent_id)
            || self.message
                != handoff_request_message(
                    project,
                    &self.intent_id,
                    &self.source_task_id,
                    &self.native_thread_id,
                )
            || self.message.len() > 2048
            || self
                .native_thread_id
                .as_ref()
                .is_some_and(|id| !valid_id(id))
            || !self.native_workspace.is_absolute()
        {
            return Err("WEB_HANDOFF_REQUEST_INVALID".into());
        }
        self.source.validate()?;
        Ok(())
    }
}

struct ProjectStore {
    root: PathBuf,
    _lock: File,
}

/// Hold this project's existing rollover lock across a central work request
/// or resolution. Rollover takes this lock before its target and registry
/// reads, so callers must acquire this lease before those same reads.
pub(crate) struct WorkLease {
    _store: ProjectStore,
    approved_target: Option<Target>,
}

impl WorkLease {
    /// A completed continuation authorizes only its proved generation.
    /// Manual target binding does not take ProjectStore's lock, so the caller
    /// rechecks its selected target against this witness before changing work.
    pub(crate) fn check_target(&self, current: &Target) -> Result<(), String> {
        current.validate()?;
        if self
            .approved_target
            .as_ref()
            .is_some_and(|approved| approved != current)
        {
            return Err("WEB_WORK_ROLLOVER_PENDING".into());
        }
        Ok(())
    }
}

fn continuation_ready_for_work(
    store: &ProjectStore,
    wake: &Store,
    workspace: &Path,
    project: &str,
    current: &Target,
    record: &Record,
) -> Result<(), String> {
    if record.phase != Phase::ContinuationQueued
        || record.context_meter_status == ContextMeterStatus::Pending
        || record.bound_target.as_ref() != Some(current)
        || record.handoff_event_id.is_none()
        || (record.context_meter_status == ContextMeterStatus::RetryRequired
            && (record.reconciliation_reason.is_some()
                || !record.source_snapshot_verified
                || record.source_fingerprint.is_none()))
    {
        return Err("WEB_WORK_ROLLOVER_PENDING".into());
    }
    let request = store
        .handoff_request()
        .map_err(|_| "WEB_WORK_ROLLOVER_PENDING")?
        .ok_or("WEB_WORK_ROLLOVER_PENDING")?;
    let registered =
        registered_project(workspace, project).map_err(|_| "WEB_WORK_ROLLOVER_PENDING")?;
    let delivery = wake
        .delivery(&record.continuation_event_id)
        .map_err(|_| "WEB_WORK_ROLLOVER_PENDING")?
        .ok_or("WEB_WORK_ROLLOVER_PENDING")?;
    let timer = wake
        .timer(&record.continuation_event_id)
        .map_err(|_| "WEB_WORK_ROLLOVER_PENDING")?
        .ok_or("WEB_WORK_ROLLOVER_PENDING")?;
    if continuation_task_from_proof(record, &request, &registered, current, &delivery, &timer)
        .ok()
        .flatten()
        .is_none()
    {
        return Err("WEB_WORK_ROLLOVER_PENDING".into());
    }
    Ok(())
}

fn completed_retry_context_allows_work(
    pressure: Option<&PressureStatus>,
    record: &Record,
    current: &Target,
) -> bool {
    let Some(pressure) = pressure else {
        return false;
    };
    // A failed advisory observation can leave the new key active. It must
    // still be free of its own pending rollover before another task starts.
    if pressure.key == key(&record.project_id, current) {
        return pressure.rollover_intent.is_none();
    }
    // If binding the meter failed, only the exact predecessor request may
    // remain active. The caller has already proved the new paired target and
    // completed continuation before this exception is considered.
    pressure.key == key(&record.project_id, &record.source)
        && pressure.rollover_intent.as_ref().is_some_and(|intent| {
            intent.intent_id == record.intent_id
                && intent.key == pressure.key
                && intent.target_sha256 == record.source.digest
                && intent.source_task_id == record.source_task_id
                && intent.status == "REQUESTED"
        })
}

pub(crate) fn acquire_web_work_lease(workspace: &Path, project: &str) -> Result<WorkLease, String> {
    let store = ProjectStore::open(workspace, project)?;
    let active = store.active()?;
    let approved_target = match active.as_ref() {
        None if store.handoff_request()?.is_none() => None,
        Some(record) if record.phase == Phase::ContinuationQueued => {
            let current = paired(workspace, project).map_err(|_| "WEB_WORK_ROLLOVER_PENDING")?;
            let root = crate::wake_protocol_client::wake_root_for_workspace(workspace)
                .map_err(|_| "WEB_WORK_ROLLOVER_PENDING")?;
            let wake = Store::open(&root).map_err(|_| "WEB_WORK_ROLLOVER_PENDING")?;
            continuation_ready_for_work(&store, &wake, workspace, project, &current, record)?;
            Some(current)
        }
        _ => return Err("WEB_WORK_ROLLOVER_PENDING".into()),
    };
    // A new context intent can precede its handoff file, including after a
    // previous rollover completed. Sample its active key and intent together.
    let context_root = workspace.join(".catdesk/web-context");
    let pressure = if context_root
        .try_exists()
        .map_err(|_| "WEB_WORK_ROLLOVER_PENDING")?
    {
        WebContextStore::open_nonblocking(&context_root)
            .and_then(|meter| meter.active_pressure(project))
            .map_err(|_| "WEB_WORK_ROLLOVER_PENDING")?
    } else {
        None
    };
    match (active.as_ref(), approved_target.as_ref()) {
        (Some(record), Some(current))
            if record.context_meter_status == ContextMeterStatus::RetryRequired =>
        {
            if !completed_retry_context_allows_work(pressure.as_ref(), record, current) {
                return Err("WEB_WORK_ROLLOVER_PENDING".into());
            }
        }
        _ if pressure
            .as_ref()
            .is_some_and(|value| value.rollover_intent.is_some()) =>
        {
            return Err("WEB_WORK_ROLLOVER_PENDING".into());
        }
        _ => {}
    }
    Ok(WorkLease {
        _store: store,
        approved_target,
    })
}

fn plain(path: &Path, directory: bool) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(|_| "WEB_ROLLOVER_STATE_UNAVAILABLE")?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err("WEB_ROLLOVER_REDIRECTED_STATE".into());
        }
    }
    if metadata.file_type().is_symlink()
        || (directory && !metadata.is_dir())
        || (!directory && !metadata.is_file())
    {
        return Err("WEB_ROLLOVER_REDIRECTED_STATE".into());
    }
    Ok(())
}

fn directory(path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        directory(parent)?;
    }
    match fs::symlink_metadata(path) {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => match fs::create_dir(path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(_) => return Err("WEB_ROLLOVER_STATE_UNAVAILABLE".into()),
        },
        Err(_) => return Err("WEB_ROLLOVER_STATE_UNAVAILABLE".into()),
    }
    plain(path, true)
}

fn existing_directory(path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        existing_directory(parent)?;
    }
    plain(path, true)
}

impl ProjectStore {
    fn open(workspace: &Path, project: &str) -> Result<Self, String> {
        Self::open_with_creation(workspace, project, true)
    }

    fn open_existing(workspace: &Path, project: &str) -> Result<Self, String> {
        Self::open_with_creation(workspace, project, false)
    }

    fn open_with_creation(workspace: &Path, project: &str, create: bool) -> Result<Self, String> {
        if !valid_id(project) {
            return Err("WEB_PROJECT_INVALID".into());
        }
        let root = workspace.join(".catdesk/web-rollovers").join(project);
        if create {
            directory(&root)?;
        } else {
            existing_directory(&root)?;
        }
        let lock_path = root.join("project.lock");
        if !create
            || lock_path
                .try_exists()
                .map_err(|_| "WEB_ROLLOVER_LOCK_UNAVAILABLE")?
        {
            plain(&lock_path, false)?;
        }
        let mut options = OpenOptions::new();
        options
            .read(true)
            .write(true)
            .create(create)
            .truncate(false);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.custom_flags(0x0020_0000);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW);
        }
        let lock = options
            .open(&lock_path)
            .map_err(|_| "WEB_ROLLOVER_LOCK_UNAVAILABLE")?;
        plain(&lock_path, false)?;
        lock.try_lock().map_err(|_| "WEB_ROLLOVER_PROJECT_BUSY")?;
        Ok(Self { root, _lock: lock })
    }
    fn active(&self) -> Result<Option<Record>, String> {
        let path = self.root.join("active.json");
        if !path
            .try_exists()
            .map_err(|_| "WEB_ROLLOVER_STATE_UNAVAILABLE")?
        {
            return Ok(None);
        }
        let id: String = read(&path)?;
        if !valid_id(&id) {
            return Err("WEB_ROLLOVER_STATE_INVALID".into());
        }
        let record: Record = read(&self.root.join(format!("{id}.json")))?;
        if record.schema_version != 1
            || record.intent_id != id
            || record.project_id != self.root.file_name().and_then(|v| v.to_str()).unwrap_or("")
        {
            return Err("WEB_ROLLOVER_STATE_INVALID".into());
        }
        record.source.validate()?;
        if record.bootstrap_progress.is_some() && record.phase == Phase::Prepared {
            return Err("WEB_ROLLOVER_STATE_INVALID".into());
        }
        if record.message_digest
            != message_digest(
                &record.project_id,
                &record.intent_id,
                &record.source.url,
                &record.handoff,
            )?
        {
            return Err("WEB_ROLLOVER_HANDOFF_CHANGED".into());
        }
        Ok(Some(record))
    }
    fn save(&self, record: &Record) -> Result<(), String> {
        atomic(
            &self.root.join(format!("{}.json", record.intent_id)),
            record,
        )
    }
    fn handoff_request(&self) -> Result<Option<HandoffRequest>, String> {
        let pointer = self.root.join("handoff-active.json");
        if !pointer
            .try_exists()
            .map_err(|_| "WEB_HANDOFF_REQUEST_UNAVAILABLE")?
        {
            return Ok(None);
        }
        let id: String = read(&pointer)?;
        if !valid_id(&id) {
            return Err("WEB_HANDOFF_REQUEST_INVALID".into());
        }
        let request: HandoffRequest = read(&self.root.join(format!("handoff-{id}.json")))?;
        request.validate(self.root.file_name().and_then(|v| v.to_str()).unwrap_or(""))?;
        if request.intent_id != id {
            return Err("WEB_HANDOFF_REQUEST_INVALID".into());
        }
        Ok(Some(request))
    }
    fn save_handoff_request(&self, request: &HandoffRequest) -> Result<(), String> {
        request.validate(&request.project_id)?;
        atomic(
            &self
                .root
                .join(format!("handoff-{}.json", request.intent_id)),
            request,
        )?;
        atomic(&self.root.join("handoff-active.json"), &request.intent_id)
    }
}

fn draft_archive_path(store: &ProjectStore, record: &Record) -> PathBuf {
    store
        .root
        .join(format!("draft-archive-{}.json", record.intent_id))
}

fn draft_archive_receipt_path(store: &ProjectStore, record: &Record) -> PathBuf {
    store
        .root
        .join(format!("draft-archive-complete-{}.json", record.intent_id))
}

fn read_draft_archive(
    store: &ProjectStore,
    record: &Record,
) -> Result<Option<DraftArchive>, String> {
    let path = draft_archive_path(store, record);
    if !path
        .try_exists()
        .map_err(|_| "WEB_DRAFT_ARCHIVE_UNAVAILABLE")?
    {
        return Ok(None);
    }
    plain(&path, false)?;
    let archive: DraftArchive = read(&path).map_err(|_| "WEB_DRAFT_ARCHIVE_INVALID")?;
    let expected = runtime::compose_bootstrap_message(
        &record.project_id,
        &record.intent_id,
        &record.source,
        &record.handoff,
    )?;
    if archive.schema_version != 1
        || archive.project_id != record.project_id
        || archive.intent_id != record.intent_id
        || archive.source != record.source
        || archive.message_digest != record.message_digest
        || archive.expected_text != expected
        || archive.actual_text.len() > 16 * 1024
        || archive.observed_bytes != archive.actual_text.len()
        || digest(
            &archive
                .actual_text
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" "),
        ) != record.message_digest
    {
        return Err("WEB_DRAFT_ARCHIVE_INVALID".into());
    }
    Ok(Some(archive))
}

fn create_private_once<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let bytes = serde_json::to_vec(value).map_err(|_| "WEB_DRAFT_ARCHIVE_INVALID")?;
    if bytes.len() > 40 * 1024 {
        return Err("WEB_DRAFT_ARCHIVE_TOO_LARGE".into());
    }
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x0020_0000);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = options
        .open(path)
        .map_err(|_| "WEB_DRAFT_ARCHIVE_UNAVAILABLE")?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| "WEB_DRAFT_ARCHIVE_UNAVAILABLE")?;
    plain(path, false)?;
    Ok(())
}

fn save_draft_archive(
    store: &ProjectStore,
    record: &Record,
    actual_text: &str,
    observed_bytes: usize,
) -> Result<(), String> {
    if actual_text.len() > 16 * 1024
        || actual_text.len() != observed_bytes
        || digest(&actual_text.split_whitespace().collect::<Vec<_>>().join(" "))
            != record.message_digest
    {
        return Err("WEB_DRAFT_ARCHIVE_WITNESS_INVALID".into());
    }
    if let Some(existing) = read_draft_archive(store, record)? {
        return if existing.actual_text == actual_text {
            Ok(())
        } else {
            Err("WEB_DRAFT_ARCHIVE_WITNESS_CHANGED".into())
        };
    }
    let archive = DraftArchive {
        schema_version: 1,
        project_id: record.project_id.clone(),
        intent_id: record.intent_id.clone(),
        source: record.source.clone(),
        message_digest: record.message_digest.clone(),
        expected_text: runtime::compose_bootstrap_message(
            &record.project_id,
            &record.intent_id,
            &record.source,
            &record.handoff,
        )?,
        actual_text: actual_text.into(),
        observed_bytes,
        recorded_utc: now(),
    };
    create_private_once(&draft_archive_path(store, record), &archive)?;
    if read_draft_archive(store, record)?.as_ref() != Some(&archive) {
        return Err("WEB_DRAFT_ARCHIVE_READBACK_FAILED".into());
    }
    Ok(())
}

fn read_draft_archive_receipt(
    store: &ProjectStore,
    record: &Record,
) -> Result<Option<DraftArchiveReceipt>, String> {
    let path = draft_archive_receipt_path(store, record);
    if !path
        .try_exists()
        .map_err(|_| "WEB_DRAFT_ARCHIVE_UNAVAILABLE")?
    {
        return Ok(None);
    }
    plain(&path, false)?;
    let receipt: DraftArchiveReceipt =
        read(&path).map_err(|_| "WEB_DRAFT_ARCHIVE_RECEIPT_INVALID")?;
    if receipt.schema_version != 1
        || receipt.project_id != record.project_id
        || receipt.intent_id != record.intent_id
        || receipt.source_digest != record.source.digest
        || receipt.message_digest != record.message_digest
        || receipt.evidence != "ARCHIVE_RECORDED_HOME_EMPTY"
        || read_draft_archive(store, record)?.is_none()
    {
        return Err("WEB_DRAFT_ARCHIVE_RECEIPT_INVALID".into());
    }
    Ok(Some(receipt))
}

fn registered_project(workspace: &Path, project: &str) -> Result<AutonomousProjectV1, String> {
    let registry =
        AutonomousProjectRegistryStoreV1::open_read_only(workspace.join(".catdesk/projects"))
            .and_then(|store| store.load_registry())
            .map_err(|_| "WEB_PROJECT_REGISTRY_UNAVAILABLE")?;
    registry
        .projects
        .iter()
        .find(|entry| entry.project_id == project)
        .cloned()
        .ok_or("WEB_PROJECT_NOT_REGISTERED".into())
}

fn native_owner_matches(request: &HandoffRequest, registered: &AutonomousProjectV1) -> bool {
    registered.project_id == request.project_id
        && registered.workspace == request.native_workspace
        && registered.codex_thread_id == request.native_thread_id
}

fn handoff_request_message(
    project: &str,
    intent: &str,
    source_task: &str,
    native_thread: &Option<String>,
) -> String {
    let owner = native_thread.as_deref().unwrap_or("UNBOUND");
    format!(
        "CatDesk handoff request for project {project}, task {source_task}, intent {intent}. Native Codex thread: {owner}. Reply with exactly one JSON object, no Markdown or surrounding prose, at most 3000 UTF-8 bytes: {{\"schemaVersion\":1,\"projectId\":\"{project}\",\"sourceTaskId\":\"{source_task}\",\"objective\":\"...\",\"completed\":[],\"remaining\":[\"...\"],\"nextAction\":\"...\",\"ownedPaths\":[],\"verification\":[],\"blockers\":[]}}. Report only your own observed work, evidence and unresolved steps. Do not infer native work completion from a process exit or queue status, claim native-owned paths, execute code, or repeat work owned by the native thread. This request is only for a durable handoff."
    )
}

fn message_digest(
    project: &str,
    intent: &str,
    source: &str,
    handoff: &str,
) -> Result<String, String> {
    runtime::bootstrap_message_digest(project, intent, &Target::new(1, source.into())?, handoff)
}

fn key(project: &str, target: &Target) -> ConversationKey {
    ConversationKey {
        project_id: project.into(),
        conversation_url: target.url.clone(),
        target_generation: target.generation,
    }
}

fn paired(workspace: &Path, project: &str) -> Result<Target, String> {
    let target = mcp::operator_read_project_web_target(workspace, project)
        .map_err(|_| "WEB_ROLLOVER_PAIRED_TARGET_REQUIRED")?
        .ok_or("WEB_ROLLOVER_TARGET_UNBOUND")?;
    Target::new(target.generation, target.url)
}

fn confirm_request_enabled(workspace: &Path, record: &Record) -> Result<(), String> {
    let meter = WebContextStore::open_nonblocking(workspace.join(".catdesk/web-context"))?;
    let intent = meter
        .request_rollover_if_due(
            &key(&record.project_id, &record.source),
            &record.source.digest,
            &record.source_task_id,
            true,
            now(),
        )?
        .ok_or("WEB_ROLLOVER_NOT_DUE_OR_DISABLED")?;
    if intent.intent_id != record.intent_id || intent.source_task_id != record.source_task_id {
        return Err("WEB_ROLLOVER_INTENT_CHANGED".into());
    }
    Ok(())
}

fn summary(record: &Record) -> Value {
    json!({"schemaVersion":1,"projectId":record.project_id,"intentId":record.intent_id,
        "sourceTaskId":record.source_task_id,"source":record.source,"phase":record.phase,
        "messageDigest":record.message_digest,"receipt":record.receipt,
        "bootstrapProgress":record.bootstrap_progress,
        "continuationEventId":record.continuation_event_id,
        "boundTarget":record.bound_target,
        "reconciliationRequired":record.reconciliation_reason.is_some(),
        "reconciliationReason":record.reconciliation_reason,
        "contextMeterStatus":record.context_meter_status,
        "handoffEventId":record.handoff_event_id,
        "sourceProgressFingerprint":record.source_fingerprint,
        "sourceSnapshotVerified":record.source_snapshot_verified,
        "quarantine":record.quarantine,
        "continuationProved":false,"continuationProofStatus":"NOT_CHECKED",
        "oldConversationPreserved":true})
}

fn resumable_bootstrap(record: &Record) -> bool {
    record.phase == Phase::Submitting
        && record.receipt.is_none()
        && record.bound_target.is_none()
        && (record.reconciliation_reason.is_none()
            || (record.bootstrap_progress == Some(runtime::BootstrapProgress::DraftReady)
                && record.reconciliation_reason.as_deref()
                    == Some("WEB_BOOTSTRAP_PRE_SEND_AMBIGUOUS_NO_RESEND")))
        && matches!(
            record.bootstrap_progress,
            Some(
                runtime::BootstrapProgress::TextEntryAuthorized
                    | runtime::BootstrapProgress::DraftReady
            )
        )
}

fn checkpoint_bootstrap_progress(
    record: &mut Record,
    stage: runtime::BootstrapProgress,
) -> Result<(), String> {
    use runtime::BootstrapProgress as Progress;
    if !matches!(record.phase, Phase::Prepared | Phase::Submitting) {
        return Err("WEB_BOOTSTRAP_PROGRESS_PHASE_INVALID".into());
    }
    let next = match record.bootstrap_progress {
        None => Progress::TextEntryAuthorized,
        Some(Progress::TextEntryAuthorized) => Progress::DraftReady,
        Some(Progress::DraftReady) => Progress::SendAttempted,
        Some(Progress::SendAttempted) => {
            return Err("WEB_BOOTSTRAP_OUTCOME_UNKNOWN_NO_RESEND".into());
        }
    };
    if stage > next {
        return Err("WEB_BOOTSTRAP_PROGRESS_ORDER_INVALID".into());
    }
    let already_verified_ready = record.bootstrap_progress == Some(Progress::DraftReady);
    record.phase = Phase::Submitting;
    if record.bootstrap_progress.is_none_or(|saved| stage > saved) {
        record.bootstrap_progress = Some(stage);
    }
    // A resumed helper emits DRAFT_READY only after rechecking the full owned
    // home draft. Before SEND_ATTEMPTED its remaining operations are reads.
    // Repair only the overly broad v2 pre-send fence, never another incident.
    if already_verified_ready
        && stage == Progress::DraftReady
        && record.reconciliation_reason.as_deref()
            == Some("WEB_BOOTSTRAP_PRE_SEND_AMBIGUOUS_NO_RESEND")
    {
        record.reconciliation_reason = None;
    }
    Ok(())
}

fn handoff_request_summary(request: &HandoffRequest) -> Value {
    json!({"projectId":request.project_id,"intentId":request.intent_id,
        "sourceTaskId":request.source_task_id,"source":request.source,
        "eventId":request.event_id,"nativeThreadId":request.native_thread_id,
        "deliveryProved":null,"completionProved":null,"handoffCollected":false})
}

pub(super) fn auto_stage(
    workspace: &Path,
    project: &str,
    current: &Target,
) -> Result<AutoStage, String> {
    let store = ProjectStore::open(workspace, project)?;
    if let Some(record) = store.active()? {
        if (matches!(
            record.phase,
            Phase::Prepared | Phase::ResponsePending | Phase::Created | Phase::Bound
        ) || resumable_bootstrap(&record))
            && (record.handoff_event_id.is_none() || record.source_fingerprint.is_none())
        {
            return Ok(AutoStage::SourceWitnessRequired);
        }
        match record.phase {
            Phase::Submitting => {
                return Ok(if resumable_bootstrap(&record) {
                    AutoStage::Execute
                } else {
                    AutoStage::SubmissionUnknown
                });
            }
            Phase::Quarantined => return Ok(AutoStage::Quarantined),
            Phase::Prepared | Phase::ResponsePending | Phase::Created | Phase::Bound => {
                return Ok(AutoStage::Execute);
            }
            Phase::ContinuationQueued
                if record.context_meter_status != ContextMeterStatus::Recorded =>
            {
                return Ok(AutoStage::Execute);
            }
            Phase::ContinuationQueued if record.source == *current => {
                return Err("WEB_ROLLOVER_QUEUED_SOURCE_STILL_BOUND".into());
            }
            Phase::ContinuationQueued => {}
        }
    }
    if store
        .handoff_request()?
        .is_some_and(|request| request.source == *current)
    {
        return Ok(AutoStage::CollectHandoff);
    }
    Ok(AutoStage::Observe)
}

/// Resume only the Wake-first half of an already proved automatic rollover.
/// A fresh rollover still requires the ordinary paired source authority.
pub(super) fn automatic_recovery_authority(
    workspace: &Path,
    project: &str,
) -> Result<Option<(ConversationKey, Target)>, String> {
    let store = ProjectStore::open(workspace, project)?;
    let Some(record) = store.active()? else {
        return Ok(None);
    };
    let registered = registered_project(workspace, project)?;
    let wake = Store::open(&crate::wake_protocol_client::wake_root_for_workspace(
        workspace,
    )?)?;
    let current = wake.config()?.targets.get(project).cloned();
    let request = store.handoff_request()?;
    let Some(destination) =
        recovery_destination(&record, &registered, current.as_ref(), request.as_ref())?
    else {
        return Ok(None);
    };
    Ok(Some((key(project, &record.source), destination)))
}

fn recovery_destination(
    record: &Record,
    registered: &AutonomousProjectV1,
    wake_target: Option<&Target>,
    request: Option<&HandoffRequest>,
) -> Result<Option<Target>, String> {
    if record.phase != Phase::Created
        || !record.source_snapshot_verified
        || record.handoff_event_id.is_none()
        || record
            .source_fingerprint
            .as_ref()
            .is_none_or(|fingerprint| {
                fingerprint.len() != 64
                    || !fingerprint
                        .bytes()
                        .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
            })
    {
        return Ok(None);
    }
    if request.is_none_or(|request| !handoff_owner_matches_record(request, record, registered)) {
        return Ok(None);
    }
    let receipt = record
        .receipt
        .as_ref()
        .ok_or("WEB_BOOTSTRAP_RECEIPT_REQUIRED")?;
    let destination = validate_receipt(record, receipt)?;
    if registered.chatgpt_target_url.as_deref() != Some(record.source.url.as_str())
        || registered.chatgpt_target_sha256.as_deref() != Some(record.source.digest.as_str())
        || registered.project_id != record.project_id
    {
        return Ok(None);
    }
    if wake_target != Some(&destination) {
        return Ok(None);
    }
    Ok(Some(destination))
}

pub(super) fn record_auto_status(
    workspace: &Path,
    project: &str,
    stage: &str,
    target_generation: Option<u64>,
    reason: Option<&str>,
) -> Result<(), String> {
    record_auto_status_at(workspace, project, stage, target_generation, reason, now())
}

fn record_auto_status_at(
    workspace: &Path,
    project: &str,
    stage: &str,
    target_generation: Option<u64>,
    reason: Option<&str>,
    tick_utc: u64,
) -> Result<(), String> {
    if !valid_auto_stage(stage) || tick_utc == 0 {
        return Err("WEB_AUTO_STATUS_INVALID".into());
    }
    let reason = reason.map(|value| {
        if valid_auto_reason(value) {
            value.to_owned()
        } else {
            let code = crate::operator_web_error_code(value);
            if code == "WEB_OPERATION_REFUSED" {
                "WEB_AUTO_ATTENTION".into()
            } else {
                code.into()
            }
        }
    });
    let store = ProjectStore::open(workspace, project)?;
    let previous = read_auto_status(&store).ok().flatten();
    let previous_failure = previous.as_ref().and_then(retained_auto_failure);
    let previous_retry = previous.as_ref().and_then(retained_auto_retry);
    let last_failure = if matches!(stage, "AWAITING_RESPONSE" | "ROLLOVER_ADVANCED") {
        None
    } else if stage == "ATTENTION"
        && let Some(code) = reason.as_deref()
        && code != "WEB_ROLLOVER_NOT_DUE_OR_DISABLED"
    {
        Some(AutoFailure {
            reason: code.into(),
            tick_utc,
            target_generation,
        })
    } else {
        previous_failure
    };
    let retry = if stage == "ATTENTION"
        && reason
            .as_deref()
            .is_some_and(|code| code != "WEB_ROLLOVER_NOT_DUE_OR_DISABLED")
    {
        let failures = previous_retry
            .filter(|retry| retry.target_generation == target_generation)
            .map_or(1, |retry| retry.consecutive_failures.saturating_add(1))
            .min(AUTO_RETRY_MAX_FAILURES);
        Some(AutoRetry {
            consecutive_failures: failures,
            next_attempt_utc: tick_utc.saturating_add(auto_retry_delay(failures)),
            target_generation,
        })
    } else if matches!(
        stage,
        "OBSERVED"
            | "HANDOFF_REQUESTED"
            | "AWAITING_HANDOFF"
            | "AWAITING_RESPONSE"
            | "ROLLOVER_ADVANCED"
    ) {
        // A successful browser step clears delay, independently of retained
        // failure diagnostics. Waiting/stopped/disabled ticks do not renew it.
        None
    } else {
        previous_retry
    };
    atomic(
        &store.root.join("auto-status.json"),
        &AutoStatus {
            schema_version: AUTO_STATUS_SCHEMA_VERSION,
            project_id: project.into(),
            tick_utc,
            stage: stage.into(),
            target_generation,
            reason,
            last_failure,
            retry,
        },
    )
}

fn retained_auto_failure(status: &AutoStatus) -> Option<AutoFailure> {
    status.last_failure.clone().or_else(|| {
        (status.stage == "ATTENTION")
            .then_some(status.reason.as_deref())
            .flatten()
            .filter(|reason| *reason != "WEB_ROLLOVER_NOT_DUE_OR_DISABLED")
            .map(|reason| AutoFailure {
                reason: reason.into(),
                tick_utc: status.tick_utc,
                target_generation: status.target_generation,
            })
    })
}

fn retained_auto_retry(status: &AutoStatus) -> Option<AutoRetry> {
    status.retry.clone().or_else(|| {
        // Read older attention records conservatively without rewriting them.
        if status.schema_version != 1 || status.stage != "ATTENTION" {
            return None;
        }
        let failure = retained_auto_failure(status)?;
        Some(AutoRetry {
            consecutive_failures: 1,
            next_attempt_utc: failure.tick_utc.saturating_add(AUTO_RETRY_BASE_SECONDS),
            target_generation: failure.target_generation,
        })
    })
}

pub(super) fn automatic_retry_after(
    workspace: &Path,
    project: &str,
    generation: u64,
) -> Result<Option<u64>, String> {
    if !valid_id(project) {
        return Err("WEB_PROJECT_INVALID".into());
    }
    let root = workspace.join(".catdesk/web-rollovers").join(project);
    match fs::symlink_metadata(&root) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("WEB_AUTO_STATUS_UNAVAILABLE".into()),
        Ok(_) => {}
    }
    let store = ProjectStore::open_existing(workspace, project)?;
    Ok(read_auto_status(&store)?
        .as_ref()
        .and_then(retained_auto_retry)
        .filter(|retry| retry.target_generation == Some(generation))
        .map(|retry| retry.next_attempt_utc))
}

fn valid_auto_stage(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value.bytes().all(|b| b.is_ascii_uppercase() || b == b'_')
}

fn valid_auto_reason(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
}

fn read_auto_status(store: &ProjectStore) -> Result<Option<AutoStatus>, String> {
    let path = store.root.join("auto-status.json");
    if !path
        .try_exists()
        .map_err(|_| "WEB_AUTO_STATUS_UNAVAILABLE")?
    {
        return Ok(None);
    }
    let status: AutoStatus = read(&path)?;
    if (status.schema_version != 1 && status.schema_version != AUTO_STATUS_SCHEMA_VERSION)
        || (status.schema_version == 1 && status.retry.is_some())
        || status.project_id
            != store
                .root
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("")
        || status.tick_utc == 0
        || !valid_auto_stage(&status.stage)
        || status
            .reason
            .as_deref()
            .is_some_and(|reason| !valid_auto_reason(reason))
        || status
            .last_failure
            .as_ref()
            .is_some_and(|failure| failure.tick_utc == 0 || !valid_auto_reason(&failure.reason))
        || status.retry.as_ref().is_some_and(|retry| {
            retry.consecutive_failures == 0
                || retry.consecutive_failures > AUTO_RETRY_MAX_FAILURES
                || retained_auto_failure(&status).is_none_or(|failure| {
                    failure.target_generation != retry.target_generation
                        || retry.next_attempt_utc
                            != failure
                                .tick_utc
                                .saturating_add(auto_retry_delay(retry.consecutive_failures))
                })
        })
    {
        return Err("WEB_AUTO_STATUS_INVALID".into());
    }
    Ok(Some(status))
}

pub(super) fn status_read_only(workspace: &Path, project: &str) -> Result<Value, String> {
    let store = ProjectStore::open_existing(workspace, project)?;
    let mut result = status_from_store(&store, project)?;
    if let Some(record) = store
        .active()?
        .filter(|record| record.phase == Phase::ContinuationQueued)
    {
        let proof = (|| -> Result<(), String> {
            let root = crate::wake_protocol_client::wake_root_for_workspace(workspace)?;
            let wake = Store::open_read_only(&root)?;
            let current = mcp::operator_read_project_web_target_status(workspace, project)
                .map_err(|_| "WEB_ROLLOVER_PAIRED_TARGET_REQUIRED")?
                .ok_or("WEB_ROLLOVER_TARGET_UNBOUND")?;
            let current = Target::new(current.generation, current.url)?;
            continuation_ready_for_work(&store, &wake, workspace, project, &current, &record)
        })();
        result["continuationProved"] = json!(proof.is_ok());
        result["continuationProofStatus"] = json!(match proof {
            Ok(()) => "VERIFIED",
            Err(ref reason) if reason == "WEB_WORK_ROLLOVER_PENDING" => "PENDING_OR_UNVERIFIED",
            Err(_) => "UNAVAILABLE_OR_CHANGED",
        });
    }
    Ok(result)
}

fn status_from_store(store: &ProjectStore, project: &str) -> Result<Value, String> {
    let active = store.active()?;
    let mut result = match &active {
        Some(record) => summary(record),
        None => json!({"projectId":project,"phase":"NONE"}),
    };
    if let Some(request) = store.handoff_request()? {
        let mut request_summary = handoff_request_summary(&request);
        let collected = active.as_ref().is_some_and(|record| {
            record.handoff_event_id.as_deref() == Some(&request.event_id)
                && record.intent_id == request.intent_id
                && record.source == request.source
                && record.source_task_id == request.source_task_id
        });
        request_summary["handoffCollected"] = json!(collected);
        if collected {
            request_summary["deliveryProved"] = json!(true);
            request_summary["completionProved"] = json!(true);
        }
        result["handoffRequest"] = request_summary;
    }
    result["automation"] = json!(read_auto_status(store)?);
    Ok(result)
}

pub(super) fn read_handoff(path: &Path) -> Result<Handoff, String> {
    plain(path, false)?;
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|_| "WEB_HANDOFF_UNAVAILABLE")?
        .take(10 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "WEB_HANDOFF_UNAVAILABLE")?;
    if bytes.len() > 10 * 1024 {
        return Err("WEB_HANDOFF_TOO_LARGE".into());
    }
    serde_json::from_slice(&bytes).map_err(|_| "WEB_HANDOFF_INVALID".into())
}

fn source_ready_for_handoff(snapshot: &Value) -> Result<(), String> {
    let messages = snapshot
        .get("messages")
        .and_then(Value::as_array)
        .ok_or("WEB_HANDOFF_SOURCE_SNAPSHOT_INVALID")?;
    let latest = messages
        .last()
        .ok_or("WEB_HANDOFF_SOURCE_RESPONSE_REQUIRED")?;
    if latest.get("role").and_then(Value::as_str) != Some("ASSISTANT")
        || latest.get("textComplete").and_then(Value::as_bool) != Some(true)
        || latest.get("revisionComplete").and_then(Value::as_bool) != Some(true)
        || latest
            .get("text")
            .and_then(Value::as_str)
            .is_none_or(|text| text.trim().is_empty())
    {
        return Err("WEB_HANDOFF_SOURCE_RESPONSE_REQUIRED".into());
    }
    Ok(())
}

fn request_policy_still_due(
    meter: &WebContextStore,
    request: &HandoffRequest,
) -> Result<(), String> {
    let pressure = meter.pressure(&key(&request.project_id, &request.source))?;
    let incomplete_history_due = pressure.incomplete_history_rollover
        && pressure.browser_coverage.observed
        && pressure.pressure_assessment == "UNKNOWN_OR_INCOMPLETE"
        && pressure
            .rollover_intent
            .as_ref()
            .is_some_and(|intent| intent.trigger_reason == "INCOMPLETE_HISTORY_GUARD");
    if !pressure.is_current
        || !pressure.automatic_rollover_enabled
        || !(pressure.near_capacity || incomplete_history_due)
        || pressure.rollover_intent.as_ref().is_none_or(|intent| {
            intent.intent_id != request.intent_id
                || intent.source_task_id != request.source_task_id
                || intent.key != key(&request.project_id, &request.source)
                || intent.target_sha256 != request.source.digest
        })
    {
        return Err("WEB_HANDOFF_POLICY_OR_INTENT_CHANGED".into());
    }
    Ok(())
}

pub(super) fn request_handoff(
    workspace: &Path,
    project: &str,
    source_task_id: &str,
) -> Result<Value, String> {
    request_handoff_guarded(workspace, project, source_task_id, &|| Ok(()))
}

pub(super) fn request_handoff_guarded(
    workspace: &Path,
    project: &str,
    source_task_id: &str,
    keep_running: &dyn Fn() -> Result<(), String>,
) -> Result<Value, String> {
    keep_running()?;
    if !valid_id(source_task_id) {
        return Err("WEB_HANDOFF_TASK_INVALID".into());
    }
    let store = ProjectStore::open(workspace, project)?;
    let work = WorkLedger::status(workspace, project);
    match work["status"].as_str() {
        Some("UNOBSERVED" | "DURABLE_SNAPSHOT") => {}
        _ => return Err("WEB_HANDOFF_WORK_INTENT_UNAVAILABLE".into()),
    }
    if !work["active"].is_null() && work["active"]["eventId"].as_str() != Some(source_task_id) {
        return Err("WEB_HANDOFF_WORK_INTENT_CHANGED".into());
    }
    let source = paired(workspace, project)?;
    let active = store.active()?;
    if let Some(active) = &active
        && (active.phase != Phase::ContinuationQueued || active.source == source)
    {
        return Err("WEB_ROLLOVER_EXISTING_INTENT_REQUIRES_RESUME".into());
    }
    let registered = registered_project(workspace, project)?;
    let root = crate::wake_protocol_client::wake_root_for_workspace(workspace)?;
    let wake = Store::open(&root)?;
    catdesk_wake::development_runtime::validate_selected_target_isolation(
        workspace,
        wake.root(),
        &source.url,
    )?;
    let meter = WebContextStore::open_nonblocking(workspace.join(".catdesk/web-context"))?;
    if let Some(previous) =
        saved_current_handoff_request(&store, &source, source_task_id, &registered)?
    {
        request_policy_still_due(&meter, &previous)?;
        if paired(workspace, project)? != source {
            return Err("WEB_HANDOFF_SOURCE_CHANGED".into());
        }
        catdesk_wake::development_runtime::validate_selected_target_isolation(
            workspace,
            wake.root(),
            &source.url,
        )?;
        keep_running()?;
        wake.produce_expected(
            &previous.event_id,
            project,
            "engineering_attention",
            &previous.message,
            &source,
        )?;
        return Ok(handoff_request_summary(&previous));
    }
    require_prior_continuation_owner(
        &store,
        active.as_ref(),
        &source,
        source_task_id,
        &registered,
    )?;

    // The selected Wake browser owner observes the exact idle source before the
    // advisory context ledger claims a quiescent point. This is a private read.
    let snapshot = runtime::snapshot_conversation_guarded(&wake, project, &source, keep_running)?;
    keep_running()?;
    source_ready_for_handoff(&snapshot)?;
    let source_key = key(project, &source);
    let previous = meter.active_key(project)?;
    meter.bind_observed_target(source_key.clone(), previous.as_ref())?;
    super::observe::ingest(&meter, &source_key, snapshot, now(), || {
        keep_running()?;
        if paired(workspace, project)? == source {
            Ok(())
        } else {
            Err("WEB_HANDOFF_SOURCE_CHANGED".into())
        }
    })?;
    let intent = meter
        .request_rollover_if_due(&source_key, &source.digest, source_task_id, true, now())?
        .ok_or("WEB_ROLLOVER_NOT_DUE_OR_DISABLED")?;
    if intent.source_task_id != source_task_id {
        return Err("WEB_HANDOFF_TASK_CHANGED".into());
    }
    let request = HandoffRequest {
        schema_version: 1,
        project_id: project.into(),
        intent_id: intent.intent_id.clone(),
        source_task_id: source_task_id.into(),
        source,
        native_workspace: registered.workspace.clone(),
        native_thread_id: registered.codex_thread_id.clone(),
        event_id: format!("web-handoff-{}", intent.intent_id),
        message: handoff_request_message(
            project,
            &intent.intent_id,
            source_task_id,
            &registered.codex_thread_id,
        ),
    };
    request.validate(project)?;
    // Save before queue publication. A retry after a crash republishes only
    // these exact bytes under the same Wake event ID.
    keep_running()?;
    store.save_handoff_request(&request)?;
    if paired(workspace, project)? != request.source
        || !native_owner_matches(&request, &registered_project(workspace, project)?)
    {
        return Err("WEB_HANDOFF_SOURCE_OR_OWNER_CHANGED".into());
    }
    catdesk_wake::development_runtime::validate_selected_target_isolation(
        workspace,
        wake.root(),
        &request.source.url,
    )?;
    keep_running()?;
    wake.produce_expected(
        &request.event_id,
        project,
        "engineering_attention",
        &request.message,
        &request.source,
    )?;
    Ok(handoff_request_summary(&request))
}

fn saved_current_handoff_request(
    store: &ProjectStore,
    source: &Target,
    source_task_id: &str,
    registered: &AutonomousProjectV1,
) -> Result<Option<HandoffRequest>, String> {
    let Some(previous) = store.handoff_request()? else {
        return Ok(None);
    };
    if previous.source != *source {
        return Ok(None);
    }
    if previous.source_task_id != source_task_id {
        return Err("WEB_HANDOFF_TASK_CHANGED".into());
    }
    if !native_owner_matches(&previous, registered) {
        return Err("WEB_HANDOFF_NATIVE_OWNER_CHANGED".into());
    }
    Ok(Some(previous))
}

pub(super) fn resume_saved_handoff_request_guarded(
    workspace: &Path,
    project: &str,
    keep_running: &dyn Fn() -> Result<(), String>,
) -> Result<Value, String> {
    keep_running()?;
    let store = ProjectStore::open(workspace, project)?;
    let request = store
        .handoff_request()?
        .ok_or("WEB_HANDOFF_REQUEST_REQUIRED")?;
    if paired(workspace, project)? != request.source {
        return Err("WEB_HANDOFF_SOURCE_CHANGED".into());
    }
    let task = request.source_task_id;
    drop(store);
    request_handoff_guarded(workspace, project, &task, keep_running)
}

fn completed_request_delivery(
    request: &HandoffRequest,
    delivery: &Delivery,
    timer: &TurnTimer,
) -> Result<(), String> {
    let receipt = delivery
        .receipt
        .as_ref()
        .ok_or("WEB_HANDOFF_EXACT_USER_RECEIPT_REQUIRED")?;
    if delivery.phase != DeliveryPhase::Sent
        || delivery.event.event_id != request.event_id
        || delivery.event.project_id != request.project_id
        || delivery.event.event_type != "engineering_attention"
        || delivery.event.target_generation != request.source.generation
        || delivery.event.message != request.message
        || delivery.target != request.source
        || delivery.message_digest != digest(&request.message)
        || receipt.event_id != request.event_id
        || receipt.target_generation != request.source.generation
        || receipt.target_digest != request.source.digest
        || receipt.message_digest != digest(&request.message)
        || receipt.evidence != "EXACT_USER_MESSAGE_APPENDED"
    {
        return Err("WEB_HANDOFF_EXACT_USER_RECEIPT_REQUIRED".into());
    }
    if timer.event_id != request.event_id
        || timer.project_id != request.project_id
        || timer.target_generation != request.source.generation
        || timer.target_digest != request.source.digest
        || timer.response_state != ResponseState::Complete
        || timer.completed_utc.is_none()
    {
        return Err("WEB_HANDOFF_COMPLETED_TIMER_REQUIRED".into());
    }
    Ok(())
}

fn normalize_visible(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn handoff_from_source_snapshot(
    request: &HandoffRequest,
    snapshot: &Value,
) -> Result<Handoff, String> {
    let messages = snapshot
        .get("messages")
        .and_then(Value::as_array)
        .filter(|messages| (2..=16).contains(&messages.len()))
        .ok_or("WEB_HANDOFF_SOURCE_SNAPSHOT_INVALID")?;
    if snapshot
        .get("coverage")
        .and_then(|coverage| coverage.get("returned"))
        .and_then(Value::as_u64)
        != Some(messages.len() as u64)
    {
        return Err("WEB_HANDOFF_SOURCE_SNAPSHOT_INVALID".into());
    }
    let user = &messages[messages.len() - 2];
    let assistant = &messages[messages.len() - 1];
    for (message, role) in [(user, "USER"), (assistant, "ASSISTANT")] {
        let text = message
            .get("text")
            .and_then(Value::as_str)
            .ok_or("WEB_HANDOFF_SOURCE_SNAPSHOT_INVALID")?;
        let expected_revision = digest(&format!(
            "catdesk-snapshot-revision-v1\0{}\0{}",
            request.source.digest, text
        ));
        if message.get("role").and_then(Value::as_str) != Some(role)
            || message.get("textComplete").and_then(Value::as_bool) != Some(true)
            || message.get("revisionComplete").and_then(Value::as_bool) != Some(true)
            || message.get("revisionHash").and_then(Value::as_str)
                != Some(expected_revision.as_str())
        {
            return Err("WEB_HANDOFF_SOURCE_SNAPSHOT_INVALID".into());
        }
    }
    if normalize_visible(user["text"].as_str().unwrap_or("")) != request.message {
        return Err("WEB_HANDOFF_LATEST_USER_NOT_REQUEST".into());
    }
    let response = assistant["text"]
        .as_str()
        .ok_or("WEB_HANDOFF_JSON_REQUIRED")?;
    if response.len() > 3500 {
        return Err("WEB_HANDOFF_TOO_LARGE".into());
    }
    let handoff: Handoff =
        serde_json::from_str(response).map_err(|_| "WEB_HANDOFF_JSON_REQUIRED")?;
    handoff.validate(&request.project_id)?;
    if handoff.source_task_id != request.source_task_id {
        return Err("WEB_HANDOFF_TASK_CHANGED".into());
    }
    if !handoff.owned_paths.is_empty() {
        return Err("WEB_HANDOFF_OWNERSHIP_UNVERIFIED".into());
    }
    Ok(handoff)
}

pub(super) fn collect_handoff(workspace: &Path, project: &str) -> Result<Value, String> {
    collect_handoff_guarded(workspace, project, &|| Ok(()))
}

pub(super) fn collect_handoff_guarded(
    workspace: &Path,
    project: &str,
    keep_running: &dyn Fn() -> Result<(), String>,
) -> Result<Value, String> {
    keep_running()?;
    let store = ProjectStore::open(workspace, project)?;
    let request = store
        .handoff_request()?
        .ok_or("WEB_HANDOFF_REQUEST_REQUIRED")?;
    if let Some(existing) = store.active()?
        && existing.handoff_event_id.as_deref() == Some(&request.event_id)
        && existing.intent_id == request.intent_id
        && existing.source == request.source
    {
        return Ok(summary(&existing));
    }
    if paired(workspace, project)? != request.source {
        return Err("WEB_HANDOFF_SOURCE_CHANGED".into());
    }
    if !native_owner_matches(&request, &registered_project(workspace, project)?) {
        return Err("WEB_HANDOFF_NATIVE_OWNER_CHANGED".into());
    }
    let meter = WebContextStore::open_nonblocking(workspace.join(".catdesk/web-context"))?;
    request_policy_still_due(&meter, &request)?;
    let root = crate::wake_protocol_client::wake_root_for_workspace(workspace)?;
    let wake = Store::open(&root)?;
    catdesk_wake::development_runtime::validate_selected_target_isolation(
        workspace,
        wake.root(),
        &request.source.url,
    )?;
    let delivery = wake
        .delivery(&request.event_id)?
        .ok_or("WEB_HANDOFF_EXACT_USER_RECEIPT_REQUIRED")?;
    let timer = wake
        .timer(&request.event_id)?
        .ok_or("WEB_HANDOFF_COMPLETED_TIMER_REQUIRED")?;
    completed_request_delivery(&request, &delivery, &timer)?;
    let source_fingerprint =
        wake.source_progress_fingerprint(project, &request.source, &request.event_id)?;
    // The timer proves completion of the exact Wake turn. A fresh browser
    // snapshot still has to show that turn as the latest USER and the JSON
    // answer as its following ASSISTANT before preparing any replacement.
    catdesk_wake::development_runtime::validate_selected_target_isolation(
        workspace,
        wake.root(),
        &request.source.url,
    )?;
    let snapshot =
        runtime::snapshot_conversation_guarded(&wake, project, &request.source, keep_running)?;
    keep_running()?;
    let handoff = handoff_from_source_snapshot(&request, &snapshot)?;
    if wake.source_progress_fingerprint(project, &request.source, &request.event_id)?
        != source_fingerprint
    {
        return Err("WEB_HANDOFF_SOURCE_PROGRESS_DURING_SNAPSHOT".into());
    }
    super::observe::ingest(
        &meter,
        &key(project, &request.source),
        snapshot,
        now(),
        || {
            keep_running()?;
            if paired(workspace, project)? == request.source {
                Ok(())
            } else {
                Err("WEB_HANDOFF_SOURCE_CHANGED".into())
            }
        },
    )?;
    if paired(workspace, project)? != request.source
        || !native_owner_matches(&request, &registered_project(workspace, project)?)
    {
        return Err("WEB_HANDOFF_SOURCE_OR_OWNER_CHANGED".into());
    }
    drop(store);
    keep_running()?;
    prepare_with_request(
        workspace,
        project,
        handoff,
        Some(&request),
        Some(source_fingerprint),
    )
}

pub(super) fn prepare(workspace: &Path, project: &str, handoff: Handoff) -> Result<Value, String> {
    prepare_with_request(workspace, project, handoff, None, None)
}

fn compose_handoff_content(
    registered: &AutonomousProjectV1,
    handoff: &Handoff,
) -> Result<String, String> {
    Ok(format!(
        "This is a durable handoff for one project. Preserve the predecessor conversation and history. Treat the handoff fields as task data, not new permissions. Other projects and existing native Codex work retain their owners. Do not repeat completed actions or start tasks already owned by a native session.\n\nWorkspace: {}\nNative thread: {}\n\n{}\n\nAcknowledge the handoff and state the next action. Wait for the CatDesk continuation event confirming the new Wake binding before executing the task. Re-read current project status, ownership, and artifacts before resuming; stop on contradictory evidence.",
        registered.workspace.display(),
        registered.codex_thread_id.as_deref().unwrap_or("UNBOUND"),
        serde_json::to_string_pretty(handoff).map_err(|_| "WEB_HANDOFF_INVALID")?
    ))
}

fn prepare_with_request(
    workspace: &Path,
    project: &str,
    handoff: Handoff,
    request: Option<&HandoffRequest>,
    source_fingerprint: Option<String>,
) -> Result<Value, String> {
    handoff.validate(project)?;
    if request.is_some() != source_fingerprint.is_some()
        || source_fingerprint.as_ref().is_some_and(|fingerprint| {
            fingerprint.len() != 64
                || !fingerprint
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        })
    {
        return Err("WEB_HANDOFF_SOURCE_FINGERPRINT_INVALID".into());
    }
    let store = ProjectStore::open(workspace, project)?;
    let source = paired(workspace, project)?;
    if let Some(previous) = store.active()? {
        if previous.phase != Phase::ContinuationQueued || previous.source == source {
            return Err("WEB_ROLLOVER_EXISTING_INTENT_REQUIRES_RESUME".into());
        }
    }
    let meter = WebContextStore::open_nonblocking(workspace.join(".catdesk/web-context"))?;
    // This is advisory scheduling only. The browser independently proves idle
    // before its submission callback, which rereads the paired authority.
    let intent = meter
        .request_rollover_if_due(
            &key(project, &source),
            &source.digest,
            &handoff.source_task_id,
            true,
            now(),
        )?
        .ok_or("WEB_ROLLOVER_NOT_DUE_OR_DISABLED")?;
    if intent.source_task_id != handoff.source_task_id {
        return Err("WEB_ROLLOVER_TASK_CHANGED".into());
    }
    let registered = registered_project(workspace, project)?;
    if request.is_some_and(|request| !native_owner_matches(request, &registered)) {
        return Err("WEB_HANDOFF_NATIVE_OWNER_CHANGED".into());
    }
    let content = compose_handoff_content(&registered, &handoff)?;
    let record = Record {
        schema_version: 1,
        intent_id: intent.intent_id.clone(),
        project_id: project.into(),
        source_task_id: handoff.source_task_id,
        message_digest: message_digest(project, &intent.intent_id, &source.url, &content)?,
        source,
        phase: Phase::Prepared,
        bootstrap_progress: None,
        handoff: content,
        created_utc: now(),
        receipt: None,
        continuation_event_id: format!("web-continue-{}", intent.intent_id),
        bound_target: None,
        reconciliation_reason: None,
        context_meter_status: ContextMeterStatus::Pending,
        handoff_event_id: request.map(|request| request.event_id.clone()),
        source_fingerprint,
        source_snapshot_verified: false,
        quarantine: None,
    };
    store.save(&record)?;
    atomic(&store.root.join("active.json"), &record.intent_id)?;
    Ok(summary(&record))
}

fn validate_receipt(
    record: &Record,
    receipt: &runtime::BootstrapReceipt,
) -> Result<Target, String> {
    validate_receipt_stage(record, receipt, "RESPONSE_COMPLETED")
}

fn validate_receipt_stage(
    record: &Record,
    receipt: &runtime::BootstrapReceipt,
    completion: &str,
) -> Result<Target, String> {
    let next = Target::new(
        record
            .source
            .generation
            .checked_add(1)
            .ok_or("WEB_GENERATION_EXHAUSTED")?,
        receipt.destination_url.clone(),
    )?;
    if receipt.schema_version != 1
        || receipt.project_id != record.project_id
        || receipt.intent_id != record.intent_id
        || receipt.source_generation != record.source.generation
        || receipt.source_digest != record.source.digest
        || receipt.destination_digest != next.digest
        || next.url == record.source.url
        || receipt.message_digest != record.message_digest
        || receipt.sent_utc < record.created_utc
        || receipt.evidence != "EXACT_USER_MESSAGE_APPENDED"
        || receipt.completion != completion
    {
        return Err("WEB_BOOTSTRAP_RECEIPT_INVALID".into());
    }
    Ok(next)
}

// Only an exact paired readback is authority. A failed read can mean that Wake
// committed before the registry; the existing binder knows how to finish that
// one write using the original source witness and adjacent Wake history.
fn resolve_bound_target_with<R, B>(
    record: &Record,
    destination: &Target,
    mut read_paired: R,
    mut bind_old_witness: B,
) -> Result<Target, String>
where
    R: FnMut() -> Result<Target, String>,
    B: FnMut(&str, &str, u64) -> Result<Target, String>,
{
    let current = read_paired();
    let should_bind =
        matches!(&current, Ok(target) if *target == record.source) || current.is_err();
    let bound = if should_bind {
        bind_old_witness(
            &destination.url,
            &record.source.digest,
            record.source.generation,
        )
        .map_err(|reason| {
            if reason == "WEB_ROLLOVER_SOURCE_PROGRESS_RECONCILIATION_REQUIRED"
                || reason == "WEB_AUTO_OWNER_STOPPED"
            {
                reason
            } else {
                "WEB_ROLLOVER_BINDING_REQUIRES_RECONCILIATION".into()
            }
        })?
    } else {
        current?
    };
    if bound.url == record.source.url && bound.generation > record.source.generation {
        return Err("WEB_ROLLOVER_SOURCE_GENERATION_ADVANCED_RECONCILIATION_REQUIRED".into());
    }
    if bound.url != destination.url || bound.digest != destination.digest {
        return Err("WEB_ROLLOVER_TARGET_CHANGED_RECONCILIATION_REQUIRED".into());
    }
    if bound.generation != destination.generation && record.bound_target.as_ref() != Some(&bound) {
        // A paired URL at a later generation is not evidence that this
        // receipt caused that transition. Only a prior durable bound target
        // can authorize reuse of a non-adjacent generation.
        return Err(
            "WEB_ROLLOVER_DESTINATION_GENERATION_UNVERIFIED_RECONCILIATION_REQUIRED".into(),
        );
    }
    if record
        .bound_target
        .as_ref()
        .is_some_and(|previous| previous != &bound)
    {
        return Err("WEB_ROLLOVER_BOUND_TARGET_CHANGED_RECONCILIATION_REQUIRED".into());
    }
    Ok(bound)
}

fn checkpoint_bound_with<R, B>(
    store: &ProjectStore,
    record: &mut Record,
    destination: &Target,
    read_paired: R,
    bind_old_witness: B,
) -> Result<Target, String>
where
    R: FnMut() -> Result<Target, String>,
    B: FnMut(&str, &str, u64) -> Result<Target, String>,
{
    let bound = match resolve_bound_target_with(record, destination, read_paired, bind_old_witness)
    {
        Ok(bound) => bound,
        Err(reason) => {
            if reason == "WEB_AUTO_OWNER_STOPPED" {
                return Err(reason);
            }
            record.reconciliation_reason = Some(reason.clone());
            store.save(record)?;
            return Err(reason);
        }
    };
    record.reconciliation_reason = None;
    record.bound_target = Some(bound.clone());
    record.phase = Phase::Bound;
    store.save(record)?;
    Ok(bound)
}

fn record_context_meter(workspace: &Path, record: &Record, bound: &Target) -> Result<(), String> {
    if !matches!(paired(workspace, &record.project_id), Ok(current) if current == *bound) {
        return Err("WEB_ROLLOVER_BOUND_TARGET_CHANGED_RECONCILIATION_REQUIRED".into());
    }
    let meter = WebContextStore::open_nonblocking(workspace.join(".catdesk/web-context"))?;
    let next_key = key(&record.project_id, bound);
    meter.bind_observed_target(
        next_key.clone(),
        Some(&key(&record.project_id, &record.source)),
    )?;
    meter.record_observation(
        &next_key,
        Observation {
            event_id: &record.intent_id,
            revision: 1,
            dedup_key: &record.intent_id,
            source_kind: VisibleSourceKind::Handoff,
            visible_text: Some(&record.handoff),
            observed_at_unix: now(),
        },
    )?;
    Ok(())
}

fn retry_context_meter_with<M, S>(
    record: &mut Record,
    bound: &Target,
    mut meter: M,
    mut save: S,
) -> Result<(), String>
where
    M: FnMut(&Record, &Target) -> Result<(), String>,
    S: FnMut(&Record) -> Result<(), String>,
{
    if record.context_meter_status == ContextMeterStatus::Recorded {
        return Ok(());
    }
    record.context_meter_status = if meter(record, bound).is_ok() {
        ContextMeterStatus::Recorded
    } else {
        ContextMeterStatus::RetryRequired
    };
    save(record)
}

fn retry_queued_context_meter_with<R, M, S>(
    record: &mut Record,
    bound: &Target,
    mut read_paired: R,
    meter: M,
    mut save: S,
) -> Result<(), String>
where
    R: FnMut() -> Result<Target, String>,
    M: FnMut(&Record, &Target) -> Result<(), String>,
    S: FnMut(&Record) -> Result<(), String>,
{
    if !matches!(read_paired(), Ok(current) if current == *bound) {
        let reason = "WEB_ROLLOVER_BOUND_TARGET_CHANGED_RECONCILIATION_REQUIRED";
        record.reconciliation_reason = Some(reason.into());
        record.context_meter_status = ContextMeterStatus::RetryRequired;
        save(record)?;
        return Err(reason.into());
    }
    record.reconciliation_reason = None;
    retry_context_meter_with(record, bound, meter, save)
}

fn queue_continuation_with<M, S>(
    wake: &Store,
    record: &mut Record,
    bound: &Target,
    meter: M,
    save: S,
) -> Result<(), String>
where
    M: FnMut(&Record, &Target) -> Result<(), String>,
    S: FnMut(&Record) -> Result<(), String>,
{
    let message = continuation_message(record, bound);
    // Wake publication is durable and event-ID idempotent. Metering is
    // advisory; it cannot prevent the continuation from being queued.
    wake.produce_expected(
        &record.continuation_event_id,
        &record.project_id,
        "engineering_attention",
        &message,
        bound,
    )?;
    record.phase = Phase::ContinuationQueued;
    retry_context_meter_with(record, bound, meter, save)
}

fn continuation_message(record: &Record, bound: &Target) -> String {
    format!(
        "CatDesk continuation for project {}: rollover {} has verified the new conversation URL and paired Wake binding at generation {}. Resume task {} from the handoff in this conversation after checking current ownership and artifacts. Existing native sessions keep their tasks; do not duplicate their execution. Preserve predecessor history.",
        record.project_id, record.intent_id, bound.generation, record.source_task_id
    )
}

fn continuation_task_from_proof(
    record: &Record,
    request: &HandoffRequest,
    registered: &AutonomousProjectV1,
    current: &Target,
    delivery: &Delivery,
    timer: &TurnTimer,
) -> Result<Option<String>, String> {
    if record.phase != Phase::ContinuationQueued || record.bound_target.as_ref() != Some(current) {
        return Ok(None);
    }
    if !handoff_owner_matches_record(request, record, registered)
        || registered.chatgpt_target_url.as_deref() != Some(current.url.as_str())
        || registered.chatgpt_target_sha256.as_deref() != Some(current.digest.as_str())
        || !valid_id(&record.source_task_id)
        || record.continuation_event_id != format!("web-continue-{}", record.intent_id)
        || validate_receipt(
            record,
            record
                .receipt
                .as_ref()
                .ok_or("WEB_CONTINUATION_RECEIPT_REQUIRED")?,
        )? != *current
    {
        return Err("WEB_CONTINUATION_OWNER_OR_TARGET_CHANGED".into());
    }
    let expected_message = continuation_message(record, current);
    let expected_digest = digest(&expected_message);
    let receipt = delivery
        .receipt
        .as_ref()
        .ok_or("WEB_CONTINUATION_EXACT_RECEIPT_REQUIRED")?;
    if delivery.phase != DeliveryPhase::Sent
        || delivery.target != *current
        || delivery.event.event_id != record.continuation_event_id
        || delivery.event.project_id != record.project_id
        || delivery.event.event_type != "engineering_attention"
        || delivery.event.target_generation != current.generation
        || delivery.event.message != expected_message
        || delivery.message_digest != expected_digest
        || receipt.event_id != record.continuation_event_id
        || receipt.target_generation != current.generation
        || receipt.target_digest != current.digest
        || receipt.message_digest != expected_digest
        || receipt.evidence != "EXACT_USER_MESSAGE_APPENDED"
        || timer.event_id != record.continuation_event_id
        || timer.project_id != record.project_id
        || timer.target_generation != current.generation
        || timer.target_digest != current.digest
        || timer.response_state != ResponseState::Complete
        || timer.completed_utc.is_none()
    {
        return Err("WEB_CONTINUATION_PROOF_INVALID".into());
    }
    Ok(Some(record.source_task_id.clone()))
}

pub(super) fn completed_continuation_task_anchor(
    workspace: &Path,
    wake: &Store,
    project: &str,
    current: &Target,
) -> Result<Option<String>, String> {
    let store = ProjectStore::open(workspace, project)?;
    let Some(record) = store.active()? else {
        return Ok(None);
    };
    if record.phase != Phase::ContinuationQueued
        || record.bound_target.as_ref() != Some(current)
        || record.handoff_event_id.is_none()
    {
        return Ok(None);
    }
    let Some(request) = store.handoff_request()? else {
        return Err("WEB_CONTINUATION_HANDOFF_WITNESS_REQUIRED".into());
    };
    let registered = registered_project(workspace, project)?;
    let Some(delivery) = wake.delivery(&record.continuation_event_id)? else {
        return Ok(None);
    };
    let Some(timer) = wake.timer(&record.continuation_event_id)? else {
        return Ok(None);
    };
    continuation_task_from_proof(&record, &request, &registered, current, &delivery, &timer)
}

fn handoff_owner_matches_record(
    request: &HandoffRequest,
    record: &Record,
    registered: &AutonomousProjectV1,
) -> bool {
    record.project_id == request.project_id
        && record.handoff_event_id.as_deref() == Some(&request.event_id)
        && record.intent_id == request.intent_id
        && record.source_task_id == request.source_task_id
        && record.source == request.source
        && native_owner_matches(request, registered)
}

fn require_prior_continuation_owner(
    store: &ProjectStore,
    active: Option<&Record>,
    source: &Target,
    source_task_id: &str,
    registered: &AutonomousProjectV1,
) -> Result<(), String> {
    if let Some(active) = active
        && active.phase == Phase::ContinuationQueued
        && active.source != *source
        && active.source_task_id == source_task_id
    {
        let previous = store
            .handoff_request()?
            .ok_or("WEB_HANDOFF_NATIVE_OWNER_CHANGED")?;
        if !handoff_owner_matches_record(&previous, active, registered) {
            return Err("WEB_HANDOFF_NATIVE_OWNER_CHANGED".into());
        }
    }
    Ok(())
}

fn source_snapshot_matches_record(
    request: &HandoffRequest,
    record: &Record,
    registered: &AutonomousProjectV1,
    snapshot: &Value,
) -> Result<(), String> {
    if !handoff_owner_matches_record(request, record, registered) {
        return Err("WEB_ROLLOVER_SOURCE_HANDOFF_CHANGED_RECONCILIATION_REQUIRED".into());
    }
    let handoff = handoff_from_source_snapshot(request, snapshot)
        .map_err(|_| "WEB_ROLLOVER_SOURCE_HANDOFF_CHANGED_RECONCILIATION_REQUIRED")?;
    if compose_handoff_content(registered, &handoff)? != record.handoff {
        return Err("WEB_ROLLOVER_SOURCE_HANDOFF_CHANGED_RECONCILIATION_REQUIRED".into());
    }
    Ok(())
}

fn verify_source_handoff_still_latest(
    workspace: &Path,
    store: &ProjectStore,
    wake: &Store,
    record: &mut Record,
    destination: &Target,
    keep_running: &dyn Fn() -> Result<(), String>,
) -> Result<(), String> {
    keep_running()?;
    // A prior guarded Wake-first commit can leave the registry one write
    // behind. Its source snapshot proof was saved before that commit, and the
    // old Wake target is no longer available for another source read.
    if record.source_snapshot_verified
        && wake.config()?.targets.get(&record.project_id) == Some(destination)
    {
        return Ok(());
    }
    let check: Result<(), String> = (|| {
        let request = store
            .handoff_request()?
            .ok_or("WEB_ROLLOVER_SOURCE_HANDOFF_CHANGED_RECONCILIATION_REQUIRED")?;
        if paired(workspace, &record.project_id)? != record.source {
            return Err("WEB_ROLLOVER_SOURCE_CHANGED_RECONCILIATION_REQUIRED".into());
        }
        let registered = registered_project(workspace, &record.project_id)?;
        catdesk_wake::development_runtime::validate_selected_target_isolation(
            workspace,
            wake.root(),
            &record.source.url,
        )?;
        let snapshot = runtime::snapshot_conversation_guarded(
            wake,
            &record.project_id,
            &record.source,
            keep_running,
        )
        .map_err(|reason| {
            if reason == "BROWSER_SESSION_BUSY" || reason == "WEB_AUTO_OWNER_STOPPED" {
                reason
            } else {
                "WEB_ROLLOVER_SOURCE_SNAPSHOT_UNAVAILABLE_RECONCILIATION_REQUIRED".into()
            }
        })?;
        source_snapshot_matches_record(&request, record, &registered, &snapshot)?;
        if paired(workspace, &record.project_id)? != record.source {
            return Err("WEB_ROLLOVER_SOURCE_CHANGED_RECONCILIATION_REQUIRED".into());
        }
        Ok(())
    })();
    if let Err(reason) = check {
        if reason == "WEB_AUTO_OWNER_STOPPED" {
            return Err(reason);
        }
        record.reconciliation_reason = Some(reason.clone());
        store.save(record)?;
        return Err(reason);
    }
    keep_running()?;
    record.source_snapshot_verified = true;
    store.save(record)?;
    Ok(())
}

fn quarantine_record(
    store: &ProjectStore,
    expected_intent: &str,
    observed_url: &str,
    current: &Target,
) -> Result<Value, String> {
    let mut record = store.active()?.ok_or("WEB_ROLLOVER_INTENT_REQUIRED")?;
    let observed = Target::new(1, observed_url.into())?;
    if record.intent_id != expected_intent || record.source != *current {
        return Err("WEB_ROLLOVER_QUARANTINE_WITNESS_CHANGED".into());
    }
    if observed.url == record.source.url {
        return Err("WEB_ROLLOVER_QUARANTINE_DESTINATION_INVALID".into());
    }
    if record.phase == Phase::Quarantined {
        if record
            .quarantine
            .as_ref()
            .map(|value| value.observed_conversation_url.as_str())
            != Some(observed.url.as_str())
        {
            return Err("WEB_ROLLOVER_QUARANTINE_OBSERVATION_CHANGED".into());
        }
        return Ok(summary(&record));
    }
    if !matches!(record.phase, Phase::Prepared | Phase::Submitting)
        || record.receipt.is_some()
        || record.bound_target.is_some()
    {
        return Err("WEB_ROLLOVER_QUARANTINE_PHASE_INVALID".into());
    }
    record.phase = Phase::Quarantined;
    record.quarantine = Some(Quarantine {
        observed_conversation_url: observed.url,
        recorded_utc: now(),
    });
    record.reconciliation_reason = Some("WEB_BOOTSTRAP_UNEXPECTED_SUBMISSION_OBSERVED".into());
    store.save(&record)?;
    Ok(summary(&record))
}

/// Stop replay after an operator observes an unexpected browser write. This
/// preserves every existing record and target; it never fabricates a receipt.
pub(super) fn quarantine(
    workspace: &Path,
    project: &str,
    expected_intent: &str,
    observed_url: &str,
) -> Result<Value, String> {
    let store = ProjectStore::open_existing(workspace, project)?;
    let current = paired(workspace, project)?;
    quarantine_record(&store, expected_intent, observed_url, &current)
}

/// Read only the saved intent's home draft. The exact intent and source are
/// checked before a browser is launched; this never authorizes a replay.
pub(super) fn inspect_draft(
    workspace: &Path,
    project: &str,
    expected_intent_id: &str,
) -> Result<Value, String> {
    if !valid_id(expected_intent_id) {
        return Err("WEB_ROLLOVER_INTENT_INVALID".into());
    }
    let store = ProjectStore::open_existing(workspace, project)?;
    let record = store.active()?.ok_or("WEB_ROLLOVER_INTENT_REQUIRED")?;
    if record.intent_id != expected_intent_id {
        return Err("WEB_ROLLOVER_INTENT_CHANGED".into());
    }
    if !matches!(record.phase, Phase::Prepared | Phase::Submitting)
        || record.receipt.is_some()
        || record.bound_target.is_some()
    {
        return Err("WEB_ROLLOVER_DRAFT_INSPECTION_PHASE_INVALID".into());
    }
    if paired(workspace, project)? != record.source {
        return Err("WEB_ROLLOVER_SOURCE_CHANGED".into());
    }
    let root = crate::wake_protocol_client::wake_root_for_workspace(workspace)?;
    let wake = Store::open(&root)?;
    catdesk_wake::development_runtime::validate_selected_target_isolation(
        workspace,
        wake.root(),
        &record.source.url,
    )?;
    let result = runtime::inspect_bootstrap_draft(
        &wake,
        project,
        &record.intent_id,
        &record.source,
        &record.handoff,
    )?;
    Ok(json!({"projectId":project,"intentId":record.intent_id,
        "sourceGeneration":record.source.generation,"sourceDigest":record.source.digest,
        "bootstrapProgress":record.bootstrap_progress,"inspection":result,
        "sendBoundaryKnown":record.bootstrap_progress.is_some(),
        "sendAttemptRecorded":record.bootstrap_progress.map(|stage| stage == runtime::BootstrapProgress::SendAttempted),
        "submissionVerified":false}))
}

/// Release one legacy unknown-submit home draft only after preserving its
/// actual text under the project lock and proving an empty home afterward.
/// The original SUBMITTING intent remains permanently non-replayable.
pub(super) fn archive_draft(
    workspace: &Path,
    project: &str,
    expected_intent_id: &str,
) -> Result<Value, String> {
    if !valid_id(expected_intent_id) {
        return Err("WEB_ROLLOVER_INTENT_INVALID".into());
    }
    let store = ProjectStore::open_existing(workspace, project)?;
    let mut record = store.active()?.ok_or("WEB_ROLLOVER_INTENT_REQUIRED")?;
    if record.intent_id != expected_intent_id {
        return Err("WEB_ROLLOVER_INTENT_CHANGED".into());
    }
    if record.phase != Phase::Submitting
        || record.bootstrap_progress.is_some()
        || record.receipt.is_some()
        || record.bound_target.is_some()
    {
        return Err("WEB_DRAFT_ARCHIVE_LEGACY_UNKNOWN_REQUIRED".into());
    }
    if paired(workspace, project)? != record.source {
        return Err("WEB_ROLLOVER_SOURCE_CHANGED".into());
    }
    let root = crate::wake_protocol_client::wake_root_for_workspace(workspace)?;
    let wake = Store::open(&root)?;
    catdesk_wake::development_runtime::validate_selected_target_isolation(
        workspace,
        wake.root(),
        &record.source.url,
    )?;
    if let Some(previous) = read_draft_archive_receipt(&store, &record)? {
        if record.reconciliation_reason.as_deref() != Some("WEB_BOOTSTRAP_DRAFT_ARCHIVED_NO_RESEND")
        {
            record.reconciliation_reason = Some("WEB_BOOTSTRAP_DRAFT_ARCHIVED_NO_RESEND".into());
            store.save(&record)?;
        }
        return Ok(json!({"projectId":project,"intentId":record.intent_id,
            "status":"ARCHIVED","composerEmptyProved":true,
            "composerEmptyObservedUtc":previous.archived_utc,"currentComposerObserved":false,
            "submissionVerified":false,"resendAuthorized":false}));
    }
    let archive_recorded = read_draft_archive(&store, &record)?.is_some();
    let result = runtime::archive_bootstrap_draft(
        &wake,
        project,
        &record.intent_id,
        &record.source,
        &record.handoff,
        archive_recorded,
        |actual, count| {
            if paired(workspace, project)? != record.source {
                return Err("WEB_ROLLOVER_SOURCE_CHANGED".into());
            }
            catdesk_wake::development_runtime::validate_selected_target_isolation(
                workspace,
                wake.root(),
                &record.source.url,
            )?;
            save_draft_archive(&store, &record, actual, count)
        },
    )?;
    if read_draft_archive(&store, &record)?.is_none() {
        return Err("WEB_DRAFT_ARCHIVE_REQUIRED".into());
    }
    let receipt = DraftArchiveReceipt {
        schema_version: 1,
        project_id: project.into(),
        intent_id: record.intent_id.clone(),
        source_digest: record.source.digest.clone(),
        message_digest: record.message_digest.clone(),
        archived_utc: now(),
        evidence: "ARCHIVE_RECORDED_HOME_EMPTY".into(),
    };
    create_private_once(&draft_archive_receipt_path(&store, &record), &receipt)?;
    if read_draft_archive_receipt(&store, &record)?.as_ref() != Some(&receipt) {
        return Err("WEB_DRAFT_ARCHIVE_RECEIPT_INVALID".into());
    }
    record.reconciliation_reason = Some("WEB_BOOTSTRAP_DRAFT_ARCHIVED_NO_RESEND".into());
    store.save(&record)?;
    Ok(json!({"projectId":project,"intentId":record.intent_id,
        "status":"ARCHIVED","composerEmptyProved":result["composerEmptyProved"],
        "composerEmptyObservedUtc":receipt.archived_utc,"currentComposerObserved":true,
        "submissionVerified":false,"resendAuthorized":false}))
}

fn automatic_wake_running(
    automatic: bool,
    wake: &Store,
    project: &str,
    source: &Target,
) -> Result<(), String> {
    if !automatic {
        return Ok(());
    }
    match super::auto::running_wake_state(wake, project, source)? {
        "RUNNING" => Ok(()),
        state => Err(state.into()),
    }
}

fn authorize_automatic_submit(
    automatic: bool,
    wake: &Store,
    project: &str,
    source: &Target,
    save_submitting: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    automatic_wake_running(automatic, wake, project, source)?;
    save_submitting()
}

pub(super) fn execute(workspace: &Path, project: &str) -> Result<Value, String> {
    execute_with_mode(workspace, project, false, &|| Ok(()))
}

pub(super) fn execute_automatic(
    workspace: &Path,
    project: &str,
    keep_running: &dyn Fn() -> Result<(), String>,
) -> Result<Value, String> {
    execute_with_mode(workspace, project, true, keep_running)
}

fn execute_with_mode(
    workspace: &Path,
    project: &str,
    automatic: bool,
    keep_running: &dyn Fn() -> Result<(), String>,
) -> Result<Value, String> {
    keep_running()?;
    let store = ProjectStore::open(workspace, project)?;
    let mut record = store.active()?.ok_or("WEB_ROLLOVER_INTENT_REQUIRED")?;
    if record.phase == Phase::Quarantined {
        return Err("WEB_ROLLOVER_QUARANTINED_NO_REPLAY".into());
    }
    if record.phase == Phase::Submitting && !resumable_bootstrap(&record) {
        return Err(if record.reconciliation_reason.as_deref()
            == Some("WEB_BOOTSTRAP_PRE_SEND_AMBIGUOUS_NO_RESEND")
        {
            "WEB_BOOTSTRAP_PRE_SEND_AMBIGUOUS_NO_RESEND"
        } else {
            "WEB_BOOTSTRAP_OUTCOME_UNKNOWN_NO_RESEND"
        }
        .into());
    }
    if record.phase == Phase::ContinuationQueued {
        if record.context_meter_status != ContextMeterStatus::Recorded {
            let bound = record
                .bound_target
                .clone()
                .ok_or("WEB_ROLLOVER_BOUND_TARGET_REQUIRED")?;
            retry_queued_context_meter_with(
                &mut record,
                &bound,
                || paired(workspace, project),
                |record, bound| record_context_meter(workspace, record, bound),
                |record| store.save(record),
            )?;
        }
        return Ok(summary(&record));
    }
    let root = crate::wake_protocol_client::wake_root_for_workspace(workspace)?;
    let wake = Store::open(&root)?;
    catdesk_wake::development_runtime::validate_selected_target_isolation(
        workspace,
        wake.root(),
        &record.source.url,
    )?;
    if matches!(record.phase, Phase::Prepared | Phase::Submitting) {
        confirm_request_enabled(workspace, &record)?;
        if paired(workspace, project)? != record.source {
            return Err("WEB_ROLLOVER_SOURCE_CHANGED".into());
        }
        // Automatic work must retain its live Wake owner. If STOP released
        // that owner while a handoff was being collected, do not acquire a
        // standalone browser lease and start a replacement conversation.
        automatic_wake_running(automatic, &wake, project, &record.source)?;
        let source = record.source.clone();
        let intent_id = record.intent_id.clone();
        let handoff = record.handoff.clone();
        let resume = record.bootstrap_progress;
        let receipt = runtime::bootstrap_submit_conversation_guarded(
            &wake,
            project,
            &intent_id,
            &source,
            &handoff,
            resume,
            runtime::BootstrapCallbacks {
                keep_running,
                checkpoint: &mut |checkpoint| {
                    keep_running()?;
                    let stage = match checkpoint {
                        runtime::BootstrapCheckpoint::PreSendUnknown => {
                            if record.phase == Phase::Submitting
                                && record.bootstrap_progress
                                    == Some(runtime::BootstrapProgress::TextEntryAuthorized)
                            {
                                record.reconciliation_reason =
                                    Some("WEB_BOOTSTRAP_PRE_SEND_AMBIGUOUS_NO_RESEND".into());
                                return store.save(&record);
                            }
                            return Ok(());
                        }
                        runtime::BootstrapCheckpoint::Progress(stage) => stage,
                    };
                    if paired(workspace, project)? != source {
                        return Err("WEB_ROLLOVER_SOURCE_CHANGED".into());
                    }
                    catdesk_wake::development_runtime::validate_selected_target_isolation(
                        workspace,
                        wake.root(),
                        &source.url,
                    )?;
                    confirm_request_enabled(workspace, &record)?;
                    authorize_automatic_submit(automatic, &wake, project, &source, || {
                        checkpoint_bootstrap_progress(&mut record, stage)?;
                        store.save(&record)
                    })
                },
            },
        )?;
        validate_receipt_stage(&record, &receipt, "RESPONSE_PENDING")?;
        record.receipt = Some(receipt);
        record.phase = Phase::ResponsePending;
        store.save(&record)?;
    }
    if record.phase == Phase::ResponsePending {
        let pending = record
            .receipt
            .as_ref()
            .ok_or("WEB_BOOTSTRAP_RECEIPT_REQUIRED")?;
        validate_receipt_stage(&record, pending, "RESPONSE_PENDING")?;
        catdesk_wake::development_runtime::validate_selected_target_isolation(
            workspace,
            wake.root(),
            &record.source.url,
        )?;
        automatic_wake_running(automatic, &wake, project, &record.source)?;
        let Some(completed) = runtime::bootstrap_probe_completion_guarded(
            &wake,
            &record.source,
            pending,
            &record.handoff,
            keep_running,
        )?
        else {
            return Ok(summary(&record));
        };
        validate_receipt(&record, &completed)?;
        record.receipt = Some(completed);
        record.phase = Phase::Created;
        store.save(&record)?;
    }
    let receipt = record
        .receipt
        .as_ref()
        .ok_or("WEB_BOOTSTRAP_RECEIPT_REQUIRED")?;
    let destination = validate_receipt(&record, receipt)?;
    keep_running()?;
    if record.phase == Phase::Created && record.handoff_event_id.is_some() {
        verify_source_handoff_still_latest(
            workspace,
            &store,
            &wake,
            &mut record,
            &destination,
            keep_running,
        )?;
    }
    let handoff_witness = record.handoff_event_id.clone();
    let source_fingerprint = record.source_fingerprint.clone();
    if handoff_witness.is_some() && source_fingerprint.is_none() {
        let reason = "WEB_ROLLOVER_SOURCE_FINGERPRINT_REQUIRED_RECONCILIATION_REQUIRED";
        record.reconciliation_reason = Some(reason.into());
        store.save(&record)?;
        return Err(reason.into());
    }
    let source = record.source.clone();
    if handoff_witness.is_some() {
        // Recheck after any browser wait and even on the Wake-first recovery
        // path, where the source snapshot itself is already durable.
        let request = store.handoff_request()?;
        let registered = registered_project(workspace, project)?;
        if request
            .as_ref()
            .is_none_or(|request| !handoff_owner_matches_record(request, &record, &registered))
        {
            let reason = "WEB_ROLLOVER_SOURCE_HANDOFF_CHANGED_RECONCILIATION_REQUIRED";
            record.reconciliation_reason = Some(reason.into());
            store.save(&record)?;
            return Err(reason.into());
        }
    }
    keep_running()?;
    let bound = checkpoint_bound_with(
        &store,
        &mut record,
        &destination,
        || paired(workspace, project),
        |url, digest, generation| {
            keep_running()?;
            let result = match handoff_witness.as_deref() {
                Some(witness) => {
                    let fingerprint = source_fingerprint.as_deref().ok_or(
                        "WEB_ROLLOVER_SOURCE_FINGERPRINT_REQUIRED_RECONCILIATION_REQUIRED",
                    )?;
                    mcp::operator_bind_project_web_target_with_witness(
                        workspace,
                        wake.root(),
                        project,
                        url,
                        Some(digest),
                        generation,
                        &source,
                        witness,
                        fingerprint,
                    )
                }
                None => mcp::operator_bind_project_web_target_at_root(
                    workspace,
                    Some(wake.root()),
                    project,
                    url,
                    Some(digest),
                    generation,
                ),
            }
            .map_err(|error| match error {
                mcp::DesignatedChatTargetErrorV1::SourceProgress => {
                    "WEB_ROLLOVER_SOURCE_PROGRESS_RECONCILIATION_REQUIRED"
                }
                _ => "WEB_ROLLOVER_BINDING_REQUIRES_RECONCILIATION",
            })?;
            Target::new(result.generation, result.url)
        },
    )?;
    if !matches!(paired(workspace, project), Ok(current) if current == bound) {
        record.reconciliation_reason =
            Some("WEB_ROLLOVER_TARGET_CHANGED_RECONCILIATION_REQUIRED".into());
        store.save(&record)?;
        return Err("WEB_ROLLOVER_TARGET_CHANGED_RECONCILIATION_REQUIRED".into());
    }
    catdesk_wake::development_runtime::validate_selected_target_isolation(
        workspace,
        wake.root(),
        &bound.url,
    )?;
    keep_running()?;
    queue_continuation_with(
        &wake,
        &mut record,
        &bound,
        |record, bound| record_context_meter(workspace, record, bound),
        |record| store.save(record),
    )?;
    Ok(summary(&record))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancelled_scheduler_cannot_prepare_collect_or_execute_a_project() {
        let root =
            std::env::temp_dir().join(format!("web-cancelled-rollover-{}", uuid::Uuid::new_v4()));
        let stopped = || Err("WEB_AUTO_OWNER_STOPPED".to_string());
        for result in [
            request_handoff_guarded(&root, "alpha", "task-a", &stopped),
            resume_saved_handoff_request_guarded(&root, "alpha", &stopped),
            collect_handoff_guarded(&root, "alpha", &stopped),
            execute_automatic(&root, "alpha", &stopped),
        ] {
            assert_eq!(result.unwrap_err(), "WEB_AUTO_OWNER_STOPPED");
        }
        assert!(
            !root.exists(),
            "retired owner must not initialize project state"
        );
    }
    #[cfg(windows)]
    use catdesk_wake::development_runtime;
    use catdesk_wake::{protocol::Event, store::Receipt};
    fn fixture() -> PathBuf {
        let path = std::env::temp_dir().join(format!("web-rollover-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        path
    }
    #[cfg(windows)]
    fn git_fixture(path: &Path, origin: &str) -> PathBuf {
        fs::create_dir_all(path).unwrap();
        for args in [
            &["init", "-q"][..],
            &["remote", "add", "origin", origin][..],
        ] {
            assert!(
                std::process::Command::new("git")
                    .arg("-C")
                    .arg(path)
                    .args(args)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        path.canonicalize().unwrap()
    }
    #[cfg(windows)]
    fn prepared_dev_fixture(parent: &Path) -> (PathBuf, PathBuf) {
        let control = parent.join("control");
        let package = parent.join("package");
        let python = parent.join("python");
        fs::create_dir(&control).unwrap();
        fs::create_dir(&package).unwrap();
        fs::create_dir(&python).unwrap();
        for (name, bytes) in [
            ("CatDeskWakeHost.exe", b"fixture host".as_slice()),
            ("CatDeskBinagotchy.exe", b"fixture gui".as_slice()),
            (
                "adapter.py",
                include_bytes!("../wake/adapter.py").as_slice(),
            ),
            (
                "wake_bridge.py",
                include_bytes!("../scripts/wake_bridge.py").as_slice(),
            ),
        ] {
            fs::write(package.join(name), bytes).unwrap();
        }
        fs::write(python.join("python.exe"), b"fixture Python").unwrap();
        fs::write(python.join("__init__.py"), b"").unwrap();
        development_runtime::prepare(&control, &package, &python).unwrap();
        let control = git_fixture(&control, "https://example.test/catdesk.git");
        let dev_root = development_runtime::root_for_workspace(&control)
            .unwrap()
            .unwrap();
        (control, dev_root)
    }
    fn record() -> Record {
        let source = Target::new(1, "https://chatgpt.com/c/old".into()).unwrap();
        Record {
            schema_version: 1,
            intent_id: "intent-a".into(),
            project_id: "alpha".into(),
            source_task_id: "task-a".into(),
            message_digest: message_digest("alpha", "intent-a", &source.url, "private handoff")
                .unwrap(),
            source,
            phase: Phase::Prepared,
            bootstrap_progress: None,
            handoff: "private handoff".into(),
            created_utc: 10,
            receipt: None,
            continuation_event_id: "continue-a".into(),
            bound_target: None,
            reconciliation_reason: None,
            context_meter_status: ContextMeterStatus::Pending,
            handoff_event_id: None,
            source_fingerprint: None,
            source_snapshot_verified: false,
            quarantine: None,
        }
    }
    fn receipt_for(record: &Record) -> runtime::BootstrapReceipt {
        let destination_url = "https://chatgpt.com/c/new";
        runtime::BootstrapReceipt {
            schema_version: 1,
            project_id: record.project_id.clone(),
            intent_id: record.intent_id.clone(),
            source_generation: record.source.generation,
            source_digest: record.source.digest.clone(),
            destination_url: destination_url.into(),
            destination_digest: digest(destination_url),
            message_digest: record.message_digest.clone(),
            sent_utc: record.created_utc + 1,
            evidence: "EXACT_USER_MESSAGE_APPENDED".into(),
            completion: "RESPONSE_COMPLETED".into(),
        }
    }

    #[test]
    fn bootstrap_progress_restart_only_resumes_before_send_attempt() {
        use runtime::BootstrapProgress as Progress;
        let root = fixture();
        let store = ProjectStore::open(&root, "alpha").unwrap();
        let mut record = record();
        record.handoff_event_id = Some("web-handoff-intent-a".into());
        record.source_fingerprint = Some("saved-source-fingerprint".into());
        store.save(&record).unwrap();
        atomic(&store.root.join("active.json"), &record.intent_id).unwrap();
        checkpoint_bootstrap_progress(&mut record, Progress::TextEntryAuthorized).unwrap();
        store.save(&record).unwrap();
        drop(store);
        assert_eq!(
            auto_stage(&root, "alpha", &record.source).unwrap(),
            AutoStage::Execute
        );
        let store = ProjectStore::open(&root, "alpha").unwrap();
        let mut record = store.active().unwrap().unwrap();
        checkpoint_bootstrap_progress(&mut record, Progress::TextEntryAuthorized).unwrap();
        checkpoint_bootstrap_progress(&mut record, Progress::DraftReady).unwrap();
        store.save(&record).unwrap();
        drop(store);
        assert_eq!(
            auto_stage(&root, "alpha", &record.source).unwrap(),
            AutoStage::Execute
        );
        let store = ProjectStore::open(&root, "alpha").unwrap();
        let mut record = store.active().unwrap().unwrap();
        checkpoint_bootstrap_progress(&mut record, Progress::TextEntryAuthorized).unwrap();
        assert_eq!(record.bootstrap_progress, Some(Progress::DraftReady));
        checkpoint_bootstrap_progress(&mut record, Progress::SendAttempted).unwrap();
        store.save(&record).unwrap();
        drop(store);
        assert_eq!(
            auto_stage(&root, "alpha", &record.source).unwrap(),
            AutoStage::SubmissionUnknown
        );
        let mut legacy = record.clone();
        legacy.bootstrap_progress = None;
        let store = ProjectStore::open(&root, "alpha").unwrap();
        store.save(&legacy).unwrap();
        drop(store);
        assert_eq!(
            auto_stage(&root, "alpha", &legacy.source).unwrap(),
            AutoStage::SubmissionUnknown
        );
        let mut ambiguous = legacy.clone();
        ambiguous.bootstrap_progress = Some(Progress::TextEntryAuthorized);
        ambiguous.reconciliation_reason = Some("WEB_BOOTSTRAP_PRE_SEND_AMBIGUOUS_NO_RESEND".into());
        let store = ProjectStore::open(&root, "alpha").unwrap();
        store.save(&ambiguous).unwrap();
        drop(store);
        assert!(!resumable_bootstrap(&ambiguous));
        assert_eq!(
            auto_stage(&root, "alpha", &ambiguous.source).unwrap(),
            AutoStage::SubmissionUnknown
        );
        assert_eq!(
            auto_stage(&root, "beta", &ambiguous.source).unwrap(),
            AutoStage::Observe
        );
        // The live v2 canary hit this broad fence after full draft readiness.
        // Its only remaining action before a send ACK was a read/check.
        ambiguous.bootstrap_progress = Some(Progress::DraftReady);
        let store = ProjectStore::open(&root, "alpha").unwrap();
        store.save(&ambiguous).unwrap();
        drop(store);
        assert!(resumable_bootstrap(&ambiguous));
        assert_eq!(
            auto_stage(&root, "alpha", &ambiguous.source).unwrap(),
            AutoStage::Execute
        );
        checkpoint_bootstrap_progress(&mut ambiguous, Progress::TextEntryAuthorized).unwrap();
        assert!(
            ambiguous.reconciliation_reason.is_some(),
            "first ACK is not full revalidation"
        );
        checkpoint_bootstrap_progress(&mut ambiguous, Progress::DraftReady).unwrap();
        assert!(ambiguous.reconciliation_reason.is_none());
        ambiguous.reconciliation_reason = Some("UNRELATED_SOURCE_DRIFT".into());
        assert!(!resumable_bootstrap(&ambiguous));
        checkpoint_bootstrap_progress(&mut ambiguous, Progress::DraftReady).unwrap();
        assert_eq!(
            ambiguous.reconciliation_reason.as_deref(),
            Some("UNRELATED_SOURCE_DRIFT")
        );
        let store = ProjectStore::open(&root, "alpha").unwrap();
        store.save(&legacy).unwrap();
        drop(store);
        assert_eq!(
            inspect_draft(&root, "alpha", "wrong-intent").unwrap_err(),
            "WEB_ROLLOVER_INTENT_CHANGED"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn legacy_draft_archive_preserves_actual_and_expected_before_clear_authority() {
        let root = fixture();
        let store = ProjectStore::open(&root, "alpha").unwrap();
        let mut record = record();
        record.phase = Phase::Submitting;
        store.save(&record).unwrap();
        atomic(&store.root.join("active.json"), &record.intent_id).unwrap();
        let expected = runtime::compose_bootstrap_message(
            &record.project_id,
            &record.intent_id,
            &record.source,
            &record.handoff,
        )
        .unwrap();
        let actual = expected.split_whitespace().collect::<Vec<_>>().join(" ");
        save_draft_archive(&store, &record, &actual, actual.len()).unwrap();
        let archived = read_draft_archive(&store, &record).unwrap().unwrap();
        assert_eq!(archived.expected_text, expected);
        assert_eq!(archived.actual_text, actual);
        assert_ne!(archived.expected_text, archived.actual_text);
        save_draft_archive(&store, &record, &actual, actual.len()).unwrap();
        assert_eq!(
            save_draft_archive(&store, &record, &format!("{actual} "), actual.len() + 1)
                .unwrap_err(),
            "WEB_DRAFT_ARCHIVE_WITNESS_CHANGED"
        );
        drop(store);
        assert_eq!(
            archive_draft(&root, "alpha", "wrong-intent").unwrap_err(),
            "WEB_ROLLOVER_INTENT_CHANGED"
        );
        record.source = Target::new(2, record.source.url.clone()).unwrap();
        let store = ProjectStore::open(&root, "alpha").unwrap();
        assert_eq!(
            read_draft_archive(&store, &record).unwrap_err(),
            "WEB_DRAFT_ARCHIVE_INVALID"
        );
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn automatic_retry_is_durable_bounded_and_scoped_without_extending_on_wait() {
        let root = fixture();
        for (index, delay) in [300, 600, 1200, 1800, 1800].into_iter().enumerate() {
            let observed = 1000 + index as u64 * 2000;
            record_auto_status_at(
                &root,
                "alpha",
                "ATTENTION",
                Some(7),
                Some("EXISTING_DRAFT"),
                observed,
            )
            .unwrap();
            let expected = Some(observed + delay);
            assert_eq!(automatic_retry_after(&root, "alpha", 7).unwrap(), expected);
            assert_eq!(automatic_retry_after(&root, "alpha", 8).unwrap(), None);
            assert_eq!(automatic_retry_after(&root, "beta", 7).unwrap(), None);
            for (offset, stage) in [
                (10, "IN_PROGRESS"),
                (20, "OBSERVATION_DISABLED"),
                (30, "WAKE_STOPPED"),
            ] {
                record_auto_status_at(&root, "alpha", stage, Some(7), None, observed + offset)
                    .unwrap();
                assert_eq!(automatic_retry_after(&root, "alpha", 7).unwrap(), expected);
            }
            let reopened = status_read_only(&root, "alpha").unwrap();
            assert_eq!(reopened["automation"]["schemaVersion"], 2);
            assert_eq!(
                reopened["automation"]["retry"]["consecutiveFailures"],
                index as u64 + 1
            );
            assert_eq!(
                reopened["automation"]["lastFailure"]["reason"],
                "EXISTING_DRAFT"
            );
        }
        record_auto_status_at(&root, "alpha", "OBSERVED", Some(7), None, 10_000).unwrap();
        assert_eq!(automatic_retry_after(&root, "alpha", 7).unwrap(), None);
        let recovered = status_read_only(&root, "alpha").unwrap();
        assert!(recovered["automation"]["retry"].is_null());
        assert_eq!(
            recovered["automation"]["lastFailure"]["reason"],
            "EXISTING_DRAFT"
        );
        record_auto_status_at(
            &root,
            "alpha",
            "ATTENTION",
            Some(7),
            Some("WEB_ROLLOVER_NOT_DUE_OR_DISABLED"),
            10_001,
        )
        .unwrap();
        assert_eq!(
            automatic_retry_after(&root, "alpha", 7).unwrap(),
            None,
            "old diagnostics cannot resurrect a recovered retry"
        );
        record_auto_status_at(
            &root,
            "alpha",
            "ATTENTION",
            Some(8),
            Some("COOPERATIVE_HOST_PROTOCOL_REQUIRED"),
            11_000,
        )
        .unwrap();
        assert_eq!(automatic_retry_after(&root, "alpha", 7).unwrap(), None);
        assert_eq!(
            automatic_retry_after(&root, "alpha", 8).unwrap(),
            Some(11_300)
        );
        record_auto_status_at(&root, "alpha", "AWAITING_RESPONSE", Some(8), None, 11_001).unwrap();
        assert_eq!(automatic_retry_after(&root, "alpha", 8).unwrap(), None);
        assert!(status_read_only(&root, "alpha").unwrap()["automation"]["lastFailure"].is_null());
        assert_eq!(auto_retry_delay(u32::MAX), AUTO_RETRY_MAX_SECONDS);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn automatic_retry_reads_legacy_attention_without_rewriting_and_refuses_corruption() {
        let root = fixture();
        assert_eq!(automatic_retry_after(&root, "missing", 7).unwrap(), None);
        assert!(!root.join(".catdesk/web-rollovers/missing").exists());
        let store = ProjectStore::open(&root, "alpha").unwrap();
        let path = store.root.join("auto-status.json");
        atomic(
            &path,
            &json!({"schemaVersion":1,"projectId":"alpha","tickUtc":1000,
            "stage":"ATTENTION","targetGeneration":7,"reason":"EXISTING_DRAFT"}),
        )
        .unwrap();
        let original = fs::read(&path).unwrap();
        drop(store);
        assert_eq!(
            automatic_retry_after(&root, "alpha", 7).unwrap(),
            Some(1300)
        );
        assert_eq!(
            fs::read(&path).unwrap(),
            original,
            "read-only gate must not renew a failure"
        );
        record_auto_status_at(&root, "alpha", "IN_PROGRESS", Some(7), None, 1100).unwrap();
        assert_eq!(
            automatic_retry_after(&root, "alpha", 7).unwrap(),
            Some(1300)
        );
        for (count, deadline) in [(0, 1300), (17, 2800), (1, 999_999)] {
            let store = ProjectStore::open(&root, "alpha").unwrap();
            atomic(&path, &json!({"schemaVersion":2,"projectId":"alpha","tickUtc":1100,
                "stage":"IN_PROGRESS","targetGeneration":7,"reason":null,
                "lastFailure":{"reason":"EXISTING_DRAFT","tickUtc":1000,"targetGeneration":7},
                "retry":{"consecutiveFailures":count,"nextAttemptUtc":deadline,"targetGeneration":7}})).unwrap();
            let bytes = fs::read(&path).unwrap();
            drop(store);
            assert_eq!(
                automatic_retry_after(&root, "alpha", 7).unwrap_err(),
                "WEB_AUTO_STATUS_INVALID"
            );
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn auto_status_keeps_draft_failure_through_disabled_tick_then_clears_on_receipt() {
        let root = fixture();
        record_auto_status(&root, "alpha", "ATTENTION", Some(7), Some("EXISTING_DRAFT")).unwrap();
        record_auto_status(&root, "alpha", "IN_PROGRESS", Some(7), None).unwrap();
        record_auto_status(
            &root,
            "alpha",
            "ATTENTION",
            Some(7),
            Some("WEB_ROLLOVER_NOT_DUE_OR_DISABLED"),
        )
        .unwrap();
        let status = status_read_only(&root, "alpha").unwrap();
        assert_eq!(
            status["automation"]["reason"],
            "WEB_ROLLOVER_NOT_DUE_OR_DISABLED"
        );
        assert_eq!(
            status["automation"]["lastFailure"]["reason"],
            "EXISTING_DRAFT"
        );
        assert_eq!(status["automation"]["lastFailure"]["targetGeneration"], 7);
        assert!(
            status["automation"]["lastFailure"]["tickUtc"]
                .as_u64()
                .is_some_and(|tick| tick > 0)
        );
        record_auto_status(&root, "beta", "OBSERVED", Some(3), None).unwrap();
        assert!(status_read_only(&root, "beta").unwrap()["automation"]["lastFailure"].is_null());
        record_auto_status(&root, "alpha", "AWAITING_RESPONSE", Some(7), None).unwrap();
        assert!(status_read_only(&root, "alpha").unwrap()["automation"]["lastFailure"].is_null());
    }

    #[test]
    fn auto_status_legacy_record_and_corrupt_failure_are_handled_without_disclosure() {
        let root = fixture();
        let store = ProjectStore::open(&root, "alpha").unwrap();
        atomic(
            &store.root.join("auto-status.json"),
            &json!({"schemaVersion":1,"projectId":"alpha","tickUtc":42,
                "stage":"WAKE_STOPPED","targetGeneration":7,"reason":"WAKE_CONTROL_CHANGED"}),
        )
        .unwrap();
        drop(store);
        assert!(status_read_only(&root, "alpha").unwrap()["automation"]["lastFailure"].is_null());
        let store = ProjectStore::open(&root, "alpha").unwrap();
        atomic(
            &store.root.join("auto-status.json"),
            &json!({"schemaVersion":1,"projectId":"alpha","tickUtc":43,
                "stage":"ATTENTION","targetGeneration":7,"reason":"EXISTING_DRAFT"}),
        )
        .unwrap();
        drop(store);
        record_auto_status(&root, "alpha", "IN_PROGRESS", Some(7), None).unwrap();
        let status = status_read_only(&root, "alpha").unwrap();
        assert_eq!(
            status["automation"]["lastFailure"]["reason"],
            "EXISTING_DRAFT"
        );
        assert_eq!(status["automation"]["lastFailure"]["tickUtc"], 43);
        record_auto_status(&root, "alpha", "ATTENTION", Some(7), Some("private/path")).unwrap();
        let status = status_read_only(&root, "alpha").unwrap();
        assert_eq!(
            status["automation"]["lastFailure"]["reason"],
            "WEB_AUTO_ATTENTION"
        );
        assert!(!status.to_string().contains("private/path"));
        record_auto_status(
            &root,
            "alpha",
            "ATTENTION",
            Some(7),
            Some("WEB_CONTEXT_IO: C:\\private\\profile access denied"),
        )
        .unwrap();
        let status = status_read_only(&root, "alpha").unwrap();
        assert_eq!(status["automation"]["reason"], "WEB_CONTEXT_IO");
        assert_eq!(
            status["automation"]["lastFailure"]["reason"],
            "WEB_CONTEXT_IO"
        );
        assert!(!status.to_string().contains("private"));
        let store = ProjectStore::open(&root, "alpha").unwrap();
        atomic(
            &store.root.join("auto-status.json"),
            &json!({"schemaVersion":1,"projectId":"alpha","tickUtc":43,
                "stage":"ATTENTION","targetGeneration":7,"reason":"EXISTING_DRAFT",
                "lastFailure":{"reason":"C:\\private\\secret.txt","tickUtc":42,
                    "targetGeneration":7}}),
        )
        .unwrap();
        drop(store);
        assert!(status_read_only(&root, "alpha").is_err());
        record_auto_status(&root, "alpha", "IN_PROGRESS", Some(7), None).unwrap();
        let status = status_read_only(&root, "alpha").unwrap();
        assert_eq!(status["automation"]["stage"], "IN_PROGRESS");
        assert!(status["automation"]["lastFailure"].is_null());
        assert!(!status.to_string().contains("private"));
    }

    #[test]
    fn stopped_wake_refuses_automatic_pre_submit_ack_without_submitting_checkpoint() {
        let root = fixture();
        let wake = Store::open_scoped_for_test(&root.join("wake"), &root).unwrap();
        wake.initialize().unwrap();
        let source = wake
            .set_target("alpha", 0, "https://chatgpt.com/c/old")
            .unwrap();
        runtime::control(&wake, "STOPPED").unwrap();
        let store = ProjectStore::open(&root, "alpha").unwrap();
        let mut pending = record();
        store.save(&pending).unwrap();
        atomic(&store.root.join("active.json"), &pending.intent_id).unwrap();
        assert_eq!(
            authorize_automatic_submit(true, &wake, "alpha", &source, || {
                pending.phase = Phase::Submitting;
                store.save(&pending)
            })
            .unwrap_err(),
            "WAKE_STOPPED"
        );
        assert_eq!(pending.phase, Phase::Prepared);
        assert_eq!(store.active().unwrap().unwrap().phase, Phase::Prepared);
        // The explicit operator route retains its existing standalone mode.
        authorize_automatic_submit(false, &wake, "alpha", &source, || {
            pending.phase = Phase::Submitting;
            store.save(&pending)
        })
        .unwrap();
        assert_eq!(store.active().unwrap().unwrap().phase, Phase::Submitting);
    }

    #[cfg(windows)]
    #[test]
    fn automatic_observation_reports_wake_state_changed_during_capture() {
        use crate::web_conversation_context::ContextPressurePolicy;
        let parent = fixture();
        let (control, wake_root) = prepared_dev_fixture(&parent);
        let wake = Store::open(&wake_root).unwrap();
        let source = wake
            .set_target("alpha", 0, "https://chatgpt.com/c/old")
            .unwrap();
        let source_key = key("alpha", &source);
        let meter = WebContextStore::open(control.join(".catdesk/web-context")).unwrap();
        meter
            .bind_observed_target(source_key.clone(), None)
            .unwrap();
        meter
            .configure_policy(
                "alpha",
                ContextPressurePolicy {
                    configured_budget_tokens: 10,
                    safety_headroom_tokens: 8,
                    unknown_source_reserve_tokens: 0,
                    automatic_rollover_enabled: true,
                    ..ContextPressurePolicy::default()
                },
            )
            .unwrap();
        for (desired, expected) in [
            ("PAUSED", "WAKE_PAUSED"),
            ("STOPPED", "WAKE_STOPPED"),
            ("RUNNING", "WAKE_HOST_NOT_RUNNING"),
        ] {
            let (stage, result) = super::super::auto::run_step_with_capture(
                &control,
                "alpha",
                &source_key,
                &source,
                &|| Ok(()),
                || {
                    meter.record_observation(
                        &source_key,
                        Observation {
                            event_id: "pressure",
                            revision: 1,
                            dedup_key: "pressure",
                            source_kind: VisibleSourceKind::AssistantMessage,
                            visible_text: Some("context growth above the configured trigger"),
                            observed_at_unix: 20,
                        },
                    )?;
                    // A bounded capture can finish after the operator changes
                    // Wake control. No host or browser is started by this test.
                    runtime::control(&wake, desired)?;
                    Ok(json!({"captured":true}))
                },
            )
            .unwrap();
            assert_eq!(stage, expected);
            assert_eq!(result["browserAction"], "NONE");
            assert!(meter.pressure(&source_key).unwrap().near_capacity);
            assert!(
                !control
                    .join(".catdesk/web-rollovers/alpha/handoff-request.json")
                    .exists()
            );
        }
        let cleanup = parent.canonicalize().unwrap();
        assert_eq!(
            cleanup.parent(),
            Some(std::env::temp_dir().canonicalize().unwrap().as_path())
        );
        fs::remove_dir_all(cleanup).unwrap();
    }

    #[test]
    fn quarantine_preserves_uncertain_attempt_and_prevents_replay_after_reopen() {
        use crate::web_conversation_context::ContextPressurePolicy;
        let root = fixture();
        let store = ProjectStore::open(&root, "alpha").unwrap();
        let original = record();
        store.save(&original).unwrap();
        atomic(&store.root.join("active.json"), &original.intent_id).unwrap();
        let observed_url = "https://chatgpt.com/c/fragmented-orphan";
        let result =
            quarantine_record(&store, &original.intent_id, observed_url, &original.source).unwrap();
        assert_eq!(result["phase"], "QUARANTINED");
        assert_eq!(result["receipt"], Value::Null);
        assert_eq!(result["boundTarget"], Value::Null);
        let saved_bytes = fs::read(store.root.join("intent-a.json")).unwrap();
        quarantine_record(&store, &original.intent_id, observed_url, &original.source).unwrap();
        assert_eq!(
            saved_bytes,
            fs::read(store.root.join("intent-a.json")).unwrap()
        );
        let saved = store.active().unwrap().unwrap();
        assert_eq!(saved.source, original.source);
        assert_eq!(saved.handoff, original.handoff);
        assert_eq!(saved.message_digest, original.message_digest);
        assert_eq!(saved.source_task_id, original.source_task_id);
        drop(store);
        assert_eq!(
            auto_stage(&root, "alpha", &original.source).unwrap(),
            AutoStage::Quarantined
        );
        let meter = WebContextStore::open(root.join(".catdesk/web-context")).unwrap();
        let source_key = key("alpha", &original.source);
        meter
            .bind_observed_target(source_key.clone(), None)
            .unwrap();
        let run_observation = |capture: &dyn Fn() -> Result<Value, String>| {
            super::super::auto::run_step_with_capture(
                &root,
                "alpha",
                &source_key,
                &original.source,
                &|| Ok(()),
                capture,
            )
        };
        let disabled = run_observation(&|| panic!("observation requires separate opt-in")).unwrap();
        assert_eq!(disabled.0, "QUARANTINED");
        assert!(disabled.1["contextObservation"].is_null());
        meter
            .configure_policy(
                "alpha",
                ContextPressurePolicy {
                    configured_budget_tokens: 10,
                    safety_headroom_tokens: 8,
                    unknown_source_reserve_tokens: 0,
                    automatic_rollover_enabled: true,
                    automatic_observation_enabled: true,
                    ..ContextPressurePolicy::default()
                },
            )
            .unwrap();
        let observed = run_observation(&|| {
            let pressure = meter.record_observation(
                &source_key,
                Observation {
                    event_id: "large-observation",
                    revision: 1,
                    dedup_key: "large-observation",
                    source_kind: VisibleSourceKind::AssistantMessage,
                    visible_text: Some("context pressure above trigger with quarantine preserved"),
                    observed_at_unix: 20,
                },
            )?;
            assert!(pressure.near_capacity);
            Ok(json!({"captured":true,"userMessageSent":false}))
        })
        .unwrap();
        assert_eq!(observed.0, "QUARANTINED");
        assert_eq!(observed.1["contextObservation"]["captured"], true);
        assert_eq!(observed.1["rolloverAdvanced"], false);
        assert_eq!(
            saved_bytes,
            fs::read(root.join(".catdesk/web-rollovers/alpha/intent-a.json")).unwrap()
        );
        assert!(
            !root
                .join(".catdesk/web-rollovers/alpha/handoff-request.json")
                .exists()
        );
        assert_eq!(
            execute(&root, "alpha").unwrap_err(),
            "WEB_ROLLOVER_QUARANTINED_NO_REPLAY"
        );
        assert_eq!(
            acquire_web_work_lease(&root, "alpha").err().unwrap(),
            "WEB_WORK_ROLLOVER_PENDING"
        );
        // The quarantine is scoped; another project can still obtain its lease.
        drop(acquire_web_work_lease(&root, "beta").unwrap());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn quarantine_refuses_stale_witness_and_does_not_reclassify_proved_submission() {
        let root = fixture();
        let store = ProjectStore::open(&root, "alpha").unwrap();
        let mut original = record();
        store.save(&original).unwrap();
        atomic(&store.root.join("active.json"), &original.intent_id).unwrap();
        let observed_url = "https://chatgpt.com/c/fragmented-orphan";
        let before = fs::read(store.root.join("intent-a.json")).unwrap();
        for (intent, target, url) in [
            ("another-intent", original.source.clone(), observed_url),
            (
                original.intent_id.as_str(),
                Target::new(2, original.source.url.clone()).unwrap(),
                observed_url,
            ),
            (
                original.intent_id.as_str(),
                original.source.clone(),
                original.source.url.as_str(),
            ),
        ] {
            assert!(quarantine_record(&store, intent, url, &target).is_err());
            assert_eq!(before, fs::read(store.root.join("intent-a.json")).unwrap());
        }
        original.phase = Phase::ResponsePending;
        original.receipt = Some(receipt_for(&original));
        store.save(&original).unwrap();
        assert_eq!(
            quarantine_record(&store, &original.intent_id, observed_url, &original.source)
                .unwrap_err(),
            "WEB_ROLLOVER_QUARANTINE_PHASE_INVALID"
        );
        assert_eq!(
            store.active().unwrap().unwrap().phase,
            Phase::ResponsePending
        );
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }
    fn handoff_request(root: &Path) -> HandoffRequest {
        let project_id = "alpha";
        let intent_id = "intent-a";
        let source_task_id = "task-a";
        let native_thread_id = Some("native-thread-a".to_string());
        HandoffRequest {
            schema_version: 1,
            project_id: project_id.into(),
            intent_id: intent_id.into(),
            source_task_id: source_task_id.into(),
            source: Target::new(1, "https://chatgpt.com/c/old".into()).unwrap(),
            native_workspace: root.to_path_buf(),
            native_thread_id: native_thread_id.clone(),
            event_id: format!("web-handoff-{intent_id}"),
            message: handoff_request_message(
                project_id,
                intent_id,
                source_task_id,
                &native_thread_id,
            ),
        }
    }
    fn completed_delivery(request: &HandoffRequest) -> (Delivery, TurnTimer) {
        let message_digest = digest(&request.message);
        let event = Event {
            schema_version: 1,
            event_id: request.event_id.clone(),
            project_id: request.project_id.clone(),
            event_type: "engineering_attention".into(),
            created_utc: 10,
            message: request.message.clone(),
            target_generation: request.source.generation,
            audit_references: Vec::new(),
        };
        let delivery = Delivery {
            schema_version: 1,
            event,
            target: request.source.clone(),
            phase: DeliveryPhase::Sent,
            owner: "wake-owner".into(),
            updated_utc: 11,
            message_digest: message_digest.clone(),
            receipt: Some(Receipt {
                schema_version: 1,
                event_id: request.event_id.clone(),
                target_generation: request.source.generation,
                target_digest: request.source.digest.clone(),
                message_digest,
                sent_utc: 11,
                evidence: "EXACT_USER_MESSAGE_APPENDED".into(),
            }),
            reason: None,
        };
        let timer = TurnTimer {
            schema_version: 1,
            event_id: request.event_id.clone(),
            project_id: request.project_id.clone(),
            target_generation: request.source.generation,
            target_digest: request.source.digest.clone(),
            started_utc: 11,
            completed_utc: Some(12),
            response_state: ResponseState::Complete,
        };
        (delivery, timer)
    }
    fn source_snapshot(request: &HandoffRequest, user: &str, answer: &Handoff) -> Value {
        let assistant = serde_json::to_string(answer).unwrap();
        let message = |role: &str, text: &str| {
            json!({"role":role,"text":text,"visibleUnits":text.len(),
                "textComplete":true,"revisionComplete":true,"identityBasis":"MESSAGE_ID",
                "revisionHash":digest(&format!("catdesk-snapshot-revision-v1\0{}\0{}",request.source.digest,text)),
                "messageHash":digest(&format!("test-{role}-{text}")),
                "turnHash":null,"roleTurnHash":null})
        };
        json!({"messages":[message("USER",user),message("ASSISTANT",&assistant)],
            "truncated":false,"coverage":{"anchorStability":"UNVERIFIED","returned":2}})
    }
    fn source_handoff() -> Handoff {
        Handoff {
            schema_version: 1,
            project_id: "alpha".into(),
            source_task_id: "task-a".into(),
            objective: "Finish the project MVP".into(),
            completed: vec!["Reviewed current project status".into()],
            remaining: vec!["Confirm task ownership".into()],
            next_action: "Read project status and check owners".into(),
            owned_paths: Vec::new(),
            verification: vec!["Source conversation report only".into()],
            blockers: Vec::new(),
        }
    }

    #[test]
    fn web_work_lease_excludes_rollover_and_refuses_pending_handoff() {
        let root = fixture();
        let lease = acquire_web_work_lease(&root, "alpha").unwrap();
        assert_eq!(
            ProjectStore::open(&root, "alpha").err().unwrap(),
            "WEB_ROLLOVER_PROJECT_BUSY"
        );
        drop(lease);
        let request = handoff_request(&root);
        let store = ProjectStore::open(&root, "alpha").unwrap();
        store.save_handoff_request(&request).unwrap();
        drop(store);
        assert_eq!(
            acquire_web_work_lease(&root, "alpha").err().unwrap(),
            "WEB_WORK_ROLLOVER_PENDING"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn web_work_lease_refuses_context_intent_before_handoff_file_exists() {
        use crate::web_conversation_context::{BrowserCoverageObservation, ContextPressurePolicy};

        let root = fixture();
        fs::create_dir(root.join(".catdesk")).unwrap();
        let meter = WebContextStore::open(root.join(".catdesk/web-context")).unwrap();
        let key = ConversationKey {
            project_id: "alpha".into(),
            conversation_url: "https://chatgpt.com/c/old".into(),
            target_generation: 1,
        };
        meter.bind_observed_target(key.clone(), None).unwrap();
        meter
            .configure_policy(
                "alpha",
                ContextPressurePolicy {
                    automatic_rollover_enabled: true,
                    incomplete_history_rollover: true,
                    ..Default::default()
                },
            )
            .unwrap();
        meter
            .record_browser_coverage(
                &key,
                BrowserCoverageObservation {
                    observed_at_unix: 1,
                    initial_history_gap: true,
                    identity_gap: false,
                    text_gap: false,
                    overflow_gap: false,
                },
            )
            .unwrap();
        assert!(
            meter
                .request_rollover_if_due(&key, &key.target_sha256(), "task-a", true, 100)
                .unwrap()
                .is_some()
        );
        assert_eq!(
            acquire_web_work_lease(&root, "alpha").err().unwrap(),
            "WEB_WORK_ROLLOVER_PENDING"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn web_work_handoff_rechecks_active_event_under_project_lock() {
        let root = fixture();
        fs::create_dir(root.join(".catdesk")).unwrap();
        let source = Target::new(1, "https://chatgpt.com/c/old".into()).unwrap();
        let event_id = format!("web-work-{}", digest("alpha\0central-task"));
        WorkLedger::open(&root, "alpha")
            .unwrap()
            .prepare(
                "central-task",
                &event_id,
                &digest("bounded request"),
                &source,
            )
            .unwrap();
        assert_eq!(
            request_handoff(&root, "alpha", "older-task").err().unwrap(),
            "WEB_HANDOFF_WORK_INTENT_CHANGED"
        );
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn handoff_request_is_durable_and_wake_publication_is_idempotent_after_restart() {
        let root = fixture();
        let request = handoff_request(&root);
        let project = ProjectStore::open(&root, "alpha").unwrap();
        project.save_handoff_request(&request).unwrap();
        drop(project);
        let project = ProjectStore::open(&root, "alpha").unwrap();
        assert_eq!(project.handoff_request().unwrap(), Some(request.clone()));
        let wake = Store::open_scoped_for_test(&root.join("wake"), &root).unwrap();
        wake.initialize().unwrap();
        assert_eq!(
            wake.set_target("alpha", 0, &request.source.url).unwrap(),
            request.source
        );
        let first = wake
            .produce_expected(
                &request.event_id,
                &request.project_id,
                "engineering_attention",
                &request.message,
                &request.source,
            )
            .unwrap();
        let retry = wake
            .produce_expected(
                &request.event_id,
                &request.project_id,
                "engineering_attention",
                &request.message,
                &request.source,
            )
            .unwrap();
        assert_eq!(first, retry);
        assert_eq!(wake.events().unwrap(), vec![first]);
    }

    #[test]
    fn handoff_status_distinguishes_unchecked_from_collected_proof() {
        let root = fixture();
        let request = handoff_request(&root);
        let project = ProjectStore::open(&root, "alpha").unwrap();
        project.save_handoff_request(&request).unwrap();
        drop(project);
        let pending = status_read_only(&root, "alpha").unwrap();
        assert!(pending["handoffRequest"]["deliveryProved"].is_null());
        assert!(pending["handoffRequest"]["completionProved"].is_null());
        assert_eq!(pending["handoffRequest"]["handoffCollected"], false);

        let project = ProjectStore::open(&root, "alpha").unwrap();
        let mut prepared = record();
        prepared.handoff_event_id = Some(request.event_id);
        project.save(&prepared).unwrap();
        atomic(&project.root.join("active.json"), &prepared.intent_id).unwrap();
        drop(project);
        let collected = status_read_only(&root, "alpha").unwrap();
        assert_eq!(collected["handoffRequest"]["deliveryProved"], true);
        assert_eq!(collected["handoffRequest"]["completionProved"], true);
        assert_eq!(collected["handoffRequest"]["handoffCollected"], true);
    }

    #[test]
    fn automatic_stage_resumes_durable_request_and_never_replays_unknown_submit() {
        let root = fixture();
        let request = handoff_request(&root);
        let project = ProjectStore::open(&root, "alpha").unwrap();
        project.save_handoff_request(&request).unwrap();
        drop(project);
        assert_eq!(
            auto_stage(&root, "alpha", &request.source).unwrap(),
            AutoStage::CollectHandoff
        );

        let project = ProjectStore::open(&root, "alpha").unwrap();
        let mut prepared = record();
        prepared.handoff_event_id = Some(request.event_id.clone());
        prepared.source_fingerprint = Some("0".repeat(64));
        project.save(&prepared).unwrap();
        atomic(&project.root.join("active.json"), &prepared.intent_id).unwrap();
        drop(project);
        assert_eq!(
            auto_stage(&root, "alpha", &request.source).unwrap(),
            AutoStage::Execute
        );

        let project = ProjectStore::open(&root, "alpha").unwrap();
        prepared.phase = Phase::Submitting;
        project.save(&prepared).unwrap();
        drop(project);
        assert_eq!(
            auto_stage(&root, "alpha", &request.source).unwrap(),
            AutoStage::SubmissionUnknown
        );

        // Once the exact USER/new URL receipt is saved, restarting advances
        // through read-only response probes and never enters submit again.
        let project = ProjectStore::open(&root, "alpha").unwrap();
        prepared.phase = Phase::ResponsePending;
        let mut pending = receipt_for(&prepared);
        pending.completion = "RESPONSE_PENDING".into();
        validate_receipt_stage(&prepared, &pending, "RESPONSE_PENDING").unwrap();
        prepared.receipt = Some(pending.clone());
        project.save(&prepared).unwrap();
        drop(project);
        let project = ProjectStore::open(&root, "alpha").unwrap();
        let restored = project.active().unwrap().unwrap();
        assert_eq!(restored.phase, Phase::ResponsePending);
        assert_eq!(restored.receipt, Some(pending));
        drop(project);
        assert_eq!(
            auto_stage(&root, "alpha", &request.source).unwrap(),
            AutoStage::Execute
        );

        let next = Target::new(2, "https://chatgpt.com/c/new".into()).unwrap();
        let project = ProjectStore::open(&root, "alpha").unwrap();
        prepared.phase = Phase::ContinuationQueued;
        prepared.bound_target = Some(next.clone());
        prepared.context_meter_status = ContextMeterStatus::RetryRequired;
        project.save(&prepared).unwrap();
        drop(project);
        assert_eq!(
            auto_stage(&root, "alpha", &next).unwrap(),
            AutoStage::Execute
        );
    }

    #[test]
    fn automatic_driver_does_not_execute_unwitnessed_manual_rollover() {
        let root = fixture();
        let project = ProjectStore::open(&root, "alpha").unwrap();
        let prepared = record();
        project.save(&prepared).unwrap();
        atomic(&project.root.join("active.json"), &prepared.intent_id).unwrap();
        drop(project);
        assert_eq!(
            auto_stage(&root, "alpha", &prepared.source).unwrap(),
            AutoStage::SourceWitnessRequired
        );
    }

    #[test]
    fn automatic_recovery_accepts_only_saved_guarded_wake_first_evidence() {
        let root = fixture();
        let request = handoff_request(&root);
        let mut saved = record();
        saved.phase = Phase::Created;
        saved.receipt = Some(receipt_for(&saved));
        saved.handoff_event_id = Some("web-handoff-intent-a".into());
        saved.source_fingerprint = Some("a".repeat(64));
        saved.source_snapshot_verified = true;
        let destination = Target::new(2, "https://chatgpt.com/c/new".into()).unwrap();
        let registered = AutonomousProjectV1 {
            project_id: "alpha".into(),
            workspace: root,
            git_identity: "fixture".into(),
            verification_profile: "fixture".into(),
            codex_thread_id: Some("native-thread-a".into()),
            chatgpt_target_url: Some(saved.source.url.clone()),
            chatgpt_target_sha256: Some(saved.source.digest.clone()),
            chatgpt_target_history: vec![saved.source.url.clone()],
        };
        assert_eq!(
            recovery_destination(&saved, &registered, Some(&destination), Some(&request)).unwrap(),
            Some(destination.clone())
        );
        assert!(
            recovery_destination(&saved, &registered, Some(&saved.source), Some(&request))
                .unwrap()
                .is_none()
        );
        saved.source_snapshot_verified = false;
        assert!(
            recovery_destination(&saved, &registered, Some(&destination), Some(&request))
                .unwrap()
                .is_none()
        );
        saved.source_snapshot_verified = true;
        assert!(
            recovery_destination(&saved, &registered, Some(&destination), None)
                .unwrap()
                .is_none()
        );
        let mut changed_owner = registered.clone();
        changed_owner.codex_thread_id = Some("different-native-owner".into());
        assert!(
            recovery_destination(&saved, &changed_owner, Some(&destination), Some(&request))
                .unwrap()
                .is_none()
        );
        changed_owner = registered.clone();
        changed_owner.workspace = registered.workspace.join("different-worktree");
        assert!(
            recovery_destination(&saved, &changed_owner, Some(&destination), Some(&request))
                .unwrap()
                .is_none()
        );
        let mut changed_request = request.clone();
        changed_request.source_task_id = "different-task".into();
        assert!(
            recovery_destination(
                &saved,
                &registered,
                Some(&destination),
                Some(&changed_request)
            )
            .unwrap()
            .is_none()
        );
    }

    #[test]
    fn handoff_collection_requires_exact_receipt_completed_timer_and_latest_user() {
        let root = fixture();
        let request = handoff_request(&root);
        let (delivery, timer) = completed_delivery(&request);
        completed_request_delivery(&request, &delivery, &timer).unwrap();
        let snapshot = source_snapshot(&request, &request.message, &source_handoff());
        assert_eq!(
            handoff_from_source_snapshot(&request, &snapshot)
                .unwrap()
                .source_task_id,
            request.source_task_id
        );
        let mut incomplete = timer.clone();
        incomplete.response_state = ResponseState::Observing;
        incomplete.completed_utc = None;
        assert_eq!(
            completed_request_delivery(&request, &delivery, &incomplete).unwrap_err(),
            "WEB_HANDOFF_COMPLETED_TIMER_REQUIRED"
        );
        let mut not_sent = delivery.clone();
        not_sent.phase = DeliveryPhase::Submitting;
        assert_eq!(
            completed_request_delivery(&request, &not_sent, &timer).unwrap_err(),
            "WEB_HANDOFF_EXACT_USER_RECEIPT_REQUIRED"
        );
        let wrong_user = request.message.replace("project alpha", "project beta");
        let wrong_snapshot = source_snapshot(&request, &wrong_user, &source_handoff());
        assert_eq!(
            handoff_from_source_snapshot(&request, &wrong_snapshot).unwrap_err(),
            "WEB_HANDOFF_LATEST_USER_NOT_REQUEST"
        );
    }

    #[test]
    fn forged_task_and_native_path_claims_cannot_be_collected() {
        let root = fixture();
        let request = handoff_request(&root);
        let mut wrong_task = source_handoff();
        wrong_task.source_task_id = "task-b".into();
        assert_eq!(
            handoff_from_source_snapshot(
                &request,
                &source_snapshot(&request, &request.message, &wrong_task)
            )
            .unwrap_err(),
            "WEB_HANDOFF_TASK_CHANGED"
        );
        let mut claimed_native_path = source_handoff();
        claimed_native_path.owned_paths.push("src/main.rs".into());
        assert_eq!(
            handoff_from_source_snapshot(
                &request,
                &source_snapshot(&request, &request.message, &claimed_native_path)
            )
            .unwrap_err(),
            "WEB_HANDOFF_OWNERSHIP_UNVERIFIED"
        );
    }

    #[test]
    fn created_rollover_requires_same_latest_source_handoff_before_bind() {
        let root = fixture();
        let request = handoff_request(&root);
        let registered = AutonomousProjectV1 {
            project_id: "alpha".into(),
            workspace: root,
            git_identity: "fixture".into(),
            verification_profile: "fixture".into(),
            codex_thread_id: Some("native-thread-a".into()),
            chatgpt_target_url: Some(request.source.url.clone()),
            chatgpt_target_sha256: Some(request.source.digest.clone()),
            chatgpt_target_history: vec![request.source.url.clone()],
        };
        let handoff = source_handoff();
        let mut prepared = record();
        prepared.handoff_event_id = Some(request.event_id.clone());
        prepared.handoff = compose_handoff_content(&registered, &handoff).unwrap();
        let exact = source_snapshot(&request, &request.message, &handoff);
        source_snapshot_matches_record(&request, &prepared, &registered, &exact).unwrap();

        let newer_user = source_snapshot(&request, "new work after handoff", &handoff);
        assert_eq!(
            source_snapshot_matches_record(&request, &prepared, &registered, &newer_user)
                .unwrap_err(),
            "WEB_ROLLOVER_SOURCE_HANDOFF_CHANGED_RECONCILIATION_REQUIRED"
        );
        let mut newer_handoff = handoff;
        newer_handoff.next_action = "Changed action".into();
        let changed = source_snapshot(&request, &request.message, &newer_handoff);
        assert_eq!(
            source_snapshot_matches_record(&request, &prepared, &registered, &changed).unwrap_err(),
            "WEB_ROLLOVER_SOURCE_HANDOFF_CHANGED_RECONCILIATION_REQUIRED"
        );
    }

    #[test]
    fn handoff_request_retains_registered_native_owner_context() {
        let root = fixture();
        let request = handoff_request(&root);
        let mut registered = AutonomousProjectV1 {
            project_id: "alpha".into(),
            workspace: root,
            git_identity: "fixture".into(),
            verification_profile: "fixture".into(),
            codex_thread_id: Some("native-thread-a".into()),
            chatgpt_target_url: Some(request.source.url.clone()),
            chatgpt_target_sha256: Some(request.source.digest.clone()),
            chatgpt_target_history: vec![request.source.url.clone()],
        };
        assert!(native_owner_matches(&request, &registered));
        assert!(request.message.contains("native-thread-a"));
        registered.codex_thread_id = Some("native-thread-b".into());
        assert!(!native_owner_matches(&request, &registered));
    }

    #[test]
    fn completed_continuation_reuses_original_task_only_with_exact_owner_and_turn_proof() {
        let root = fixture();
        let request = handoff_request(&root);
        let current = Target::new(2, "https://chatgpt.com/c/new".into()).unwrap();
        let mut record = record();
        record.phase = Phase::ContinuationQueued;
        record.receipt = Some(receipt_for(&record));
        record.bound_target = Some(current.clone());
        record.handoff_event_id = Some(request.event_id.clone());
        record.continuation_event_id = format!("web-continue-{}", record.intent_id);
        let mut registered = AutonomousProjectV1 {
            project_id: "alpha".into(),
            workspace: root,
            git_identity: "fixture".into(),
            verification_profile: "fixture".into(),
            codex_thread_id: Some("native-thread-a".into()),
            chatgpt_target_url: Some(current.url.clone()),
            chatgpt_target_sha256: Some(current.digest.clone()),
            chatgpt_target_history: vec![request.source.url.clone(), current.url.clone()],
        };
        let (mut delivery, mut timer) = completed_delivery(&request);
        let message = continuation_message(&record, &current);
        let message_digest = digest(&message);
        delivery.event.event_id = record.continuation_event_id.clone();
        delivery.event.target_generation = current.generation;
        delivery.event.message = message;
        delivery.target = current.clone();
        delivery.message_digest = message_digest.clone();
        let receipt = delivery.receipt.as_mut().unwrap();
        receipt.event_id = record.continuation_event_id.clone();
        receipt.target_generation = current.generation;
        receipt.target_digest = current.digest.clone();
        receipt.message_digest = message_digest;
        timer.event_id = record.continuation_event_id.clone();
        timer.target_generation = current.generation;
        timer.target_digest = current.digest.clone();

        assert_eq!(
            continuation_task_from_proof(
                &record,
                &request,
                &registered,
                &current,
                &delivery,
                &timer,
            )
            .unwrap(),
            Some("task-a".into())
        );
        timer.response_state = ResponseState::Observing;
        assert_eq!(
            continuation_task_from_proof(
                &record,
                &request,
                &registered,
                &current,
                &delivery,
                &timer,
            )
            .unwrap_err(),
            "WEB_CONTINUATION_PROOF_INVALID"
        );
        timer.response_state = ResponseState::Complete;
        registered.codex_thread_id = Some("different-thread".into());
        assert_eq!(
            continuation_task_from_proof(
                &record,
                &request,
                &registered,
                &current,
                &delivery,
                &timer,
            )
            .unwrap_err(),
            "WEB_CONTINUATION_OWNER_OR_TARGET_CHANGED"
        );
    }

    #[test]
    fn next_handoff_rechecks_owner_and_reuses_saved_current_request() {
        let root = fixture();
        let request = handoff_request(&root);
        let current = Target::new(2, "https://chatgpt.com/c/new".into()).unwrap();
        let mut previous = record();
        previous.phase = Phase::ContinuationQueued;
        previous.bound_target = Some(current.clone());
        previous.handoff_event_id = Some(request.event_id.clone());
        let store = ProjectStore::open(&root, "alpha").unwrap();
        store.save_handoff_request(&request).unwrap();
        let mut registered = AutonomousProjectV1 {
            project_id: "alpha".into(),
            workspace: root,
            git_identity: "fixture".into(),
            verification_profile: "fixture".into(),
            codex_thread_id: Some("native-thread-a".into()),
            chatgpt_target_url: Some(current.url.clone()),
            chatgpt_target_sha256: Some(current.digest.clone()),
            chatgpt_target_history: vec![request.source.url.clone(), current.url.clone()],
        };
        require_prior_continuation_owner(&store, Some(&previous), &current, "task-a", &registered)
            .unwrap();
        assert!(
            saved_current_handoff_request(&store, &current, "task-a", &registered)
                .unwrap()
                .is_none()
        );
        registered.codex_thread_id = Some("native-thread-b".into());
        assert_eq!(
            require_prior_continuation_owner(
                &store,
                Some(&previous),
                &current,
                "task-a",
                &registered,
            )
            .unwrap_err(),
            "WEB_HANDOFF_NATIVE_OWNER_CHANGED"
        );

        registered.codex_thread_id = Some("native-thread-a".into());
        let second = HandoffRequest {
            schema_version: 1,
            project_id: "alpha".into(),
            intent_id: "intent-b".into(),
            source_task_id: "task-a".into(),
            source: current.clone(),
            native_workspace: registered.workspace.clone(),
            native_thread_id: registered.codex_thread_id.clone(),
            event_id: "web-handoff-intent-b".into(),
            message: handoff_request_message(
                "alpha",
                "intent-b",
                "task-a",
                &registered.codex_thread_id,
            ),
        };
        store.save_handoff_request(&second).unwrap();
        assert_eq!(
            saved_current_handoff_request(&store, &current, "task-a", &registered).unwrap(),
            Some(second)
        );
        registered.codex_thread_id = Some("native-thread-b".into());
        assert_eq!(
            saved_current_handoff_request(&store, &current, "task-a", &registered).unwrap_err(),
            "WEB_HANDOFF_NATIVE_OWNER_CHANGED"
        );
    }

    #[cfg(windows)]
    #[test]
    fn persisted_completed_continuation_anchors_next_generation() {
        let parent = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join(format!("web-continuation-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&parent).unwrap();
        let (root, dev_root) = prepared_dev_fixture(&parent);
        let alpha_workspace = git_fixture(&parent.join("alpha"), "https://example.test/alpha.git");
        let current = Target::new(2, "https://chatgpt.com/c/new".into()).unwrap();
        let mut record = record();
        let meter = WebContextStore::open(root.join(".catdesk/web-context")).unwrap();
        meter
            .configure_policy(
                "alpha",
                crate::web_conversation_context::ContextPressurePolicy {
                    configured_budget_tokens: 128,
                    safety_headroom_tokens: 127,
                    unknown_source_reserve_tokens: 0,
                    automatic_rollover_enabled: true,
                    ..Default::default()
                },
            )
            .unwrap();
        let source_key = key("alpha", &record.source);
        meter
            .bind_observed_target(source_key.clone(), None)
            .unwrap();
        meter
            .record_observation(
                &source_key,
                Observation {
                    event_id: "source-message",
                    revision: 1,
                    dedup_key: "source-message",
                    source_kind: VisibleSourceKind::UserMessage,
                    visible_text: Some("source context before rollover"),
                    observed_at_unix: 100,
                },
            )
            .unwrap();
        let intent = meter
            .request_rollover_if_due(
                &source_key,
                &record.source.digest,
                &record.source_task_id,
                true,
                101,
            )
            .unwrap()
            .unwrap();
        record.intent_id = intent.intent_id;
        record.message_digest = message_digest(
            &record.project_id,
            &record.intent_id,
            &record.source.url,
            &record.handoff,
        )
        .unwrap();
        let mut request = handoff_request(&alpha_workspace);
        request.intent_id = record.intent_id.clone();
        request.event_id = format!("web-handoff-{}", record.intent_id);
        request.message = handoff_request_message(
            "alpha",
            &record.intent_id,
            &record.source_task_id,
            &request.native_thread_id,
        );
        record.phase = Phase::ContinuationQueued;
        record.receipt = Some(receipt_for(&record));
        record.bound_target = Some(current.clone());
        record.handoff_event_id = Some(request.event_id.clone());
        record.continuation_event_id = format!("web-continue-{}", record.intent_id);
        record.context_meter_status = ContextMeterStatus::RetryRequired;
        record.source_snapshot_verified = true;
        record.source_fingerprint = Some("a".repeat(64));
        let project_store = ProjectStore::open(&root, "alpha").unwrap();
        project_store.save_handoff_request(&request).unwrap();
        project_store.save(&record).unwrap();
        atomic(&project_store.root.join("active.json"), &record.intent_id).unwrap();
        drop(project_store);

        let registry =
            AutonomousProjectRegistryStoreV1::open(root.join(".catdesk/projects")).unwrap();
        registry.initialize(1, 1).unwrap();
        registry
            .register_project(AutonomousProjectV1 {
                project_id: "catdesk".into(),
                workspace: root.clone(),
                git_identity: "https://example.test/catdesk.git".into(),
                verification_profile: "web_only_v1".into(),
                codex_thread_id: None,
                chatgpt_target_url: None,
                chatgpt_target_sha256: None,
                chatgpt_target_history: Vec::new(),
            })
            .unwrap();
        registry
            .register_project(AutonomousProjectV1 {
                project_id: "alpha".into(),
                workspace: alpha_workspace,
                git_identity: "https://example.test/alpha.git".into(),
                verification_profile: "fixture".into(),
                codex_thread_id: Some("native-thread-a".into()),
                chatgpt_target_url: Some(current.url.clone()),
                chatgpt_target_sha256: Some(current.digest.clone()),
                chatgpt_target_history: vec![request.source.url.clone(), current.url.clone()],
            })
            .unwrap();

        let wake = Store::open(&dev_root).unwrap();
        assert_eq!(
            wake.set_target("alpha", 0, &request.source.url).unwrap(),
            request.source
        );
        assert_eq!(wake.set_target("alpha", 1, &current.url).unwrap(), current);
        let pending_status = status_read_only(&root, "alpha").unwrap();
        assert_eq!(pending_status["continuationProved"], false);
        assert_eq!(
            pending_status["continuationProofStatus"],
            "PENDING_OR_UNVERIFIED"
        );
        let pending_store = ProjectStore::open(&root, "alpha").unwrap();
        assert_eq!(
            continuation_ready_for_work(&pending_store, &wake, &root, "alpha", &current, &record,)
                .unwrap_err(),
            "WEB_WORK_ROLLOVER_PENDING"
        );
        drop(pending_store);
        assert_eq!(
            acquire_web_work_lease(&root, "alpha").err().unwrap(),
            "WEB_WORK_ROLLOVER_PENDING"
        );
        drop(acquire_web_work_lease(&root, "beta").unwrap());
        let message = continuation_message(&record, &current);
        let event = wake
            .produce_expected(
                &record.continuation_event_id,
                "alpha",
                "engineering_attention",
                &message,
                &current,
            )
            .unwrap();
        let lease = wake.host_lock().unwrap();
        let claimed = wake.claim(&event, &lease).unwrap();
        wake.transition(
            &event.event_id,
            lease.owner(),
            DeliveryPhase::Submitting,
            None,
            None,
        )
        .unwrap();
        let receipt = Receipt {
            schema_version: 1,
            event_id: event.event_id.clone(),
            target_generation: current.generation,
            target_digest: current.digest.clone(),
            message_digest: digest(&message),
            sent_utc: now(),
            evidence: "EXACT_USER_MESSAGE_APPENDED".into(),
        };
        wake.record_exact_receipt_timer(&event.event_id, &receipt)
            .unwrap();
        wake.transition(
            &event.event_id,
            &claimed.owner,
            DeliveryPhase::Sent,
            Some(receipt),
            None,
        )
        .unwrap();
        wake.complete_timer(&event.event_id).unwrap();
        let verified_status = status_read_only(&root, "alpha").unwrap();
        assert_eq!(verified_status["continuationProved"], true);
        assert_eq!(verified_status["continuationProofStatus"], "VERIFIED");
        let missing_logs = wake.root().join("logs");
        let saved_logs = wake.root().join("logs-saved-fixture");
        fs::rename(&missing_logs, &saved_logs).unwrap();
        let unavailable_status = status_read_only(&root, "alpha").unwrap();
        assert_eq!(unavailable_status["continuationProved"], false);
        assert_eq!(
            unavailable_status["continuationProofStatus"],
            "UNAVAILABLE_OR_CHANGED"
        );
        assert!(
            !missing_logs.exists(),
            "status must not recreate missing Wake state"
        );
        fs::rename(&saved_logs, &missing_logs).unwrap();
        let completed_store = ProjectStore::open(&root, "alpha").unwrap();
        continuation_ready_for_work(&completed_store, &wake, &root, "alpha", &current, &record)
            .unwrap();
        let mut retry = record.clone();
        retry.context_meter_status = ContextMeterStatus::RetryRequired;
        retry.source_snapshot_verified = true;
        retry.source_fingerprint = Some("a".repeat(64));
        continuation_ready_for_work(&completed_store, &wake, &root, "alpha", &current, &retry)
            .unwrap();
        retry.reconciliation_reason = Some("WEB_ROLLOVER_BOUND_TARGET_CHANGED".into());
        assert_eq!(
            continuation_ready_for_work(&completed_store, &wake, &root, "alpha", &current, &retry)
                .unwrap_err(),
            "WEB_WORK_ROLLOVER_PENDING"
        );
        retry.reconciliation_reason = None;
        retry.source_snapshot_verified = false;
        assert!(
            continuation_ready_for_work(&completed_store, &wake, &root, "alpha", &current, &retry)
                .is_err()
        );
        retry.source_snapshot_verified = true;
        retry.source_fingerprint = None;
        assert!(
            continuation_ready_for_work(&completed_store, &wake, &root, "alpha", &current, &retry)
                .is_err()
        );
        retry.source_fingerprint = Some("a".repeat(64));
        retry.context_meter_status = ContextMeterStatus::Pending;
        assert!(
            continuation_ready_for_work(&completed_store, &wake, &root, "alpha", &current, &retry)
                .is_err()
        );
        retry.context_meter_status = ContextMeterStatus::RetryRequired;
        let changed = Target::new(3, "https://chatgpt.com/c/later".into()).unwrap();
        assert!(
            continuation_ready_for_work(&completed_store, &wake, &root, "alpha", &changed, &retry)
                .is_err()
        );
        let approved = WorkLease {
            _store: completed_store,
            approved_target: Some(current.clone()),
        };
        approved.check_target(&current).unwrap();
        assert_eq!(
            approved.check_target(&changed).unwrap_err(),
            "WEB_WORK_ROLLOVER_PENDING"
        );
        drop(approved);
        let work = acquire_web_work_lease(&root, "alpha").unwrap();
        work.check_target(&current).unwrap();
        drop(work);
        drop(acquire_web_work_lease(&root, "beta").unwrap());
        let project_file = fs::read_dir(root.join(".catdesk/web-context/projects"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let original_meter = fs::read(&project_file).unwrap();
        fs::write(&project_file, b"corrupt").unwrap();
        assert_eq!(
            acquire_web_work_lease(&root, "alpha").err().unwrap(),
            "WEB_WORK_ROLLOVER_PENDING"
        );
        drop(acquire_web_work_lease(&root, "beta").unwrap());
        fs::write(&project_file, original_meter).unwrap();
        drop(acquire_web_work_lease(&root, "alpha").unwrap());
        assert_eq!(
            completed_continuation_task_anchor(&root, &wake, "alpha", &current).unwrap(),
            Some("task-a".into())
        );
        let current_key = key("alpha", &current);
        meter
            .bind_observed_target(current_key.clone(), Some(&source_key))
            .unwrap();
        let pressure = meter.pressure(&current_key).unwrap();
        assert!(pressure.rollover_intent.is_none());
        assert_eq!(
            super::super::auto::source_task_anchor(
                &root,
                &wake,
                "alpha",
                &current,
                &current_key,
                &pressure,
            )
            .unwrap(),
            Some("task-a".into())
        );
        drop(lease);
        drop(wake);
        drop(meter);
        fs::remove_dir_all(parent).unwrap();
    }

    #[test]
    fn completed_retry_context_accepts_only_exact_predecessor_or_unreserved_destination() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join(format!("web-retry-context-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let meter_root = root.join(".catdesk/web-context");
        let meter = WebContextStore::open(&meter_root).unwrap();
        meter
            .configure_policy(
                "alpha",
                crate::web_conversation_context::ContextPressurePolicy {
                    configured_budget_tokens: 128,
                    safety_headroom_tokens: 127,
                    unknown_source_reserve_tokens: 0,
                    automatic_rollover_enabled: true,
                    ..Default::default()
                },
            )
            .unwrap();
        let mut record = record();
        let source_key = key("alpha", &record.source);
        let current = Target::new(2, "https://chatgpt.com/c/new".into()).unwrap();
        let current_key = key("alpha", &current);
        assert!(meter.active_pressure("alpha").unwrap().is_none());
        meter
            .bind_observed_target(source_key.clone(), None)
            .unwrap();
        meter
            .record_observation(
                &source_key,
                Observation {
                    event_id: "user-a",
                    revision: 1,
                    dedup_key: "user-a",
                    source_kind: VisibleSourceKind::UserMessage,
                    visible_text: Some("A visible source message"),
                    observed_at_unix: 100,
                },
            )
            .unwrap();
        let intent = meter
            .request_rollover_if_due(
                &source_key,
                &record.source.digest,
                &record.source_task_id,
                true,
                101,
            )
            .unwrap()
            .unwrap();
        record.intent_id = intent.intent_id.clone();
        let pressure = meter.active_pressure("alpha").unwrap().unwrap();
        assert_eq!(pressure.key, source_key);
        assert_eq!(pressure.rollover_intent, Some(intent.clone()));
        assert!(completed_retry_context_allows_work(
            Some(&pressure),
            &record,
            &current
        ));
        assert!(!completed_retry_context_allows_work(
            None, &record, &current
        ));
        let mut unrelated = pressure.clone();
        unrelated.rollover_intent.as_mut().unwrap().intent_id = "another-intent".into();
        assert!(!completed_retry_context_allows_work(
            Some(&unrelated),
            &record,
            &current
        ));
        unrelated = pressure.clone();
        unrelated.rollover_intent.as_mut().unwrap().source_task_id = "another-task".into();
        assert!(!completed_retry_context_allows_work(
            Some(&unrelated),
            &record,
            &current
        ));
        unrelated = pressure.clone();
        unrelated.rollover_intent.as_mut().unwrap().target_sha256 = "0".repeat(64);
        assert!(!completed_retry_context_allows_work(
            Some(&unrelated),
            &record,
            &current
        ));
        meter
            .bind_observed_target(current_key.clone(), Some(&source_key))
            .unwrap();
        let destination = meter.active_pressure("alpha").unwrap().unwrap();
        assert_eq!(destination.key, current_key);
        assert!(destination.rollover_intent.is_none());
        assert!(completed_retry_context_allows_work(
            Some(&destination),
            &record,
            &current
        ));
        meter
            .record_observation(
                &current_key,
                Observation {
                    event_id: "user-b",
                    revision: 1,
                    dedup_key: "user-b",
                    source_kind: VisibleSourceKind::UserMessage,
                    visible_text: Some("A new generation message"),
                    observed_at_unix: 102,
                },
            )
            .unwrap();
        assert!(
            meter
                .request_rollover_if_due(&current_key, &current.digest, "task-b", true, 103)
                .unwrap()
                .is_some()
        );
        let new_intent = meter.active_pressure("alpha").unwrap().unwrap();
        assert!(!completed_retry_context_allows_work(
            Some(&new_intent),
            &record,
            &current
        ));
        let project_file = fs::read_dir(meter_root.join("projects"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        fs::write(project_file, b"corrupt").unwrap();
        assert!(meter.active_pressure("alpha").is_err());
        drop(meter);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn uncertain_submission_is_durable_and_other_project_lock_is_independent() {
        let root = fixture();
        let store = ProjectStore::open(&root, "alpha").unwrap();
        assert!(ProjectStore::open(&root, "alpha").is_err());
        let beta = ProjectStore::open(&root, "beta").unwrap();
        assert!(beta.active().unwrap().is_none());
        let mut record = record();
        record.phase = Phase::Submitting;
        store.save(&record).unwrap();
        atomic(&store.root.join("active.json"), &record.intent_id).unwrap();
        drop(store);
        let reopened = ProjectStore::open(&root, "alpha").unwrap();
        assert_eq!(reopened.active().unwrap().unwrap().phase, Phase::Submitting);
        assert!(!summary(&record).to_string().contains("private handoff"));
    }
    #[test]
    fn receipt_is_bound_to_one_source_task_and_real_new_url() {
        let record = record();
        let mut receipt = receipt_for(&record);
        assert!(validate_receipt(&record, &receipt).is_ok());
        receipt.project_id = "beta".into();
        assert!(validate_receipt(&record, &receipt).is_err());
        receipt.project_id = "alpha".into();
        receipt.destination_url = record.source.url.clone();
        receipt.destination_digest = record.source.digest.clone();
        assert!(validate_receipt(&record, &receipt).is_err());
    }

    #[test]
    fn saved_receipt_recovers_wake_first_partial_binding_with_exact_old_witness() {
        let root = fixture();
        let store = ProjectStore::open(&root, "alpha").unwrap();
        let mut record = record();
        record.phase = Phase::Created;
        record.receipt = Some(receipt_for(&record));
        store.save(&record).unwrap();
        atomic(&store.root.join("active.json"), &record.intent_id).unwrap();
        let destination = validate_receipt(&record, record.receipt.as_ref().unwrap()).unwrap();
        let source_digest = record.source.digest.clone();
        let bound = checkpoint_bound_with(
            &store,
            &mut record,
            &destination,
            || Err("registry and Wake differ".into()),
            |url, digest, generation| {
                assert_eq!(url, destination.url);
                assert_eq!(digest, source_digest);
                assert_eq!(generation, 1);
                Ok(destination.clone())
            },
        )
        .unwrap();
        assert_eq!(bound, destination);
        let saved = store.active().unwrap().unwrap();
        assert_eq!(saved.phase, Phase::Bound);
        assert_eq!(saved.bound_target, Some(destination));
        assert!(saved.receipt.is_some());
    }

    #[test]
    fn cancelled_owner_before_binding_preserves_receipt_without_reconciliation_fence() {
        let root = fixture();
        let store = ProjectStore::open(&root, "alpha").unwrap();
        let mut record = record();
        record.phase = Phase::Created;
        record.receipt = Some(receipt_for(&record));
        store.save(&record).unwrap();
        atomic(&store.root.join("active.json"), &record.intent_id).unwrap();
        let before = serde_json::to_value(store.active().unwrap().unwrap()).unwrap();
        let destination = validate_receipt(&record, record.receipt.as_ref().unwrap()).unwrap();
        let source = record.source.clone();
        assert_eq!(
            checkpoint_bound_with(
                &store,
                &mut record,
                &destination,
                || Ok(source.clone()),
                |_, _, _| Err("WEB_AUTO_OWNER_STOPPED".into())
            )
            .unwrap_err(),
            "WEB_AUTO_OWNER_STOPPED"
        );
        assert!(record.reconciliation_reason.is_none());
        assert_eq!(
            serde_json::to_value(store.active().unwrap().unwrap()).unwrap(),
            before
        );
    }

    #[test]
    fn compensated_source_requires_explicit_reconciliation_and_preserves_other_project() {
        let root = fixture();
        let alpha = ProjectStore::open(&root, "alpha").unwrap();
        let beta = ProjectStore::open(&root, "beta").unwrap();
        let mut record = record();
        record.phase = Phase::Created;
        record.receipt = Some(receipt_for(&record));
        alpha.save(&record).unwrap();
        atomic(&alpha.root.join("active.json"), &record.intent_id).unwrap();
        let destination = validate_receipt(&record, record.receipt.as_ref().unwrap()).unwrap();
        let compensated = Target::new(3, record.source.url.clone()).unwrap();
        let error = checkpoint_bound_with(
            &alpha,
            &mut record,
            &destination,
            || Ok(compensated.clone()),
            |_, _, _| panic!("a higher-generation source must not be rebound"),
        )
        .unwrap_err();
        assert_eq!(
            error,
            "WEB_ROLLOVER_SOURCE_GENERATION_ADVANCED_RECONCILIATION_REQUIRED"
        );
        let saved = alpha.active().unwrap().unwrap();
        assert_eq!(saved.phase, Phase::Created);
        assert!(saved.receipt.is_some());
        assert!(saved.reconciliation_reason.is_some());
        assert!(beta.active().unwrap().is_none());

        // Even the requested URL at a later generation is not proof that this
        // receipt caused the transition. A separate verified repair is needed.
        let unrelated_later_target = Target::new(4, destination.url.clone()).unwrap();
        let error = checkpoint_bound_with(
            &alpha,
            &mut record,
            &destination,
            || Ok(unrelated_later_target.clone()),
            |_, _, _| panic!("a paired destination must not be rebound"),
        )
        .unwrap_err();
        assert_eq!(
            error,
            "WEB_ROLLOVER_DESTINATION_GENERATION_UNVERIFIED_RECONCILIATION_REQUIRED"
        );
        let saved = alpha.active().unwrap().unwrap();
        assert_eq!(saved.phase, Phase::Created);
        assert_eq!(saved.bound_target, None);
        assert!(saved.receipt.is_some());
        assert_eq!(saved.reconciliation_reason.as_deref(), Some(error.as_str()));
        assert!(beta.active().unwrap().is_none());
    }

    #[test]
    fn created_receipt_rejects_same_destination_at_unrelated_higher_generation() {
        let mut record = record();
        record.phase = Phase::Created;
        record.receipt = Some(receipt_for(&record));
        let destination = validate_receipt(&record, record.receipt.as_ref().unwrap()).unwrap();
        let unrelated = Target::new(destination.generation + 1, destination.url.clone()).unwrap();
        assert_eq!(
            resolve_bound_target_with(
                &record,
                &destination,
                || Ok(unrelated.clone()),
                |_, _, _| panic!("the old witness must not be used for unrelated pairing"),
            )
            .unwrap_err(),
            "WEB_ROLLOVER_DESTINATION_GENERATION_UNVERIFIED_RECONCILIATION_REQUIRED"
        );
    }

    #[test]
    fn meter_failure_does_not_block_continuation_and_retry_never_resends() {
        let root = fixture();
        let wake = Store::open_scoped_for_test(&root.join("wake"), &root).unwrap();
        wake.initialize().unwrap();
        let mut record = record();
        let destination = wake
            .set_target("alpha", 0, "https://chatgpt.com/c/new")
            .unwrap();
        let beta = wake
            .set_target("beta", 0, "https://chatgpt.com/c/beta")
            .unwrap();
        record.phase = Phase::Bound;
        record.bound_target = Some(destination.clone());
        let store = ProjectStore::open(&root, "alpha").unwrap();
        store.save(&record).unwrap();
        atomic(&store.root.join("active.json"), &record.intent_id).unwrap();
        queue_continuation_with(
            &wake,
            &mut record,
            &destination,
            |_, _| Err("meter unavailable".into()),
            |record| store.save(record),
        )
        .unwrap();
        assert_eq!(record.phase, Phase::ContinuationQueued);
        assert_eq!(
            record.context_meter_status,
            ContextMeterStatus::RetryRequired
        );
        assert_eq!(wake.events().unwrap().len(), 1);
        let mut reloaded = store.active().unwrap().unwrap();
        retry_context_meter_with(
            &mut reloaded,
            &destination,
            |_, _| Ok(()),
            |record| store.save(record),
        )
        .unwrap();
        assert_eq!(reloaded.context_meter_status, ContextMeterStatus::Recorded);
        assert_eq!(wake.events().unwrap().len(), 1);
        assert_eq!(wake.config().unwrap().targets.get("beta"), Some(&beta));
    }

    #[test]
    fn continuation_publication_survives_record_save_interruption() {
        let root = fixture();
        let wake = Store::open_scoped_for_test(&root.join("wake"), &root).unwrap();
        wake.initialize().unwrap();
        let destination = wake
            .set_target("alpha", 0, "https://chatgpt.com/c/new")
            .unwrap();
        let store = ProjectStore::open(&root, "alpha").unwrap();
        let mut record = record();
        record.phase = Phase::Bound;
        record.bound_target = Some(destination.clone());
        store.save(&record).unwrap();
        atomic(&store.root.join("active.json"), &record.intent_id).unwrap();
        assert_eq!(
            queue_continuation_with(
                &wake,
                &mut record,
                &destination,
                |_, _| Ok(()),
                |_| Err("simulated save interruption".into()),
            )
            .unwrap_err(),
            "simulated save interruption"
        );
        let first = wake.events().unwrap();
        assert_eq!(first.len(), 1);
        assert_eq!(store.active().unwrap().unwrap().phase, Phase::Bound);
        let mut recovered = store.active().unwrap().unwrap();
        queue_continuation_with(
            &wake,
            &mut recovered,
            &destination,
            |_, _| Ok(()),
            |record| store.save(record),
        )
        .unwrap();
        assert_eq!(wake.events().unwrap(), first);
        assert_eq!(
            store.active().unwrap().unwrap().phase,
            Phase::ContinuationQueued
        );
    }

    #[test]
    fn queued_meter_retry_rejects_stale_binding_before_any_mirror_write() {
        let root = fixture();
        let store = ProjectStore::open(&root, "alpha").unwrap();
        let mut record = record();
        record.phase = Phase::ContinuationQueued;
        record.context_meter_status = ContextMeterStatus::RetryRequired;
        let bound = Target::new(2, "https://chatgpt.com/c/new".into()).unwrap();
        record.bound_target = Some(bound.clone());
        store.save(&record).unwrap();
        atomic(&store.root.join("active.json"), &record.intent_id).unwrap();
        let newer = Target::new(3, "https://chatgpt.com/c/later".into()).unwrap();
        let error = retry_queued_context_meter_with(
            &mut record,
            &bound,
            || Ok(newer.clone()),
            |_, _| panic!("stale rollover must not write the meter"),
            |record| store.save(record),
        )
        .unwrap_err();
        assert_eq!(
            error,
            "WEB_ROLLOVER_BOUND_TARGET_CHANGED_RECONCILIATION_REQUIRED"
        );
        let saved = store.active().unwrap().unwrap();
        assert_eq!(saved.phase, Phase::ContinuationQueued);
        assert_eq!(
            saved.context_meter_status,
            ContextMeterStatus::RetryRequired
        );
        assert_eq!(saved.reconciliation_reason.as_deref(), Some(error.as_str()));
        assert_eq!(saved.bound_target, Some(bound));
    }

    #[test]
    fn prepared_rollover_obeys_later_opt_out_before_browser_submission() {
        use crate::web_conversation_context::ContextPressurePolicy;
        let root = fixture();
        let meter = WebContextStore::open(root.join(".catdesk/web-context")).unwrap();
        let mut record = record();
        let key = key("alpha", &record.source);
        let mut policy = ContextPressurePolicy {
            configured_budget_tokens: 100,
            safety_headroom_tokens: 99,
            unknown_source_reserve_tokens: 0,
            automatic_rollover_enabled: true,
            automatic_observation_enabled: false,
            incomplete_history_rollover: false,
        };
        meter.configure_policy("alpha", policy.clone()).unwrap();
        meter.bind_observed_target(key.clone(), None).unwrap();
        meter
            .record_observation(
                &key,
                Observation {
                    event_id: "message-a",
                    revision: 1,
                    dedup_key: "message-a",
                    source_kind: VisibleSourceKind::UserMessage,
                    visible_text: Some("visible canary text"),
                    observed_at_unix: 11,
                },
            )
            .unwrap();
        record.intent_id = meter
            .request_rollover_if_due(
                &key,
                &record.source.digest,
                &record.source_task_id,
                true,
                12,
            )
            .unwrap()
            .unwrap()
            .intent_id;
        confirm_request_enabled(&root, &record).unwrap();
        let before = meter.project_history("alpha").unwrap();
        let held = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(root.join(".catdesk/web-context/ledger.lock"))
            .unwrap();
        held.lock().unwrap();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let release = std::thread::spawn(move || {
            let _ = release_rx.recv_timeout(std::time::Duration::from_secs(10));
            drop(held);
        });
        let refused = confirm_request_enabled(&root, &record);
        let released_before_timeout = release_tx.send(()).is_ok();
        release.join().unwrap();
        assert!(
            released_before_timeout,
            "rollover confirmation waited on metadata"
        );
        assert_eq!(refused.unwrap_err(), "WEB_CONTEXT_BUSY");
        assert_eq!(record.phase, Phase::Prepared);
        assert_eq!(meter.project_history("alpha").unwrap(), before);
        confirm_request_enabled(&root, &record).unwrap();
        policy.automatic_rollover_enabled = false;
        meter.configure_policy("alpha", policy).unwrap();
        assert_eq!(
            confirm_request_enabled(&root, &record).unwrap_err(),
            "WEB_ROLLOVER_NOT_DUE_OR_DISABLED"
        );
        assert_eq!(record.phase, Phase::Prepared);
    }
}
