//! Private payloads for already-granted Web tasks. Workspace journals retain
//! only exact references. The existing browser transaction sends the payload.

use super::{ProjectStore, Record, directory, plain};
use crate::project_mcp_task::{exact_file, read_assignment_bytes};
use catdesk_wake::protocol::{digest, valid_id};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Reference {
    storage_id: String,
    payload_sha256: String,
    base_sha256: String,
    pub(super) request_id: String,
    pub(super) task_id: String,
    pub(super) assignment_sha256: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Payload {
    schema_version: u32,
    project_id: String,
    intent_id: String,
    source_task_id: String,
    base_sha256: String,
    context: Value,
    handoff: String,
}

fn sha(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

fn private_root(reference: &Reference) -> Result<PathBuf, String> {
    if !sha(&reference.storage_id)
        || !sha(&reference.payload_sha256)
        || !sha(&reference.base_sha256)
        || !sha(&reference.assignment_sha256)
        || !valid_id(&reference.request_id)
        || !valid_id(&reference.task_id)
    {
        return Err("WEB_TASK_HANDOFF_REFERENCE_INVALID".into());
    }
    let root = crate::delegated::autonomy_runtime::fixed_current_user_catdesk_state_root()
        .map_err(|_| "WEB_TASK_HANDOFF_STORE_UNAVAILABLE")?;
    Ok(root.join("WebTaskHandoffs").join(&reference.storage_id))
}

pub(super) fn artifact_root(record: &Record) -> Result<Option<PathBuf>, String> {
    let Some(reference) = &record.task_handoff else {
        return Ok(None);
    };
    let root = private_root(reference)?;
    plain(
        root.parent().ok_or("WEB_TASK_HANDOFF_STORE_UNAVAILABLE")?,
        true,
    )?;
    plain(&root, true)?;
    Ok(Some(root))
}

pub(super) fn prepare(
    store: &ProjectStore,
    record: &mut Record,
    context: Value,
) -> Result<(), String> {
    if context["projectId"].as_str() != Some(&record.project_id)
        || context["sourceEventId"].as_str() != Some(&record.source_task_id)
        || !context["executionKey"].as_str().is_some_and(sha)
    {
        return Err("WEB_TASK_HANDOFF_CONTEXT_INVALID".into());
    }
    if record.handoff.contains(
        context["executionKey"]
            .as_str()
            .ok_or("WEB_TASK_HANDOFF_CONTEXT_INVALID")?,
    ) {
        return Err("WEB_TASK_HANDOFF_CREDENTIAL_IN_SOURCE_REFUSED".into());
    }
    let base_sha256 = digest(&record.handoff);
    let wire_handoff = format!(
        "{}\n\nLocally verified continuation context for this exact assigned task only:\n{}\n\nKeep executionKey private: do not echo it in replies, logs, files, commits or other conversations. Wait for the exact CatDesk continuation event before making tool calls. Use its verified target generation and digest with this context. Keep completed call IDs unchanged and never replay them or invent a new ID for an uncertain operation. Native sessions retain their work. Re-read assigned artifacts with explicitly bounded read arguments before continuing the unfinished task.",
        record.handoff,
        serde_json::to_string(&context).map_err(|_| "WEB_TASK_HANDOFF_CONTEXT_INVALID")?
    );
    // Enforce the actual existing browser wire bound before storing anything.
    catdesk_wake::runtime::compose_bootstrap_message(
        &record.project_id,
        &record.intent_id,
        &record.source,
        &wire_handoff,
    )?;
    let payload = Payload {
        schema_version: 1,
        project_id: record.project_id.clone(),
        intent_id: record.intent_id.clone(),
        source_task_id: record.source_task_id.clone(),
        base_sha256: base_sha256.clone(),
        context,
        handoff: wire_handoff,
    };
    let bytes = serde_json::to_vec(&payload).map_err(|_| "WEB_TASK_HANDOFF_CONTEXT_INVALID")?;
    let text = std::str::from_utf8(&bytes).map_err(|_| "WEB_TASK_HANDOFF_CONTEXT_INVALID")?;
    let reference = Reference {
        storage_id: storage_id(store, record)?,
        payload_sha256: digest(text),
        base_sha256,
        request_id: payload.context["requestId"]
            .as_str()
            .ok_or("WEB_TASK_HANDOFF_CONTEXT_INVALID")?
            .into(),
        task_id: payload.context["taskId"]
            .as_str()
            .ok_or("WEB_TASK_HANDOFF_CONTEXT_INVALID")?
            .into(),
        assignment_sha256: payload.context["assignmentSha256"]
            .as_str()
            .ok_or("WEB_TASK_HANDOFF_CONTEXT_INVALID")?
            .into(),
    };
    let root = private_root(&reference)?;
    let control = store
        .root
        .parent()
        .and_then(std::path::Path::parent)
        .and_then(std::path::Path::parent)
        .ok_or("WEB_TASK_HANDOFF_STORE_UNAVAILABLE")?
        .canonicalize()
        .map_err(|_| "WEB_TASK_HANDOFF_STORE_UNAVAILABLE")?;
    if root.starts_with(&control) || control.starts_with(&root) {
        return Err("WEB_TASK_HANDOFF_STORE_OVERLAP".into());
    }
    for field in ["executionWorkspace", "registeredWorkspace"] {
        let workspace = payload.context[field]
            .as_str()
            .ok_or("WEB_TASK_HANDOFF_CONTEXT_INVALID")?;
        let workspace = std::path::Path::new(workspace)
            .canonicalize()
            .map_err(|_| "WEB_TASK_HANDOFF_CONTEXT_INVALID")?;
        if root.starts_with(&workspace) || workspace.starts_with(&root) {
            return Err("WEB_TASK_HANDOFF_STORE_OVERLAP".into());
        }
    }
    directory(&root)?;
    exact_file(&root.join("payload.json"), &bytes)?;
    record.task_handoff = Some(reference);
    if content(record)? != payload.handoff {
        return Err("WEB_TASK_HANDOFF_READBACK_CHANGED".into());
    }
    Ok(())
}

fn storage_id(store: &ProjectStore, record: &Record) -> Result<String, String> {
    Ok(digest(
        &serde_json::to_string(&(
            store
                .root
                .canonicalize()
                .map_err(|_| "WEB_TASK_HANDOFF_STORE_UNAVAILABLE")?,
            &record.intent_id,
            digest(&record.handoff),
        ))
        .map_err(|_| "WEB_TASK_HANDOFF_REFERENCE_INVALID")?,
    ))
}

pub(super) fn validate_reference(store: &ProjectStore, record: &Record) -> Result<(), String> {
    if let Some(reference) = &record.task_handoff
        && reference.storage_id != storage_id(store, record)?
    {
        return Err("WEB_TASK_HANDOFF_CONTROL_SCOPE_CHANGED".into());
    }
    Ok(())
}

pub(super) fn content(record: &Record) -> Result<String, String> {
    if record.task_handoff.is_none() {
        return Ok(record.handoff.clone());
    }
    Ok(payload(record)?.handoff)
}

fn payload(record: &Record) -> Result<Payload, String> {
    let reference = record
        .task_handoff
        .as_ref()
        .ok_or("WEB_TASK_HANDOFF_REFERENCE_REQUIRED")?;
    let root = artifact_root(record)?.ok_or("WEB_TASK_HANDOFF_STORE_UNAVAILABLE")?;
    let bytes = read_assignment_bytes(&root.join("payload.json"))
        .map_err(|_| "WEB_TASK_HANDOFF_PAYLOAD_UNAVAILABLE")?;
    let raw = std::str::from_utf8(&bytes).map_err(|_| "WEB_TASK_HANDOFF_PAYLOAD_INVALID")?;
    if digest(raw) != reference.payload_sha256 {
        return Err("WEB_TASK_HANDOFF_PAYLOAD_CHANGED".into());
    }
    let payload: Payload =
        serde_json::from_slice(&bytes).map_err(|_| "WEB_TASK_HANDOFF_PAYLOAD_INVALID")?;
    if payload.schema_version != 1
        || payload.project_id != record.project_id
        || payload.intent_id != record.intent_id
        || payload.source_task_id != record.source_task_id
        || payload.base_sha256 != digest(&record.handoff)
        || payload.base_sha256 != reference.base_sha256
        || payload.context["requestId"].as_str() != Some(&reference.request_id)
        || payload.context["taskId"].as_str() != Some(&reference.task_id)
        || payload.context["assignmentSha256"].as_str() != Some(&reference.assignment_sha256)
        || payload.context["sourceEventId"].as_str() != Some(&record.source_task_id)
        || payload.context["projectId"].as_str() != Some(&record.project_id)
        || !payload.handoff.starts_with(&record.handoff)
    {
        return Err("WEB_TASK_HANDOFF_PAYLOAD_SCOPE_MISMATCH".into());
    }
    Ok(payload)
}

pub(super) fn authorize(workspace: &std::path::Path, record: &Record) -> Result<(), String> {
    let Some(reference) = &record.task_handoff else {
        return Ok(());
    };
    let payload = payload(record)?;
    let scope = crate::delegated::autonomy_projects::resolve_registered_project_from_control_root(
        workspace,
        &record.project_id,
    )
    .map_err(|_| "WEB_TASK_HANDOFF_SCOPE_CHANGED")?;
    let work = crate::web_work_state::WorkLedger::open(workspace, &record.project_id)?
        .record(&reference.request_id)?
        .ok_or("WEB_TASK_HANDOFF_SOURCE_CHANGED")?;
    if work.phase != "OPEN"
        || work.event_id != record.source_task_id
        || work.task_assignment.as_ref().is_none_or(|link| {
            link.task_id != reference.task_id
                || link.assignment_sha256 != reference.assignment_sha256
        })
    {
        return Err("WEB_TASK_HANDOFF_SOURCE_CHANGED".into());
    }
    crate::project_mcp_task::authorized_web_assignment(
        &scope,
        &work,
        payload.context["executionKey"]
            .as_str()
            .ok_or("WEB_TASK_HANDOFF_CONTEXT_INVALID")?,
    )?;
    Ok(())
}

pub(super) fn continuation(
    record: &Record,
    bound: &catdesk_wake::protocol::Target,
) -> Option<String> {
    record.task_handoff.as_ref().map(|reference| format!(" Verified task selection: {}. Use the private executionKey already in this conversation's bootstrap with catdesk_web_task_tool. Do not disclose it.",
        json!({"projectId":record.project_id,"requestId":reference.request_id,"sourceEventId":record.source_task_id,
            "taskId":reference.task_id,"assignmentSha256":reference.assignment_sha256,
            "expectedTargetGeneration":bound.generation,"expectedTargetSha256":bound.digest})))
}
