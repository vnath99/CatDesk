//! One completed task-journal intent through the existing bounded sender.
use super::*;
use catdesk_wake::{
    protocol::{Event, Target},
    runtime,
    store::{Store, read},
};

pub(crate) fn deliver(
    workspace: &Path,
    project: &str,
    request_id: &str,
    call_id: &str,
    event_id: &str,
) -> Result<Value, String> {
    deliver_with_sender(
        workspace,
        project,
        request_id,
        call_id,
        event_id,
        |store, event, target, before, keep, handoff| {
            runtime::run_scoped_manual_end_turn_handoff(
                store, event, target, 180, before, keep, handoff,
            )
        },
    )
}

fn deliver_with_sender(
    workspace: &Path,
    project: &str,
    request_id: &str,
    call_id: &str,
    event_id: &str,
    send: impl FnOnce(
        &Store,
        &Event,
        &Target,
        &mut dyn FnMut(&Event, &Target) -> Result<(), String>,
        &dyn Fn() -> Result<(), String>,
        &mut dyn FnMut(&Event, &Target) -> Result<(), String>,
    ) -> Result<Value, String>,
) -> Result<Value, String> {
    if ![project, request_id, call_id, event_id]
        .iter()
        .all(|id| protocol::valid_id(id))
    {
        return Err("WEB_WORK_ARGUMENTS_INVALID".into());
    }
    let prepared = PreparedProjectScopeV1::resolve(workspace, project)
        .map_err(|_| "WEB_WORK_PROJECT_UNAVAILABLE")?;
    let lease = crate::operator_facade::acquire_web_work_lease(workspace, project)?;
    let ledger = WorkLedger::open_read_only(workspace, project)?;
    ledger.require_ordinary_request(request_id)?;
    let record = ledger
        .record(request_id)?
        .ok_or("WEB_WORK_REQUEST_MISSING")?;
    let paired = mcp::operator_read_project_web_target(workspace, project)
        .map_err(|_| "WEB_WORK_PAIRED_TARGET_REQUIRED")?
        .ok_or("WEB_WORK_PAIRED_TARGET_REQUIRED")?;
    let target = Target::new(paired.generation, paired.url)?;
    lease.check_source_task(&record.target, &target, &record.event_id)?;
    require_assignment_source(workspace, project, &record, record.target == target)?;
    let wake = Store::open(&wake_protocol_client::wake_root_for_workspace(workspace)?)?;
    let event: Event = match wake.delivery(event_id)? {
        Some(delivery) => delivery.event,
        None => read(&wake.root().join("queue").join(format!("{event_id}.json")))?,
    };
    if event.event_id != event_id {
        return Err("WEB_TASK_MANUAL_WAKE_SELECTION_CHANGED".into());
    }
    catdesk_wake::development_runtime::validate_selected_target_isolation(
        workspace,
        wake.root(),
        &target.url,
    )?;
    let held = std::cell::RefCell::new(Some(lease));
    let guard = || {
        if let Some(lease) = held.borrow().as_ref() {
            lease.check_source_task(&record.target, &target, &record.event_id)?;
        }
        mcp::with_current_project_web_target(
            workspace,
            project,
            target.generation,
            &target.digest,
            |_, current| {
                prepared
                    .revalidate_registration()
                    .map_err(|_| "WEB_WORK_PROJECT_REGISTRATION_CHANGED")?;
                if current.url != target.url || ledger.record(request_id)?.as_ref() != Some(&record)
                {
                    return Err("WEB_WORK_DELIVERY_SELECTION_CHANGED".into());
                }
                ledger.require_ordinary_request(request_id)?;
                crate::project_mcp_task::authorize_manual_wake_delivery(
                    prepared.scope(),
                    &record,
                    &target,
                    &event,
                    call_id,
                    held.borrow().is_some(),
                )
            },
        )
    };
    guard()?;
    let mut before = |actual: &Event, actual_target: &Target| {
        guard()?;
        if actual != &event || actual_target != &target {
            return Err("WEB_TASK_MANUAL_WAKE_SELECTION_CHANGED".into());
        }
        Ok(())
    };
    let mut handoff = |actual: &Event, actual_target: &Target| {
        if actual != &event || actual_target != &target {
            return Err("WEB_TASK_MANUAL_WAKE_SELECTION_CHANGED".into());
        }
        let delivery = wake
            .delivery(event_id)?
            .ok_or("WEB_TASK_MANUAL_WAKE_HANDOFF_UNPROVEN")?;
        if delivery.event != event || delivery.target != target {
            return Err("WEB_TASK_MANUAL_WAKE_HANDOFF_UNPROVEN".into());
        }
        wake.require_submitting_witness(&delivery)?;
        guard()?;
        held.borrow_mut()
            .take()
            .ok_or("WEB_TASK_MANUAL_WAKE_HANDOFF_UNPROVEN")?;
        Ok(())
    };
    let result = send(&wake, &event, &target, &mut before, &guard, &mut handoff)?;
    Ok(finish_delivery_result(result, request_id, call_id))
}

