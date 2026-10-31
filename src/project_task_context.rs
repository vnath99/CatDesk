//! Initial private context for an already-granted task. Uses the existing
//! private handoff store and browser owner, never the shared Wake message queue.
use super::*;
use catdesk_wake::{
    protocol::{Target, digest},
    runtime::{TaskContextOperation, TaskContextSubmission},
    store::{Receipt, atomic},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InitialContextAction {
    Send,
    Reconcile,
    Continue,
    DeliverContinuation,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Reference {
    schema_version: u32,
    project_id: String,
    request_id: String,
    source_event_id: String,
    assignment: crate::web_work_state::WorkTaskAssignment,
    target: Target,
    storage_id: String,
    payload_sha256: String,
    message_sha256: String,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Payload {
    schema_version: u32,
    context: Value,
    message: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct State {
    schema_version: u32,
    payload_sha256: String,
    phase: String,
    receipt: Option<Receipt>,
    created_utc: u64,
    updated_utc: u64,
    attempts: u8,
    submission_fenced_utc: Option<u64>,
}

fn filename(target: &Target) -> String {
    format!(
        "initial-context-{}-{}.json",
        target.generation, target.digest
    )
}
fn require_target(scope: &ResolvedProjectScopeV1, target: &Target) -> Result<(), String> {
    target.validate()?;
    if scope.project.chatgpt_target_url.as_deref() != Some(target.url.as_str())
        || scope.project.chatgpt_target_sha256.as_deref() != Some(target.digest.as_str())
    {
        return Err("WEB_TASK_CONTEXT_TARGET_CHANGED".into());
    }
    let root = crate::wake_protocol_client::wake_root_for_workspace(&scope.control_workspace)?;
    let store = catdesk_wake::store::Store::open_read_only(&root)?;
    if store.config()?.targets.get(&scope.project.project_id) != Some(target) {
        return Err("WEB_TASK_CONTEXT_TARGET_CHANGED".into());
    }
    Ok(())
}
fn reference_path(
    scope: &ResolvedProjectScopeV1,
    record: &crate::web_work_state::WorkRecord,
    target: &Target,
) -> Result<PathBuf, String> {
    let link = record
        .task_assignment
        .as_ref()
        .ok_or("WEB_TASK_GRANT_REQUIRED")?;
    Ok(task_directory(scope, &link.task_id, false)?.join(filename(target)))
}
fn storage_id(
    scope: &ResolvedProjectScopeV1,
    record: &crate::web_work_state::WorkRecord,
    target: &Target,
) -> Result<String, String> {
    Ok(digest(
        &serde_json::to_string(&(
            "INITIAL_TASK_CONTEXT_V1",
            &scope.control_workspace,
            &scope.project.project_id,
            &record.request_id,
            &record.event_id,
            &record.task_assignment,
            target,
        ))
        .map_err(|_| "WEB_TASK_CONTEXT_INVALID")?,
    ))
}
fn private_root(
    scope: &ResolvedProjectScopeV1,
    reference: &Reference,
    create: bool,
) -> Result<PathBuf, String> {
    if reference.storage_id.len() != 64
        || !reference
            .storage_id
            .bytes()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
    {
        return Err("WEB_TASK_CONTEXT_REFERENCE_INVALID".into());
    }
    let base = crate::delegated::autonomy_runtime::fixed_current_user_catdesk_state_root()
        .map_err(|_| "WEB_TASK_CONTEXT_STORE_UNAVAILABLE")?
        .join("WebTaskHandoffs");
    if create && !base.exists() {
        fs::create_dir(&base).map_err(|_| "WEB_TASK_CONTEXT_STORE_UNAVAILABLE")?;
    }
    plain_directory(&base)?;
    let root = base.join(&reference.storage_id);
    if create && !root.exists() {
        fs::create_dir(&root).map_err(|_| "WEB_TASK_CONTEXT_STORE_UNAVAILABLE")?;
    }
    plain_directory(&root)?;
    let canonical = root
        .canonicalize()
        .map_err(|_| "WEB_TASK_CONTEXT_STORE_UNAVAILABLE")?;
    if canonical.starts_with(&scope.control_workspace)
        || scope.control_workspace.starts_with(&canonical)
    {
        return Err("WEB_TASK_CONTEXT_STORE_OVERLAP".into());
    }
    unregistered_task_root(scope, &canonical).map_err(|_| "WEB_TASK_CONTEXT_STORE_OVERLAP")?;
    Ok(root)
}
fn load(
    scope: &ResolvedProjectScopeV1,
    record: &crate::web_work_state::WorkRecord,
    target: &Target,
) -> Result<(Reference, Payload, PathBuf), String> {
    require_target(scope, target)?;
    let reference: Reference = serde_json::from_slice(&read_assignment_bytes(&reference_path(
        scope, record, target,
    )?)?)
    .map_err(|_| "WEB_TASK_CONTEXT_REFERENCE_INVALID")?;
    if reference.schema_version != 1
        || reference.project_id != scope.project.project_id
        || reference.request_id != record.request_id
        || reference.source_event_id != record.event_id
        || Some(&reference.assignment) != record.task_assignment.as_ref()
        || reference.target != *target
        || reference.storage_id != storage_id(scope, record, target)?
    {
        return Err("WEB_TASK_CONTEXT_SCOPE_CHANGED".into());
    }
    let root = private_root(scope, &reference, false)?;
    let bytes = read_assignment_bytes(&root.join("initial-payload.json"))?;
    if digest(std::str::from_utf8(&bytes).map_err(|_| "WEB_TASK_CONTEXT_INVALID")?)
        != reference.payload_sha256
    {
        return Err("WEB_TASK_CONTEXT_PAYLOAD_CHANGED".into());
    }
    let payload: Payload =
        serde_json::from_slice(&bytes).map_err(|_| "WEB_TASK_CONTEXT_INVALID")?;
    if payload.schema_version != 1
        || payload.message.len() > 16 * 1024
        || payload.context["projectId"].as_str() != Some(&scope.project.project_id)
        || payload.context["requestId"].as_str() != Some(&record.request_id)
        || payload.context["sourceEventId"].as_str() != Some(&record.event_id)
        || payload.context["taskId"].as_str() != Some(&reference.assignment.task_id)
        || payload.context["assignmentSha256"].as_str()
            != Some(&reference.assignment.assignment_sha256)
        || payload.context["expectedTargetGeneration"].as_u64() != Some(target.generation)
        || payload.context["expectedTargetSha256"].as_str() != Some(target.digest.as_str())
        || digest(
            &payload
                .message
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" "),
        ) != reference.message_sha256
    {
        return Err("WEB_TASK_CONTEXT_SCOPE_CHANGED".into());
    }
    validate_wire(&reference, &payload.message)?;
    authorize(scope, record, &payload)?;
    Ok((reference, payload, root))
}
fn authorize(
    scope: &ResolvedProjectScopeV1,
    record: &crate::web_work_state::WorkRecord,
    payload: &Payload,
) -> Result<(), String> {
    require_registered_scope(scope)?;
    let current = crate::web_work_state::WorkLedger::open(
        &scope.control_workspace,
        &scope.project.project_id,
    )?
    .record(&record.request_id)?
    .ok_or("WEB_TASK_CONTEXT_SOURCE_CHANGED")?;
    if current.phase != "OPEN"
        || current.event_id != record.event_id
        || current.task_assignment != record.task_assignment
    {
        return Err("WEB_TASK_CONTEXT_SOURCE_CHANGED".into());
    }
    authorized_web_assignment(
        scope,
        &current,
        payload.context["executionKey"]
            .as_str()
            .ok_or("WEB_TASK_GRANT_REQUIRED")?,
    )?;
    Ok(())
}
fn validate_wire(reference: &Reference, message: &str) -> Result<(), String> {
    let event_id = format!("task-context-{}", reference.storage_id);
    let mut request = TaskContextSubmission {
        project_id: &reference.project_id,
        event_id: &event_id,
        target: &reference.target,
        message,
        operation: TaskContextOperation::Submit,
        maximum_seconds: 120,
    };
    request.validate()?;
    // Leave room for the existing read-only reconciliation flag as well.
    request.operation = TaskContextOperation::Reconcile;
    request.validate()
}
fn state(root: &Path, reference: &Reference) -> Result<State, String> {
    let value: State =
        serde_json::from_slice(&read_assignment_bytes(&root.join("initial-delivery.json"))?)
            .map_err(|_| "WEB_TASK_CONTEXT_STATE_INVALID")?;
    if value.schema_version != 1
        || value.created_utc == 0
        || value.updated_utc < value.created_utc
        || value.attempts > 3
        || value
            .submission_fenced_utc
            .is_some_and(|t| t < value.created_utc || t > value.updated_utc)
        || value.payload_sha256 != reference.payload_sha256
        || !matches!(
            value.phase.as_str(),
            "PREPARED"
                | "OBSERVING"
                | "SUBMITTING"
                | "SENT"
                | "ATTENTION_PRE_SUBMIT"
                | "OUTCOME_UNKNOWN"
        )
        || (value.phase == "SENT") != value.receipt.is_some()
    {
        return Err("WEB_TASK_CONTEXT_STATE_INVALID".into());
    }
    if let Some(receipt) = &value.receipt {
        validate_receipt(reference, receipt)?;
    }
    Ok(value)
}
fn validate_receipt(reference: &Reference, receipt: &Receipt) -> Result<(), String> {
    if receipt.schema_version != 1
        || receipt.event_id != format!("task-context-{}", reference.storage_id)
        || receipt.target_generation != reference.target.generation
        || receipt.target_digest != reference.target.digest
        || receipt.message_digest != reference.message_sha256
        || receipt.sent_utc == 0
        || receipt.evidence != "EXACT_USER_MESSAGE_APPENDED"
    {
        return Err("WEB_TASK_CONTEXT_RECEIPT_INVALID".into());
    }
    Ok(())
}
fn transition(
    root: &Path,
    reference: &Reference,
    expected: &[&str],
    phase: &str,
    receipt: Option<Receipt>,
) -> Result<(), String> {
    let mut current = state(root, reference)?;
    if !expected.contains(&current.phase.as_str()) {
        return Err("WEB_TASK_CONTEXT_STATE_CONFLICT".into());
    }
    if let Some(value) = &receipt {
        validate_receipt(reference, value)?;
    }
    let path = root.join("initial-delivery.json");
    if !regular(&path) {
        return Err("WEB_TASK_CONTEXT_STATE_INVALID".into());
    }
    if phase == "OBSERVING" {
        current.attempts = current
            .attempts
            .checked_add(1)
            .filter(|n| *n <= 3)
            .ok_or("WEB_TASK_CONTEXT_RETRY_LIMIT")?;
    }
    if phase == "SUBMITTING" {
        if current.submission_fenced_utc.is_some() {
            return Err("WEB_TASK_CONTEXT_REPLAY_REFUSED".into());
        }
        current.submission_fenced_utc = Some(catdesk_wake::store::now());
    }
    current.phase = phase.into();
    current.receipt = receipt;
    current.updated_utc = catdesk_wake::store::now();
    atomic(&path, &current)
}
fn status(reference: &Reference, state: &State) -> Value {
    json!({"status":"PRIVATE_TASK_CONTEXT","projectId":reference.project_id,"requestId":reference.request_id,
        "taskId":reference.assignment.task_id,"targetGeneration":reference.target.generation,"targetSha256":reference.target.digest,
        "messageSha256":reference.message_sha256,"payloadSha256":reference.payload_sha256,"phase":state.phase,
        "receipt":state.receipt,"attempts":state.attempts,"createdUtc":state.created_utc,"updatedUtc":state.updated_utc,
        "submissionFencedUtc":state.submission_fenced_utc,
        "executionKeyDisclosed":false,"taskCompleted":false,"nativeSessionAction":"NONE"})
}

fn continuation_identity(
    reference: &Reference,
    record: &crate::web_work_state::WorkRecord,
) -> Result<String, String> {
    let turn = format!("initial-context-{}", reference.storage_id);
    catdesk_wake::store::WakeIntent {
        purpose: catdesk_wake::protocol::WakePurpose::NewTask,
        source_event_id: &record.event_id,
        source_turn_id: &turn,
        priority: 0,
        ttl_seconds: 600,
    }
    .event_id(&reference.project_id)
}

fn meter_status(result: Result<(), String>) -> &'static str {
    match result.as_ref().map_err(String::as_str) {
        Ok(()) => "RECORDED",
        Err("WEB_CONTEXT_BUSY" | "WEB_CONTEXT_REGISTRATION_BUSY") => "BUSY",
        Err("WEB_CONTEXT_OLD_GENERATION" | "WEB_CONTEXT_TARGET_CHANGED") => "TARGET_CHANGED",
        Err("WEB_CONTEXT_EVENT_LIMIT") => "AT_LIMIT",
        Err(_) => "UNAVAILABLE",
    }
}

/// Advisory accounting never changes the already-established delivery outcome.
/// It may be retried on SENT through context-reconcile, without browser work.
fn account_sent_context(
    scope: &ResolvedProjectScopeV1,
    record: &crate::web_work_state::WorkRecord,
    target: &Target,
    wake: &catdesk_wake::store::Store,
) -> Result<Value, String> {
    use crate::web_conversation_context::{ConversationKey, WebContextStore};
    let (reference, payload, private) = load(scope, record, target)?;
    let current = state(&private, &reference)?;
    if current.phase != "SENT" {
        return Err("WEB_TASK_CONTEXT_RECEIPT_REQUIRED".into());
    }
    let key = crate::mcp::with_current_project_web_target(
        &scope.control_workspace,
        &scope.project.project_id,
        target.generation,
        &target.digest,
        |control, paired| {
            let key = ConversationKey {
                project_id: scope.project.project_id.clone(),
                conversation_url: paired.url.clone(),
                target_generation: paired.generation,
            };
            let meter = WebContextStore::open_nonblocking(control.join(".catdesk/web-context"))?;
            let previous = meter.active_key(&key.project_id)?;
            meter.bind_observed_target(key.clone(), previous.as_ref())?;
            Ok(key)
        },
    )?;
    // Tokenization retains no global registry or paired-target lock.
    let meter =
        WebContextStore::open_nonblocking(scope.control_workspace.join(".catdesk/web-context"))?;
    let initial = meter_status(
        meter
            .record_delivered_user(
                &key,
                &payload.message,
                current
                    .receipt
                    .as_ref()
                    .ok_or("WEB_TASK_CONTEXT_RECEIPT_REQUIRED")?,
            )
            .map(|_| ()),
    );
    // Pure identity lookup: do not recreate a queue file or renew an old TTL.
    let id = continuation_identity(&reference, record)?;
    let continuation = match wake.delivery(&id)? {
        Some(delivery)
            if delivery.phase == catdesk_wake::store::Phase::Sent
                && delivery.target == *target
                && delivery.event.project_id == scope.project.project_id =>
        {
            meter_status(
                meter
                    .record_delivered_user(
                        &key,
                        &delivery.event.message,
                        delivery
                            .receipt
                            .as_ref()
                            .ok_or("WEB_TASK_CONTEXT_RECEIPT_REQUIRED")?,
                    )
                    .map(|_| ()),
            )
        }
        _ => "NOT_SENT",
    };
    Ok(json!({"initial":initial,"continuation":continuation,
        "rawTextPersisted":false,"browserStarted":false,"providerContextExact":false}))
}

fn with_context_accounting(
    mut output: Value,
    scope: &ResolvedProjectScopeV1,
    record: &crate::web_work_state::WorkRecord,
    target: &Target,
    store: &catdesk_wake::store::Store,
) -> Value {
    output["contextAccounting"] = account_sent_context(scope, record, target, store)
        .unwrap_or_else(|reason| {
            json!({"initial":meter_status(Err(reason)),
            "continuation":"UNAVAILABLE","deliveryOutcomeChanged":false})
        });
    output
}

pub(crate) fn prepare_initial_context(
    scope: &ResolvedProjectScopeV1,
    record: &crate::web_work_state::WorkRecord,
    target: &Target,
) -> Result<Value, String> {
    require_target(scope, target)?;
    let path = reference_path(scope, record, target)?;
    if path.exists() {
        let (reference, _, root) = load(scope, record, target)?;
        return Ok(status(&reference, &state(&root, &reference)?));
    }
    let mut context =
        granted_handoff_context(scope, record, target)?.ok_or("WEB_TASK_GRANT_REQUIRED")?;
    context["expectedTargetGeneration"] = json!(target.generation);
    context["expectedTargetSha256"] = json!(target.digest);
    context["targetSelection"] = json!("MATCH_THIS_VERIFIED_TARGET_AND_THE_SEPARATE_CONTINUATION");
    let message = format!(
        "CatDesk private assigned-task context. Project: {}. Request: {}.\n{}\nKeep executionKey private. Do not echo it, log it, write it into project files or disclose it to another conversation. Acknowledge receipt only; wait for the separate exact CatDesk assigned-task continuation event before making tool calls. Preserve current native work and completed/unknown call IDs. This context grants only the existing task scope; it grants no new permissions.",
        scope.project.project_id,
        record.request_id,
        serde_json::to_string(&context).map_err(|_| "WEB_TASK_CONTEXT_INVALID")?
    );
    if message.len() > 16 * 1024 {
        return Err("WEB_TASK_CONTEXT_TOO_LARGE".into());
    }
    let payload = Payload {
        schema_version: 1,
        context,
        message,
    };
    let bytes = serde_json::to_vec(&payload).map_err(|_| "WEB_TASK_CONTEXT_INVALID")?;
    let reference = Reference {
        schema_version: 1,
        project_id: scope.project.project_id.clone(),
        request_id: record.request_id.clone(),
        source_event_id: record.event_id.clone(),
        assignment: record
            .task_assignment
            .clone()
            .ok_or("WEB_TASK_GRANT_REQUIRED")?,
        target: target.clone(),
        storage_id: storage_id(scope, record, target)?,
        payload_sha256: digest(
            std::str::from_utf8(&bytes).map_err(|_| "WEB_TASK_CONTEXT_INVALID")?,
        ),
        message_sha256: digest(
            &payload
                .message
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" "),
        ),
    };
    validate_wire(&reference, &payload.message)?;
    let root = private_root(scope, &reference, true)?;
    exact_file(&root.join("initial-payload.json"), &bytes)?;
    let value = State {
        schema_version: 1,
        payload_sha256: reference.payload_sha256.clone(),
        phase: "PREPARED".into(),
        receipt: None,
        created_utc: catdesk_wake::store::now(),
        updated_utc: catdesk_wake::store::now(),
        attempts: 0,
        submission_fenced_utc: None,
    };
    exact_file(
        &root.join("initial-delivery.json"),
        &serde_json::to_vec(&value).map_err(|_| "WEB_TASK_CONTEXT_INVALID")?,
    )?;
    exact_file(
        &path,
        &serde_json::to_vec(&reference).map_err(|_| "WEB_TASK_CONTEXT_INVALID")?,
    )?;
    load(scope, record, target)?;
    Ok(status(&reference, &value))
}

/// Prepare the SAME immutable context only after a proven failure before ACK.
pub(crate) fn retry_initial_context(
    scope: &ResolvedProjectScopeV1,
    record: &crate::web_work_state::WorkRecord,
    target: &Target,
) -> Result<Value, String> {
    let (reference, _, root) = load(scope, record, target)?;
    let current = state(&root, &reference)?;
    if current.phase != "ATTENTION_PRE_SUBMIT" || current.submission_fenced_utc.is_some() {
        return Err("WEB_TASK_CONTEXT_RETRY_NOT_PRE_SUBMIT".into());
    }
    if current.attempts >= 3 {
        return Err("WEB_TASK_CONTEXT_RETRY_LIMIT".into());
    }
    transition(
        &root,
        &reference,
        &["ATTENTION_PRE_SUBMIT"],
        "PREPARED",
        None,
    )?;
    Ok(status(&reference, &state(&root, &reference)?))
}

pub(crate) fn run_initial_context(
    scope: &ResolvedProjectScopeV1,
    record: &crate::web_work_state::WorkRecord,
    target: &Target,
    action: InitialContextAction,
    keep_running: &dyn Fn() -> Result<(), String>,
) -> Result<Value, String> {
    let root = crate::wake_protocol_client::wake_root_for_workspace(&scope.control_workspace)?;
    let store = catdesk_wake::store::Store::open(&root)?;
    if matches!(
        action,
        InitialContextAction::Continue | InitialContextAction::DeliverContinuation
    ) {
        keep_running()?;
        let queued = queue_continuation(scope, record, target, &store)?;
        if action == InitialContextAction::Continue {
            return Ok(queued);
        }
        let event_id = queued["eventId"]
            .as_str()
            .ok_or("WEB_TASK_CONTEXT_CONTINUATION_INVALID")?;
        // SENT/unknown deliveries may already be archived. Read their durable
        // event only for identity; the existing sender still refuses replay.
        let delivery = store.delivery(event_id)?;
        let event = match delivery {
            Some(delivery) => delivery.event,
            None => catdesk_wake::store::read::<catdesk_wake::protocol::Event>(
                &store.root().join("queue").join(format!("{event_id}.json")),
            )?,
        };
        let mut before = |actual: &catdesk_wake::protocol::Event, actual_target: &Target| {
            keep_running()?;
            load(scope, record, target)?;
            if actual != &event || actual_target != target {
                return Err("WEB_TASK_CONTEXT_CONTINUATION_CHANGED".into());
            }
            Ok(())
        };
        let result = catdesk_wake::runtime::run_scoped_task_continuation(
            &store,
            &event,
            target,
            120,
            &mut before,
            keep_running,
        );
        return result.map(|output| with_context_accounting(output, scope, record, target, &store));
    }
    run_with_sender(
        scope,
        record,
        target,
        action,
        &store,
        keep_running,
        |store, request, before, keep| {
            catdesk_wake::runtime::run_private_task_context(store, request, before, keep)
        },
    )
}
fn queue_continuation(
    scope: &ResolvedProjectScopeV1,
    record: &crate::web_work_state::WorkRecord,
    target: &Target,
    store: &catdesk_wake::store::Store,
) -> Result<Value, String> {
    let (reference, _, root) = load(scope, record, target)?;
    if state(&root, &reference)?.phase != "SENT" {
        return Err("WEB_TASK_CONTEXT_RECEIPT_REQUIRED".into());
    }
    let selection = json!({"projectId":scope.project.project_id,"requestId":record.request_id,
        "sourceEventId":record.event_id,"assignmentSha256":reference.assignment.assignment_sha256,
        "taskId":reference.assignment.task_id,"expectedTargetGeneration":target.generation,
        "expectedTargetSha256":target.digest});
    let message = format!(
        "CatDesk assigned-task continuation. Use only the private assigned-task context with USER message SHA-256 {} already delivered in this conversation. Verified selection: {}. Keep executionKey private. Continue that existing unfinished task through its granted CatDesk tools. Use the native queue only if the private grant explicitly includes it; preserve all other native sessions. Keep stable call IDs; never replay a completed or unknown call. Acknowledge a definitive admission-busy refusal before trying that same call again; report uncertain outcomes instead of retrying them. Report actual artifacts, verification and blockers; browser completion alone is not task completion.",
        reference.message_sha256, selection
    );
    let turn = format!("initial-context-{}", reference.storage_id);
    let event = store.produce_expected_wake(
        &scope.project.project_id,
        &message,
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
        json!({"status":"TASK_CONTINUATION_QUEUED","projectId":scope.project.project_id,
        "requestId":record.request_id,"taskId":reference.assignment.task_id,"eventId":event.event_id,
        "messageSha256":digest(&event.message),"targetGeneration":target.generation,"targetSha256":target.digest,
        "deliveryPhase":delivery.map(|value| value.phase),"queueOnly":true,"hostStarted":false,
        "executionKeyDisclosed":false,"nativeSessionAction":"NONE","taskCompleted":false}),
    )
}
#[allow(clippy::too_many_arguments)]
fn run_with_sender(
    scope: &ResolvedProjectScopeV1,
    record: &crate::web_work_state::WorkRecord,
    target: &Target,
    action: InitialContextAction,
    store: &catdesk_wake::store::Store,
    keep_running: &dyn Fn() -> Result<(), String>,
    send: impl FnOnce(
        &catdesk_wake::store::Store,
        &TaskContextSubmission<'_>,
        &mut dyn FnMut() -> Result<(), String>,
        &dyn Fn() -> Result<(), String>,
    ) -> Result<Receipt, String>,
) -> Result<Value, String> {
    let (reference, payload, root) = load(scope, record, target)?;
    let current = state(&root, &reference)?;
    if current.phase == "SENT" {
        return Ok(with_context_accounting(
            status(&reference, &current),
            scope,
            record,
            target,
            store,
        ));
    }
    match action {
        InitialContextAction::Send if current.phase != "PREPARED" => {
            return Err("WEB_TASK_CONTEXT_REPLAY_REFUSED".into());
        }
        InitialContextAction::Reconcile
            if !matches!(
                current.phase.as_str(),
                "OBSERVING" | "SUBMITTING" | "OUTCOME_UNKNOWN"
            ) =>
        {
            return Err("WEB_TASK_CONTEXT_RECONCILIATION_NOT_REQUIRED".into());
        }
        _ => {}
    }
    let guard = || {
        keep_running()?;
        authorize(scope, record, &payload)
    };
    guard()?;
    if action == InitialContextAction::Send {
        transition(&root, &reference, &["PREPARED"], "OBSERVING", None)?;
    }
    let event_id = format!("task-context-{}", reference.storage_id);
    let request = TaskContextSubmission {
        project_id: &scope.project.project_id,
        event_id: &event_id,
        target,
        message: &payload.message,
        operation: if action == InitialContextAction::Send {
            TaskContextOperation::Submit
        } else {
            TaskContextOperation::Reconcile
        },
        maximum_seconds: 120,
    };
    let outcome = send(
        store,
        &request,
        &mut || {
            guard()?;
            if action != InitialContextAction::Send {
                return Err("WEB_TASK_CONTEXT_REPLAY_REFUSED".into());
            }
            transition(&root, &reference, &["OBSERVING"], "SUBMITTING", None)
        },
        &guard,
    )
    .and_then(|receipt| {
        validate_receipt(&reference, &receipt)?;
        if action == InitialContextAction::Send && state(&root, &reference)?.phase != "SUBMITTING" {
            return Err("WEB_TASK_CONTEXT_SUBMISSION_FENCE_REQUIRED".into());
        }
        Ok(receipt)
    });
    match outcome {
        Ok(receipt) => {
            validate_receipt(&reference, &receipt)?;
            transition(
                &root,
                &reference,
                &["OBSERVING", "SUBMITTING", "OUTCOME_UNKNOWN"],
                "SENT",
                Some(receipt),
            )?;
            Ok(with_context_accounting(
                status(&reference, &state(&root, &reference)?),
                scope,
                record,
                target,
                store,
            ))
        }
        Err(reason) => {
            let current = state(&root, &reference)?;
            let next = if action == InitialContextAction::Reconcile || current.phase == "SUBMITTING"
            {
                "OUTCOME_UNKNOWN"
            } else {
                "ATTENTION_PRE_SUBMIT"
            };
            transition(&root, &reference, &[&current.phase], next, None)?;
            Err(reason)
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use catdesk_wake::store::{Phase, Store, now};

    fn fixture(
        granted: bool,
    ) -> (
        ResolvedProjectScopeV1,
        crate::web_work_state::WorkRecord,
        Store,
        Value,
    ) {
        let (control, assignment, store, args, event) =
            crate::web_work::tests::assignment_fixture_for("alpha");
        store.publish(&event).unwrap();
        crate::web_work::associate(&control, args.clone(), &assignment).unwrap();
        // Explicit synthetic source receipt in this disposable scope only.
        let host = store.host_lock().unwrap();
        let delivery = store.claim(&event, &host).unwrap();
        store
            .transition(
                &event.event_id,
                &delivery.owner,
                Phase::Submitting,
                None,
                None,
            )
            .unwrap();
        let target = store.config().unwrap().targets["alpha"].clone();
        let receipt = Receipt {
            schema_version: 1,
            event_id: event.event_id.clone(),
            target_generation: target.generation,
            target_digest: target.digest,
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
                &delivery.owner,
                Phase::Sent,
                Some(receipt),
                None,
            )
            .unwrap();
        store.complete_timer(&event.event_id).unwrap();
        drop(host);
        if granted {
            crate::web_work::execution_permission(&control, args.clone(), false).unwrap();
        }
        let scope =
            crate::delegated::autonomy_projects::resolve_registered_project_from_control_root(
                &control, "alpha",
            )
            .unwrap();
        let record = crate::web_work_state::WorkLedger::open(&control, "alpha")
            .unwrap()
            .record("request-a")
            .unwrap()
            .unwrap();
        (scope, record, store, args)
    }
    fn proof(request: &TaskContextSubmission<'_>) -> Receipt {
        Receipt {
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
        }
    }
    fn sent(
        scope: &ResolvedProjectScopeV1,
        record: &crate::web_work_state::WorkRecord,
        store: &Store,
    ) {
        prepare_initial_context(scope, record, &record.target).unwrap();
        run_with_sender(
            scope,
            record,
            &record.target,
            InitialContextAction::Send,
            store,
            &|| Ok(()),
            |_, request, checkpoint, _| {
                checkpoint()?;
                Ok(proof(request))
            },
        )
        .unwrap();
    }
    #[test]
    fn initial_context_freezes_private_payload_and_only_safe_reference_in_workspace() {
        let (scope, record, store, args) = fixture(true);
        let queue_before = store.events().unwrap();
        let output = crate::web_work::context_action(
            &scope.control_workspace,
            args.clone(),
            "context-prepare",
        )
        .unwrap();
        assert_eq!(output["phase"], "PREPARED");
        assert_eq!(output["executionKeyDisclosed"], false);
        let (reference, payload, root) = load(&scope, &record, &record.target).unwrap();
        let key = payload.context["executionKey"].as_str().unwrap();
        assert!(payload.message.contains(key));
        assert!(!root.starts_with(&scope.control_workspace));
        assert!(!root.starts_with(&scope.project.workspace));
        let metadata =
            fs::read_to_string(reference_path(&scope, &record, &record.target).unwrap()).unwrap();
        assert!(!metadata.contains(key));
        assert!(!output.to_string().contains(key));
        let before = fs::read(root.join("initial-payload.json")).unwrap();
        assert_eq!(
            crate::web_work::context_action(&scope.control_workspace, args, "context-prepare")
                .unwrap(),
            output
        );
        assert_eq!(fs::read(root.join("initial-payload.json")).unwrap(), before);
        assert_eq!(
            reference.message_sha256,
            digest(
                &payload
                    .message
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
            )
        );
        assert_eq!(store.events().unwrap(), queue_before);
    }
    #[test]
    fn initial_context_requires_existing_grant_and_exact_target_before_preparation() {
        let (scope, record, _, _) = fixture(false);
        assert_eq!(
            prepare_initial_context(&scope, &record, &record.target).unwrap_err(),
            "WEB_TASK_GRANT_REQUIRED"
        );
        assert!(
            !reference_path(&scope, &record, &record.target)
                .unwrap()
                .exists()
        );
        let foreign = Target::new(record.target.generation + 1, record.target.url.clone()).unwrap();
        assert_eq!(
            prepare_initial_context(&scope, &record, &foreign).unwrap_err(),
            "WEB_TASK_CONTEXT_TARGET_CHANGED"
        );
    }
    #[test]
    fn initial_context_sent_is_idempotent_without_another_browser_or_ack() {
        let (scope, record, store, _) = fixture(true);
        sent(&scope, &record, &store);
        let output = run_with_sender(
            &scope,
            &record,
            &record.target,
            InitialContextAction::Send,
            &store,
            &|| Ok(()),
            |_, _, _, _| panic!("completed delivery cannot start a browser"),
        )
        .unwrap();
        assert_eq!(output["phase"], "SENT");
        assert!(output["receipt"].is_object());
        assert_eq!(output["taskCompleted"], false);
    }
    #[test]
    fn initial_context_unknown_cannot_resend_but_readonly_reconciliation_can_prove_it() {
        let (scope, record, store, _) = fixture(true);
        prepare_initial_context(&scope, &record, &record.target).unwrap();
        assert_eq!(
            run_with_sender(
                &scope,
                &record,
                &record.target,
                InitialContextAction::Send,
                &store,
                &|| Ok(()),
                |_, _, checkpoint, _| {
                    checkpoint()?;
                    Err("TASK_CONTEXT_OUTCOME_UNKNOWN".into())
                }
            )
            .unwrap_err(),
            "TASK_CONTEXT_OUTCOME_UNKNOWN"
        );
        assert_eq!(
            run_with_sender(
                &scope,
                &record,
                &record.target,
                InitialContextAction::Send,
                &store,
                &|| Ok(()),
                |_, _, _, _| panic!("unknown submission cannot restart")
            )
            .unwrap_err(),
            "WEB_TASK_CONTEXT_REPLAY_REFUSED"
        );
        let output = run_with_sender(
            &scope,
            &record,
            &record.target,
            InitialContextAction::Reconcile,
            &store,
            &|| Ok(()),
            |_, request, _, _| {
                assert_eq!(request.operation, TaskContextOperation::Reconcile);
                Ok(proof(request))
            },
        )
        .unwrap();
        assert_eq!(output["phase"], "SENT");
    }
    #[test]
    fn initial_context_crash_before_ack_and_known_prewrite_failure_keep_the_fence() {
        for interrupted in [true, false] {
            let (scope, record, store, _) = fixture(true);
            prepare_initial_context(&scope, &record, &record.target).unwrap();
            let (reference, _, root) = load(&scope, &record, &record.target).unwrap();
            if interrupted {
                transition(&root, &reference, &["PREPARED"], "OBSERVING", None).unwrap();
            } else {
                assert!(
                    run_with_sender(
                        &scope,
                        &record,
                        &record.target,
                        InitialContextAction::Send,
                        &store,
                        &|| Ok(()),
                        |_, _, _, _| Err("TASK_CONTEXT_BROWSER_ATTENTION".into())
                    )
                    .is_err()
                );
                assert_eq!(
                    state(&root, &reference).unwrap().phase,
                    "ATTENTION_PRE_SUBMIT"
                );
            }
            assert_eq!(
                run_with_sender(
                    &scope,
                    &record,
                    &record.target,
                    InitialContextAction::Send,
                    &store,
                    &|| Ok(()),
                    |_, _, _, _| panic!("no automatic retry")
                )
                .unwrap_err(),
                "WEB_TASK_CONTEXT_REPLAY_REFUSED"
            );
        }
    }
    #[test]
    fn initial_context_revoke_and_payload_change_refuse_without_browser_or_replacement() {
        let (scope, record, store, _) = fixture(true);
        prepare_initial_context(&scope, &record, &record.target).unwrap();
        let (reference, _, root) = load(&scope, &record, &record.target).unwrap();
        fs::write(root.join("initial-payload.json"), b"changed").unwrap();
        assert_eq!(
            run_with_sender(
                &scope,
                &record,
                &record.target,
                InitialContextAction::Send,
                &store,
                &|| Ok(()),
                |_, _, _, _| panic!()
            )
            .unwrap_err(),
            "WEB_TASK_CONTEXT_PAYLOAD_CHANGED"
        );
        assert_eq!(
            fs::read(root.join("initial-payload.json")).unwrap(),
            b"changed"
        );
        assert_eq!(state(&root, &reference).unwrap().phase, "PREPARED");
        let (scope, record, store, _) = fixture(true);
        prepare_initial_context(&scope, &record, &record.target).unwrap();
        grant_web_execution(&scope, &record, &record.target, true).unwrap();
        assert_eq!(
            run_with_sender(
                &scope,
                &record,
                &record.target,
                InitialContextAction::Send,
                &store,
                &|| Ok(()),
                |_, _, _, _| panic!()
            )
            .unwrap_err(),
            "WEB_TASK_GRANT_REVOKED"
        );
    }
    #[test]
    fn initial_context_receipt_cannot_be_recorded_before_the_durable_submit_fence() {
        let (scope, record, store, _) = fixture(true);
        prepare_initial_context(&scope, &record, &record.target).unwrap();
        assert_eq!(
            run_with_sender(
                &scope,
                &record,
                &record.target,
                InitialContextAction::Send,
                &store,
                &|| Ok(()),
                |_, request, _, _| Ok(proof(request))
            )
            .unwrap_err(),
            "WEB_TASK_CONTEXT_SUBMISSION_FENCE_REQUIRED"
        );
        let (reference, _, root) = load(&scope, &record, &record.target).unwrap();
        assert_eq!(
            state(&root, &reference).unwrap().phase,
            "ATTENTION_PRE_SUBMIT"
        );
    }
    #[test]
    fn initial_context_continuation_is_key_free_deduplicated_and_requires_the_receipt() {
        let (scope, record, store, _) = fixture(true);
        prepare_initial_context(&scope, &record, &record.target).unwrap();
        assert_eq!(
            queue_continuation(&scope, &record, &record.target, &store).unwrap_err(),
            "WEB_TASK_CONTEXT_RECEIPT_REQUIRED"
        );
        assert_eq!(
            run_initial_context(
                &scope,
                &record,
                &record.target,
                InitialContextAction::DeliverContinuation,
                &|| Ok(())
            )
            .unwrap_err(),
            "WEB_TASK_CONTEXT_RECEIPT_REQUIRED"
        );
        sent(&scope, &record, &store);
        let first = queue_continuation(&scope, &record, &record.target, &store).unwrap();
        let events = store.events().unwrap();
        let second = queue_continuation(&scope, &record, &record.target, &store).unwrap();
        assert_eq!(first["eventId"], second["eventId"]);
        assert_eq!(store.events().unwrap(), events);
        let (_, payload, _) = load(&scope, &record, &record.target).unwrap();
        let key = payload.context["executionKey"].as_str().unwrap();
        assert!(!serde_json::to_string(&events).unwrap().contains(key));
        let event = events
            .iter()
            .find(|e| e.event_id == first["eventId"].as_str().unwrap())
            .unwrap();
        assert_eq!(
            event.wake_metadata().unwrap().unwrap().purpose,
            catdesk_wake::protocol::WakePurpose::NewTask
        );
        assert_eq!(first["queueOnly"], true);
        assert_eq!(first["hostStarted"], false);
    }

    #[test]
    fn initial_context_explicit_prewrite_retry_keeps_payload_and_has_a_fixed_limit() {
        let (scope, record, store, _) = fixture(true);
        prepare_initial_context(&scope, &record, &record.target).unwrap();
        let (reference, _, root) = load(&scope, &record, &record.target).unwrap();
        let frozen = fs::read(root.join("initial-payload.json")).unwrap();
        for attempt in 1..=3 {
            assert!(
                run_with_sender(
                    &scope,
                    &record,
                    &record.target,
                    InitialContextAction::Send,
                    &store,
                    &|| Ok(()),
                    |_, _, _, _| Err("TASK_CONTEXT_BROWSER_ATTENTION".into())
                )
                .is_err()
            );
            assert_eq!(state(&root, &reference).unwrap().attempts, attempt);
            if attempt < 3 {
                assert_eq!(
                    retry_initial_context(&scope, &record, &record.target).unwrap()["phase"],
                    "PREPARED"
                );
            }
        }
        assert_eq!(
            retry_initial_context(&scope, &record, &record.target).unwrap_err(),
            "WEB_TASK_CONTEXT_RETRY_LIMIT"
        );
        assert_eq!(fs::read(root.join("initial-payload.json")).unwrap(), frozen);
        assert!(
            state(&root, &reference)
                .unwrap()
                .submission_fenced_utc
                .is_none()
        );
    }

    #[test]
    fn initial_context_accounting_requires_sent_and_recovers_continuation_without_queue_recreation()
    {
        use crate::web_conversation_context::{ConversationKey, WebContextStore};
        let (scope, record, store, _) = fixture(true);
        prepare_initial_context(&scope, &record, &record.target).unwrap();
        assert_eq!(
            account_sent_context(&scope, &record, &record.target, &store).unwrap_err(),
            "WEB_TASK_CONTEXT_RECEIPT_REQUIRED"
        );
        sent(&scope, &record, &store);
        let key = ConversationKey {
            project_id: scope.project.project_id.clone(),
            conversation_url: record.target.url.clone(),
            target_generation: record.target.generation,
        };
        let meter =
            WebContextStore::open(scope.control_workspace.join(".catdesk/web-context")).unwrap();
        assert_eq!(
            meter.pressure(&key).unwrap().known_delivered_user_messages,
            1
        );
        let queued = queue_continuation(&scope, &record, &record.target, &store).unwrap();
        let id = queued["eventId"].as_str().unwrap();
        let path = store.root().join("queue").join(format!("{id}.json"));
        let bytes = fs::read(&path).unwrap();
        let output = run_with_sender(
            &scope,
            &record,
            &record.target,
            InitialContextAction::Reconcile,
            &store,
            &|| Ok(()),
            |_, _, _, _| panic!("SENT accounting cannot open browser"),
        )
        .unwrap();
        assert_eq!(output["contextAccounting"]["initial"], "RECORDED");
        assert_eq!(output["contextAccounting"]["continuation"], "NOT_SENT");
        assert_eq!(fs::read(&path).unwrap(), bytes);
        let event: catdesk_wake::protocol::Event = serde_json::from_slice(&bytes).unwrap();
        let host = store.host_lock().unwrap();
        let delivery = store.claim(&event, &host).unwrap();
        store
            .transition(id, &delivery.owner, Phase::Submitting, None, None)
            .unwrap();
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
            .transition(id, &delivery.owner, Phase::Sent, Some(receipt), None)
            .unwrap();
        drop(host);
        let (_, _, private) = load(&scope, &record, &record.target).unwrap();
        let before_state = fs::read(private.join("initial-delivery.json")).unwrap();
        for _ in 0..2 {
            let output = run_with_sender(
                &scope,
                &record,
                &record.target,
                InitialContextAction::Reconcile,
                &store,
                &|| Ok(()),
                |_, _, _, _| panic!("cannot resend completed context"),
            )
            .unwrap();
            assert_eq!(output["contextAccounting"]["continuation"], "RECORDED");
            assert_eq!(
                meter.pressure(&key).unwrap().known_delivered_user_messages,
                2
            );
            assert!(!path.exists());
            assert_eq!(
                fs::read(private.join("initial-delivery.json")).unwrap(),
                before_state
            );
            assert_eq!(
                fs::read(store.root().join("archive").join(format!("{id}.json"))).unwrap(),
                bytes
            );
        }
    }

    #[test]
    fn initial_context_busy_meter_never_downgrades_sent_or_reopens_the_browser() {
        let (scope, record, store, _) = fixture(true);
        sent(&scope, &record, &store);
        let (_, _, private) = load(&scope, &record, &record.target).unwrap();
        let before = fs::read(private.join("initial-delivery.json")).unwrap();
        let held = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(
                scope
                    .control_workspace
                    .join(".catdesk/web-context/ledger.lock"),
            )
            .unwrap();
        held.lock().unwrap();
        let output = run_with_sender(
            &scope,
            &record,
            &record.target,
            InitialContextAction::Reconcile,
            &store,
            &|| Ok(()),
            |_, _, _, _| panic!("accounting busy cannot resend"),
        )
        .unwrap();
        assert_eq!(output["phase"], "SENT");
        assert_eq!(output["contextAccounting"]["initial"], "BUSY");
        assert_eq!(
            fs::read(private.join("initial-delivery.json")).unwrap(),
            before
        );
        drop(held);
        let output = run_with_sender(
            &scope,
            &record,
            &record.target,
            InitialContextAction::Reconcile,
            &store,
            &|| Ok(()),
            |_, _, _, _| panic!("accounting recovery cannot resend"),
        )
        .unwrap();
        assert_eq!(output["contextAccounting"]["initial"], "RECORDED");
        assert_eq!(
            fs::read(private.join("initial-delivery.json")).unwrap(),
            before
        );
    }

    #[test]
    fn initial_context_attempted_or_unknown_submission_never_gets_prewrite_retry() {
        let (scope, record, store, _) = fixture(true);
        prepare_initial_context(&scope, &record, &record.target).unwrap();
        assert!(
            run_with_sender(
                &scope,
                &record,
                &record.target,
                InitialContextAction::Send,
                &store,
                &|| Ok(()),
                |_, _, checkpoint, _| {
                    checkpoint()?;
                    Err("TASK_CONTEXT_OUTCOME_UNKNOWN".into())
                }
            )
            .is_err()
        );
        assert_eq!(
            retry_initial_context(&scope, &record, &record.target).unwrap_err(),
            "WEB_TASK_CONTEXT_RETRY_NOT_PRE_SUBMIT"
        );
        let (reference, _, root) = load(&scope, &record, &record.target).unwrap();
        let current = state(&root, &reference).unwrap();
        assert_eq!(current.phase, "OUTCOME_UNKNOWN");
        assert!(current.submission_fenced_utc.is_some());
    }

    #[test]
    fn initial_context_same_url_generation_change_refuses_before_any_browser_action() {
        let (scope, record, store, _) = fixture(true);
        prepare_initial_context(&scope, &record, &record.target).unwrap();
        let (_, payload, root) = load(&scope, &record, &record.target).unwrap();
        assert_eq!(
            payload.context["expectedTargetGeneration"],
            record.target.generation
        );
        assert_eq!(
            payload.context["expectedTargetSha256"],
            record.target.digest
        );
        let frozen = fs::read(root.join("initial-payload.json")).unwrap();
        // Same-URL set_target is deliberately idempotent. Exercise a real ABA
        // binding change through two supported CAS operations in this fixture.
        let other = store
            .set_target(
                "alpha",
                record.target.generation,
                "https://chatgpt.com/c/context-other",
            )
            .unwrap();
        let returned = store
            .set_target("alpha", other.generation, &record.target.url)
            .unwrap();
        assert!(returned.generation > record.target.generation);
        assert_eq!(
            run_with_sender(
                &scope,
                &record,
                &record.target,
                InitialContextAction::Send,
                &store,
                &|| Ok(()),
                |_, _, _, _| panic!("stale generation cannot reach browser")
            )
            .unwrap_err(),
            "WEB_TASK_CONTEXT_TARGET_CHANGED"
        );
        assert_eq!(fs::read(root.join("initial-payload.json")).unwrap(), frozen);
    }
}
