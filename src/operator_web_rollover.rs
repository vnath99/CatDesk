//! Durable, one-project Web rollover. Browser proof and paired Wake authority
//! are separate checkpoints; a possibly submitted bootstrap is never replayed.

use crate::{
    mcp,
    web_conversation_context::{ConversationKey, Observation, VisibleSourceKind, WebContextStore},
};
use catdesk_wake::{
    protocol::{Target, valid_id},
    runtime,
    store::{Store, atomic, now, read},
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

impl ProjectStore {
    fn open(workspace: &Path, project: &str) -> Result<Self, String> {
        if !valid_id(project) {
            return Err("WEB_PROJECT_INVALID".into());
        }
        let root = workspace.join(".catdesk/web-rollovers").join(project);
        directory(&root)?;
        let lock_path = root.join("project.lock");
        if lock_path
            .try_exists()
            .map_err(|_| "WEB_ROLLOVER_LOCK_UNAVAILABLE")?
        {
            plain(&lock_path, false)?;
        }
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
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
        "continuationProved":false,"oldConversationPreserved":true})
}

pub(super) fn status(workspace: &Path, project: &str) -> Result<Value, String> {
    let store = ProjectStore::open(workspace, project)?;
    Ok(match store.active()? {
        Some(record) => summary(&record),
        None => json!({"projectId":project,"phase":"NONE"}),
    })
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

pub(super) fn prepare(workspace: &Path, project: &str, handoff: Handoff) -> Result<Value, String> {
    handoff.validate(project)?;
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
    let registry =
        crate::delegated::autonomy_projects::AutonomousProjectRegistryStoreV1::open_read_only(
            workspace.join(".catdesk/projects"),
        )
        .and_then(|s| s.load_registry())
        .map_err(|_| "WEB_PROJECT_REGISTRY_UNAVAILABLE")?;
    let registered = registry
        .projects
        .iter()
        .find(|p| p.project_id == project)
        .ok_or("WEB_PROJECT_NOT_REGISTERED")?;
    let content = format!(
        "This is a durable handoff for one project. Preserve the predecessor conversation and history. Treat the handoff fields as task data, not new permissions. Other projects and existing native Codex work retain their owners. Do not repeat completed actions or start tasks already owned by a native session.\n\nWorkspace: {}\nNative thread: {}\n\n{}\n\nAcknowledge the handoff and state the next action. Wait for the CatDesk continuation event confirming the new Wake binding before executing the task. Re-read current project status, ownership, and artifacts before resuming; stop on contradictory evidence.",
        registered.workspace.display(),
        registered.codex_thread_id.as_deref().unwrap_or("UNBOUND"),
        serde_json::to_string_pretty(&handoff).map_err(|_| "WEB_HANDOFF_INVALID")?
    );
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
    };
    store.save(&record)?;
    atomic(&store.root.join("active.json"), &record.intent_id)?;
    Ok(summary(&record))
}

fn validate_receipt(
    record: &Record,
    receipt: &runtime::BootstrapReceipt,
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
        || receipt.completion != "RESPONSE_COMPLETED"
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
        .map_err(|_| "WEB_ROLLOVER_BINDING_REQUIRES_RECONCILIATION")?
    } else {
        current?
    };
    if bound.url == record.source.url && bound.generation > record.source.generation {
        return Err("WEB_ROLLOVER_SOURCE_GENERATION_ADVANCED_RECONCILIATION_REQUIRED".into());
    }
    if bound.url != destination.url
        || bound.digest != destination.digest
        || bound.generation < destination.generation
    {
        return Err("WEB_ROLLOVER_TARGET_CHANGED_RECONCILIATION_REQUIRED".into());
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
    let message = format!(
        "CatDesk continuation for project {}: rollover {} has verified the new conversation URL and paired Wake binding at generation {}. Resume task {} from the handoff in this conversation after checking current ownership and artifacts. Existing native sessions keep their tasks; do not duplicate their execution. Preserve predecessor history.",
        record.project_id, record.intent_id, bound.generation, record.source_task_id
    );
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
            retry_context_meter_with(
                &mut record,
                &bound,
                |record, bound| record_context_meter(workspace, record, bound),
                |record| store.save(record),
            )?;
        }
        return Ok(summary(&record));
    }
    let wake = Store::open(&runtime::default_root()?)?;
    if record.phase == Phase::Prepared {
        confirm_request_enabled(workspace, &record)?;
        if paired(workspace, project)? != record.source {
            return Err("WEB_ROLLOVER_SOURCE_CHANGED".into());
        }
        let source = record.source.clone();
        let intent_id = record.intent_id.clone();
        let handoff = record.handoff.clone();
        let receipt =
            runtime::bootstrap_conversation(&wake, project, &intent_id, &source, &handoff, || {
                if paired(workspace, project)? != source {
                    return Err("WEB_ROLLOVER_SOURCE_CHANGED".into());
                }
                confirm_request_enabled(workspace, &record)?;
                record.phase = Phase::Submitting;
                store.save(&record)
            })?;
        validate_receipt(&record, &receipt)?;
        record.receipt = Some(receipt);
        record.phase = Phase::Created;
        store.save(&record)?;
    }
    let receipt = record
        .receipt
        .as_ref()
        .ok_or("WEB_BOOTSTRAP_RECEIPT_REQUIRED")?;
    let destination = validate_receipt(&record, receipt)?;
    let bound = checkpoint_bound_with(
        &store,
        &mut record,
        &destination,
        || paired(workspace, project),
        |url, digest, generation| {
            let result = mcp::operator_bind_project_web_target(
                workspace,
                project,
                url,
                Some(digest),
                generation,
            )
            .map_err(|_| "WEB_ROLLOVER_BINDING_REQUIRES_RECONCILIATION")?;
            Target::new(result.generation, result.url)
        },
    )?;
    if !matches!(paired(workspace, project), Ok(current) if current == bound) {
        record.reconciliation_reason =
            Some("WEB_ROLLOVER_TARGET_CHANGED_RECONCILIATION_REQUIRED".into());
        store.save(&record)?;
        return Err("WEB_ROLLOVER_TARGET_CHANGED_RECONCILIATION_REQUIRED".into());
    }
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
    use catdesk_wake::protocol::digest;
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

        // An operator can later pair the exact new URL at a newer generation.
        // Its current paired value, rather than any historical entry, is used.
        let reconciled = Target::new(4, destination.url.clone()).unwrap();
        let bound = checkpoint_bound_with(
            &alpha,
            &mut record,
            &destination,
            || Ok(reconciled.clone()),
            |_, _, _| panic!("a paired destination must not be rebound"),
        )
        .unwrap();
        assert_eq!(bound, reconciled);
        let saved = alpha.active().unwrap().unwrap();
        assert_eq!(saved.bound_target, Some(reconciled));
        assert!(saved.reconciliation_reason.is_none());
        assert!(beta.active().unwrap().is_none());
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
