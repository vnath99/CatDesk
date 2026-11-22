//! One completed task-journal intent through the existing bounded sender.
use super::*;
use catdesk_wake::{
    protocol::{Event, Target},
    runtime,
    store::{Store, read},
};

#[derive(Clone, Copy)]
enum Selection<'a> {
    Exact {
        request_id: &'a str,
        call_id: &'a str,
        event_id: &'a str,
    },
    Next,
}

pub(crate) fn deliver(
    workspace: &Path,
    project: &str,
    request_id: &str,
    call_id: &str,
    event_id: &str,
) -> Result<Value, String> {
    deliver_selected(
        workspace,
        project,
        Selection::Exact {
            request_id,
            call_id,
            event_id,
        },
    )
}

pub(crate) fn deliver_next(workspace: &Path, project: &str) -> Result<Value, String> {
    deliver_selected(workspace, project, Selection::Next)
}

fn deliver_selected(
    workspace: &Path,
    project: &str,
    selection: Selection<'_>,
) -> Result<Value, String> {
    deliver_with_sender(
        workspace,
        project,
        selection,
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
    selection: Selection<'_>,
    send: impl FnOnce(
        &Store,
        &Event,
        &Target,
        &mut dyn FnMut(&Event, &Target) -> Result<(), String>,
        &dyn Fn() -> Result<(), String>,
        &mut dyn FnMut(&Event, &Target) -> Result<(), String>,
    ) -> Result<Value, String>,
) -> Result<Value, String> {
    if !protocol::valid_id(project)
        || matches!(selection, Selection::Exact { request_id, call_id, event_id }
        if ![request_id, call_id, event_id].into_iter().all(protocol::valid_id))
    {
        return Err("WEB_WORK_ARGUMENTS_INVALID".into());
    }
    let prepared = PreparedProjectScopeV1::resolve(workspace, project)
        .map_err(|_| "WEB_WORK_PROJECT_UNAVAILABLE")?;
    let lease = crate::operator_facade::acquire_web_work_lease(workspace, project)?;
    let ledger = WorkLedger::open_read_only(workspace, project)?;
    let record = match selection {
        Selection::Exact { request_id, .. } => ledger.record(request_id)?,
        Selection::Next => ledger.active_record()?,
    }
    .ok_or("WEB_WORK_REQUEST_MISSING")?;
    let request_id = record.request_id.as_str();
    ledger.require_ordinary_request(request_id)?;
    let paired = mcp::operator_read_project_web_target(workspace, project)
        .map_err(|_| "WEB_WORK_PAIRED_TARGET_REQUIRED")?
        .ok_or("WEB_WORK_PAIRED_TARGET_REQUIRED")?;
    let target = Target::new(paired.generation, paired.url)?;
    lease.check_source_task(&record.target, &target, &record.event_id)?;
    require_assignment_source(workspace, project, &record, record.target == target)?;
    let wake = Store::open(&wake_protocol_client::wake_root_for_workspace(workspace)?)?;
    let (call_id, event) = match selection {
        Selection::Exact {
            call_id, event_id, ..
        } => {
            let event: Event = match wake.delivery(event_id)? {
                Some(delivery) => delivery.event,
                None => read(&wake.root().join("queue").join(format!("{event_id}.json")))?,
            };
            if event.event_id != event_id {
                return Err("WEB_TASK_MANUAL_WAKE_SELECTION_CHANGED".into());
            }
            (call_id.to_owned(), event)
        }
        Selection::Next => {
            crate::project_mcp_task::next_manual_wake(prepared.scope(), &record, &target, &wake)?
        }
    };
    let call_id = call_id.as_str();
    let event_id = event.event_id.as_str();
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
    crate::project_mcp_task::require_manual_wake_checkpoint(prepared.scope(), &record)?;
    let mut before = |actual: &Event, actual_target: &Target| {
        guard()?;
        if actual != &event || actual_target != &target {
            return Err("WEB_TASK_MANUAL_WAKE_SELECTION_CHANGED".into());
        }
        crate::project_mcp_task::require_manual_wake_checkpoint(prepared.scope(), &record)
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
pub(crate) fn fixture_manual_submission_fence(
    workspace: &Path,
    project: &str,
) -> Result<Value, String> {
    deliver_with_sender(
        workspace,
        project,
        Selection::Next,
        |wake, event, target, before, keep, handoff| {
            before(event, target)?;
            let owner = wake.host_lock()?;
            wake.claim(event, &owner)?;
            // End the inert sender at the same durable cut as an interrupted
            // browser adapter. No browser or USER submission runs in this test.
            let delivery = wake.transition(
                &event.event_id,
                owner.owner(),
                catdesk_wake::store::Phase::Submitting,
                None,
                None,
            )?;
            handoff(event, target)?;
            keep()?;
            Ok(json!({"phase":delivery.phase,"eventId":event.event_id}))
        },
    )
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
        let queue = |call_id, turn_id| {
            let request = crate::project_mcp_task::ManualWakeRequest::parse(
                json!({"sourceTurnId":turn_id,"priority":1,"ttlSeconds":600}),
            )
            .unwrap();
            crate::project_mcp_task::journal_manual_wake(
                prepared.scope(),
                &record,
                &key,
                call_id,
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
                            turn_id,
                        ),
                        &target,
                        WakeIntent {
                            purpose: protocol::WakePurpose::ManualEndTurn,
                            source_event_id: &source.event_id,
                            source_turn_id: turn_id,
                            priority: 1,
                            ttl_seconds: 600,
                        },
                    )
                },
            )
            .unwrap()
        };
        let older = queue("wake-older", "earlier-turn");
        let published = queue("wake-one", "done-turn");
        let event_id = published["eventId"].as_str().unwrap();
        let older_id = older["eventId"].as_str().unwrap();
        let resolution = json!({"projectId":"alpha","requestId":"request-a",
            "expectedTargetGeneration":target.generation,"expectedTargetSha256":target.digest,
            "outcome":"completed","summary":"Verified task fixture"});
        let assert_pending = || {
            assert_eq!(
                crate::web_work::resolve(&control, resolution.clone()).unwrap_err(),
                "WEB_WORK_CONTINUATION_PENDING"
            );
            assert_eq!(
                WorkLedger::open_read_only(&control, "alpha")
                    .unwrap()
                    .record("request-a")
                    .unwrap(),
                Some(record.clone())
            );
        };
        assert_pending();
        let older_event: Event =
            read(&wake.root().join("queue").join(format!("{older_id}.json"))).unwrap();

        let forbidden_sender = |_: &Store,
                                _: &Event,
                                _: &Target,
                                _: &mut dyn FnMut(&Event, &Target) -> Result<(), String>,
                                _: &dyn Fn() -> Result<(), String>,
                                _: &mut dyn FnMut(&Event, &Target) -> Result<(), String>|
         -> Result<Value, String> {
            panic!("a held or ambiguous Wake must not start a sender")
        };
        assert_eq!(
            deliver_with_sender(&control, "alpha", Selection::Next, forbidden_sender).unwrap_err(),
            "WEB_TASK_MANUAL_WAKE_MULTIPLE_PENDING",
        );
        assert!(wake.delivery(event_id).unwrap().is_none());

        let host = wake.host_lock().unwrap();
        let prior = wake.claim(&older_event, &host).unwrap();
        let prior = wake
            .transition(older_id, &prior.owner, Phase::Submitting, None, None)
            .unwrap();
        drop(host);
        assert_eq!(
            deliver_with_sender(&control, "alpha", Selection::Next, forbidden_sender).unwrap_err(),
            "WEB_TASK_MANUAL_WAKE_DELIVERY_HELD",
        );
        assert_eq!(wake.delivery(older_id).unwrap(), Some(prior.clone()));
        assert!(wake.delivery(event_id).unwrap().is_none());

        // A synthetic receipt resolves the older event so the other exact
        // call becomes the sole candidate. No real browser is used.
        let receipt = Receipt {
            schema_version: 1,
            event_id: older_id.into(),
            target_generation: prior.target.generation,
            target_digest: prior.target.digest.clone(),
            message_digest: prior.message_digest.clone(),
            sent_utc: now(),
            evidence: "EXACT_USER_MESSAGE_APPENDED".into(),
        };
        wake.record_exact_receipt_timer(older_id, &receipt).unwrap();
        wake.transition(older_id, &prior.owner, Phase::Sent, Some(receipt), None)
            .unwrap();
        wake.complete_timer(older_id).unwrap();
        let unrelated = wake.root().join("queue/unrelated-fixture.json");
        std::fs::write(&unrelated, b"unrelated corrupt queue object").unwrap();

        let output = deliver_with_sender(
            &control,
            "alpha",
            Selection::Next,
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
        assert_eq!(output["requestId"], "request-a");
        assert_eq!(output["taskCallId"], "wake-one");
        assert_eq!(
            std::fs::read(unrelated).unwrap(),
            b"unrelated corrupt queue object"
        );
        assert_eq!(
            wake.delivery(event_id).unwrap().unwrap().phase,
            Phase::Submitting
        );
        assert!(wake.delivery(event_id).unwrap().unwrap().receipt.is_none());
        assert_eq!(
            crate::web_work::resolve(&control, resolution.clone()).unwrap_err(),
            "PROJECT_TASK_RECOVERY_REQUIRED"
        );
        use crate::delegated::{
            contracts::{RunId, ToolCallId},
            journal::{DelegatedJournal, ToolCallStatus},
        };
        let journal = DelegatedJournal::open(
            &control.join(".catdesk/project-mcp-tasks/alpha/task-fixture/journal"),
        )
        .unwrap();
        journal
            .transition_tool_call(
                &RunId::new("task-fixture").unwrap(),
                &ToolCallId::new("continued-read").unwrap(),
                ToolCallStatus::Failed,
                None,
                Some("Fixture call ended without an effect".into()),
            )
            .unwrap();
        assert_pending();
        let submitting = wake.delivery(event_id).unwrap().unwrap();
        crate::project_mcp_task::require_journaled_manual_wake(
            prepared.scope(),
            &record,
            &submitting,
        )
        .unwrap();
        let mut wrong_message = submitting.clone();
        wrong_message.event.message.push_str(" unexpected text");
        assert_eq!(
            crate::project_mcp_task::require_journaled_manual_wake(
                prepared.scope(),
                &record,
                &wrong_message
            )
            .unwrap_err(),
            "WEB_TASK_MANUAL_WAKE_SELECTION_CHANGED"
        );
        let receipt = Receipt {
            schema_version: 1,
            event_id: event_id.into(),
            target_generation: target.generation,
            target_digest: target.digest.clone(),
            message_digest: submitting.message_digest.clone(),
            sent_utc: now(),
            evidence: "EXACT_USER_MESSAGE_APPENDED".into(),
        };
        wake.record_exact_receipt_timer(event_id, &receipt).unwrap();
        wake.reconcile_task_continuation_sent_expected(event_id, receipt, &submitting)
            .unwrap();
        assert_pending();
        wake.complete_timer(event_id).unwrap();
        let closed = crate::web_work::resolve(&control, resolution.clone()).unwrap();
        assert_eq!(closed["phase"], "CLOSED");
        assert_eq!(closed["taskCompletionVerified"], false);
        assert_eq!(
            crate::web_work::resolve(&control, resolution).unwrap(),
            closed
        );
    }
}
