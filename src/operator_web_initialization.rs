//! Explicit local acceptance of a completed fresh-chat initialization.
//! This proves eligibility for NEW work, never predecessor task resumption.
use super::*;
use sha2::{Digest, Sha256};

pub(super) const ACK: &str = "NEW_TASKS_ONLY_PRESERVE_PREDECESSOR_OWNERS";

#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Acceptance {
    schema_version: u32,
    purpose: String,
    project_id: String,
    intent_id: String,
    target: Target,
    record_sha256: String,
    registration_sha256: String,
    continuation_proof_sha256: String,
    quiescent_progress_sha256: String,
    closed_work_record_count: u64,
    accepted_utc: u64,
    acknowledgement: String,
}

fn hash(value: &impl Serialize) -> Result<String, String> {
    serde_json::to_vec(value)
        .map(|bytes| format!("{:x}", Sha256::digest(bytes)))
        .map_err(|_| "WEB_INITIALIZATION_EVIDENCE_INVALID".into())
}

fn path(store: &ProjectStore, record: &Record) -> Result<PathBuf, String> {
    if !valid_id(&record.intent_id) {
        return Err("WEB_INITIALIZATION_INTENT_INVALID".into());
    }
    Ok(store
        .root
        .join(format!("initialization-{}.json", record.intent_id)))
}

fn registration_hash(registered: &AutonomousProjectV1) -> Result<String, String> {
    hash(registered)
}

fn current_context(workspace: &Path, project: &str, current: &Target) -> Result<(), String> {
    let meter = WebContextStore::open_read_only(workspace.join(".catdesk/web-context"))?;
    let pressure = meter
        .active_pressure(project)?
        .ok_or("WEB_INITIALIZATION_CURRENT_CONTEXT_REQUIRED")?;
    if pressure.key != key(project, current) || pressure.rollover_intent.is_some() {
        return Err("WEB_INITIALIZATION_CURRENT_CONTEXT_REQUIRED".into());
    }
    Ok(())
}

fn completed_proof(wake: &Store, record: &Record, current: &Target) -> Result<String, String> {
    if record.task_handoff.is_some()
        || record.handoff_event_id.is_some()
        || record.source_fingerprint.is_some()
        || record.source_snapshot_verified
        || record.quarantine.is_some()
        || record.reconciliation_reason.is_some()
        || record.context_meter_status != ContextMeterStatus::Recorded
    {
        return Err("WEB_INITIALIZATION_OPERATOR_BOOTSTRAP_REQUIRED".into());
    }
    let (delivery, timer) = completed_continuation(
        wake,
        record,
        current,
        "WEB_INITIALIZATION_CONTINUATION_REQUIRED",
    )?;
    hash(&(delivery, timer))
}

