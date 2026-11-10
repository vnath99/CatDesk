//! Read-only authority for delivery of an already journaled manual Wake.
//! This never creates a grant, task call, event or replacement identity.
use super::*;
use crate::delegated::journal::{ToolCallRecordV1, ToolMutationKind};
use catdesk_wake::protocol::{Event, Target, WakePurpose};

pub(crate) fn authorize_manual_wake_delivery(
    project: &ResolvedProjectScopeV1,
    record: &crate::web_work_state::WorkRecord,
    target: &Target,
    event: &Event,
    call_id: &str,
    require_quiescent: bool,
) -> Result<(), String> {
    let link = record
        .task_assignment
        .as_ref()
        .ok_or("WEB_TASK_GRANT_REQUIRED")?;
    let key: String = serde_json::from_slice(&read_assignment_bytes(&execution_key_path(
        project, link, false,
    )?)?)
    .map_err(|_| "WEB_TASK_GRANT_INVALID")?;
    let archive = authorized_manual_wake_assignment(project, record, &key)?;
    let assignment: Assignment = serde_json::from_slice(&read_assignment_bytes(&archive)?)
        .map_err(|_| "PROJECT_TASK_ASSIGNMENT_INVALID")?;
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
    let call = calls
        .get(call_id)
        .ok_or("WEB_TASK_MANUAL_WAKE_CALL_UNPROVEN")?;
    validate_selection(project, record, target, event, call_id, call)
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
