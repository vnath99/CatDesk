//! Link different digest domains without changing the original context or grant.
use super::*;

#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Clarification {
    schema_version: u32,
    reference: Reference,
    full_private_context_sha256: String,
    prior_continuation_receipt: Receipt,
    native_selection: crate::operator_codex_cli::NativeQueueSelection,
}

pub(super) fn capsule_full_context_digest(payload: &Payload) -> Result<Option<String>, String> {
    if !payload
        .message
        .starts_with("CatDesk private assigned-task context (native-only).")
    {
        return Ok(None);
    }
    let capsule: Value = serde_json::from_str(
        payload
            .message
            .lines()
            .nth(1)
            .ok_or("WEB_TASK_CONTEXT_INVALID")?,
    )
    .map_err(|_| "WEB_TASK_CONTEXT_INVALID")?;
    let full =
        digest(&serde_json::to_string(&payload.context).map_err(|_| "WEB_TASK_CONTEXT_INVALID")?);
    let call = &capsule["nativeCall"];
    let fields = [
        "projectId",
        "requestId",
        "sourceEventId",
        "taskId",
        "assignmentSha256",
        "expectedTargetGeneration",
        "expectedTargetSha256",
        "executionKey",
    ];
    if capsule.as_object().is_none_or(|v| v.len() != 2)
        || capsule["fullPrivateContextSha256"] != full
        || call.as_object().is_none_or(|v| v.len() != fields.len() + 3)
        || fields.iter().any(|key| call[*key] != payload.context[*key])
        || call["callId"] != "native-dispatch-1"
        || call["tool"] != "native.codex.dispatch"
        || call["arguments"] != json!({})
    {
        return Err("WEB_TASK_CONTEXT_IDENTITY_MISMATCH".into());
    }
    Ok(Some(full))
}

pub(super) fn legacy_native_message(reference: &Reference) -> String {
    format!(
        "CatDesk assigned-task continuation for {}/{}. Use nativeCall from the private context USER SHA-256 {} already delivered here. Dispatch that frozen native task once now. Preserve its grant, stable call ID and other sessions; never replay completed/unknown calls. Report actual edits, tests and blockers.",
        reference.project_id, reference.request_id, reference.message_sha256
    )
}

fn require_fresh_snapshot(
    snapshot: &Value,
    selection: &crate::operator_codex_cli::NativeQueueSelection,
) -> Result<(), String> {
    let job = &snapshot["job"];
    if snapshot["bindingStillMatches"] != true
        || snapshot["originalBindingStillMatches"] != true
        || snapshot["originTaskId"] != selection.task_id
        || job["taskId"] != selection.task_id
        || job["requestSha256"] != selection.request_sha256
        || job["state"] != "READY"
        || job.get("attemptId") != Some(&Value::Null)
        || job.get("receiptId") != Some(&Value::Null)
        || snapshot.get("receipt") != Some(&Value::Null)
    {
        return Err("WEB_TASK_CONTEXT_NATIVE_EFFECT_UNPROVEN".into());
    }
    Ok(())
}

fn proof(
    scope: &ResolvedProjectScopeV1,
    record: &crate::web_work_state::WorkRecord,
    target: &Target,
    store: &catdesk_wake::store::Store,
) -> Result<(Clarification, PathBuf), String> {
    let (reference, payload, root) = load(scope, record, target)?;
    if state(&root, &reference)?.phase != "SENT" {
        return Err("WEB_TASK_CONTEXT_RECEIPT_REQUIRED".into());
    }
    let full =
        capsule_full_context_digest(&payload)?.ok_or("WEB_TASK_CONTEXT_NATIVE_NOT_GRANTED")?;
    let old_id = continuation_identity(&reference, record)?;
    let old = store
        .delivery(&old_id)?
        .ok_or("WEB_TASK_CONTEXT_PRIOR_CONTINUATION_REQUIRED")?;
    let receipt = old
        .receipt
        .clone()
        .ok_or("WEB_TASK_CONTEXT_PRIOR_CONTINUATION_REQUIRED")?;
    let metadata = old
        .event
        .wake_metadata()?
        .ok_or("WEB_TASK_CONTEXT_PRIOR_CONTINUATION_REQUIRED")?;
    if old.phase != catdesk_wake::store::Phase::Sent
        || old.target != *target
        || old.event.project_id != scope.project.project_id
        || old.event.target_generation != target.generation
        || metadata.purpose != catdesk_wake::protocol::WakePurpose::NewTask
        || metadata.source_event_id != record.event_id
        || metadata.source_turn_id != format!("initial-context-{}", reference.storage_id)
        || old.event.message != legacy_native_message(&reference)
        || receipt.event_id != old_id
        || receipt.target_generation != target.generation
        || receipt.target_digest != target.digest
        || receipt.evidence != "EXACT_USER_MESSAGE_APPENDED"
        || receipt.message_digest != digest(&old.event.message)
        || store.timer(&old_id)?.is_none_or(|timer| {
            timer.response_state != catdesk_wake::store::ResponseState::Complete
                || timer.event_id != old_id
                || timer.project_id != scope.project.project_id
                || timer.target_generation != target.generation
                || timer.target_digest != target.digest
        })
    {
        return Err("WEB_TASK_CONTEXT_PRIOR_CONTINUATION_REQUIRED".into());
    }
    let key = payload.context["executionKey"]
        .as_str()
        .ok_or("WEB_TASK_GRANT_REQUIRED")?;
    let selection = super::super::native_status_selection(scope, record, key)?;
    if serde_json::to_value(&selection).map_err(|_| "WEB_TASK_CONTEXT_INVALID")?
        != payload.context["nativeQueue"]["selection"]
    {
        return Err("WEB_TASK_CONTEXT_IDENTITY_MISMATCH".into());
    }
    let directory = super::super::task_directory(scope, &reference.assignment.task_id, false)?;
    match fs::symlink_metadata(directory.join("journal")) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        _ => return Err("WEB_TASK_CONTEXT_NATIVE_EFFECT_UNPROVEN".into()),
    }
    let snapshot = crate::operator_codex_cli::native_queue_selection_status(
        &scope.control_workspace,
        &scope.project.project_id,
        &selection,
    )?;
    require_fresh_snapshot(&snapshot, &selection)?;
    // Existing read-only assignment/worktree inspection; no call journal/claim.
    super::super::inspect_native_task_call(scope, record, key, &reference.assignment.task_id)?;
    Ok((
        Clarification {
            schema_version: 1,
            reference,
            full_private_context_sha256: full,
            prior_continuation_receipt: receipt,
            native_selection: selection,
        },
        root,
    ))
}

fn bytes(value: &Clarification) -> Result<Vec<u8>, String> {
    serde_json::to_vec(value).map_err(|_| "WEB_TASK_CONTEXT_INVALID".into())
}

pub(super) fn validate(
    scope: &ResolvedProjectScopeV1,
    record: &crate::web_work_state::WorkRecord,
    target: &Target,
    store: &catdesk_wake::store::Store,
) -> Result<(), String> {
    let (value, root) = proof(scope, record, target, store)?;
    if read_assignment_bytes(&root.join("identity-clarification.json"))? != bytes(&value)? {
        return Err("WEB_TASK_CONTEXT_IDENTITY_MISMATCH".into());
    }
    Ok(())
}

fn message(value: &Clarification) -> String {
    let r = &value.reference;
    format!(
        "CatDesk context identity clarification for {}/{}. fullPrivateContextSha256={} matches the same-named field in the original private capsule. deliveredUserMessageSha256={} identifies its USER delivery receipt; it is a different hash domain, not a replacement full-context hash. assignmentSha256={}. Prior continuation USER digest={} used the USER receipt hash. All original frozen bytes, assignment, grant and native selection are unchanged and independently reconciled. If any same-named field mismatches, refuse. Native pre-submit proof is READY with no attempt, receipt or task journal. Use that capsule's existing nativeCall (native-dispatch-1, native.codex.dispatch, arguments {{}}) once now via catdesk_web_task_tool or run_command CATDESK_WEB_TASK=<nativeCall JSON>. Keep the key private; preserve ownership and never replay completed/unknown calls. Report actual edits/tests/blockers.",
        r.project_id,
        r.request_id,
        value.full_private_context_sha256,
        r.message_sha256,
        r.assignment.assignment_sha256,
        value.prior_continuation_receipt.message_digest
    )
}

