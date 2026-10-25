//! Explicit operator disposition of an unknown rollover. The original record,
//! handoff and browser history remain untouched. No provider outcome is inferred.

use super::*;
use crate::delegated::{
    autonomy_runtime::acquire_project_writer_lock, autonomy_state::AutonomousStateStoreV1,
};
use crate::web_conversation_context::RolloverIntent;
use sha2::{Digest, Sha256};

pub(super) const ACKNOWLEDGEMENT: &str = "ABANDONED_UNKNOWN_NO_REPLAY";
const MAX_BYTES: usize = 48 * 1024;
const TTL_SECONDS: u64 = 600;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Evidence {
    project_id: String,
    intent_id: String,
    source: Target,
    source_task_id: String,
    original_record_sha256: String,
    original_handoff_sha256: String,
    source_task_proof_sha256: String,
    source_progress_sha256: String,
    registration_sha256: String,
    context_intent: RolloverIntent,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Prepared {
    schema_version: u32,
    resolution_id: String,
    confirmation_token: String,
    expires_at_unix: u64,
    acknowledgement: String,
    evidence: Evidence,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Disposition {
    schema_version: u32,
    status: String,
    prepared: Prepared,
    confirmed_at_unix: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Completion {
    schema_version: u32,
    status: String,
    disposition_sha256: String,
}

fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn bounded_bytes(path: &Path) -> Result<Vec<u8>, String> {
    plain(path, false)?;
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|_| "WEB_QUARANTINE_EVIDENCE_UNAVAILABLE")?
        .take(MAX_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "WEB_QUARANTINE_EVIDENCE_UNAVAILABLE")?;
    if bytes.is_empty() || bytes.len() > MAX_BYTES {
        return Err("WEB_QUARANTINE_EVIDENCE_INVALID".into());
    }
    Ok(bytes)
}

fn read_checked<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, String> {
    serde_json::from_slice(&bounded_bytes(path)?)
        .map_err(|_| "WEB_QUARANTINE_EVIDENCE_INVALID".into())
}

/// Call only while holding ProjectStore's exclusive project lock. Publish a
/// complete atomic record so process interruption cannot expose half a receipt.
/// Existing immutable evidence is compared, never overwritten on retry.
fn persist_once<T: Serialize + serde::de::DeserializeOwned + PartialEq>(
    path: &Path,
    value: &T,
) -> Result<(), String> {
    if path
        .try_exists()
        .map_err(|_| "WEB_QUARANTINE_EVIDENCE_UNAVAILABLE")?
    {
        return if read_checked::<T>(path)? == *value {
            Ok(())
        } else {
            Err("WEB_QUARANTINE_EVIDENCE_CHANGED".into())
        };
    }
    if serde_json::to_vec_pretty(value)
        .map_err(|_| "WEB_QUARANTINE_EVIDENCE_INVALID")?
        .len()
        > MAX_BYTES
    {
        return Err("WEB_QUARANTINE_EVIDENCE_INVALID".into());
    }
    atomic(path, value)?;
    if read_checked::<T>(path)? != *value {
        return Err("WEB_QUARANTINE_EVIDENCE_CHANGED".into());
    }
    Ok(())
}

fn path(store: &ProjectStore, kind: &str, id: &str) -> PathBuf {
    store.root.join(format!("quarantine-{kind}-{id}.json"))
}

fn original_hashes(store: &ProjectStore, record: &Record) -> Result<(String, String), String> {
    Ok((
        sha(&bounded_bytes(
            &store.root.join(format!("{}.json", record.intent_id)),
        )?),
        sha(&bounded_bytes(
            &store
                .root
                .join(format!("handoff-{}.json", record.intent_id)),
        )?),
    ))
}

fn validate_prepared(prepared: &Prepared) -> Result<(), String> {
    if prepared.schema_version != 1
        || prepared.acknowledgement != ACKNOWLEDGEMENT
        || !valid_id(&prepared.resolution_id)
        || !valid_id(&prepared.confirmation_token)
        || !valid_id(&prepared.evidence.project_id)
        || !valid_id(&prepared.evidence.intent_id)
        || !valid_id(&prepared.evidence.source_task_id)
        || [
            &prepared.evidence.original_record_sha256,
            &prepared.evidence.original_handoff_sha256,
            &prepared.evidence.source_task_proof_sha256,
            &prepared.evidence.source_progress_sha256,
            &prepared.evidence.registration_sha256,
        ]
        .iter()
        .any(|value| {
            value.len() != 64
                || !value
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        })
        || prepared.expires_at_unix == 0
        || prepared.evidence.context_intent.status != "REQUESTED"
        || prepared.evidence.context_intent.intent_id != prepared.evidence.intent_id
        || prepared.evidence.context_intent.source_task_id != prepared.evidence.source_task_id
        || prepared.evidence.context_intent.key
            != key(&prepared.evidence.project_id, &prepared.evidence.source)
    {
        return Err("WEB_QUARANTINE_DISPOSITION_INVALID".into());
    }
    prepared.evidence.source.validate()
}

fn check_confirmation(
    prepared: &Prepared,
    project: &str,
    intent: &str,
    resolution: &str,
    token: &str,
    already_started: bool,
    now_unix: u64,
) -> Result<(), String> {
    validate_prepared(prepared)?;
    if prepared.resolution_id != resolution
        || prepared.confirmation_token != token
        || prepared.evidence.project_id != project
        || prepared.evidence.intent_id != intent
    {
        return Err("WEB_QUARANTINE_CONFIRMATION_CHANGED".into());
    }
    if !already_started && prepared.expires_at_unix < now_unix {
        return Err("WEB_QUARANTINE_CONFIRMATION_EXPIRED".into());
    }
    Ok(())
}

fn result(record: &Record, resolution: &str, replayed: bool) -> Value {
    json!({"action":"quarantine_disposition_confirm","status":ACKNOWLEDGEMENT,
        "projectId":record.project_id,"intentId":record.intent_id,"resolutionId":resolution,
        "originalJournalPreserved":true,"providerOutcome":"UNKNOWN","receiptCreated":false,
        "continuationAuthorized":false,"bindingChanged":false,"messagesSent":0,"replayed":replayed})
}

fn matching_disposition(
    store: &ProjectStore,
    record: &Record,
) -> Result<Option<Disposition>, String> {
    let candidate = path(store, "disposition", &record.intent_id);
    if !candidate
        .try_exists()
        .map_err(|_| "WEB_QUARANTINE_EVIDENCE_UNAVAILABLE")?
    {
        return Ok(None);
    }
    let disposition: Disposition = read_checked(&candidate)?;
    validate_prepared(&disposition.prepared)?;
    let evidence = &disposition.prepared.evidence;
    let hashes = original_hashes(store, record)?;
    if disposition.schema_version != 1
        || disposition.status != ACKNOWLEDGEMENT
        || disposition.confirmed_at_unix == 0
        || record.phase != Phase::Quarantined
        || record.receipt.is_some()
        || record.bound_target.is_some()
        || evidence.project_id != record.project_id
        || evidence.intent_id != record.intent_id
        || evidence.source != record.source
        || evidence.source_task_id != record.source_task_id
        || evidence.original_record_sha256 != hashes.0
        || evidence.original_handoff_sha256 != hashes.1
    {
        return Err("WEB_QUARANTINE_DISPOSITION_CHANGED".into());
    }
    Ok(Some(disposition))
}

pub(super) fn completed(store: &ProjectStore, record: &Record) -> Result<Option<String>, String> {
    let Some(disposition) = matching_disposition(store, record)? else {
        return Ok(None);
    };
    let completion_path = path(store, "complete", &record.intent_id);
    if !completion_path
        .try_exists()
        .map_err(|_| "WEB_QUARANTINE_EVIDENCE_UNAVAILABLE")?
    {
        return Ok(None);
    }
    let completion: Completion = read_checked(&completion_path)?;
    if completion.schema_version != 1
        || completion.status != ACKNOWLEDGEMENT
        || completion.disposition_sha256
            != sha(&bounded_bytes(&path(
                store,
                "disposition",
                &record.intent_id,
            ))?)
    {
        return Err("WEB_QUARANTINE_DISPOSITION_CHANGED".into());
    }
    Ok(Some(disposition.prepared.resolution_id))
}

pub(super) fn context_allows_new_work(
    store: &ProjectStore,
    record: &Record,
    pressure: Option<&PressureStatus>,
) -> Result<bool, String> {
    let Some(id) = completed(store, record)? else {
        return Ok(false);
    };
    Ok(pressure.is_some_and(|value| {
        value.key == key(&record.project_id, &record.source)
            && value.rollover_intent.as_ref().is_some_and(|intent| {
                intent.intent_id == record.intent_id
                    && intent.abandonment_resolution_id() == Some(id.as_str())
            })
    }))
}

pub(super) fn status(store: &ProjectStore, record: &Record) -> Result<Option<Value>, String> {
    let Some(saved) = matching_disposition(store, record)? else {
        return Ok(None);
    };
    Ok(Some(
        json!({"status":ACKNOWLEDGEMENT,"resolutionId":saved.prepared.resolution_id,
        "complete":completed(store, record)?.is_some(),"providerOutcome":"UNKNOWN",
        "originalJournalPreserved":true,"replayAllowed":false}),
    ))
}

pub(super) fn validate_new_source_task(
    store: &ProjectStore,
    record: &Record,
    wake: &Store,
    source_task: &str,
) -> Result<(), String> {
    if completed(store, record)?.is_none() {
        return Ok(());
    }
    let saved = matching_disposition(store, record)?.ok_or("WEB_QUARANTINE_DISPOSITION_CHANGED")?;
    let delivery = wake
        .delivery(source_task)?
        .ok_or("WEB_QUARANTINE_NEW_COMPLETED_TASK_REQUIRED")?;
    let timer = wake
        .timer(source_task)?
        .ok_or("WEB_QUARANTINE_NEW_COMPLETED_TASK_REQUIRED")?;
    if source_task == record.source_task_id
        || record.handoff_event_id.as_deref() == Some(source_task)
        || delivery.event.created_utc <= saved.confirmed_at_unix
        || delivery.event.project_id != record.project_id
        || delivery.target != record.source
        || delivery.phase != DeliveryPhase::Sent
        || delivery.receipt.is_none()
        || timer.project_id != record.project_id
        || timer.target_generation != record.source.generation
        || timer.target_digest != record.source.digest
        || timer.response_state != ResponseState::Complete
        || timer.completed_utc.is_none()
    {
        return Err("WEB_QUARANTINE_NEW_COMPLETED_TASK_REQUIRED".into());
    }
    Ok(())
}

fn native_quiescent(workspace: &Path, registered: &AutonomousProjectV1) -> Result<(), String> {
    // A managed native binding is deliberately not stopped or assumed idle.
    if registered.codex_thread_id.is_some() || registered.verification_profile != "web_only_v1" {
        return Err("WEB_QUARANTINE_NATIVE_OWNER_REQUIRES_HANDOFF".into());
    }
    let work = WorkLedger::status(workspace, &registered.project_id);
    if !matches!(
        work["status"].as_str(),
        Some("UNOBSERVED" | "DURABLE_SNAPSHOT")
    ) || !work["active"].is_null()
    {
        return Err("WEB_QUARANTINE_ACTIVE_WORK".into());
    }
    let tasks = workspace
        .join(".catdesk/project-mcp-tasks")
        .join(&registered.project_id);
    if tasks
        .try_exists()
        .map_err(|_| "WEB_QUARANTINE_TASK_STATE_UNAVAILABLE")?
    {
        plain(&tasks, true)?;
        if fs::read_dir(&tasks)
            .map_err(|_| "WEB_QUARANTINE_TASK_STATE_UNAVAILABLE")?
            .next()
            .is_some()
        {
            return Err("WEB_QUARANTINE_TASK_RECOVERY_REQUIRED".into());
        }
    }
    let autonomy = registered.workspace.join(".catdesk/autonomy");
    if autonomy
        .try_exists()
        .map_err(|_| "WEB_QUARANTINE_TASK_STATE_UNAVAILABLE")?
    {
        plain(&autonomy, true)?;
        let states = AutonomousStateStoreV1::open(&autonomy)
            .map_err(|_| "WEB_QUARANTINE_TASK_STATE_UNAVAILABLE")?;
        for session in states
            .list_sessions_strict()
            .map_err(|_| "WEB_QUARANTINE_TASK_STATE_UNAVAILABLE")?
        {
            let contract = states
                .load_contract(&session.session_id)
                .map_err(|_| "WEB_QUARANTINE_TASK_STATE_UNAVAILABLE")?;
            if contract.project_id == registered.project_id
                && (session.active || !session.state.is_terminal())
            {
                return Err("WEB_QUARANTINE_ACTIVE_WORK".into());
            }
        }
    }
    Ok(())
}

fn with_gates(
    workspace: &Path,
    project: &str,
    intent: &str,
    action: impl FnOnce(
        &ProjectStore,
        &Record,
        &AutonomousProjectV1,
        &str,
        &str,
    ) -> Result<Value, String>,
) -> Result<Value, String> {
    if !valid_id(project) || !valid_id(intent) {
        return Err("WEB_QUARANTINE_REQUEST_INVALID".into());
    }
    let store = ProjectStore::open_existing(workspace, project)?;
    let record = store.active()?.ok_or("WEB_ROLLOVER_INTENT_REQUIRED")?;
    if record.intent_id != intent
        || record.phase != Phase::Quarantined
        || record.receipt.is_some()
        || record.bound_target.is_some()
    {
        return Err("WEB_QUARANTINE_EXACT_UNKNOWN_INTENT_REQUIRED".into());
    }
    let registered = registered_project(workspace, project)?;
    let _writer = acquire_project_writer_lock(&registered.workspace)
        .map_err(|_| "WEB_QUARANTINE_NATIVE_QUIESCENCE_UNPROVEN")?;
    native_quiescent(workspace, &registered)?;
    let current = paired(workspace, project)?;
    if current != record.source {
        return Err("WEB_QUARANTINE_SOURCE_CHANGED".into());
    }
    let root = crate::wake_protocol_client::wake_root_for_workspace(workspace)?;
    let wake = Store::open(&root)?;
    catdesk_wake::development_runtime::validate_selected_target_isolation(
        workspace,
        &root,
        &current.url,
    )?;
    let _browser = runtime::acquire_browser_quiescence(&wake)?;
    let registry =
        AutonomousProjectRegistryStoreV1::open_read_only(workspace.join(".catdesk/projects"))
            .map_err(|_| "WEB_QUARANTINE_REGISTRY_UNAVAILABLE")?;
    registry.with_registration_lock(|| {
        if registered_project(workspace, project)? != registered {
            return Err("WEB_QUARANTINE_REGISTRATION_CHANGED".into());
        }
        native_quiescent(workspace, &registered)?;
        let request = store
            .handoff_request()?
            .ok_or("WEB_HANDOFF_REQUEST_REQUIRED")?;
        if !handoff_owner_matches_record(&request, &record, &registered) {
            return Err("WEB_QUARANTINE_HANDOFF_CHANGED".into());
        }
        // The queue lock remains held only for the bounded local journal write.
        let resume_id = task_resume_id(&record);
        wake.with_quiescent_source(
            project,
            &current,
            &request.event_id,
            &record.source_task_id,
            &[&record.continuation_event_id, &resume_id],
            |fingerprint, source_task_proof| {
                action(&store, &record, &registered, fingerprint, source_task_proof)
            },
        )
    })
}

fn evidence(
    workspace: &Path,
    store: &ProjectStore,
    record: &Record,
    registered: &AutonomousProjectV1,
    progress: &str,
    task_proof: &str,
) -> Result<Evidence, String> {
    let hashes = original_hashes(store, record)?;
    let pressure = WebContextStore::open_read_only(workspace.join(".catdesk/web-context"))?
        .active_pressure(&record.project_id)?
        .ok_or("WEB_QUARANTINE_CONTEXT_REQUIRED")?;
    let context_intent = pressure
        .rollover_intent
        .ok_or("WEB_QUARANTINE_CONTEXT_REQUIRED")?;
    Ok(Evidence {
        project_id: record.project_id.clone(),
        intent_id: record.intent_id.clone(),
        source: record.source.clone(),
        source_task_id: record.source_task_id.clone(),
        original_record_sha256: hashes.0,
        original_handoff_sha256: hashes.1,
        source_task_proof_sha256: task_proof.into(),
        source_progress_sha256: progress.into(),
        registration_sha256: sha(
            &serde_json::to_vec(registered).map_err(|_| "WEB_QUARANTINE_REGISTRATION_CHANGED")?
        ),
        context_intent,
    })
}

fn prepared_result(prepared: &Prepared, recovery_only: bool) -> Value {
    json!({"action":"quarantine_disposition_prepare","status":"PREFLIGHT_READY",
        "projectId":prepared.evidence.project_id,"intentId":prepared.evidence.intent_id,
        "resolutionId":prepared.resolution_id,"confirmationToken":prepared.confirmation_token,
        "expiresAtUnix":prepared.expires_at_unix,"recoveryOnly":recovery_only,
        "originalRecordSha256":prepared.evidence.original_record_sha256,
        "originalHandoffSha256":prepared.evidence.original_handoff_sha256,
        "sourceGeneration":prepared.evidence.source.generation,"sourceDigest":prepared.evidence.source.digest,
        "outcome":ACKNOWLEDGEMENT,"workAdmissionChanged":false,"messagesSent":0})
}

pub(super) fn prepare(
    workspace: &Path,
    project: &str,
    intent: &str,
    resolution: &str,
    acknowledgement: &str,
) -> Result<Value, String> {
    if !valid_id(resolution) || acknowledgement != ACKNOWLEDGEMENT {
        return Err("WEB_QUARANTINE_EXPLICIT_ACKNOWLEDGEMENT_REQUIRED".into());
    }
    with_gates(
        workspace,
        project,
        intent,
        |store, record, registered, progress, task_proof| {
            if let Some(saved) = matching_disposition(store, record)? {
                if saved.prepared.resolution_id != resolution {
                    return Err("WEB_QUARANTINE_DISPOSITION_CONFIRM_REQUIRED".into());
                }
                return Ok(prepared_result(&saved.prepared, true));
            }
            let observed = evidence(workspace, store, record, registered, progress, task_proof)?;
            let prepared_path = path(store, "prepared", resolution);
            let prepared = if prepared_path
                .try_exists()
                .map_err(|_| "WEB_QUARANTINE_EVIDENCE_UNAVAILABLE")?
            {
                let saved: Prepared = read_checked(&prepared_path)?;
                validate_prepared(&saved)?;
                if saved.evidence != observed || saved.resolution_id != resolution {
                    return Err("WEB_QUARANTINE_PREFLIGHT_CHANGED".into());
                }
                if saved.expires_at_unix < now() {
                    return Err(
                        "WEB_QUARANTINE_PREFLIGHT_EXPIRED_NEW_RESOLUTION_ID_REQUIRED".into(),
                    );
                }
                saved
            } else {
                let prepared = Prepared {
                    schema_version: 1,
                    resolution_id: resolution.into(),
                    confirmation_token: uuid::Uuid::new_v4().simple().to_string(),
                    expires_at_unix: now().saturating_add(TTL_SECONDS),
                    acknowledgement: acknowledgement.into(),
                    evidence: observed,
                };
                validate_prepared(&prepared)?;
                persist_once(&prepared_path, &prepared)?;
                prepared
            };
            Ok(prepared_result(&prepared, false))
        },
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CommitCheckpoint {
    DispositionRecorded,
    ContextRecorded,
}

fn finish(
    workspace: &Path,
    store: &ProjectStore,
    record: &Record,
    prepared: &Prepared,
    mut checkpoint: impl FnMut(CommitCheckpoint) -> Result<(), String>,
) -> Result<Value, String> {
    validate_prepared(prepared)?;
    let disposition_path = path(store, "disposition", &record.intent_id);
    if let Some(saved) = matching_disposition(store, record)? {
        if saved.prepared != *prepared {
            return Err("WEB_QUARANTINE_DISPOSITION_CHANGED".into());
        }
    } else {
        let disposition = Disposition {
            schema_version: 1,
            status: ACKNOWLEDGEMENT.into(),
            prepared: prepared.clone(),
            confirmed_at_unix: now(),
        };
        persist_once(&disposition_path, &disposition)?;
    }
    // This durable record precedes every admission change. A crash here leaves
    // an explicit pending disposition, not permission to execute or replay.
    checkpoint(CommitCheckpoint::DispositionRecorded)?;
    let meter = WebContextStore::open_nonblocking(workspace.join(".catdesk/web-context"))?;
    meter.abandon_rollover_intent(
        &key(&record.project_id, &record.source),
        &prepared.evidence.context_intent,
        &prepared.resolution_id,
    )?;
    checkpoint(CommitCheckpoint::ContextRecorded)?;
    let completion = Completion {
        schema_version: 1,
        status: ACKNOWLEDGEMENT.into(),
        disposition_sha256: sha(&bounded_bytes(&disposition_path)?),
    };
    let completion_path = path(store, "complete", &record.intent_id);
    if completion_path
        .try_exists()
        .map_err(|_| "WEB_QUARANTINE_EVIDENCE_UNAVAILABLE")?
    {
        if read_checked::<Completion>(&completion_path)? != completion {
            return Err("WEB_QUARANTINE_DISPOSITION_CHANGED".into());
        }
    } else {
        persist_once(&completion_path, &completion)?;
    }
    if completed(store, record)?.as_deref() != Some(prepared.resolution_id.as_str()) {
        return Err("WEB_QUARANTINE_COMPLETION_UNPROVEN".into());
    }
    Ok(result(record, &prepared.resolution_id, false))
}

pub(super) fn confirm(
    workspace: &Path,
    project: &str,
    intent: &str,
    resolution: &str,
    token: &str,
    acknowledgement: &str,
) -> Result<Value, String> {
    if !valid_id(project)
        || !valid_id(intent)
        || !valid_id(resolution)
        || !valid_id(token)
        || acknowledgement != ACKNOWLEDGEMENT
    {
        return Err("WEB_QUARANTINE_EXPLICIT_ACKNOWLEDGEMENT_REQUIRED".into());
    }
    // A completed retry is a readback, even after new work or a subsequent
    // rollover. It cannot reacquire old execution authority or write context.
    {
        let store = ProjectStore::open_existing(workspace, project)?;
        if path(&store, "complete", intent)
            .try_exists()
            .map_err(|_| "WEB_QUARANTINE_EVIDENCE_UNAVAILABLE")?
        {
            let record: Record = read_checked(&store.root.join(format!("{intent}.json")))?;
            if completed(&store, &record)?.is_none() {
                return Err("WEB_QUARANTINE_COMPLETION_UNPROVEN".into());
            }
            let saved = matching_disposition(&store, &record)?
                .ok_or("WEB_QUARANTINE_COMPLETION_UNPROVEN")?;
            check_confirmation(
                &saved.prepared,
                project,
                intent,
                resolution,
                token,
                true,
                now(),
            )?;
            return Ok(result(&record, resolution, true));
        }
    }
    with_gates(
        workspace,
        project,
        intent,
        |store, record, registered, progress, task_proof| {
            let prepared: Prepared = read_checked(&path(store, "prepared", resolution))?;
            let already_started = matching_disposition(store, record)?.is_some();
            check_confirmation(
                &prepared,
                project,
                intent,
                resolution,
                token,
                already_started,
                now(),
            )?;
            let mut observed =
                evidence(workspace, store, record, registered, progress, task_proof)?;
            if already_started
                && observed.context_intent.abandonment_resolution_id() == Some(resolution)
            {
                observed.context_intent.status = "REQUESTED".into();
            }
            if observed != prepared.evidence {
                return Err("WEB_QUARANTINE_PREFLIGHT_CHANGED".into());
            }
            finish(workspace, store, record, &prepared, |_| Ok(()))
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::web_conversation_context::{ContextPressurePolicy, VisibleSourceKind};

    fn fixture() -> (PathBuf, Record, Prepared) {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join(format!("quarantine-disposition-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let root = fs::canonicalize(root).unwrap();
        let source = Target::new(1, "https://chatgpt.com/c/quarantine-alpha".into()).unwrap();
        let (record, prepared) = fixture_record(&root, source, &root);
        (root, record, prepared)
    }

    fn fixture_record(root: &Path, source: Target, native_workspace: &Path) -> (Record, Prepared) {
        let meter = WebContextStore::open(root.join(".catdesk/web-context")).unwrap();
        let key = key("alpha", &source);
        meter.bind_observed_target(key.clone(), None).unwrap();
        meter
            .configure_policy(
                "alpha",
                ContextPressurePolicy {
                    configured_budget_tokens: 10,
                    safety_headroom_tokens: 1,
                    unknown_source_reserve_tokens: 0,
                    automatic_rollover_enabled: true,
                    ..Default::default()
                },
            )
            .unwrap();
        meter
            .record_observation(
                &key,
                Observation {
                    event_id: "source-event",
                    revision: 1,
                    dedup_key: "source-event",
                    source_kind: VisibleSourceKind::Handoff,
                    visible_text: Some(&"review ".repeat(100)),
                    observed_at_unix: 100,
                },
            )
            .unwrap();
        let intent = meter
            .request_rollover_if_due(&key, &source.digest, "source-task", true, 100)
            .unwrap()
            .unwrap();
        let store = ProjectStore::open(root, "alpha").unwrap();
        let record = Record {
            schema_version: 1,
            intent_id: intent.intent_id.clone(),
            project_id: "alpha".into(),
            source_task_id: "source-task".into(),
            source: source.clone(),
            phase: Phase::Quarantined,
            bootstrap_progress: None,
            handoff: "preserved unknown handoff".into(),
            task_handoff: None,
            task_resume_event_id: None,
            message_digest: message_digest(
                "alpha",
                &intent.intent_id,
                &source.url,
                "preserved unknown handoff",
            )
            .unwrap(),
            created_utc: 100,
            receipt: None,
            continuation_event_id: format!("web-continue-{}", intent.intent_id),
            bound_target: None,
            reconciliation_reason: Some("WEB_BOOTSTRAP_UNEXPECTED_SUBMISSION_OBSERVED".into()),
            context_meter_status: ContextMeterStatus::Pending,
            handoff_event_id: Some(format!("web-handoff-{}", intent.intent_id)),
            source_fingerprint: Some("a".repeat(64)),
            source_snapshot_verified: false,
            quarantine: Some(Quarantine {
                observed_conversation_url: "https://chatgpt.com/c/unknown-candidate".into(),
                recorded_utc: 101,
            }),
        };
        store.save(&record).unwrap();
        atomic(&store.root.join("active.json"), &record.intent_id).unwrap();
        let handoff = HandoffRequest {
            schema_version: 2,
            project_id: "alpha".into(),
            intent_id: record.intent_id.clone(),
            source_task_id: record.source_task_id.clone(),
            source: source.clone(),
            native_workspace: native_workspace.to_path_buf(),
            native_thread_id: None,
            event_id: record.handoff_event_id.clone().unwrap(),
            message: handoff_request_message_v2("alpha", &record.intent_id, "source-task", &None),
        };
        store.save_handoff_request(&handoff).unwrap();
        let hashes = original_hashes(&store, &record).unwrap();
        let prepared = Prepared {
            schema_version: 1,
            resolution_id: "operator-disposition".into(),
            confirmation_token: "confirmation".into(),
            expires_at_unix: u64::MAX,
            acknowledgement: ACKNOWLEDGEMENT.into(),
            evidence: Evidence {
                project_id: "alpha".into(),
                intent_id: record.intent_id.clone(),
                source,
                source_task_id: "source-task".into(),
                original_record_sha256: hashes.0,
                original_handoff_sha256: hashes.1,
                source_task_proof_sha256: "b".repeat(64),
                source_progress_sha256: "c".repeat(64),
                registration_sha256: "d".repeat(64),
                context_intent: intent,
            },
        };
        drop(store);
        (record, prepared)
    }

    #[cfg(windows)]
    fn complete_fixture_event(wake: &Store, source: &Target, id: &str, message: &str) {
        use catdesk_wake::store::Receipt;
        let event = wake
            .produce_expected(id, "alpha", "engineering_attention", message, source)
            .unwrap();
        let lease = wake.host_lock().unwrap();
        wake.claim(&event, &lease).unwrap();
        wake.transition(id, lease.owner(), DeliveryPhase::Submitting, None, None)
            .unwrap();
        let receipt = Receipt {
            schema_version: 1,
            event_id: id.into(),
            target_generation: source.generation,
            target_digest: source.digest.clone(),
            message_digest: digest(message),
            sent_utc: now(),
            evidence: "EXACT_USER_MESSAGE_APPENDED".into(),
        };
        wake.record_exact_receipt_timer(id, &receipt).unwrap();
        wake.transition(id, lease.owner(), DeliveryPhase::Sent, Some(receipt), None)
            .unwrap();
        wake.complete_timer(id).unwrap();
    }

    #[cfg(windows)]
    fn public_fixture() -> (PathBuf, PathBuf, PathBuf, Record) {
        let (parent, control, wake_root, source, _) = super::super::tests::paired_handoff_fixture();
        let registered = registered_project(&control, "alpha").unwrap();
        let (record, _) = fixture_record(&control, source, &registered.workspace);
        let wake = Store::open(&wake_root).unwrap();
        complete_fixture_event(&wake, &record.source, &record.source_task_id, "source task");
        // Source and handoff have separate completed turns. The real source
        // guard conservatively refuses equal-second completion boundaries.
        std::thread::sleep(std::time::Duration::from_millis(1100));
        let store = ProjectStore::open_existing(&control, "alpha").unwrap();
        let request = store.handoff_request().unwrap().unwrap();
        complete_fixture_event(&wake, &record.source, &request.event_id, &request.message);
        drop(store);
        (parent, control, wake_root, record)
    }

    #[cfg(windows)]
    #[test]
    fn quarantine_disposition_public_prepare_confirm_proves_first_transition() {
        let (parent, control, wake_root, record) = public_fixture();
        let wake = Store::open(&wake_root).unwrap();
        let project_dir = control.join(".catdesk/web-rollovers/alpha");
        let preserved_paths = [
            project_dir.join(format!("{}.json", record.intent_id)),
            project_dir.join(format!("handoff-{}.json", record.intent_id)),
            project_dir.join("active.json"),
            wake_root.join("config.json"),
            control.join(".catdesk/projects/projects.json"),
        ];
        let preserved: Vec<_> = preserved_paths
            .iter()
            .map(|path| fs::read(path).unwrap())
            .collect();
        let prepared = prepare(
            &control,
            "alpha",
            &record.intent_id,
            "public-first",
            ACKNOWLEDGEMENT,
        )
        .unwrap();
        let token = prepared["confirmationToken"].as_str().unwrap();
        assert_eq!(prepared["status"], "PREFLIGHT_READY");
        assert!(
            project_dir
                .join("quarantine-prepared-public-first.json")
                .exists()
        );
        assert!(
            !project_dir
                .join(format!("quarantine-disposition-{}.json", record.intent_id))
                .exists()
        );
        assert_eq!(
            acquire_web_work_lease(&control, "alpha").err().unwrap(),
            "WEB_WORK_ROLLOVER_PENDING"
        );

        // A valid registry edit between preflight and confirmation is drift,
        // even when its current URL remains paired with the same Wake target.
        let registry =
            AutonomousProjectRegistryStoreV1::open_read_only(control.join(".catdesk/projects"))
                .unwrap();
        let mut changed = registry.load_registry().unwrap();
        changed
            .projects
            .iter_mut()
            .find(|project| project.project_id == "alpha")
            .unwrap()
            .chatgpt_target_history
            .push("https://chatgpt.com/c/fixture-history".into());
        atomic(&preserved_paths[4], &changed).unwrap();
        assert_eq!(
            confirm(
                &control,
                "alpha",
                &record.intent_id,
                "public-first",
                token,
                ACKNOWLEDGEMENT
            )
            .unwrap_err(),
            "WEB_QUARANTINE_PREFLIGHT_CHANGED"
        );
        fs::write(&preserved_paths[4], &preserved[4]).unwrap();

        let source_path = wake_root
            .join("deliveries")
            .join(format!("{}.json", record.source_task_id));
        let original_source = fs::read(&source_path).unwrap();
        let mut source_proof: Delivery = read_checked(&source_path).unwrap();
        source_proof.updated_utc += 1;
        atomic(&source_path, &source_proof).unwrap();
        assert_eq!(
            confirm(
                &control,
                "alpha",
                &record.intent_id,
                "public-first",
                token,
                ACKNOWLEDGEMENT
            )
            .unwrap_err(),
            "WEB_QUARANTINE_PREFLIGHT_CHANGED"
        );
        fs::write(&source_path, original_source).unwrap();

        let registered = registered_project(&control, "alpha").unwrap();
        let writer = acquire_project_writer_lock(&registered.workspace).unwrap();
        assert_eq!(
            confirm(
                &control,
                "alpha",
                &record.intent_id,
                "public-first",
                token,
                ACKNOWLEDGEMENT
            )
            .unwrap_err(),
            "WEB_QUARANTINE_NATIVE_QUIESCENCE_UNPROVEN"
        );
        drop(writer);
        let browser = runtime::acquire_browser_quiescence(&wake).unwrap();
        assert_eq!(
            confirm(
                &control,
                "alpha",
                &record.intent_id,
                "public-first",
                token,
                ACKNOWLEDGEMENT
            )
            .unwrap_err(),
            "BROWSER_SESSION_BUSY"
        );
        drop(browser);

        let result = confirm(
            &control,
            "alpha",
            &record.intent_id,
            "public-first",
            token,
            ACKNOWLEDGEMENT,
        )
        .unwrap();
        assert_eq!(result["status"], ACKNOWLEDGEMENT);
        assert_eq!(result["providerOutcome"], "UNKNOWN");
        assert_eq!(result["replayed"], false);
        for (path, bytes) in preserved_paths.iter().zip(&preserved) {
            assert_eq!(&fs::read(path).unwrap(), bytes);
        }
        let store = ProjectStore::open_existing(&control, "alpha").unwrap();
        assert_eq!(
            completed(&store, &record).unwrap().as_deref(),
            Some("public-first")
        );
        let disposition: Disposition =
            read_checked(&path(&store, "disposition", &record.intent_id)).unwrap();
        assert_eq!(
            disposition.prepared.evidence.source_task_proof_sha256.len(),
            64
        );
        drop(store);
        let lease = acquire_web_work_lease(&control, "alpha").unwrap();
        assert_eq!(
            lease
                .check_source_task(&record.source, &record.source, &record.source_task_id)
                .unwrap_err(),
            "WEB_WORK_ABANDONED_SOURCE_TASK"
        );
        drop(lease);
        assert!(acquire_web_work_lease(&control, "beta").is_ok());
        assert_eq!(
            execute(&control, "alpha").unwrap_err(),
            "WEB_ROLLOVER_QUARANTINED_NO_REPLAY"
        );

        std::thread::sleep(std::time::Duration::from_millis(1100));
        complete_fixture_event(
            &wake,
            &record.source,
            "fresh-completed-source",
            "new authorized source task",
        );
        let store = ProjectStore::open_existing(&control, "alpha").unwrap();
        validate_new_source_task(&store, &record, &wake, "fresh-completed-source").unwrap();
        drop(store);
        let meter = WebContextStore::open(control.join(".catdesk/web-context")).unwrap();
        let source = key("alpha", &record.source);
        let next = meter
            .request_rollover_if_due(
                &source,
                &record.source.digest,
                "fresh-completed-source",
                true,
                now(),
            )
            .unwrap()
            .unwrap();
        assert_ne!(next.intent_id, record.intent_id);
        assert_eq!(
            meter
                .request_rollover_if_due(
                    &source,
                    &record.source.digest,
                    "fresh-completed-source",
                    true,
                    now()
                )
                .unwrap()
                .unwrap(),
            next
        );
        assert_eq!(
            acquire_web_work_lease(&control, "alpha").err().unwrap(),
            "WEB_WORK_ROLLOVER_PENDING"
        );
        drop(meter);
        fs::remove_dir_all(parent).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn quarantine_disposition_public_confirmation_refuses_queued_old_continuation() {
        let (parent, control, wake_root, record) = public_fixture();
        let prepared = prepare(
            &control,
            "alpha",
            &record.intent_id,
            "queued-refusal",
            ACKNOWLEDGEMENT,
        )
        .unwrap();
        let wake = Store::open(&wake_root).unwrap();
        wake.produce_expected(
            &record.continuation_event_id,
            "alpha",
            "engineering_attention",
            "must never run",
            &record.source,
        )
        .unwrap();
        assert_eq!(
            confirm(
                &control,
                "alpha",
                &record.intent_id,
                "queued-refusal",
                prepared["confirmationToken"].as_str().unwrap(),
                ACKNOWLEDGEMENT
            )
            .unwrap_err(),
            "CONFIG_QUARANTINE_CONTINUATION_ALREADY_RECORDED"
        );
        let store = ProjectStore::open_existing(&control, "alpha").unwrap();
        assert!(completed(&store, &record).unwrap().is_none());
        assert!(!path(&store, "disposition", &record.intent_id).exists());
        drop(store);
        assert_eq!(
            acquire_web_work_lease(&control, "alpha").err().unwrap(),
            "WEB_WORK_ROLLOVER_PENDING"
        );
        fs::remove_dir_all(parent).unwrap();
    }

    #[test]
    fn quarantine_disposition_crash_preserves_unknown_and_requires_completion() {
        let (root, record, prepared) = fixture();
        let store = ProjectStore::open_existing(&root, "alpha").unwrap();
        let original =
            bounded_bytes(&store.root.join(format!("{}.json", record.intent_id))).unwrap();
        let active = fs::read(store.root.join("active.json")).unwrap();
        let handoff = fs::read(
            store
                .root
                .join(format!("handoff-{}.json", record.intent_id)),
        )
        .unwrap();
        assert_eq!(
            finish(&root, &store, &record, &prepared, |_| Err(
                "INTERRUPTED".into()
            ))
            .unwrap_err(),
            "INTERRUPTED"
        );
        assert!(completed(&store, &record).unwrap().is_none());
        drop(store);
        assert_eq!(
            acquire_web_work_lease(&root, "alpha").err().unwrap(),
            "WEB_WORK_ROLLOVER_PENDING"
        );
        let store = ProjectStore::open_existing(&root, "alpha").unwrap();
        let result = finish(&root, &store, &record, &prepared, |_| Ok(())).unwrap();
        assert_eq!(result["providerOutcome"], "UNKNOWN");
        assert_eq!(result["receiptCreated"], false);
        let receipt_before = fs::read(path(&store, "complete", &record.intent_id)).unwrap();
        finish(&root, &store, &record, &prepared, |_| Ok(())).unwrap();
        assert_eq!(
            fs::read(path(&store, "complete", &record.intent_id)).unwrap(),
            receipt_before
        );
        assert_eq!(
            bounded_bytes(&store.root.join(format!("{}.json", record.intent_id))).unwrap(),
            original
        );
        assert_eq!(fs::read(store.root.join("active.json")).unwrap(), active);
        assert_eq!(
            fs::read(
                store
                    .root
                    .join(format!("handoff-{}.json", record.intent_id))
            )
            .unwrap(),
            handoff
        );
        drop(store);
        let lease = acquire_web_work_lease(&root, "alpha").unwrap();
        assert_eq!(
            lease
                .check_source_task(&record.source, &record.source, &record.source_task_id)
                .unwrap_err(),
            "WEB_WORK_ABANDONED_SOURCE_TASK"
        );
        drop(lease);
        assert!(acquire_web_work_lease(&root, "beta").is_ok());
        assert_eq!(
            execute(&root, "alpha").unwrap_err(),
            "WEB_ROLLOVER_QUARANTINED_NO_REPLAY"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn quarantine_disposition_context_successor_has_fresh_stable_identity() {
        let (root, record, prepared) = fixture();
        let store = ProjectStore::open_existing(&root, "alpha").unwrap();
        finish(&root, &store, &record, &prepared, |_| Ok(())).unwrap();
        let meter = WebContextStore::open(root.join(".catdesk/web-context")).unwrap();
        let source = key("alpha", &record.source);
        assert_eq!(
            meter
                .request_rollover_if_due(&source, &record.source.digest, "source-task", true, 200)
                .unwrap_err(),
            "WEB_CONTEXT_ABANDONED_TASK_REPLAY_FORBIDDEN"
        );
        let next = meter
            .request_rollover_if_due(
                &source,
                &record.source.digest,
                "new-completed-task",
                true,
                200,
            )
            .unwrap()
            .unwrap();
        assert_ne!(next.intent_id, record.intent_id);
        assert_eq!(
            meter
                .request_rollover_if_due(
                    &source,
                    &record.source.digest,
                    "new-completed-task",
                    true,
                    201
                )
                .unwrap()
                .unwrap(),
            next
        );
        assert!(completed(&store, &record).unwrap().is_some());
        drop(store);
        assert_eq!(
            acquire_web_work_lease(&root, "alpha").err().unwrap(),
            "WEB_WORK_ROLLOVER_PENDING"
        );
        let replay = confirm(
            &root,
            "alpha",
            &record.intent_id,
            &prepared.resolution_id,
            &prepared.confirmation_token,
            ACKNOWLEDGEMENT,
        )
        .unwrap();
        assert_eq!(replay["replayed"], true);
        assert_eq!(meter.pressure(&source).unwrap().rollover_intent, Some(next));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn quarantine_disposition_context_write_alone_never_grants_admission() {
        let (root, record, prepared) = fixture();
        let store = ProjectStore::open_existing(&root, "alpha").unwrap();
        let error = finish(&root, &store, &record, &prepared, |phase| {
            if phase == CommitCheckpoint::ContextRecorded {
                Err("INTERRUPTED_AFTER_CONTEXT".into())
            } else {
                Ok(())
            }
        })
        .unwrap_err();
        assert_eq!(error, "INTERRUPTED_AFTER_CONTEXT");
        assert!(completed(&store, &record).unwrap().is_none());
        drop(store);
        assert_eq!(
            acquire_web_work_lease(&root, "alpha").err().unwrap(),
            "WEB_WORK_ROLLOVER_PENDING"
        );
        let store = ProjectStore::open_existing(&root, "alpha").unwrap();
        finish(&root, &store, &record, &prepared, |_| Ok(())).unwrap();
        assert!(completed(&store, &record).unwrap().is_some());
        drop(store);
        assert!(acquire_web_work_lease(&root, "alpha").is_ok());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn quarantine_disposition_refuses_changed_evidence_and_unacknowledged_outcome() {
        let (root, record, mut prepared) = fixture();
        prepared.acknowledgement = "FAILED".into();
        assert!(validate_prepared(&prepared).is_err());
        prepared.acknowledgement = ACKNOWLEDGEMENT.into();
        let store = ProjectStore::open_existing(&root, "alpha").unwrap();
        finish(&root, &store, &record, &prepared, |_| Ok(())).unwrap();
        let old = store
            .root
            .join(format!("handoff-{}.json", record.intent_id));
        fs::write(old, b"changed after disposition").unwrap();
        assert_eq!(
            completed(&store, &record).unwrap_err(),
            "WEB_QUARANTINE_DISPOSITION_CHANGED"
        );
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn quarantine_disposition_confirmation_is_exact_and_only_pending_commit_survives_expiry() {
        let (root, record, mut prepared) = fixture();
        prepared.expires_at_unix = 200;
        assert_eq!(
            check_confirmation(
                &prepared,
                "alpha",
                &record.intent_id,
                &prepared.resolution_id,
                &prepared.confirmation_token,
                false,
                201
            )
            .unwrap_err(),
            "WEB_QUARANTINE_CONFIRMATION_EXPIRED"
        );
        assert!(
            check_confirmation(
                &prepared,
                "alpha",
                &record.intent_id,
                &prepared.resolution_id,
                &prepared.confirmation_token,
                true,
                201
            )
            .is_ok()
        );
        for (project, intent, resolution, token) in [
            (
                "beta",
                record.intent_id.as_str(),
                prepared.resolution_id.as_str(),
                prepared.confirmation_token.as_str(),
            ),
            (
                "alpha",
                "other-intent",
                prepared.resolution_id.as_str(),
                prepared.confirmation_token.as_str(),
            ),
            (
                "alpha",
                record.intent_id.as_str(),
                "other-resolution",
                prepared.confirmation_token.as_str(),
            ),
            (
                "alpha",
                record.intent_id.as_str(),
                prepared.resolution_id.as_str(),
                "other-token",
            ),
        ] {
            assert_eq!(
                check_confirmation(&prepared, project, intent, resolution, token, true, 201)
                    .unwrap_err(),
                "WEB_QUARANTINE_CONFIRMATION_CHANGED"
            );
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn quarantine_disposition_refuses_native_binding_and_unclosed_task_journals() {
        let (root, _record, _prepared) = fixture();
        let mut project = AutonomousProjectV1 {
            project_id: "alpha".into(),
            workspace: root.clone(),
            git_identity: "https://example.test/alpha.git".into(),
            verification_profile: "web_only_v1".into(),
            codex_thread_id: Some("owned-native-thread".into()),
            chatgpt_target_url: None,
            chatgpt_target_sha256: None,
            chatgpt_target_history: vec![],
        };
        assert_eq!(
            native_quiescent(&root, &project).unwrap_err(),
            "WEB_QUARANTINE_NATIVE_OWNER_REQUIRES_HANDOFF"
        );
        project.codex_thread_id = None;
        assert!(native_quiescent(&root, &project).is_ok());
        let task = root.join(".catdesk/project-mcp-tasks/alpha/old-task");
        fs::create_dir_all(task).unwrap();
        assert_eq!(
            native_quiescent(&root, &project).unwrap_err(),
            "WEB_QUARANTINE_TASK_RECOVERY_REQUIRED"
        );
        fs::remove_dir_all(root).unwrap();
    }
}
