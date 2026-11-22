//! Read-only authority for delivery of an already journaled manual Wake.
//! This never creates a grant, task call, event or replacement identity.
use super::*;
use crate::delegated::journal::{ToolCallRecordV1, ToolMutationKind};
use catdesk_wake::{
    protocol::{Event, Target, WakePurpose, valid_id},
    store::{Phase, ResponseState, Store, read},
};

pub(crate) fn authorize_manual_wake_delivery(
    project: &ResolvedProjectScopeV1,
    record: &crate::web_work_state::WorkRecord,
    target: &Target,
    event: &Event,
    call_id: &str,
    require_quiescent: bool,
) -> Result<(), String> {
    let calls = manual_wake_calls(project, record, require_quiescent)?;
    let call = calls
        .get(call_id)
        .ok_or("WEB_TASK_MANUAL_WAKE_CALL_UNPROVEN")?;
    validate_selection(project, record, target, event, call_id, call)
}

/// Recheck the checkpoint from the original assignment just before a send.
/// The project lease still excludes a concurrent scoped tool call here.
pub(crate) fn require_manual_wake_checkpoint(
    project: &ResolvedProjectScopeV1,
    record: &crate::web_work_state::WorkRecord,
) -> Result<(), String> {
    let link = record
        .task_assignment
        .as_ref()
        .ok_or("WEB_TASK_GRANT_REQUIRED")?;
    let directory = task_directory(project, &link.task_id, false)?;
    if !git_checkpoint::load_policy(&directory, &link.assignment_sha256)?.checkpoint {
        return Ok(());
    }
    let inspected = inspect_assignment(project, &directory.join("assignment.json"), 180)?;
    check_record_assignment(project, &inspected, record)?;
    git_checkpoint::require_end_turn_checkpoint(project, &inspected)
}

fn manual_wake_calls(
    project: &ResolvedProjectScopeV1,
    record: &crate::web_work_state::WorkRecord,
    require_quiescent: bool,
) -> Result<std::collections::BTreeMap<String, ToolCallRecordV1>, String> {
    let link = record
        .task_assignment
        .as_ref()
        .ok_or("WEB_TASK_GRANT_REQUIRED")?;
    let key: String = serde_json::from_slice(&read_assignment_bytes(&execution_key_path(
        project, link, false,
    )?)?)
    .map_err(|_| "WEB_TASK_GRANT_INVALID")?;
    let archive = authorized_manual_wake_assignment(project, record, &key)?;
    read_task_calls(project, record, &archive, require_quiescent)
}

fn read_task_calls(
    project: &ResolvedProjectScopeV1,
    record: &crate::web_work_state::WorkRecord,
    archive: &Path,
    require_quiescent: bool,
) -> Result<std::collections::BTreeMap<String, ToolCallRecordV1>, String> {
    let link = record
        .task_assignment
        .as_ref()
        .ok_or("WEB_TASK_GRANT_REQUIRED")?;
    let bytes = read_assignment_bytes(archive)?;
    if catdesk_wake::protocol::digest(
        std::str::from_utf8(&bytes).map_err(|_| "PROJECT_TASK_ASSIGNMENT_INVALID")?,
    ) != link.assignment_sha256
        || link.scope_sha256 != assignment_scope_sha256(project)
    {
        return Err("PROJECT_TASK_CONTRACT_CHANGED".into());
    }
    let assignment: Assignment =
        serde_json::from_slice(&bytes).map_err(|_| "PROJECT_TASK_ASSIGNMENT_INVALID")?;
    if assignment.schema_version != 1
        || assignment.project_id != project.project.project_id
        || assignment.contract.task_id != link.task_id
        || !assignment.web_work.as_ref().is_some_and(|work| {
            work.request_id == record.request_id && work.event_id == record.event_id
        })
    {
        return Err("WEB_TASK_MANUAL_WAKE_SELECTION_CHANGED".into());
    }
    let root = task_directory(project, &link.task_id, false)?.join("journal");
    // DelegatedJournal::open creates its root when absent. Delivery must only
    // read an existing completed task, including plain ancestors and files.
    plain_directory(&root)?;
    let run = RunId::new(&link.task_id).map_err(|_| "PROJECT_TASK_CONTRACT_INVALID")?;
    let journal = DelegatedJournal::open(&root).map_err(|_| "PROJECT_TASK_RECOVERY_REQUIRED")?;
    let run_root = journal
        .run_dir(&run)
        .map_err(|_| "PROJECT_TASK_RECOVERY_REQUIRED")?;
    plain_directory(&run_root)?;
    for name in ["contract.json", "tool_calls.json"] {
        if !regular(&run_root.join(name)) {
            return Err("PROJECT_TASK_RECOVERY_REQUIRED".into());
        }
    }
    if journal
        .load_contract(&run)
        .map_err(|_| "PROJECT_TASK_RECOVERY_REQUIRED")?
        != assignment.contract
    {
        return Err("PROJECT_TASK_CONTRACT_CHANGED".into());
    }
    let calls = journal
        .load_tool_calls(&run)
        .map_err(|_| "PROJECT_TASK_RECOVERY_REQUIRED")?;
    // Before submission, no older uncertain effect may be routed around.
    // After the durable handoff the continued task may legitimately be busy;
    // its own tool admission still enforces every existing no-replay fence.
    if require_quiescent
        && calls.values().any(|call| {
            !matches!(
                call.status,
                ToolCallStatus::Completed | ToolCallStatus::Failed
            )
        })
    {
        return Err("PROJECT_TASK_RECOVERY_REQUIRED".into());
    }
    Ok(calls)
}