pub(super) fn queue(
    scope: &ResolvedProjectScopeV1,
    record: &crate::web_work_state::WorkRecord,
    target: &Target,
    store: &catdesk_wake::store::Store,
) -> Result<Value, String> {
    let (value, root) = proof(scope, record, target, store)?;
    let encoded = bytes(&value)?;
    exact_file(&root.join("identity-clarification.json"), &encoded)?;
    let clarification_sha =
        digest(std::str::from_utf8(&encoded).map_err(|_| "WEB_TASK_CONTEXT_INVALID")?);
    let turn = format!("initial-context-{clarification_sha}");
    let event = store.produce_expected_wake(
        &scope.project.project_id,
        &message(&value),
        target,
        catdesk_wake::store::WakeIntent {
            purpose: catdesk_wake::protocol::WakePurpose::NewTask,
            source_event_id: &record.event_id,
            source_turn_id: &turn,
            priority: 0,
            ttl_seconds: 600,
        },
    )?;
    Ok(
        json!({"status":"TASK_IDENTITY_CLARIFICATION_QUEUED","projectId":scope.project.project_id,
        "requestId":record.request_id,"taskId":value.reference.assignment.task_id,"eventId":event.event_id,
        "messageSha256":digest(&event.message),"fullPrivateContextSha256":value.full_private_context_sha256,
        "deliveredUserMessageSha256":value.reference.message_sha256,"assignmentSha256":value.reference.assignment.assignment_sha256,
        "clarificationSha256":clarification_sha,"priorContinuationEventId":value.prior_continuation_receipt.event_id,
        "targetGeneration":target.generation,"targetSha256":target.digest,"queueOnly":true,"newGrantCreated":false,
        "nativeSessionAction":"NONE","taskCompleted":false,"executionKeyDisclosed":false}),
    )
}

/// A later advisory formatting turn, using the same private context and native
/// call. It cannot retire an unknown native effect or replace any frozen bytes.
#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TransportClarification {
    schema_version: u32,
    identity: Clarification,
    prior_identity_receipt: Receipt,
    canonical_cached_envelope_sha256: String,
}

fn transport_proof(
    scope: &ResolvedProjectScopeV1,
    record: &crate::web_work_state::WorkRecord,
    target: &Target,
    store: &catdesk_wake::store::Store,
) -> Result<(TransportClarification, PathBuf), String> {
    // Reuses exact original capsule/grant/selection/worktree checks, READY with
    // no attempt/receipt, and the absence of any native task journal.
    let (identity, root) = proof(scope, record, target, store)?;
    let encoded = bytes(&identity)?;
    if read_assignment_bytes(&root.join("identity-clarification.json"))? != encoded {
        return Err("WEB_TASK_CONTEXT_IDENTITY_MISMATCH".into());
    }
    let turn = format!(
        "initial-context-{}",
        digest(std::str::from_utf8(&encoded).map_err(|_| "WEB_TASK_CONTEXT_INVALID")?)
    );
    let id = catdesk_wake::store::WakeIntent {
        purpose: catdesk_wake::protocol::WakePurpose::NewTask,
        source_event_id: &record.event_id,
        source_turn_id: &turn,
        priority: 0,
        ttl_seconds: 600,
    }
    .event_id(&scope.project.project_id)?;
    let prior = store
        .delivery(&id)?
        .ok_or("WEB_TASK_CONTEXT_TRANSPORT_PRIOR_REQUIRED")?;
    let receipt = prior
        .receipt
        .clone()
        .ok_or("WEB_TASK_CONTEXT_TRANSPORT_PRIOR_REQUIRED")?;
    let metadata = prior
        .event
        .wake_metadata()?
        .ok_or("WEB_TASK_CONTEXT_TRANSPORT_PRIOR_REQUIRED")?;
    if prior.phase != catdesk_wake::store::Phase::Sent
        || prior.target != *target
        || prior.event.project_id != scope.project.project_id
        || prior.event.event_id != id
        || prior.event.target_generation != target.generation
        || metadata.purpose != catdesk_wake::protocol::WakePurpose::NewTask
        || metadata.source_event_id != record.event_id
        || metadata.source_turn_id != turn
        || prior.event.message != message(&identity)
        || receipt.schema_version != 1
        || receipt.event_id != id
        || receipt.target_generation != target.generation
        || receipt.target_digest != target.digest
        || receipt.message_digest != digest(&prior.event.message)
        || receipt.evidence != "EXACT_USER_MESSAGE_APPENDED"
        || receipt.sent_utc == 0
        || store.timer(&id)?.is_none_or(|timer| {
            timer.response_state != catdesk_wake::store::ResponseState::Complete
                || timer.event_id != id
                || timer.project_id != scope.project.project_id
                || timer.target_generation != target.generation
                || timer.target_digest != target.digest
        })
    {
        return Err("WEB_TASK_CONTEXT_TRANSPORT_PRIOR_REQUIRED".into());
    }
    let (_, payload, _) = load(scope, record, target)?;
    let capsule: Value = serde_json::from_str(
        payload
            .message
            .lines()
            .nth(1)
            .ok_or("WEB_TASK_CONTEXT_INVALID")?,
    )
    .map_err(|_| "WEB_TASK_CONTEXT_INVALID")?;
    let envelope =
        crate::web_task_dispatch::canonical_cached_envelope_sha256(&capsule["nativeCall"])?;
    Ok((
        TransportClarification {
            schema_version: 1,
            identity,
            prior_identity_receipt: receipt,
            canonical_cached_envelope_sha256: envelope,
        },
        root,
    ))
}

fn transport_bytes(value: &TransportClarification) -> Result<Vec<u8>, String> {
    serde_json::to_vec(value).map_err(|_| "WEB_TASK_CONTEXT_INVALID".into())
}

pub(super) fn validate_transport(
    scope: &ResolvedProjectScopeV1,
    record: &crate::web_work_state::WorkRecord,
    target: &Target,
    store: &catdesk_wake::store::Store,
) -> Result<(), String> {
    let (value, root) = transport_proof(scope, record, target, store)?;
    if read_assignment_bytes(&root.join("transport-clarification.json"))?
        != transport_bytes(&value)?
    {
        return Err("WEB_TASK_CONTEXT_TRANSPORT_CHANGED".into());
    }
    Ok(())
}

fn transport_message(value: &TransportClarification) -> String {
    let r = &value.identity.reference;
    format!(
        "CatDesk same-task transport clarification for {}/{}. Original capsule fullPrivateContextSha256={} and assignmentSha256={} remain unchanged. Prior identity-clarification USER digest={}. Independent local pre-admission checks still show the original native task READY with no attempt, receipt or task journal. This changes only outer transport instructions, never the task, nativeCall, grant, key, workspace or ownership. Use the existing capsule's exact nativeCall (native-dispatch-1, native.codex.dispatch, arguments {{}}). Invoke CatDesk v5 run_command ONCE with ONLY its command argument: the literal prefix CATDESK_WEB_TASK= immediately followed by the JSON serialization of that unchanged nativeCall. Send the literal serialized string, not JavaScript, a shell command or a placeholder. Omit ALL optional outer arguments, including cwd, timeout, dry_run, include_full_output and allow_without_plan. Keep the key private. If any original identity is unavailable or mismatched, stop. Never replay a completed or uncertain native call. Report actual tool outcome, edits, tests and blockers; do not substitute a local-only execution path.",
        r.project_id,
        r.request_id,
        value.identity.full_private_context_sha256,
        r.assignment.assignment_sha256,
        value.prior_identity_receipt.message_digest
    )
}

