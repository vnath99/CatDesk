//! One existing ordinary work invitation through the stopped-host sender.
//! The request journal selects the event; no caller message or queue scan.
use super::*;
use catdesk_wake::{
    protocol::{Event, Target},
    runtime,
    store::{Store, read},
};

fn validate_selection(
    project: &str,
    record: &crate::web_work_state::WorkRecord,
    target: &Target,
    event: &Event,
) -> Result<(), String> {
    event.validate()?;
    let expected_id = format!(
        "web-work-{}",
        protocol::digest(&format!("{}\0{}", project, record.request_id))
    );
    if record.phase != "OPEN"
        || record.target != *target
        || record.event_id != expected_id
        || event.event_id != expected_id
        || event.project_id != project
        || event.event_type != "engineering_attention"
        || event.target_generation != target.generation
        || !event.audit_references.is_empty()
        || protocol::digest(&event.message) != record.message_sha256
    {
        return Err("WEB_WORK_DELIVERY_SELECTION_CHANGED".into());
    }
    Ok(())
}

enum Action<'a> {
    Deliver,
    Inspect,
    Retry(&'a str),
    Reconcile,
    Response,
}

fn receipt_unproven_frame(
    project: &str,
    record: &crate::web_work_state::WorkRecord,
    target: &Target,
    prior: &catdesk_wake::store::Delivery,
    source_delivery_sha256: &str,
    observation: &Value,
) -> Result<Value, String> {
    const INVALID: &str = "WEB_WORK_RECEIPT_DIAGNOSIS_INVALID";
    validate_selection(project, record, target, &prior.event)?;
    if prior.phase != catdesk_wake::store::Phase::Submitting
        || prior.receipt.is_some()
        || prior.target != *target
        || prior.message_digest != record.message_sha256
        || source_delivery_sha256.len() != 64
        || !source_delivery_sha256
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        || observation["action"] != "web_work_inspect"
        || observation["projectId"] != project
        || observation["eventId"] != record.event_id
        || observation["targetGeneration"] != target.generation
        || observation["targetDigest"] != target.digest
        || observation["sourceDeliverySha256"] != source_delivery_sha256
        || observation.get("persistedReceipt") != Some(&Value::Null)
        || observation["receiptProofRequested"] != true
        || observation["browserReadOnly"] != true
        || observation["sendAttempted"] != false
        || observation["receiptCreated"] != false
        || observation["stateChanged"] != false
        || observation["retryAuthorized"] != false
        || observation["workResolved"] != false
        || observation["hostStarted"] != false
        || observation["schedulerStarted"] != false
        || observation["reviewAuthorityGranted"] != false
    {
        return Err(INVALID.into());
    }
    let diagnosis = runtime::receipt_failure_diagnosis(observation)?;
    // Task identity comes from the authoritative ledger, never the browser row.
    let task = record.task_assignment.as_ref().map(|assignment| {
        json!({
            "taskId":assignment.task_id, "assignmentSha256":assignment.assignment_sha256,
            "scopeSha256":assignment.scope_sha256, "executionVerified":false,
        })
    });
    Ok(json!({"action":"web_work_reconcile", "status":"attention",
        "errorCode":"WEB_WORK_RECEIPT_UNPROVEN", "projectId":project, "requestId":record.request_id,
        "eventId":record.event_id, "targetGeneration":target.generation, "targetDigest":target.digest,
        "messageSha256":record.message_sha256, "sourceDeliverySha256":source_delivery_sha256,
        "taskAttribution":task, "deliveryPhase":"SUBMITTING", "receiptCommitState":"UNPROVEN",
        "submissionOutcome":"UNKNOWN", "browserReadOnly":true, "sendAttempted":false,
        "receiptCreated":false, "stateChanged":false, "retryAuthorized":false,
        "nativeGrantCreated":false, "hostStarted":false, "workResolved":false,
        "diagnosis":diagnosis}))
}