/// A coordinator's terminal report cannot discard a live or uncertain task call
/// or replace the exact response evidence for a self-Wake the task requested.
pub(crate) fn require_completed_task_continuations(
    project: &ResolvedProjectScopeV1,
    record: &crate::web_work_state::WorkRecord,
    wake: &Store,
) -> Result<(), String> {
    let Some(link) = record.task_assignment.as_ref() else {
        return Ok(());
    };
    let directory = task_directory(project, &link.task_id, false)?;
    match fs::symlink_metadata(directory.join("journal")) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return match fs::symlink_metadata(directory.join("execution-grant.json")) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                _ => Err("PROJECT_TASK_RECOVERY_REQUIRED".into()),
            };
        }
        Err(_) => return Err("PROJECT_TASK_RECOVERY_REQUIRED".into()),
    }
    let calls = read_task_calls(project, record, &directory.join("assignment.json"), true)?;
    for (call_id, call) in calls {
        if !is_completed_manual_wake(&call) {
            continue;
        }
        let id = manual_wake_event_id(&call)?;
        let delivery = wake
            .delivery(id)?
            .filter(|delivery| delivery.phase == Phase::Sent)
            .ok_or("WEB_WORK_CONTINUATION_PENDING")?;
        validate_selection(
            project,
            record,
            &delivery.target,
            &delivery.event,
            &call_id,
            &call,
        )?;
        let timer = wake.timer(id)?.ok_or("WEB_WORK_CONTINUATION_PENDING")?;
        if timer.project_id != project.project.project_id
            || timer.target_generation != delivery.target.generation
            || timer.target_digest != delivery.target.digest
            || timer.response_state != ResponseState::Complete
        {
            return Err("WEB_WORK_CONTINUATION_PENDING".into());
        }
    }
    Ok(())
}

pub(crate) fn require_journaled_manual_wake(
    project: &ResolvedProjectScopeV1,
    record: &crate::web_work_state::WorkRecord,
    delivery: &catdesk_wake::store::Delivery,
) -> Result<(), String> {
    let link = record
        .task_assignment
        .as_ref()
        .ok_or("WEB_TASK_GRANT_REQUIRED")?;
    let directory = task_directory(project, &link.task_id, false)?;
    let calls = read_task_calls(project, record, &directory.join("assignment.json"), false)?;
    let mut matched = false;
    for (call_id, call) in &calls {
        if is_completed_manual_wake(call) && manual_wake_event_id(call)? == delivery.event.event_id
        {
            if matched {
                return Err("WEB_TASK_MANUAL_WAKE_CALL_UNPROVEN".into());
            }
            validate_selection(
                project,
                record,
                &delivery.target,
                &delivery.event,
                call_id,
                call,
            )?;
            matched = true;
        }
    }
    if !matched {
        return Err("WEB_TASK_MANUAL_WAKE_CALL_UNPROVEN".into());
    }
    Ok(())
}

fn is_completed_manual_wake(call: &ToolCallRecordV1) -> bool {
    call.tool_name == "wake.manual_end_turn" && call.status == ToolCallStatus::Completed
}

fn manual_wake_event_id(call: &ToolCallRecordV1) -> Result<&str, String> {
    // The summary is only a locator. validate_selection checks the exact event
    // against the immutable journal hashes before using any delivery evidence.
    call.outcome_summary
        .as_deref()
        .and_then(|summary| summary.strip_prefix("manual Wake intent "))
        .and_then(|summary| summary.strip_suffix(" recorded; delivery unverified"))
        .filter(|id| valid_id(id) && id.starts_with("wake-intent-"))
        .ok_or_else(|| "WEB_TASK_MANUAL_WAKE_CALL_UNPROVEN".into())
}