fn finish_delivery_result(mut result: Value, request_id: &str, call_id: &str) -> Value {
    // The exact native refusal shape retains exit2. Success attribution must
    // not decorate it into an unrecognized, apparently successful CLI result.
    if runtime::is_scoped_work_timeout(&result) {
        return result;
    }
    result["requestId"] = json!(request_id);
    result["taskCallId"] = json!(call_id);
    result["taskCompletionVerified"] = json!(false);
    result["nativeSessionAction"] = json!("NONE");
    result
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use catdesk_wake::store::{Phase, Receipt, WakeIntent};

    #[test]
    fn manual_wake_delivery_timeout_preserves_exact_refusal_for_exit2() {
        let timeout = json!({"action":"scoped_manual_end_turn_run", "status":"attention", "errorCode":"SCOPED_REVIEW_TIMEOUT",
            "projectId":"alpha", "eventId":"wake-intent-fixture", "targetGeneration":1, "targetDigest":"a".repeat(64),
            "messageSha256":"b".repeat(64), "deliveryPhase":"ATTENTION", "receiptPresent":false,
            "submissionOutcome":"UNKNOWN", "retryAuthorized":false, "nativeGrantCreatedBySender":false, "workResolved":false,
            "diagnosis":{"schemaVersion":1,"lastAdapterStage":"READINESS_OBSERVED","elapsedMs":180001,
                "maximumSeconds":180,"guardCheckCalls":5,"guardCheckMaxMs":3,"beforeSubmitCalls":0,
                "beforeSubmitMaxMs":0,"timingCapped":false}});
        assert!(runtime::is_scoped_work_timeout(&timeout));
        let output = finish_delivery_result(timeout.clone(), "request-a", "wake-one");
        assert_eq!(output, timeout);
        assert!(runtime::is_scoped_work_timeout(&output));
        assert!(output.get("taskCompletionVerified").is_none());
        let success = finish_delivery_result(json!({"phase":"TEST_SENT"}), "request-a", "wake-one");
        assert_eq!(success["requestId"], "request-a");
        assert_eq!(success["taskCallId"], "wake-one");
        assert_eq!(success["taskCompletionVerified"], false);
    }

    #[test]
    fn manual_wake_delivery_retains_lease_until_fence_then_allows_scoped_task_progress() {
        let (control, assignment, wake, pins, source) =
            crate::web_work::tests::assignment_fixture_for("alpha");
        wake.publish(&source).unwrap();
        crate::web_work::associate(&control, pins.clone(), &assignment).unwrap();
        let target = wake.config().unwrap().targets["alpha"].clone();
        let host = wake.host_lock().unwrap();
        let claimed = wake.claim(&source, &host).unwrap();
        wake.transition(
            &source.event_id,
            &claimed.owner,
            Phase::Submitting,
            None,
            None,
        )
        .unwrap();
        let receipt = Receipt {
            schema_version: 1,
            event_id: source.event_id.clone(),
            target_generation: target.generation,
            target_digest: target.digest.clone(),
            message_digest: protocol::digest(&source.message),
            sent_utc: now(),
            evidence: "EXACT_USER_MESSAGE_APPENDED".into(),
        };
        wake.transition(
            &source.event_id,
            &claimed.owner,
            Phase::Sent,
            Some(receipt.clone()),
            None,
        )
        .unwrap();
        wake.record_exact_receipt_timer(&source.event_id, &receipt)
            .unwrap();
        wake.complete_timer(&source.event_id).unwrap();
        drop(host);
        let grant =
            crate::web_work::execution_permission_with_manual_wake(&control, pins, false, true)
                .unwrap();
        let key: String = serde_json::from_slice(
            &std::fs::read(grant["executionKeyFile"].as_str().unwrap()).unwrap(),
        )
        .unwrap();
        let prepared = PreparedProjectScopeV1::resolve(&control, "alpha").unwrap();
        let record = WorkLedger::open_read_only(&control, "alpha")
            .unwrap()
            .record("request-a")
            .unwrap()
            .unwrap();
        let request = crate::project_mcp_task::ManualWakeRequest::parse(
            json!({"sourceTurnId":"done-turn","priority":1,"ttlSeconds":600}),
        )
        .unwrap();
        let published = crate::project_mcp_task::journal_manual_wake(
            prepared.scope(),
            &record,
            &key,
            "wake-one",
            &request,
            &target,
            || {
                wake.produce_expected_wake(
                    "alpha",
                    &wake_protocol_client::registered_manual_wake_message(
                        "alpha",
                        "request-a",
                        "task-fixture",
                        &source.event_id,
                        "done-turn",
                    ),
                    &target,
                    WakeIntent {
                        purpose: protocol::WakePurpose::ManualEndTurn,
                        source_event_id: &source.event_id,
                        source_turn_id: "done-turn",
                        priority: 1,
                        ttl_seconds: 600,
                    },
                )
            },
        )
        .unwrap();
        let event_id = published["eventId"].as_str().unwrap();
        let output = deliver_with_sender(
            &control,
            "alpha",
            "request-a",
            "wake-one",
            event_id,
            |store, event, target, before, keep, handoff| {
                before(event, target)?;
                assert!(crate::operator_facade::acquire_web_work_lease(&control, "alpha").is_err());
                assert!(
                    handoff(event, target).is_err(),
                    "unfenced delivery must retain the task lease"
                );
                assert!(crate::operator_facade::acquire_web_work_lease(&control, "alpha").is_err());
                let host = store.host_lock()?;
                let delivery = store.claim(event, &host)?;
                store.transition(
                    &event.event_id,
                    &delivery.owner,
                    Phase::Submitting,
                    None,
                    None,
                )?;
                handoff(event, target)?;
                let task_lease = crate::operator_facade::acquire_web_work_lease(&control, "alpha")?;
                // A real continued scoped call creates this Requested row only
                // after handoff. The sender guard must not cancel that progress.
                use crate::delegated::{
                    contracts::{RunId, ToolCallId},
                    journal::{
                        DelegatedJournal, ToolCallRecordV1, ToolCallStatus, ToolMutationKind,
                    },
                };
                let root = control.join(".catdesk/project-mcp-tasks/alpha/task-fixture/journal");
                let journal = DelegatedJournal::open(&root).unwrap();
                let run = RunId::new("task-fixture").unwrap();
                journal
                    .record_tool_call_requested(ToolCallRecordV1 {
                        schema_version: 1,
                        run_id: run,
                        tool_call_id: ToolCallId::new("continued-read").unwrap(),
                        tool_name: "read".into(),
                        request_hash: "x".into(),
                        arguments_hash: "x".into(),
                        mutation_kind: ToolMutationKind::ReadOnly,
                        status: ToolCallStatus::Requested,
                        result_hash: None,
                        outcome_summary: None,
                    })
                    .unwrap();
                keep()?;
                assert!(
                    handoff(event, target).is_err(),
                    "a handoff is released exactly once"
                );
                drop(task_lease);
                Ok(json!({"phase":"TEST_FENCED_NO_BROWSER_NO_SEND"}))
            },
        )
        .unwrap();
        assert_eq!(output["taskCompletionVerified"], false);
        assert_eq!(
            wake.delivery(event_id).unwrap().unwrap().phase,
            Phase::Submitting
        );
        assert!(wake.delivery(event_id).unwrap().unwrap().receipt.is_none());
    }
}