pub(crate) fn deliver(workspace: &Path, project: &str, request_id: &str) -> Result<Value, String> {
    selected_work(workspace, project, request_id, Action::Deliver)
}
pub(crate) fn inspect(workspace: &Path, project: &str, request_id: &str) -> Result<Value, String> {
    selected_work(workspace, project, request_id, Action::Inspect)
}
pub(crate) fn reconcile(
    workspace: &Path,
    project: &str,
    request_id: &str,
) -> Result<Value, String> {
    selected_work(workspace, project, request_id, Action::Reconcile)
}
pub(crate) fn response(workspace: &Path, project: &str, request_id: &str) -> Result<Value, String> {
    selected_work(workspace, project, request_id, Action::Response)
}
pub(crate) fn retry_pre_submit(
    workspace: &Path,
    project: &str,
    request_id: &str,
    hash: &str,
) -> Result<Value, String> {
    selected_work(workspace, project, request_id, Action::Retry(hash))
}

fn selected_work(
    workspace: &Path,
    project: &str,
    request_id: &str,
    action: Action<'_>,
) -> Result<Value, String> {
    let inspect_only = matches!(action, Action::Inspect);
    let reconcile_only = matches!(action, Action::Reconcile);
    let response_only = matches!(action, Action::Response);
    let retry_sha256 = if let Action::Retry(hash) = action {
        Some(hash)
    } else {
        None
    };
    if (inspect_only || retry_sha256.is_some() || reconcile_only || response_only)
        && wake_protocol_client::wake_runtime_mode_for_workspace(workspace)? != "INSTALLED_STORE"
    {
        return Err("WEB_BROWSER_INSTALLED_SCOPE_REQUIRED".into());
    }
    let prepared = PreparedProjectScopeV1::resolve(workspace, project)
        .map_err(|_| "WEB_WORK_PROJECT_UNAVAILABLE")?;
    let lease = crate::operator_facade::acquire_web_work_lease(workspace, project)?;
    let ledger = WorkLedger::open(workspace, project)?;
    ledger.require_ordinary_request(request_id)?;
    let record = ledger
        .record(request_id)?
        .ok_or("WEB_WORK_REQUEST_MISSING")?;
    let paired = mcp::operator_read_project_web_target(workspace, project)
        .map_err(|_| "WEB_WORK_PAIRED_TARGET_REQUIRED")?
        .ok_or("WEB_WORK_PAIRED_TARGET_REQUIRED")?;
    let target = Target::new(paired.generation, paired.url)?;
    lease.check_target(&target)?;
    let wake = Store::open(&wake_protocol_client::wake_root_for_workspace(workspace)?)?;
    // SENT, SUBMITTING and other historical outcomes are refused by the
    // existing sender before opening an adapter; they never grant replay.
    let prior = wake.delivery(&record.event_id)?;
    if (inspect_only || retry_sha256.is_some() || reconcile_only || response_only)
        && prior.is_none()
    {
        return Err("WEB_WORK_DELIVERY_MISSING".into());
    }
    let event = match &prior {
        Some(delivery) => delivery.event.clone(),
        None => read(
            &wake
                .root()
                .join("queue")
                .join(format!("{}.json", record.event_id)),
        )?,
    };
    validate_selection(project, &record, &target, &event)?;
    catdesk_wake::development_runtime::validate_selected_target_isolation(
        workspace,
        wake.root(),
        &target.url,
    )?;
    let guard = || {
        lease.check_target(&target)?;
        // Keep this metadata-only admission atomic with the existing paired
        // target/registration lock order. Never retain shared locks while the
        // adapter observes, types, submits, or drains its receipt.
        mcp::with_current_project_web_target(
            workspace,
            project,
            target.generation,
            &target.digest,
            |_, current| {
                prepared
                    .revalidate_registration()
                    .map_err(|_| "WEB_WORK_PROJECT_REGISTRATION_CHANGED")?;
                if current.url != target.url {
                    return Err("WEB_WORK_TARGET_CHANGED".into());
                }
                ledger.require_ordinary_request(request_id)?;
                if ledger.record(request_id)?.as_ref() != Some(&record) {
                    return Err("WEB_WORK_DELIVERY_SELECTION_CHANGED".into());
                }
                Ok(())
            },
        )
    };
    guard()?;
    if response_only {
        let registry =
            AutonomousProjectRegistryStoreV1::open_read_only(workspace.join(".catdesk/projects"))
                .map_err(|_| "WEB_WORK_PROJECT_UNAVAILABLE")?;
        let mut result = runtime::reobserve_sent_response_guarded(
            &wake,
            project,
            &target,
            &record.event_id,
            &guard,
            |commit| {
                registry.with_registration_lock(|| {
                    prepared
                        .revalidate_registration()
                        .map_err(|_| "WEB_WORK_PROJECT_REGISTRATION_CHANGED")?;
                    ledger.require_ordinary_request(request_id)?;
                    if ledger.record(request_id)?.as_ref() != Some(&record) {
                        return Err("WEB_WORK_DELIVERY_SELECTION_CHANGED".into());
                    }
                    lease.check_target(&target)?;
                    commit()
                })
            },
        )?;
        result["action"] = json!("web_work_response");
        result["projectId"] = json!(project);
        result["requestId"] = json!(request_id);
        return Ok(result);
    }
    if reconcile_only {
        let prior = prior.as_ref().ok_or("WEB_WORK_DELIVERY_MISSING")?;
        let observation =
            runtime::read_registered_delivery_receipt(&wake, project, &target, prior, &guard)?;
        let receipt = observation.get("persistedReceipt").filter(|v| !v.is_null());
        let Some(receipt) = receipt else {
            // Preserve the existing refusal and unknown-submission boundary,
            // while retaining this owner's bounded content-free observations.
            // This branch has no receipt/timer CAS or other state mutation.
            guard()?;
            if wake.delivery(&record.event_id)?.as_ref() != Some(prior) {
                return Err("WEB_WORK_DELIVERY_SELECTION_CHANGED".into());
            }
            let bytes = std::fs::read_to_string(
                wake.root()
                    .join("deliveries")
                    .join(format!("{}.json", record.event_id)),
            )
            .map_err(|_| "WEB_WORK_RECEIPT_DIAGNOSIS_INVALID")?;
            return receipt_unproven_frame(
                project,
                &record,
                &target,
                prior,
                &protocol::digest(&bytes),
                &observation,
            );
        };
        let receipt: catdesk_wake::store::Receipt =
            serde_json::from_value(receipt.clone()).map_err(|_| "WEB_WORK_RECEIPT_UNPROVEN")?;
        // The browser has exited. Only this short local commit holds shared
        // metadata locks; the original request and target remain authoritative.
        let _host = wake.host_lock()?;
        let _browser = runtime::acquire_browser_quiescence(&wake)?;
        guard()?;
        let registry =
            AutonomousProjectRegistryStoreV1::open_read_only(workspace.join(".catdesk/projects"))
                .map_err(|_| "WEB_WORK_PROJECT_UNAVAILABLE")?;
        return registry.with_registration_lock(|| {
            prepared
                .revalidate_registration()
                .map_err(|_| "WEB_WORK_PROJECT_REGISTRATION_CHANGED")?;
            ledger.require_ordinary_request(request_id)?;
            if ledger.record(request_id)?.as_ref() != Some(&record) {
                return Err("WEB_WORK_DELIVERY_SELECTION_CHANGED".into());
            }
            lease.check_target(&target)?;
            validate_selection(project, &record, &target, &prior.event)?;
            let result = wake.reconcile_ordinary_submitting_sent_expected(
                &record.event_id,
                receipt.clone(),
                prior,
            )?;
            wake.record_exact_receipt_timer(&record.event_id, &receipt)?;
            Ok(
                json!({"action":"web_work_reconcile","projectId":project,"requestId":request_id,
                "eventId":record.event_id,"phase":result.phase,"receipt":result.receipt,
                "browserReadOnly":true,"sendAttempted":false,"assistantRetryAttempted":false,
                "hostStarted":false,"nativeGrantCreated":false,"workResolved":false}),
            )
        });
    }
    if let Some(hash) = retry_sha256 {
        let _host = wake.host_lock()?;
        let _browser = runtime::acquire_browser_quiescence(&wake)?;
        guard()?;
        let registry =
            AutonomousProjectRegistryStoreV1::open_read_only(workspace.join(".catdesk/projects"))
                .map_err(|_| "WEB_WORK_PROJECT_UNAVAILABLE")?;
        return registry.with_registration_lock(|| {
            prepared
                .revalidate_registration()
                .map_err(|_| "WEB_WORK_PROJECT_REGISTRATION_CHANGED")?;
            ledger.require_ordinary_request(request_id)?;
            if ledger.record(request_id)?.as_ref() != Some(&record) {
                return Err("WEB_WORK_DELIVERY_SELECTION_CHANGED".into());
            }
            lease.check_target(&target)?;
            retry_selected(
                &wake,
                project,
                &record,
                &target,
                prior.as_ref().ok_or("WEB_WORK_DELIVERY_MISSING")?,
                hash,
            )
        });
    }
    if inspect_only {
        let mut result = runtime::inspect_registered_delivery_readiness(
            &wake,
            project,
            &target,
            prior.as_ref().ok_or("WEB_WORK_DELIVERY_MISSING")?,
            &guard,
        )?;
        result["requestId"] = json!(request_id);
        return Ok(result);
    }
    let mut before_submit = |actual: &Event, actual_target: &Target| {
        guard()?;
        if actual != &event || actual_target != &target {
            return Err("WEB_WORK_DELIVERY_SELECTION_CHANGED".into());
        }
        validate_selection(project, &record, actual_target, actual)
    };
    runtime::run_scoped_work_request(&wake, &event, &target, 120, &mut before_submit, &guard)
}