pub(crate) fn next_manual_wake(
    project: &ResolvedProjectScopeV1,
    record: &crate::web_work_state::WorkRecord,
    target: &Target,
    wake: &Store,
) -> Result<(String, Event), String> {
    let mut pending = None;
    for (call_id, call) in manual_wake_calls(project, record, true)? {
        if !is_completed_manual_wake(&call) {
            continue;
        }
        let id = manual_wake_event_id(&call)?;
        let delivery = wake.delivery(id)?;
        let event: Event = match &delivery {
            Some(saved) => saved.event.clone(),
            None => read(&wake.root().join("queue").join(format!("{id}.json")))?,
        };
        if event.event_id != id {
            return Err("WEB_TASK_MANUAL_WAKE_SELECTION_CHANGED".into());
        }
        validate_selection(
            project,
            record,
            delivery.as_ref().map_or(target, |saved| &saved.target),
            &event,
            &call_id,
            &call,
        )?;
        if let Some(saved) = delivery {
            match saved.phase {
                Phase::Sent => continue,
                Phase::Claimed if saved.target == *target => {}
                _ => return Err("WEB_TASK_MANUAL_WAKE_DELIVERY_HELD".into()),
            }
        }
        if pending.replace((call_id, event)).is_some() {
            return Err("WEB_TASK_MANUAL_WAKE_MULTIPLE_PENDING".into());
        }
    }
    pending.ok_or_else(|| "WEB_TASK_MANUAL_WAKE_NO_PENDING_INTENT".into())
}

fn validate_selection(
    project: &ResolvedProjectScopeV1,
    record: &crate::web_work_state::WorkRecord,
    target: &Target,
    event: &Event,
    call_id: &str,
    call: &ToolCallRecordV1,
) -> Result<(), String> {
    event.validate()?;
    target.validate()?;
    let link = record
        .task_assignment
        .as_ref()
        .ok_or("WEB_TASK_GRANT_REQUIRED")?;
    let metadata = event
        .wake_metadata()?
        .ok_or("QUEUE_WAKE_METADATA_INVALID")?;
    let request = ManualWakeRequest::parse(json!({
        "sourceTurnId":metadata.source_turn_id,"priority":metadata.priority,
        "ttlSeconds":metadata.expires_utc.checked_sub(event.created_utc)
            .ok_or("QUEUE_WAKE_METADATA_INVALID")?,
    }))?;
    let intent = catdesk_wake::store::WakeIntent {
        purpose: WakePurpose::ManualEndTurn,
        source_event_id: &record.event_id,
        source_turn_id: &request.source_turn_id,
        priority: request.priority,
        ttl_seconds: request.ttl_seconds,
    };
    let mut exact = event.clone();
    exact.audit_references.clear();
    let exact = exact.with_wake_metadata(metadata.clone())?;
    let arguments_hash = stable_text_hash(
        &serde_json::to_string(&request).map_err(|_| "PROJECT_TASK_CALL_INVALID")?,
    );
    let result_hash = catdesk_wake::protocol::digest(
        &manual_wake_result(event, &request.source_turn_id, metadata.expires_utc).to_string(),
    );
    if record.phase != "OPEN"
        || event.project_id != project.project.project_id
        || event.event_type != "engineering_attention"
        || event.target_generation != target.generation
        || metadata.purpose != WakePurpose::ManualEndTurn
        || metadata.source_event_id != record.event_id
        || event.event_id != intent.event_id(&project.project.project_id)?
        || exact != *event
        || event.message
            != crate::wake_protocol_client::registered_manual_wake_message(
                &project.project.project_id,
                &record.request_id,
                &link.task_id,
                &record.event_id,
                &request.source_turn_id,
            )
        || call.schema_version != 1
        || call.run_id.as_str() != link.task_id
        || call.tool_call_id.as_str() != call_id
        || call.tool_name != "wake.manual_end_turn"
        || call.mutation_kind != ToolMutationKind::Mutating
        || call.status != ToolCallStatus::Completed
        || call.request_hash != arguments_hash
        || call.arguments_hash != arguments_hash
        || call.result_hash.as_deref() != Some(result_hash.as_str())
    {
        return Err("WEB_TASK_MANUAL_WAKE_SELECTION_CHANGED".into());
    }
    Ok(())
}
