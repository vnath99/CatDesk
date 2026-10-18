//! Durable, one-project Web rollover. Browser proof and paired Wake authority
//! are separate checkpoints; a possibly submitted bootstrap is never replayed.

use crate::{
    delegated::autonomy_projects::{AutonomousProjectRegistryStoreV1, AutonomousProjectV1},
    mcp,
    web_conversation_context::{ConversationKey, Observation, VisibleSourceKind, WebContextStore},
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
    io::Read,
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
    let meter = WebContextStore::open(workspace.join(".catdesk/web-context"))?;
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
        "continuationEventId":record.continuation_event_id,
        "boundTarget":record.bound_target,
        "reconciliationRequired":record.reconciliation_reason.is_some(),
        "reconciliationReason":record.reconciliation_reason,
        "contextMeterStatus":record.context_meter_status,
        "handoffEventId":record.handoff_event_id,
        "sourceProgressFingerprint":record.source_fingerprint,
        "sourceSnapshotVerified":record.source_snapshot_verified,
        "continuationProved":false,"oldConversationPreserved":true})
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
        if matches!(
            record.phase,
            Phase::Prepared | Phase::ResponsePending | Phase::Created | Phase::Bound
        ) && (record.handoff_event_id.is_none() || record.source_fingerprint.is_none())
        {
            return Ok(AutoStage::SourceWitnessRequired);
        }
        match record.phase {
            Phase::Submitting => return Ok(AutoStage::SubmissionUnknown),
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
    if !stage.bytes().all(|b| b.is_ascii_uppercase() || b == b'_') {
        return Err("WEB_AUTO_STATUS_INVALID".into());
    }
    let reason = reason.map(|value| {
        if value.len() <= 128
            && value
                .bytes()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
        {
            value.to_owned()
        } else {
            "WEB_AUTO_ATTENTION".into()
        }
    });
    let store = ProjectStore::open(workspace, project)?;
    atomic(
        &store.root.join("auto-status.json"),
        &AutoStatus {
            schema_version: 1,
            project_id: project.into(),
            tick_utc: now(),
            stage: stage.into(),
            target_generation,
            reason,
        },
    )
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
    if status.schema_version != 1
        || status.project_id
            != store
                .root
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("")
        || status.tick_utc == 0
    {
        return Err("WEB_AUTO_STATUS_INVALID".into());
    }
    Ok(Some(status))
}

