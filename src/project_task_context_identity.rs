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

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use catdesk_wake::store::{Phase, Store, now};

    fn sent_event(store: &Store, event: &catdesk_wake::protocol::Event) {
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
        store.complete_timer(&event.event_id).unwrap();
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