// Caller retains the project, registration, stopped-host and browser leases.
fn retry_selected(
    wake: &Store,
    project: &str,
    record: &crate::web_work_state::WorkRecord,
    target: &Target,
    prior: &catdesk_wake::store::Delivery,
    hash: &str,
) -> Result<Value, String> {
    validate_selection(project, record, target, &prior.event)?;
    if prior.target != *target {
        return Err("WEB_WORK_DELIVERY_SELECTION_CHANGED".into());
    }
    let control: runtime::Control = read(&wake.root().join("control.json"))?;
    if control.schema_version != 1 || control.desired != "STOPPED" {
        return Err("WEB_WORK_RECOVERY_HOST_NOT_STOPPED".into());
    }
    let recovered = wake.retry_ordinary_pre_submit_expected(&record.event_id, prior, hash)?;
    Ok(
        json!({"action":"web_work_retry_pre_submit","projectId":project,"requestId":record.request_id,
        "eventId":record.event_id,"phase":recovered.phase,"priorDeliverySha256":hash,
        "priorEvidencePreserved":true,"sendAttempted":false,"receiptCreated":false,
        "browserStarted":false,"hostStarted":false,"nativeGrantCreated":false}),
    )
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    fn unproven_fixture() -> (
        crate::web_work_state::WorkRecord,
        catdesk_wake::store::Delivery,
        Value,
    ) {
        let target = Target::new(1, "https://chatgpt.com/c/fixture-alpha".into()).unwrap();
        let event = Event {
            schema_version: 1,
            event_id: format!("web-work-{}", protocol::digest("alpha\0request-a")),
            project_id: "alpha".into(),
            event_type: "engineering_attention".into(),
            created_utc: 100,
            message: "Review source".into(),
            target_generation: 1,
            audit_references: vec![],
        };
        let record = crate::web_work_state::WorkRecord {
            request_id: "request-a".into(),
            event_id: event.event_id.clone(),
            message_sha256: protocol::digest(&event.message),
            target: target.clone(),
            phase: "OPEN".into(),
            outcome: None,
            summary_sha256: None,
            reported_status: None,
            task_assignment: Some(crate::web_work_state::WorkTaskAssignment {
                task_id: "task-a".into(),
                assignment_sha256: "c".repeat(64),
                scope_sha256: "d".repeat(64),
            }),
        };
        let prior = catdesk_wake::store::Delivery {
            schema_version: 1,
            event,
            target: target.clone(),
            phase: catdesk_wake::store::Phase::Submitting,
            owner: "fixture-owner".into(),
            updated_utc: 100,
            message_digest: record.message_sha256.clone(),
            receipt: None,
            reason: None,
        };
        let observation = json!({"action":"web_work_inspect", "projectId":"alpha", "eventId":record.event_id,
            "targetGeneration":1, "targetDigest":target.digest, "sourceDeliverySha256":"e".repeat(64),
            "persistedReceipt":null, "receiptProofRequested":true, "browserReadOnly":true,
            "sendAttempted":false, "receiptCreated":false, "stateChanged":false, "retryAuthorized":false,
            "workResolved":false, "hostStarted":false, "schedulerStarted":false, "reviewAuthorityGranted":false,
            "status":"BROWSER_OBSERVATION_INCOMPLETE", "ownedTreeExited":true, "failureReason":"PROBE_PROCESS_TIMEOUT",
            "lastPhase":"OBSERVING", "finalState":null, "submissionObservationState":null,
            "browserObservation":null, "tabObservation":null, "submissionObservation":null});
        (record, prior, observation)
    }

    #[test]
    fn ordinary_receipt_refusal_retains_attribution_without_receipt_cas_or_private_content() {
        let (record, prior, mut observation) = unproven_fixture();
        let before_record = record.clone();
        let before_delivery = prior.clone();
        observation["observations"] = json!(["PRIVATE_DRAFT", "https://private.invalid"]);
        observation["taskAttribution"] = json!({"taskId":"FOREIGN_TASK"});
        let frame = receipt_unproven_frame(
            "alpha",
            &record,
            &record.target,
            &prior,
            &"e".repeat(64),
            &observation,
        )
        .unwrap();
        assert_eq!(frame["errorCode"], "WEB_WORK_RECEIPT_UNPROVEN");
        assert_eq!(frame["taskAttribution"]["taskId"], "task-a");
        assert_eq!(frame["taskAttribution"]["assignmentSha256"], "c".repeat(64));
        for key in [
            "receiptCreated",
            "stateChanged",
            "sendAttempted",
            "retryAuthorized",
            "nativeGrantCreated",
            "workResolved",
        ] {
            assert_eq!(frame[key], false, "{key}");
        }
        assert_eq!(frame["submissionOutcome"], "UNKNOWN");
        assert!(!frame.to_string().contains("PRIVATE"));
        assert!(!frame.to_string().contains("FOREIGN_TASK"));
        assert!(frame.get("receipt").is_none());
        // The builder has no Store, browser or commit callback and cannot mutate either authority.
        assert_eq!(record, before_record);
        assert_eq!(prior, before_delivery);
    }

    #[test]
    fn ordinary_receipt_refusal_rejects_changed_identity_or_non_readonly_observation() {
        for (key, bad) in [
            ("projectId", json!("beta")),
            ("eventId", json!("foreign-event")),
            ("targetGeneration", json!(2)),
            ("targetDigest", json!("f".repeat(64))),
            ("sourceDeliverySha256", json!("f".repeat(64))),
            ("receiptProofRequested", json!(false)),
            ("persistedReceipt", json!({"unverified":true})),
            ("sendAttempted", json!(true)),
            ("stateChanged", json!(true)),
            ("receiptCreated", json!(true)),
            ("retryAuthorized", json!(true)),
            ("browserObservation", json!({"body":"PRIVATE_BODY"})),
            ("failureReason", json!("UNKNOWN_PRIVATE_EXCEPTION")),
        ] {
            let (record, prior, mut observation) = unproven_fixture();
            observation[key] = bad;
            assert!(
                receipt_unproven_frame(
                    "alpha",
                    &record,
                    &record.target,
                    &prior,
                    &"e".repeat(64),
                    &observation
                )
                .is_err(),
                "{key}"
            );
        }
        let (record, mut prior, observation) = unproven_fixture();
        prior.phase = catdesk_wake::store::Phase::Attention;
        assert!(
            receipt_unproven_frame(
                "alpha",
                &record,
                &record.target,
                &prior,
                &"e".repeat(64),
                &observation
            )
            .is_err()
        );
    }
    #[test]
    fn central_metadata_recovery_preserves_work_contract_and_refuses_foreign_or_closed_requests() {
        use sha2::{Digest, Sha256};
        let (control, _, wake, _, event) = crate::web_work::tests::assignment_fixture_for("alpha");
        wake.publish(&event).unwrap();
        let target = wake.config().unwrap().targets["alpha"].clone();
        let host = wake.host_lock().unwrap();
        wake.claim(&event, &host).unwrap();
        let prior = wake
            .transition(
                &event.event_id,
                host.owner(),
                catdesk_wake::store::Phase::Attention,
                None,
                Some("BROWSER_ATTEMPT_TIMEOUT".into()),
            )
            .unwrap();
        drop(host);
        let bytes = std::fs::read(
            wake.root()
                .join("deliveries")
                .join(format!("{}.json", event.event_id)),
        )
        .unwrap();
        let hash = format!("{:x}", Sha256::digest(bytes));
        let record = WorkLedger::open(&control, "alpha")
            .unwrap()
            .record("request-a")
            .unwrap()
            .unwrap();
        let ledger_path = control.join(".catdesk/web-work/alpha.json");
        let ledger_before = std::fs::read(&ledger_path).unwrap();
        assert!(retry_selected(&wake, "beta", &record, &target, &prior, &hash).is_err());
        let mut closed = record.clone();
        closed.phase = "CLOSED".into();
        assert!(retry_selected(&wake, "alpha", &closed, &target, &prior, &hash).is_err());
        let result = retry_selected(&wake, "alpha", &record, &target, &prior, &hash).unwrap();
        assert_eq!(result["phase"], "CLAIMED");
        assert_eq!(result["sendAttempted"], false);
        assert_eq!(result["nativeGrantCreated"], false);
        assert_eq!(std::fs::read(ledger_path).unwrap(), ledger_before);
        assert!(retry_selected(&wake, "alpha", &record, &target, &prior, &hash).is_err());
    }
    #[test]
    fn ordinary_retry_cannot_select_installed_resources_from_development_scope() {
        let (control, _, wake, _, event) = crate::web_work::tests::assignment_fixture_for("alpha");
        wake.publish(&event).unwrap();
        let queue = wake
            .root()
            .join("queue")
            .join(format!("{}.json", event.event_id));
        let before = std::fs::read(&queue).unwrap();
        assert_eq!(
            retry_pre_submit(&control, "alpha", "request-a", &"a".repeat(64)).unwrap_err(),
            "WEB_BROWSER_INSTALLED_SCOPE_REQUIRED"
        );
        assert_eq!(
            reconcile(&control, "alpha", "request-a").unwrap_err(),
            "WEB_BROWSER_INSTALLED_SCOPE_REQUIRED"
        );
        assert_eq!(
            response(&control, "alpha", "request-a").unwrap_err(),
            "WEB_BROWSER_INSTALLED_SCOPE_REQUIRED"
        );
        assert_eq!(std::fs::read(queue).unwrap(), before);
        assert!(wake.delivery(&event.event_id).unwrap().is_none());
    }

    #[test]
    fn work_inspection_preserves_development_isolation_without_browser_or_queue_changes() {
        let (control, _, wake, _, event) = crate::web_work::tests::assignment_fixture_for("alpha");
        wake.publish(&event).unwrap();
        let path = wake
            .root()
            .join("queue")
            .join(format!("{}.json", event.event_id));
        let before = std::fs::read(&path).unwrap();
        assert_eq!(
            inspect(&control, "alpha", "request-a").unwrap_err(),
            "WEB_BROWSER_INSTALLED_SCOPE_REQUIRED"
        );
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert!(wake.delivery(&event.event_id).unwrap().is_none());
    }

    #[test]
    fn scoped_work_delivery_replay_refusal_preserves_real_request_and_receipt_files() {
        use catdesk_wake::store::{Phase, Receipt};
        for phase in [Phase::Submitting, Phase::Sent] {
            let (control, _, wake, _, event) =
                crate::web_work::tests::assignment_fixture_for("alpha");
            wake.publish(&event).unwrap();
            let host = wake.host_lock().unwrap();
            let claimed = wake.claim(&event, &host).unwrap();
            wake.transition(
                &event.event_id,
                &claimed.owner,
                Phase::Submitting,
                None,
                None,
            )
            .unwrap();
            if phase == Phase::Sent {
                let target = wake.config().unwrap().targets["alpha"].clone();
                let receipt = Receipt {
                    schema_version: 1,
                    event_id: event.event_id.clone(),
                    target_generation: target.generation,
                    target_digest: target.digest,
                    message_digest: protocol::digest(&event.message),
                    sent_utc: now(),
                    evidence: "EXACT_USER_MESSAGE_APPENDED".into(),
                };
                // Synthetic evidence is confined to this disposable fixture.
                wake.record_exact_receipt_timer(&event.event_id, &receipt)
                    .unwrap();
                wake.transition(
                    &event.event_id,
                    &claimed.owner,
                    Phase::Sent,
                    Some(receipt),
                    None,
                )
                .unwrap();
            }
            drop(host);
            let files = [
                control.join(".catdesk/projects/projects.json"),
                control.join(".catdesk/web-work/alpha.json"),
                wake.root()
                    .join(format!("deliveries/{}.json", event.event_id)),
            ];
            let before: Vec<_> = files
                .iter()
                .map(std::fs::read)
                .collect::<Result<_, _>>()
                .unwrap();
            assert_eq!(
                deliver(&control, "alpha", "request-a").unwrap_err(),
                "SCOPED_REVIEW_REPLAY_REFUSED"
            );
            for (file, bytes) in files.iter().zip(before) {
                assert_eq!(std::fs::read(file).unwrap(), bytes);
            }
            assert_eq!(
                wake.delivery(&event.event_id).unwrap().unwrap().phase,
                phase
            );
        }
    }

    #[test]
    fn scoped_work_delivery_busy_registration_refuses_before_claim_or_browser() {
        let (control, _, wake, _, event) = crate::web_work::tests::assignment_fixture_for("alpha");
        wake.publish(&event).unwrap();
        let registry =
            AutonomousProjectRegistryStoreV1::open_read_only(control.join(".catdesk/projects"))
                .unwrap();
        let queue = wake.root().join(format!("queue/{}.json", event.event_id));
        let before = std::fs::read(&queue).unwrap();
        registry
            .with_registration_lock(|| {
                assert_eq!(
                    deliver(&control, "alpha", "request-a").unwrap_err(),
                    "WEB_CONTEXT_REGISTRATION_BUSY"
                );
                Ok(())
            })
            .unwrap();
        assert!(wake.delivery(&event.event_id).unwrap().is_none());
        assert_eq!(std::fs::read(&queue).unwrap(), before);
    }

    #[test]
    fn scoped_work_delivery_requires_exact_open_ordinary_request() {
        let (control, _, wake, _, event) = crate::web_work::tests::assignment_fixture_for("alpha");
        let record = WorkLedger::open(&control, "alpha")
            .unwrap()
            .record("request-a")
            .unwrap()
            .unwrap();
        let target = wake.config().unwrap().targets["alpha"].clone();
        validate_selection("alpha", &record, &target, &event).unwrap();
        for change in [
            "project",
            "request",
            "event",
            "closed",
            "generation",
            "message",
            "role",
            "audit",
            "target",
        ] {
            let mut record = record.clone();
            let mut event = event.clone();
            let mut target = target.clone();
            match change {
                "project" => event.project_id = "beta".into(),
                "request" => record.request_id = "another".into(),
                "event" => event.event_id = "web-work-other".into(),
                "closed" => record.phase = "CLOSED".into(),
                "generation" => event.target_generation += 1,
                "message" => event.message.push_str(" changed"),
                "role" => event.event_type = "source_review".into(),
                "audit" => event
                    .audit_references
                    .push("catdesk-review-v1:grant".into()),
                "target" => target.generation += 1,
                _ => unreachable!(),
            }
            assert!(
                validate_selection("alpha", &record, &target, &event).is_err(),
                "{change}"
            );
        }
    }
}