pub(super) fn queue_transport(
    scope: &ResolvedProjectScopeV1,
    record: &crate::web_work_state::WorkRecord,
    target: &Target,
    store: &catdesk_wake::store::Store,
) -> Result<Value, String> {
    let (value, root) = transport_proof(scope, record, target, store)?;
    let encoded = transport_bytes(&value)?;
    exact_file(&root.join("transport-clarification.json"), &encoded)?;
    let clarification_sha =
        digest(std::str::from_utf8(&encoded).map_err(|_| "WEB_TASK_CONTEXT_INVALID")?);
    let turn = format!("initial-context-{clarification_sha}");
    let event = store.produce_expected_wake(
        &scope.project.project_id,
        &transport_message(&value),
        target,
        catdesk_wake::store::WakeIntent {
            purpose: catdesk_wake::protocol::WakePurpose::NewTask,
            source_event_id: &record.event_id,
            source_turn_id: &turn,
            priority: 0,
            ttl_seconds: 600,
        },
    )?;
    Ok(
        json!({"status":"TASK_TRANSPORT_CLARIFICATION_QUEUED","projectId":scope.project.project_id,
        "requestId":record.request_id,"taskId":value.identity.reference.assignment.task_id,"eventId":event.event_id,
        "messageSha256":digest(&event.message),"clarificationSha256":clarification_sha,
        "priorClarificationEventId":value.prior_identity_receipt.event_id,
        "fullPrivateContextSha256":value.identity.full_private_context_sha256,
        "assignmentSha256":value.identity.reference.assignment.assignment_sha256,
        "targetGeneration":target.generation,"targetSha256":target.digest,
        "queueOnly":true,"newGrantCreated":false,"nativeSessionAction":"NONE",
        "taskCompleted":false,"executionKeyDisclosed":false}),
    )
}

/// One later continuation, never another identity or transport correction.
/// Its stable identity is the unchanged task proof plus the completed prior
/// transport receipt; runtime timestamps cannot create another send.
#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ServingRecovery {
    schema_version: u32,
    purpose: String,
    transport: TransportClarification,
    prior_transport_receipt: Receipt,
}

fn serving_proof(
    scope: &ResolvedProjectScopeV1,
    record: &crate::web_work_state::WorkRecord,
    target: &Target,
    store: &catdesk_wake::store::Store,
) -> Result<(ServingRecovery, PathBuf), String> {
    let (transport, root) = transport_proof(scope, record, target, store)?;
    let encoded = transport_bytes(&transport)?;
    if read_assignment_bytes(&root.join("transport-clarification.json"))? != encoded {
        return Err("WEB_TASK_CONTEXT_TRANSPORT_CHANGED".into());
    }
    let turn = format!(
        "initial-context-{}",
        digest(std::str::from_utf8(&encoded).map_err(|_| "WEB_TASK_CONTEXT_INVALID")?)
    );
    let id = catdesk_wake::store::WakeIntent {
        purpose: catdesk_wake::protocol::WakePurpose::NewTask,
        source_event_id: &record.event_id,
        source_turn_id: &turn,
        priority: 0,
        ttl_seconds: 600,
    }
    .event_id(&scope.project.project_id)?;
    let prior = store
        .delivery(&id)?
        .ok_or("WEB_TASK_CONTEXT_SERVING_PRIOR_REQUIRED")?;
    let receipt = prior
        .receipt
        .clone()
        .ok_or("WEB_TASK_CONTEXT_SERVING_PRIOR_REQUIRED")?;
    let metadata = prior
        .event
        .wake_metadata()?
        .ok_or("WEB_TASK_CONTEXT_SERVING_PRIOR_REQUIRED")?;
    if prior.phase != catdesk_wake::store::Phase::Sent
        || prior.target != *target
        || prior.event.event_id != id
        || prior.event.project_id != scope.project.project_id
        || prior.event.target_generation != target.generation
        || prior.event.message != transport_message(&transport)
        || metadata.purpose != catdesk_wake::protocol::WakePurpose::NewTask
        || metadata.source_event_id != record.event_id
        || metadata.source_turn_id != turn
        || receipt.schema_version != 1
        || receipt.event_id != id
        || receipt.target_generation != target.generation
        || receipt.target_digest != target.digest
        || receipt.message_digest != digest(&prior.event.message)
        || receipt.evidence != "EXACT_USER_MESSAGE_APPENDED"
        || receipt.sent_utc == 0
        || store.timer(&id)?.is_none_or(|timer| {
            timer.response_state != catdesk_wake::store::ResponseState::Complete
                || timer.event_id != id
                || timer.project_id != scope.project.project_id
                || timer.target_generation != target.generation
                || timer.target_digest != target.digest
        })
    {
        return Err("WEB_TASK_CONTEXT_SERVING_PRIOR_REQUIRED".into());
    }
    Ok((
        ServingRecovery {
            schema_version: 1,
            purpose: "ORIGINAL_NATIVE_TASK_SERVING_RECOVERY".into(),
            transport,
            prior_transport_receipt: receipt,
        },
        root,
    ))
}

fn serving_bytes(value: &ServingRecovery) -> Result<Vec<u8>, String> {
    serde_json::to_vec(value).map_err(|_| "WEB_TASK_CONTEXT_INVALID".into())
}

/// One diagnostic in the original journal, never a substitute task/grant.
#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CachedRouteProbe {
    schema_version: u32,
    purpose: String,
    serving: ServingRecovery,
    prior_serving_receipt: Receipt,
    diagnostic_command: String,
}

fn route_probe_proof(
    scope: &ResolvedProjectScopeV1,
    record: &crate::web_work_state::WorkRecord,
    target: &Target,
    store: &catdesk_wake::store::Store,
) -> Result<(CachedRouteProbe, PathBuf), String> {
    // This rechecks the original READY/no-attempt/no-receipt/no-journal state,
    // capsule, grant, canonical envelope and every predecessor receipt.
    let (serving, root) = serving_proof(scope, record, target, store)?;
    let encoded = serving_bytes(&serving)?;
    if read_assignment_bytes(&root.join("serving-recovery.json"))? != encoded {
        return Err("WEB_TASK_CONTEXT_SERVING_CHANGED".into());
    }
    let turn = format!(
        "initial-context-{}",
        digest(std::str::from_utf8(&encoded).map_err(|_| "WEB_TASK_CONTEXT_INVALID")?)
    );
    let id = catdesk_wake::store::WakeIntent {
        purpose: catdesk_wake::protocol::WakePurpose::NewTask,
        source_event_id: &record.event_id,
        source_turn_id: &turn,
        priority: 0,
        ttl_seconds: 600,
    }
    .event_id(&scope.project.project_id)?;
    let prior = store
        .delivery(&id)?
        .ok_or("WEB_TASK_CONTEXT_ROUTE_PROBE_PRIOR_REQUIRED")?;
    let receipt = prior
        .receipt
        .clone()
        .ok_or("WEB_TASK_CONTEXT_ROUTE_PROBE_PRIOR_REQUIRED")?;
    let metadata = prior
        .event
        .wake_metadata()?
        .ok_or("WEB_TASK_CONTEXT_ROUTE_PROBE_PRIOR_REQUIRED")?;
    if prior.phase != catdesk_wake::store::Phase::Sent
        || prior.target != *target
        || prior.event.event_id != id
        || prior.event.project_id != scope.project.project_id
        || prior.event.target_generation != target.generation
        || prior.event.message != serving_message(&serving)
        || metadata.purpose != catdesk_wake::protocol::WakePurpose::NewTask
        || metadata.source_event_id != record.event_id
        || metadata.source_turn_id != turn
        || receipt.schema_version != 1
        || receipt.event_id != id
        || receipt.target_generation != target.generation
        || receipt.target_digest != target.digest
        || receipt.message_digest != digest(&prior.event.message)
        || receipt.evidence != "EXACT_USER_MESSAGE_APPENDED"
        || receipt.sent_utc == 0
        || store.timer(&id)?.is_none_or(|timer| {
            timer.response_state != catdesk_wake::store::ResponseState::Complete
                || timer.event_id != id
                || timer.project_id != scope.project.project_id
                || timer.target_generation != target.generation
                || timer.target_digest != target.digest
        })
    {
        return Err("WEB_TASK_CONTEXT_ROUTE_PROBE_PRIOR_REQUIRED".into());
    }
    Ok((
        CachedRouteProbe {
            schema_version: 1,
            purpose: "ORIGINAL_TASK_CACHED_ROUTE_NO_EFFECT_PROBE".into(),
            serving,
            prior_serving_receipt: receipt,
            diagnostic_command: "CATDESK_WEB_TASK={}".into(),
        },
        root,
    ))
}