pub(super) fn status_read_only(workspace: &Path, project: &str) -> Result<Value, String> {
    let store = ProjectStore::open_existing(workspace, project)?;
    status_from_store(&store, project)
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
    if !valid_id(source_task_id) {
        return Err("WEB_HANDOFF_TASK_INVALID".into());
    }
    let store = ProjectStore::open(workspace, project)?;
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
    let meter = WebContextStore::open(workspace.join(".catdesk/web-context"))?;
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
    let snapshot = runtime::snapshot_conversation(&wake, project, &source)?;
    source_ready_for_handoff(&snapshot)?;
    let source_key = key(project, &source);
    let previous = meter.active_key(project)?;
    meter.bind_observed_target(source_key.clone(), previous.as_ref())?;
    super::observe::ingest(&meter, &source_key, snapshot, now(), || {
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

pub(super) fn resume_saved_handoff_request(
    workspace: &Path,
    project: &str,
) -> Result<Value, String> {
    let store = ProjectStore::open(workspace, project)?;
    let request = store
        .handoff_request()?
        .ok_or("WEB_HANDOFF_REQUEST_REQUIRED")?;
    if paired(workspace, project)? != request.source {
        return Err("WEB_HANDOFF_SOURCE_CHANGED".into());
    }
    let task = request.source_task_id;
    drop(store);
    request_handoff(workspace, project, &task)
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
    let meter = WebContextStore::open(workspace.join(".catdesk/web-context"))?;
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
    let snapshot = runtime::snapshot_conversation(&wake, project, &request.source)?;
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
    let meter = WebContextStore::open(workspace.join(".catdesk/web-context"))?;
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
            if reason == "WEB_ROLLOVER_SOURCE_PROGRESS_RECONCILIATION_REQUIRED" {
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
    let meter = WebContextStore::open(workspace.join(".catdesk/web-context"))?;
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
) -> Result<(), String> {
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
        let snapshot = runtime::snapshot_conversation(wake, &record.project_id, &record.source)
            .map_err(|reason| {
                if reason == "BROWSER_SESSION_BUSY" {
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
        record.reconciliation_reason = Some(reason.clone());
        store.save(record)?;
        return Err(reason);
    }
    record.source_snapshot_verified = true;
    store.save(record)?;
    Ok(())
}

pub(super) fn execute(workspace: &Path, project: &str) -> Result<Value, String> {
    let store = ProjectStore::open(workspace, project)?;
    let mut record = store.active()?.ok_or("WEB_ROLLOVER_INTENT_REQUIRED")?;
    if record.phase == Phase::Submitting {
        return Err("WEB_BOOTSTRAP_OUTCOME_UNKNOWN_NO_RESEND".into());
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
    if record.phase == Phase::Prepared {
        confirm_request_enabled(workspace, &record)?;
        if paired(workspace, project)? != record.source {
            return Err("WEB_ROLLOVER_SOURCE_CHANGED".into());
        }
        let source = record.source.clone();
        let intent_id = record.intent_id.clone();
        let handoff = record.handoff.clone();
        let receipt = runtime::bootstrap_submit_conversation(
            &wake,
            project,
            &intent_id,
            &source,
            &handoff,
            || {
                if paired(workspace, project)? != source {
                    return Err("WEB_ROLLOVER_SOURCE_CHANGED".into());
                }
                catdesk_wake::development_runtime::validate_selected_target_isolation(
                    workspace,
                    wake.root(),
                    &source.url,
                )?;
                confirm_request_enabled(workspace, &record)?;
                record.phase = Phase::Submitting;
                store.save(&record)
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
        let Some(completed) =
            runtime::bootstrap_probe_completion(&wake, &record.source, pending, &record.handoff)?
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
    if record.phase == Phase::Created && record.handoff_event_id.is_some() {
        verify_source_handoff_still_latest(workspace, &store, &wake, &mut record, &destination)?;
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
    let bound = checkpoint_bound_with(
        &store,
        &mut record,
        &destination,
        || paired(workspace, project),
        |url, digest, generation| {
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
    use catdesk_wake::{protocol::Event, store::Receipt};
    fn fixture() -> PathBuf {
        let path = std::env::temp_dir().join(format!("web-rollover-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        path
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

    #[test]
    fn persisted_completed_continuation_anchors_next_generation() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join(format!("web-continuation-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let request = handoff_request(&root);
        let current = Target::new(2, "https://chatgpt.com/c/new".into()).unwrap();
        let mut record = record();
        record.phase = Phase::ContinuationQueued;
        record.receipt = Some(receipt_for(&record));
        record.bound_target = Some(current.clone());
        record.handoff_event_id = Some(request.event_id.clone());
        record.continuation_event_id = format!("web-continue-{}", record.intent_id);
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
                project_id: "alpha".into(),
                workspace: root.clone(),
                git_identity: "fixture".into(),
                verification_profile: "fixture".into(),
                codex_thread_id: Some("native-thread-a".into()),
                chatgpt_target_url: Some(current.url.clone()),
                chatgpt_target_sha256: Some(current.digest.clone()),
                chatgpt_target_history: vec![request.source.url.clone(), current.url.clone()],
            })
            .unwrap();

        let wake = Store::open_scoped_for_test(&root.join("wake"), &root).unwrap();
        wake.initialize().unwrap();
        assert_eq!(
            wake.set_target("alpha", 0, &request.source.url).unwrap(),
            request.source
        );
        assert_eq!(wake.set_target("alpha", 1, &current.url).unwrap(), current);
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
        assert_eq!(
            completed_continuation_task_anchor(&root, &wake, "alpha", &current).unwrap(),
            Some("task-a".into())
        );
        let meter = WebContextStore::open(root.join(".catdesk/web-context")).unwrap();
        let current_key = key("alpha", &current);
        meter
            .bind_observed_target(current_key.clone(), None)
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
        policy.automatic_rollover_enabled = false;
        meter.configure_policy("alpha", policy).unwrap();
        assert_eq!(
            confirm_request_enabled(&root, &record).unwrap_err(),
            "WEB_ROLLOVER_NOT_DUE_OR_DISABLED"
        );
        assert_eq!(record.phase, Phase::Prepared);
    }
}