pub(super) fn accepted(
    workspace: &Path,
    store: &ProjectStore,
    wake: &Store,
    registered: &AutonomousProjectV1,
    record: &Record,
    current: &Target,
) -> Result<bool, String> {
    let path = path(store, record)?;
    if !path
        .try_exists()
        .map_err(|_| "WEB_INITIALIZATION_EVIDENCE_UNAVAILABLE")?
    {
        return Ok(false);
    }
    plain(&path, false)?;
    if fs::metadata(&path)
        .map_err(|_| "WEB_INITIALIZATION_EVIDENCE_UNAVAILABLE")?
        .len()
        > 8192
    {
        return Err("WEB_INITIALIZATION_EVIDENCE_INVALID".into());
    }
    let saved: Acceptance = read(&path).map_err(|_| "WEB_INITIALIZATION_EVIDENCE_INVALID")?;
    current_context(workspace, &record.project_id, current)?;
    if !registered
        .chatgpt_target_history
        .contains(&record.source.url)
    {
        return Err("WEB_INITIALIZATION_PREDECESSOR_HISTORY_REQUIRED".into());
    }
    if saved.schema_version != 1
        || saved.purpose != "FRESH_WEB_INITIALIZATION_NEW_TASKS_ONLY_V1"
        || saved.project_id != record.project_id
        || saved.intent_id != record.intent_id
        || saved.target != *current
        || saved.record_sha256 != hash(record)?
        || saved.registration_sha256 != registration_hash(registered)?
        || saved.continuation_proof_sha256 != completed_proof(wake, record, current)?
        || !valid_sha256(&saved.quiescent_progress_sha256)
        || saved.accepted_utc == 0
        || saved.acknowledgement != ACK
    {
        return Err("WEB_INITIALIZATION_EVIDENCE_CHANGED".into());
    }
    Ok(true)
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

pub(super) fn accept(
    workspace: &Path,
    project: &str,
    intent: &str,
    generation: u64,
    target_digest: &str,
    acknowledgement: &str,
) -> Result<Value, String> {
    if acknowledgement != ACK || !valid_id(intent) || !valid_sha256(target_digest) {
        return Err("WEB_INITIALIZATION_EXPLICIT_ACK_REQUIRED".into());
    }
    let store = ProjectStore::open_existing(workspace, project)?;
    let record = store.active()?.ok_or("WEB_ROLLOVER_INTENT_REQUIRED")?;
    let current = paired(workspace, project)?;
    if record.intent_id != intent
        || current.generation != generation
        || current.digest != target_digest
    {
        return Err("WEB_INITIALIZATION_TARGET_OR_INTENT_CHANGED".into());
    }
    let root = crate::wake_protocol_client::wake_root_for_workspace(workspace)?;
    let wake = Store::open(&root)?;
    catdesk_wake::development_runtime::validate_selected_target_isolation(
        workspace,
        wake.root(),
        &current.url,
    )?;
    let registered = registered_project(workspace, project)?;
    if !registered
        .chatgpt_target_history
        .contains(&record.source.url)
    {
        return Err("WEB_INITIALIZATION_PREDECESSOR_HISTORY_REQUIRED".into());
    }
    if accepted(workspace, &store, &wake, &registered, &record, &current)? {
        return Ok(summary(project, intent, true));
    }
    let continuation_proof = completed_proof(&wake, &record, &current)?;
    current_context(workspace, project, &current)?;
    if store.handoff_request()?.is_some() {
        return Err("WEB_INITIALIZATION_HANDOFF_REQUEST_PENDING".into());
    }
    let work = WorkLedger::status(workspace, project);
    if !matches!(
        work["status"].as_str(),
        Some("UNOBSERVED" | "DURABLE_SNAPSHOT")
    ) || !work["active"].is_null()
    {
        return Err("WEB_INITIALIZATION_WEB_WORK_PENDING".into());
    }
    let registry =
        AutonomousProjectRegistryStoreV1::open_read_only(workspace.join(".catdesk/projects"))
            .map_err(|_| "WEB_INITIALIZATION_REGISTRY_UNAVAILABLE")?;
    registry.with_registration_lock(|| {
        if registered_project(workspace, project)? != registered {
            return Err("WEB_INITIALIZATION_REGISTRATION_CHANGED".into());
        }
        // Existing registry→Wake order and queue lock cover this local commit.
        // The continuation is the witness; it is NOT a predecessor handoff.
        wake.with_quiescent_source(
            project,
            &current,
            &record.continuation_event_id,
            &record.continuation_event_id,
            &[],
            |progress, proof| {
                if proof != continuation_proof {
                    return Err("WEB_INITIALIZATION_CONTINUATION_CHANGED".into());
                }
                create_private_once(
                    &path(&store, &record)?,
                    &Acceptance {
                        schema_version: 1,
                        purpose: "FRESH_WEB_INITIALIZATION_NEW_TASKS_ONLY_V1".into(),
                        project_id: project.into(),
                        intent_id: intent.into(),
                        target: current.clone(),
                        record_sha256: hash(&record)?,
                        registration_sha256: registration_hash(&registered)?,
                        continuation_proof_sha256: proof.into(),
                        quiescent_progress_sha256: progress.into(),
                        closed_work_record_count: work["recordCount"]
                            .as_u64()
                            .ok_or("WEB_INITIALIZATION_EVIDENCE_INVALID")?,
                        accepted_utc: now(),
                        acknowledgement: ACK.into(),
                    },
                )
            },
        )
    })?;
    if !accepted(workspace, &store, &wake, &registered, &record, &current)? {
        return Err("WEB_INITIALIZATION_EVIDENCE_UNAVAILABLE".into());
    }
    Ok(summary(project, intent, false))
}

fn summary(project: &str, intent: &str, replayed: bool) -> Value {
    json!({"action":"web_initialization_accept", "projectId":project, "intentId":intent,
        "status":"ACCEPTED_NEW_TASKS_ONLY", "replayed":replayed,
        "predecessorTaskResumeAuthorized":false, "executionGranted":false,
        "nativeSessionAction":"NONE", "browserAction":"NONE", "bindingChanged":false})
}