fn route_probe_bytes(value: &CachedRouteProbe) -> Result<Vec<u8>, String> {
    serde_json::to_vec(value).map_err(|_| "WEB_TASK_CONTEXT_INVALID".into())
}

pub(super) fn validate_route_probe(
    scope: &ResolvedProjectScopeV1,
    record: &crate::web_work_state::WorkRecord,
    target: &Target,
    store: &catdesk_wake::store::Store,
) -> Result<(), String> {
    let (value, root) = route_probe_proof(scope, record, target, store)?;
    if read_assignment_bytes(&root.join("cached-route-probe.json"))? != route_probe_bytes(&value)? {
        return Err("WEB_TASK_CONTEXT_ROUTE_PROBE_CHANGED".into());
    }
    Ok(())
}

fn route_probe_message(value: &CachedRouteProbe) -> String {
    let r = &value.serving.transport.identity.reference;
    format!(
        "CatDesk bounded no-effect transport diagnostic for original task {}/{}. Prior completed serving USER digest={}. This is NOT permission to resume or retry native work. Preserve the original task, grant, key and all sessions. Use the registered CatDesk v5 run_command tool exactly ONCE with the entire arguments object {{\"command\":\"CATDESK_WEB_TASK={{}}\"}}. Omit every optional outer argument. This intentionally empty reserved envelope has no grant or native call and must be refused before execution; never run a shell command or JavaScript. Do not use the original capsule, nativeCall, execution key, central dispatcher or any other tool. If run_command is unavailable or the platform refuses this exact call, stop. Report only actual tool name, success/error status, returned error names and invariant if supplied; distinguish client refusal from a server response and do not infer which layer failed. No retries, code edits, tests, bindings or production changes. Keep private context and credentials out of the answer.",
        r.project_id, r.request_id, value.prior_serving_receipt.message_digest
    )
}

pub(super) fn queue_route_probe(
    scope: &ResolvedProjectScopeV1,
    record: &crate::web_work_state::WorkRecord,
    target: &Target,
    store: &catdesk_wake::store::Store,
) -> Result<Value, String> {
    let (value, root) = route_probe_proof(scope, record, target, store)?;
    let encoded = route_probe_bytes(&value)?;
    exact_file(&root.join("cached-route-probe.json"), &encoded)?;
    let proof_sha = digest(std::str::from_utf8(&encoded).map_err(|_| "WEB_TASK_CONTEXT_INVALID")?);
    let turn = format!("initial-context-{proof_sha}");
    let event = store.produce_expected_wake(
        &scope.project.project_id,
        &route_probe_message(&value),
        target,
        catdesk_wake::store::WakeIntent {
            purpose: catdesk_wake::protocol::WakePurpose::NewTask,
            source_event_id: &record.event_id,
            source_turn_id: &turn,
            priority: 0,
            ttl_seconds: 600,
        },
    )?;
    let delivery = store.delivery(&event.event_id)?;
    Ok(
        json!({"status":if delivery.is_some(){"TASK_ROUTE_PROBE_ALREADY_RECORDED"}else{"TASK_ROUTE_PROBE_QUEUED"},
        "projectId":scope.project.project_id,"requestId":record.request_id,"taskId":value.serving.transport.identity.reference.assignment.task_id,
        "eventId":event.event_id,"messageSha256":digest(&event.message),"probeSha256":proof_sha,
        "priorServingEventId":value.prior_serving_receipt.event_id,"targetGeneration":target.generation,"targetSha256":target.digest,
        "deliveryPhase":delivery.as_ref().map(|v|&v.phase),"queueOnly":true,"servingReadinessAssessed":false,
        "newGrantCreated":false,"nativeDispatchRequested":false,"nativeSessionAction":"NONE","taskCompleted":false,"executionKeyDisclosed":false}),
    )
}

pub(super) fn validate_serving(
    scope: &ResolvedProjectScopeV1,
    record: &crate::web_work_state::WorkRecord,
    target: &Target,
    store: &catdesk_wake::store::Store,
) -> Result<(), String> {
    let (value, root) = serving_proof(scope, record, target, store)?;
    if read_assignment_bytes(&root.join("serving-recovery.json"))? != serving_bytes(&value)? {
        return Err("WEB_TASK_CONTEXT_SERVING_CHANGED".into());
    }
    Ok(())
}

fn serving_message(value: &ServingRecovery) -> String {
    let identity = &value.transport.identity;
    let r = &identity.reference;
    format!(
        "CatDesk original-task serving-window continuation for {}/{}. This is the same unfinished task, not a new assignment. Original fullPrivateContextSha256={} and assignmentSha256={} are unchanged. Prior completed transport-clarification USER digest={}; never resend that turn. Fresh local pre-admission proves the original native task READY with no attempt, receipt or task journal, and the canonical cached envelope still matches. Use the original capsule's exact nativeCall (native-dispatch-1, native.codex.dispatch, arguments {{}}) ONCE through registered CatDesk v5 tools. Prefer catdesk_web_task_tool if available; otherwise invoke run_command with ONLY command: literal CATDESK_WEB_TASK= immediately followed by the unchanged nativeCall JSON serialization. Omit every optional outer argument; do not use a placeholder, JavaScript or a shell command. Do not invent or replace the task, grant, key, call ID, root or native session. If any original identity/context/tool is unavailable or any native effect is uncertain, stop and report the exact bounded error/refusal. Keep the execution key private. Await the frozen native result and report actual edits, tests and blockers; do not substitute local-only execution or browser completion for task completion.",
        r.project_id,
        r.request_id,
        identity.full_private_context_sha256,
        r.assignment.assignment_sha256,
        value.prior_transport_receipt.message_digest
    )
}

pub(super) fn queue_serving(
    scope: &ResolvedProjectScopeV1,
    record: &crate::web_work_state::WorkRecord,
    target: &Target,
    store: &catdesk_wake::store::Store,
) -> Result<Value, String> {
    let (value, root) = serving_proof(scope, record, target, store)?;
    let encoded = serving_bytes(&value)?;
    exact_file(&root.join("serving-recovery.json"), &encoded)?;
    let proof_sha = digest(std::str::from_utf8(&encoded).map_err(|_| "WEB_TASK_CONTEXT_INVALID")?);
    let turn = format!("initial-context-{proof_sha}");
    let event = store.produce_expected_wake(
        &scope.project.project_id,
        &serving_message(&value),
        target,
        catdesk_wake::store::WakeIntent {
            purpose: catdesk_wake::protocol::WakePurpose::NewTask,
            source_event_id: &record.event_id,
            source_turn_id: &turn,
            priority: 0,
            ttl_seconds: 600,
        },
    )?;
    let delivery = store.delivery(&event.event_id)?;
    Ok(
        json!({"status":if delivery.is_some(){"TASK_SERVING_RECOVERY_ALREADY_RECORDED"}else{"TASK_SERVING_RECOVERY_QUEUED"},
        "projectId":scope.project.project_id,"requestId":record.request_id,"taskId":value.transport.identity.reference.assignment.task_id,
        "eventId":event.event_id,"messageSha256":digest(&event.message),"servingRecoverySha256":proof_sha,
        "priorTransportEventId":value.prior_transport_receipt.event_id,"targetGeneration":target.generation,"targetSha256":target.digest,
        "deliveryPhase":delivery.as_ref().map(|v|&v.phase),"queueOnly":true,"servingReadinessAssessed":false,
        "newGrantCreated":false,"nativeSessionAction":"NONE","taskCompleted":false,"executionKeyDisclosed":false}),
    )
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use catdesk_wake::store::{Phase, Store, now};

    fn sent_event(store: &Store, event: &catdesk_wake::protocol::Event) {
        sent_event_with_completion(store, event, true);
    }

    fn finish_queued(store: &Store, value: Value, complete: bool) -> catdesk_wake::protocol::Event {
        let event = catdesk_wake::store::read(
            &store
                .root()
                .join("queue")
                .join(format!("{}.json", value["eventId"].as_str().unwrap())),
        )
        .unwrap();
        sent_event_with_completion(store, &event, complete);
        event
    }

    #[test]
    fn route_probe_is_one_empty_envelope_and_preserves_original_grant_through_unknown_and_sent() {
        crate::web_task_dispatch::tests::with_native_context_fixture(|control, pins| {
            let scope =
                crate::delegated::autonomy_projects::resolve_registered_project_from_control_root(
                    control, "alpha",
                )
                .unwrap();
            let store = Store::open(
                &crate::wake_protocol_client::wake_root_for_workspace(control).unwrap(),
            )
            .unwrap();
            let record = crate::web_work_state::WorkLedger::open(control, "alpha")
                .unwrap()
                .record("request-a")
                .unwrap()
                .unwrap();
            let (_, payload, root) = delivered(&scope, &record, &store);
            finish_queued(
                &store,
                queue(&scope, &record, &record.target, &store).unwrap(),
                true,
            );
            finish_queued(
                &store,
                queue_transport(&scope, &record, &record.target, &store).unwrap(),
                true,
            );
            let prior = finish_queued(
                &store,
                queue_serving(&scope, &record, &record.target, &store).unwrap(),
                true,
            );
            let task_dir = super::super::super::task_directory(
                &scope,
                &record.task_assignment.as_ref().unwrap().task_id,
                false,
            )
            .unwrap();
            let paths = [
                root.join("initial-payload.json"),
                root.join("initial-delivery.json"),
                root.join("identity-clarification.json"),
                root.join("transport-clarification.json"),
                root.join("serving-recovery.json"),
                task_dir.join("execution-grant.json"),
            ];
            let before: Vec<_> = paths.iter().map(|p| fs::read(p).unwrap()).collect();
            let prepared = crate::web_work::context_action(
                control,
                pins.clone(),
                "context-prepare-route-probe",
            )
            .unwrap();
            assert_eq!(prepared["status"], "TASK_ROUTE_PROBE_QUEUED");
            assert_eq!(prepared["priorServingEventId"], prior.event_id);
            assert_eq!(prepared["nativeDispatchRequested"], false);
            assert_eq!(prepared["newGrantCreated"], false);
            assert_eq!(
                prepared,
                queue_route_probe(&scope, &record, &record.target, &store).unwrap()
            );
            let id = prepared["eventId"].as_str().unwrap();
            let event: catdesk_wake::protocol::Event =
                catdesk_wake::store::read(&store.root().join("queue").join(format!("{id}.json")))
                    .unwrap();
            assert_ne!(id, prior.event_id);
            assert!(event.message.len() <= 2048);
            assert!(
                event
                    .message
                    .contains(r#"{"command":"CATDESK_WEB_TASK={}"}"#)
            );
            assert!(
                !event
                    .message
                    .contains(payload.context["executionKey"].as_str().unwrap())
            );
            assert!(!event.message.contains("native-dispatch-1"));
            assert!(!event.message.contains("native.codex.dispatch"));
            validate_route_probe(&scope, &record, &record.target, &store).unwrap();
            let path = store.root().join("deliveries").join(format!("{id}.json"));
            {
                let host = store.host_lock().unwrap();
                store.claim(&event, &host).unwrap();
                store
                    .transition(id, host.owner(), Phase::Submitting, None, None)
                    .unwrap();
                let unknown = fs::read(&path).unwrap();
                let same = queue_route_probe(&scope, &record, &record.target, &store).unwrap();
                assert_eq!(same["status"], "TASK_ROUTE_PROBE_ALREADY_RECORDED");
                assert_eq!(same["eventId"], id);
                assert_eq!(fs::read(&path).unwrap(), unknown);
                let receipt = Receipt {
                    schema_version: 1,
                    event_id: id.into(),
                    target_generation: record.target.generation,
                    target_digest: record.target.digest.clone(),
                    message_digest: digest(&event.message),
                    sent_utc: now(),
                    evidence: "EXACT_USER_MESSAGE_APPENDED".into(),
                };
                store.record_exact_receipt_timer(id, &receipt).unwrap();
                store
                    .transition(id, host.owner(), Phase::Sent, Some(receipt), None)
                    .unwrap();
                store.complete_timer(id).unwrap();
            }
            let sent = fs::read(&path).unwrap();
            assert_eq!(
                queue_route_probe(&scope, &record, &record.target, &store).unwrap()["eventId"],
                id
            );
            assert_eq!(fs::read(path).unwrap(), sent);
            for (path, bytes) in paths.iter().zip(before) {
                assert_eq!(fs::read(path).unwrap(), bytes);
            }
            assert!(!task_dir.join("journal").exists());
        });
    }

    #[test]
    fn route_probe_requires_completed_serving_and_refuses_tamper_target_or_native_effect() {
        crate::web_task_dispatch::tests::with_native_context_fixture(|control, _| {
            let scope =
                crate::delegated::autonomy_projects::resolve_registered_project_from_control_root(
                    control, "alpha",
                )
                .unwrap();
            let store = Store::open(
                &crate::wake_protocol_client::wake_root_for_workspace(control).unwrap(),
            )
            .unwrap();
            let record = crate::web_work_state::WorkLedger::open(control, "alpha")
                .unwrap()
                .record("request-a")
                .unwrap()
                .unwrap();
            let (_, _, root) = delivered(&scope, &record, &store);
            assert!(queue_route_probe(&scope, &record, &record.target, &store).is_err());
            finish_queued(
                &store,
                queue(&scope, &record, &record.target, &store).unwrap(),
                true,
            );
            finish_queued(
                &store,
                queue_transport(&scope, &record, &record.target, &store).unwrap(),
                true,
            );
            let queued = queue_serving(&scope, &record, &record.target, &store).unwrap();
            assert_eq!(
                queue_route_probe(&scope, &record, &record.target, &store).unwrap_err(),
                "WEB_TASK_CONTEXT_ROUTE_PROBE_PRIOR_REQUIRED"
            );
            let prior = finish_queued(&store, queued, false);
            assert_eq!(
                queue_route_probe(&scope, &record, &record.target, &store).unwrap_err(),
                "WEB_TASK_CONTEXT_ROUTE_PROBE_PRIOR_REQUIRED"
            );
            let path = root.join("cached-route-probe.json");
            assert!(!path.exists());
            store.complete_timer(&prior.event_id).unwrap();
            let queued = queue_route_probe(&scope, &record, &record.target, &store).unwrap();
            let bytes = fs::read(&path).unwrap();
            let mut changed: Value = serde_json::from_slice(&bytes).unwrap();
            changed["diagnosticCommand"] = json!("ANOTHER_COMMAND");
            let tampered = serde_json::to_vec(&changed).unwrap();
            fs::write(&path, &tampered).unwrap();
            assert_eq!(
                validate_route_probe(&scope, &record, &record.target, &store).unwrap_err(),
                "WEB_TASK_CONTEXT_ROUTE_PROBE_CHANGED"
            );
            assert!(queue_route_probe(&scope, &record, &record.target, &store).is_err());
            assert_eq!(fs::read(&path).unwrap(), tampered);
            fs::write(&path, bytes).unwrap();
            let wrong =
                Target::new(record.target.generation + 1, record.target.url.clone()).unwrap();
            assert!(queue_route_probe(&scope, &record, &wrong, &store).is_err());
            let task_dir = super::super::super::task_directory(
                &scope,
                &record.task_assignment.as_ref().unwrap().task_id,
                false,
            )
            .unwrap();
            fs::create_dir(task_dir.join("journal")).unwrap();
            assert_eq!(
                queue_route_probe(&scope, &record, &record.target, &store).unwrap_err(),
                "WEB_TASK_CONTEXT_NATIVE_EFFECT_UNPROVEN"
            );
            assert!(
                store
                    .delivery(queued["eventId"].as_str().unwrap())
                    .unwrap()
                    .is_none()
            );
        });
    }

    fn sent_event_with_completion(
        store: &Store,
        event: &catdesk_wake::protocol::Event,
        complete: bool,
    ) {
        let host = store.host_lock().unwrap();
        let claimed = store.claim(event, &host).unwrap();
        store
            .transition(&event.event_id, host.owner(), Phase::Submitting, None, None)
            .unwrap();
        let receipt = Receipt {
            schema_version: 1,
            event_id: event.event_id.clone(),
            target_generation: claimed.target.generation,
            target_digest: claimed.target.digest.clone(),
            message_digest: digest(&event.message),
            sent_utc: now(),
            evidence: "EXACT_USER_MESSAGE_APPENDED".into(),
        };
        store
            .record_exact_receipt_timer(&event.event_id, &receipt)
            .unwrap();
        store
            .transition(
                &event.event_id,
                host.owner(),
                Phase::Sent,
                Some(receipt),
                None,
            )
            .unwrap();
        if complete {
            store.complete_timer(&event.event_id).unwrap();
        }
    }

    fn delivered(
        scope: &ResolvedProjectScopeV1,
        record: &crate::web_work_state::WorkRecord,
        store: &Store,
    ) -> (Reference, Payload, PathBuf) {
        prepare_initial_context(scope, record, &record.target).unwrap();
        run_with_sender(
            scope,
            record,
            &record.target,
            InitialContextAction::Send,
            store,
            &|| Ok(()),
            |_, request, before, _| {
                before()?;
                Ok(Receipt {
                    schema_version: 1,
                    event_id: request.event_id.into(),
                    target_generation: request.target.generation,
                    target_digest: request.target.digest.clone(),
                    message_digest: digest(
                        &request
                            .message
                            .split_whitespace()
                            .collect::<Vec<_>>()
                            .join(" "),
                    ),
                    sent_utc: now(),
                    evidence: "EXACT_USER_MESSAGE_APPENDED".into(),
                })
            },
        )
        .unwrap();
        let (reference, payload, root) = load(scope, record, &record.target).unwrap();
        assert!(capsule_full_context_digest(&payload).unwrap().is_some());
        let turn = format!("initial-context-{}", reference.storage_id);
        let event = store
            .produce_expected_wake(
                &scope.project.project_id,
                &legacy_native_message(&reference),
                &record.target,
                catdesk_wake::store::WakeIntent {
                    purpose: catdesk_wake::protocol::WakePurpose::NewTask,
                    source_event_id: &record.event_id,
                    source_turn_id: &turn,
                    priority: 0,
                    ttl_seconds: 600,
                },
            )
            .unwrap();
        sent_event(store, &event);
        (reference, payload, root)
    }

    #[test]
    fn clarification_links_distinct_valid_hashes_without_changing_frozen_records_or_grant() {
        crate::web_task_dispatch::tests::with_native_context_fixture(|control, pins| {
            let scope =
                crate::delegated::autonomy_projects::resolve_registered_project_from_control_root(
                    control, "alpha",
                )
                .unwrap();
            let store = Store::open(
                &crate::wake_protocol_client::wake_root_for_workspace(control).unwrap(),
            )
            .unwrap();
            let record = crate::web_work_state::WorkLedger::open(control, "alpha")
                .unwrap()
                .record("request-a")
                .unwrap()
                .unwrap();
            let (reference, payload, root) = delivered(&scope, &record, &store);
            let original = continuation_identity(&reference, &record).unwrap();
            let paths = [
                root.join("initial-payload.json"),
                root.join("initial-delivery.json"),
                reference_path(&scope, &record, &record.target).unwrap(),
                super::super::super::task_directory(&scope, &reference.assignment.task_id, false)
                    .unwrap()
                    .join("execution-grant.json"),
                store
                    .root()
                    .join("deliveries")
                    .join(format!("{original}.json")),
            ];
            let before: Vec<_> = paths.iter().map(|p| fs::read(p).unwrap()).collect();
            let queued =
                crate::web_work::context_action(control, pins.clone(), "context-clarify-identity")
                    .unwrap();
            assert_eq!(queued["status"], "TASK_IDENTITY_CLARIFICATION_QUEUED");
            let again =
                crate::web_work::context_action(control, pins.clone(), "context-clarify-identity")
                    .unwrap();
            assert_eq!(queued, again);
            let id = queued["eventId"].as_str().unwrap();
            assert_ne!(id, original);
            let event: catdesk_wake::protocol::Event =
                catdesk_wake::store::read(&store.root().join("queue").join(format!("{id}.json")))
                    .unwrap();
            assert!(event.message.len() <= 2048);
            assert!(event.message.contains(&reference.message_sha256));
            let full = capsule_full_context_digest(&payload).unwrap().unwrap();
            assert_ne!(full, reference.message_sha256);
            assert!(event.message.contains(&full));
            assert!(
                event
                    .message
                    .contains(&reference.assignment.assignment_sha256)
            );
            assert!(
                !event
                    .message
                    .contains(payload.context["executionKey"].as_str().unwrap())
            );
            validate(&scope, &record, &record.target, &store).unwrap();
            sent_event(&store, &event);
            let accounted = with_context_accounting(
                json!({"eventId":id}),
                &scope,
                &record,
                &record.target,
                &store,
            );
            assert_eq!(
                accounted["contextAccounting"]["selectedContinuation"],
                "RECORDED"
            );
            let old = queue_continuation(&scope, &record, &record.target, &store).unwrap();
            assert_eq!(old["eventId"], original);
            assert_eq!(old["deliveryPhase"], "SENT");
            assert!(
                !store
                    .root()
                    .join("queue")
                    .join(format!("{original}.json"))
                    .exists()
            );
            for (path, bytes) in paths.iter().zip(before) {
                assert_eq!(fs::read(path).unwrap(), bytes);
            }
            let dir =
                super::super::super::task_directory(&scope, &reference.assignment.task_id, false)
                    .unwrap();
            fs::create_dir(dir.join("journal")).unwrap();
            assert_eq!(
                queue(&scope, &record, &record.target, &store).unwrap_err(),
                "WEB_TASK_CONTEXT_NATIVE_EFFECT_UNPROVEN"
            );
        });
    }

    #[test]
    fn capsule_tampering_cannot_be_repaired_by_replacing_a_digest() {
        let target = Target::new(1, "https://chatgpt.com/c/fixture".into()).unwrap();
        let context = json!({"projectId":"alpha","requestId":"original","sourceEventId":"source","taskId":"task",
            "assignmentSha256":"a".repeat(64),"expectedTargetGeneration":1,"expectedTargetSha256":target.digest,"executionKey":"b".repeat(64),
            "nativeQueue":{"selection":{"taskId":"native"}},"journaledCalls":[],"patchProposals":[],"patchApplications":[],"objective":"x".repeat(3000)});
        let message = initial_context_message(&context, "alpha", "original", &target).unwrap();
        let mut payload = Payload {
            schema_version: 1,
            context,
            message,
        };
        assert!(capsule_full_context_digest(&payload).unwrap().is_some());
        payload.context["assignmentSha256"] = json!("c".repeat(64));
        assert_eq!(
            capsule_full_context_digest(&payload).unwrap_err(),
            "WEB_TASK_CONTEXT_IDENTITY_MISMATCH"
        );
    }

    #[test]
    fn transport_clarification_reuses_original_task_and_cannot_recreate_sent_or_unknown_work() {
        crate::web_task_dispatch::tests::with_native_context_fixture(|control, pins| {
            let scope =
                crate::delegated::autonomy_projects::resolve_registered_project_from_control_root(
                    control, "alpha",
                )
                .unwrap();
            let store = Store::open(
                &crate::wake_protocol_client::wake_root_for_workspace(control).unwrap(),
            )
            .unwrap();
            let record = crate::web_work_state::WorkLedger::open(control, "alpha")
                .unwrap()
                .record("request-a")
                .unwrap()
                .unwrap();
            let (_, payload, root) = delivered(&scope, &record, &store);
            let prior = queue(&scope, &record, &record.target, &store).unwrap();
            let prior_id = prior["eventId"].as_str().unwrap();
            let prior_event = catdesk_wake::store::read(
                &store.root().join("queue").join(format!("{prior_id}.json")),
            )
            .unwrap();
            sent_event(&store, &prior_event);
            let grant = super::super::super::task_directory(
                &scope,
                &record.task_assignment.as_ref().unwrap().task_id,
                false,
            )
            .unwrap()
            .join("execution-grant.json");
            let paths = [
                root.join("initial-payload.json"),
                root.join("initial-delivery.json"),
                root.join("identity-clarification.json"),
                grant,
                store
                    .root()
                    .join("deliveries")
                    .join(format!("{prior_id}.json")),
            ];
            let before: Vec<_> = paths.iter().map(|p| fs::read(p).unwrap()).collect();
            let queued =
                crate::web_work::context_action(control, pins.clone(), "context-clarify-transport")
                    .unwrap();
            assert_eq!(queued["status"], "TASK_TRANSPORT_CLARIFICATION_QUEUED");
            assert_eq!(queued["requestId"], record.request_id);
            assert_eq!(queued["newGrantCreated"], false);
            assert_eq!(
                queued,
                crate::web_work::context_action(control, pins.clone(), "context-clarify-transport")
                    .unwrap()
            );
            let id = queued["eventId"].as_str().unwrap();
            assert_ne!(id, prior_id);
            let event: catdesk_wake::protocol::Event =
                catdesk_wake::store::read(&store.root().join("queue").join(format!("{id}.json")))
                    .unwrap();
            assert!(event.message.len() <= 2048);
            assert!(event.message.contains("ONLY its command argument"));
            assert!(
                !event
                    .message
                    .contains(payload.context["executionKey"].as_str().unwrap())
            );
            validate_transport(&scope, &record, &record.target, &store).unwrap();
            {
                let host = store.host_lock().unwrap();
                store.claim(&event, &host).unwrap();
                store
                    .transition(id, host.owner(), Phase::Submitting, None, None)
                    .unwrap();
            }
            let unknown_path = store.root().join("deliveries").join(format!("{id}.json"));
            let unknown_before = fs::read(&unknown_path).unwrap();
            assert_eq!(
                queued,
                queue_transport(&scope, &record, &record.target, &store).unwrap()
            );
            assert_eq!(fs::read(&unknown_path).unwrap(), unknown_before);
            for (path, bytes) in paths.iter().zip(before) {
                assert_eq!(fs::read(path).unwrap(), bytes);
            }
            let directory = super::super::super::task_directory(
                &scope,
                &record.task_assignment.as_ref().unwrap().task_id,
                false,
            )
            .unwrap();
            fs::create_dir(directory.join("journal")).unwrap();
            assert_eq!(
                queue_transport(&scope, &record, &record.target, &store).unwrap_err(),
                "WEB_TASK_CONTEXT_NATIVE_EFFECT_UNPROVEN"
            );
        });
    }

    #[test]
    fn transport_requires_completed_identity_turn_and_rejects_frozen_proof_tampering() {
        crate::web_task_dispatch::tests::with_native_context_fixture(|control, _| {
            let scope =
                crate::delegated::autonomy_projects::resolve_registered_project_from_control_root(
                    control, "alpha",
                )
                .unwrap();
            let store = Store::open(
                &crate::wake_protocol_client::wake_root_for_workspace(control).unwrap(),
            )
            .unwrap();
            let record = crate::web_work_state::WorkLedger::open(control, "alpha")
                .unwrap()
                .record("request-a")
                .unwrap()
                .unwrap();
            let (_, _, root) = delivered(&scope, &record, &store);
            assert!(queue_transport(&scope, &record, &record.target, &store).is_err());
            assert!(!root.join("transport-clarification.json").exists());
            let prior = queue(&scope, &record, &record.target, &store).unwrap();
            assert_eq!(
                queue_transport(&scope, &record, &record.target, &store).unwrap_err(),
                "WEB_TASK_CONTEXT_TRANSPORT_PRIOR_REQUIRED"
            );
            let id = prior["eventId"].as_str().unwrap();
            let event =
                catdesk_wake::store::read(&store.root().join("queue").join(format!("{id}.json")))
                    .unwrap();
            sent_event(&store, &event);
            queue_transport(&scope, &record, &record.target, &store).unwrap();
            let path = root.join("transport-clarification.json");
            let original = fs::read(&path).unwrap();
            let mut value: Value = serde_json::from_slice(&original).unwrap();
            value["canonicalCachedEnvelopeSha256"] = json!("f".repeat(64));
            fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
            assert_eq!(
                validate_transport(&scope, &record, &record.target, &store).unwrap_err(),
                "WEB_TASK_CONTEXT_TRANSPORT_CHANGED"
            );
            assert!(queue_transport(&scope, &record, &record.target, &store).is_err());
        });
    }

    #[test]
    fn serving_recovery_keeps_original_identity_and_one_intent_through_unknown_and_sent() {
        crate::web_task_dispatch::tests::with_native_context_fixture(|control, pins| {
            let scope =
                crate::delegated::autonomy_projects::resolve_registered_project_from_control_root(
                    control, "alpha",
                )
                .unwrap();
            let store = Store::open(
                &crate::wake_protocol_client::wake_root_for_workspace(control).unwrap(),
            )
            .unwrap();
            let record = crate::web_work_state::WorkLedger::open(control, "alpha")
                .unwrap()
                .record("request-a")
                .unwrap()
                .unwrap();
            let (_, payload, root) = delivered(&scope, &record, &store);
            let identity = queue(&scope, &record, &record.target, &store).unwrap();
            let identity_event = catdesk_wake::store::read(
                &store
                    .root()
                    .join("queue")
                    .join(format!("{}.json", identity["eventId"].as_str().unwrap())),
            )
            .unwrap();
            sent_event(&store, &identity_event);
            let transport = queue_transport(&scope, &record, &record.target, &store).unwrap();
            let transport_event = catdesk_wake::store::read(
                &store
                    .root()
                    .join("queue")
                    .join(format!("{}.json", transport["eventId"].as_str().unwrap())),
            )
            .unwrap();
            sent_event(&store, &transport_event);
            let grant = super::super::super::task_directory(
                &scope,
                &record.task_assignment.as_ref().unwrap().task_id,
                false,
            )
            .unwrap()
            .join("execution-grant.json");
            let paths = [
                root.join("initial-payload.json"),
                root.join("initial-delivery.json"),
                root.join("identity-clarification.json"),
                root.join("transport-clarification.json"),
                grant,
            ];
            let before: Vec<_> = paths.iter().map(|p| fs::read(p).unwrap()).collect();
            let queued =
                crate::web_work::context_action(control, pins.clone(), "context-resume-serving")
                    .unwrap();
            assert_eq!(queued["status"], "TASK_SERVING_RECOVERY_QUEUED");
            assert_eq!(queued["requestId"], record.request_id);
            assert_eq!(queued["newGrantCreated"], false);
            assert_eq!(queued["servingReadinessAssessed"], false);
            assert_eq!(
                queued,
                queue_serving(&scope, &record, &record.target, &store).unwrap()
            );
            let id = queued["eventId"].as_str().unwrap();
            assert_ne!(id, transport_event.event_id);
            let event: catdesk_wake::protocol::Event =
                catdesk_wake::store::read(&store.root().join("queue").join(format!("{id}.json")))
                    .unwrap();
            assert!(event.message.len() <= 2048);
            assert!(event.message.contains("native-dispatch-1"));
            assert!(
                !event
                    .message
                    .contains(payload.context["executionKey"].as_str().unwrap())
            );
            validate_serving(&scope, &record, &record.target, &store).unwrap();
            {
                let host = store.host_lock().unwrap();
                store.claim(&event, &host).unwrap();
                store
                    .transition(id, host.owner(), Phase::Submitting, None, None)
                    .unwrap();
                let path = store.root().join("deliveries").join(format!("{id}.json"));
                let unknown = fs::read(&path).unwrap();
                let same = queue_serving(&scope, &record, &record.target, &store).unwrap();
                assert_eq!(same["eventId"], id);
                assert_eq!(same["status"], "TASK_SERVING_RECOVERY_ALREADY_RECORDED");
                assert_eq!(
                    same["deliveryPhase"],
                    serde_json::to_value(Phase::Submitting).unwrap()
                );
                assert_eq!(fs::read(&path).unwrap(), unknown);
                let receipt = Receipt {
                    schema_version: 1,
                    event_id: id.into(),
                    target_generation: record.target.generation,
                    target_digest: record.target.digest.clone(),
                    message_digest: digest(&event.message),
                    sent_utc: now(),
                    evidence: "EXACT_USER_MESSAGE_APPENDED".into(),
                };
                store.record_exact_receipt_timer(id, &receipt).unwrap();
                store
                    .transition(id, host.owner(), Phase::Sent, Some(receipt), None)
                    .unwrap();
                store.complete_timer(id).unwrap();
            }
            let path = store.root().join("deliveries").join(format!("{id}.json"));
            let sent = fs::read(&path).unwrap();
            let same = queue_serving(&scope, &record, &record.target, &store).unwrap();
            assert_eq!(same["eventId"], id);
            assert_eq!(
                same["deliveryPhase"],
                serde_json::to_value(Phase::Sent).unwrap()
            );
            assert_eq!(fs::read(path).unwrap(), sent);
            for (path, bytes) in paths.iter().zip(before) {
                assert_eq!(fs::read(path).unwrap(), bytes);
            }
            let directory = super::super::super::task_directory(
                &scope,
                &record.task_assignment.as_ref().unwrap().task_id,
                false,
            )
            .unwrap();
            fs::create_dir(directory.join("journal")).unwrap();
            assert_eq!(
                queue_serving(&scope, &record, &record.target, &store).unwrap_err(),
                "WEB_TASK_CONTEXT_NATIVE_EFFECT_UNPROVEN"
            );
        });
    }

    #[test]
    fn serving_recovery_requires_completed_transport_and_refuses_proof_or_target_drift() {
        crate::web_task_dispatch::tests::with_native_context_fixture(|control, _| {
            let scope =
                crate::delegated::autonomy_projects::resolve_registered_project_from_control_root(
                    control, "alpha",
                )
                .unwrap();
            let store = Store::open(
                &crate::wake_protocol_client::wake_root_for_workspace(control).unwrap(),
            )
            .unwrap();
            let record = crate::web_work_state::WorkLedger::open(control, "alpha")
                .unwrap()
                .record("request-a")
                .unwrap()
                .unwrap();
            let (_, _, root) = delivered(&scope, &record, &store);
            assert!(queue_serving(&scope, &record, &record.target, &store).is_err());
            assert!(!root.join("serving-recovery.json").exists());
            let identity = queue(&scope, &record, &record.target, &store).unwrap();
            let event = catdesk_wake::store::read(
                &store
                    .root()
                    .join("queue")
                    .join(format!("{}.json", identity["eventId"].as_str().unwrap())),
            )
            .unwrap();
            sent_event(&store, &event);
            let transport = queue_transport(&scope, &record, &record.target, &store).unwrap();
            assert_eq!(
                queue_serving(&scope, &record, &record.target, &store).unwrap_err(),
                "WEB_TASK_CONTEXT_SERVING_PRIOR_REQUIRED"
            );
            assert!(!root.join("serving-recovery.json").exists());
            let event = catdesk_wake::store::read(
                &store
                    .root()
                    .join("queue")
                    .join(format!("{}.json", transport["eventId"].as_str().unwrap())),
            )
            .unwrap();
            sent_event_with_completion(&store, &event, false);
            assert_eq!(
                queue_serving(&scope, &record, &record.target, &store).unwrap_err(),
                "WEB_TASK_CONTEXT_SERVING_PRIOR_REQUIRED"
            );
            assert!(!root.join("serving-recovery.json").exists());
            store.complete_timer(&event.event_id).unwrap();
            queue_serving(&scope, &record, &record.target, &store).unwrap();
            let path = root.join("serving-recovery.json");
            let original = fs::read(&path).unwrap();
            let mut changed: Value = serde_json::from_slice(&original).unwrap();
            changed["purpose"] = json!("ANOTHER_TASK");
            let tampered = serde_json::to_vec(&changed).unwrap();
            fs::write(&path, &tampered).unwrap();
            assert_eq!(
                validate_serving(&scope, &record, &record.target, &store).unwrap_err(),
                "WEB_TASK_CONTEXT_SERVING_CHANGED"
            );
            assert!(queue_serving(&scope, &record, &record.target, &store).is_err());
            assert_eq!(fs::read(&path).unwrap(), tampered);
            fs::write(&path, original).unwrap();
            let wrong =
                Target::new(record.target.generation + 1, record.target.url.clone()).unwrap();
            assert!(queue_serving(&scope, &record, &wrong, &store).is_err());
        });
    }

    #[test]
    fn native_state_with_any_prior_attempt_receipt_drift_or_unknown_cannot_authorize_clarification()
    {
        let selection = crate::operator_codex_cli::NativeQueueSelection {
            task_id: "native".into(),
            binding_sha256: "a".repeat(64),
            request_sha256: "b".repeat(64),
            maximum_seconds: 60,
        };
        let snapshot = json!({"job":{"state":"READY","taskId":"native","requestSha256":selection.request_sha256,"attemptId":null,"receiptId":null},
            "receipt":null,"originTaskId":"native","bindingStillMatches":true,"originalBindingStillMatches":true});
        require_fresh_snapshot(&snapshot, &selection).unwrap();
        for (field, value) in [
            ("state", json!("UNKNOWN")),
            ("state", json!("CLAIMED")),
            ("attemptId", json!("old")),
            ("receiptId", json!("old")),
            ("requestSha256", json!("c".repeat(64))),
            ("taskId", json!("another")),
        ] {
            let mut changed = snapshot.clone();
            changed["job"][field] = value;
            assert!(require_fresh_snapshot(&changed, &selection).is_err());
        }
        let mut changed = snapshot;
        changed["originalBindingStillMatches"] = json!(false);
        assert!(require_fresh_snapshot(&changed, &selection).is_err());
    }
}
