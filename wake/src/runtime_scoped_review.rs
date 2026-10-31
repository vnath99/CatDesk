//! One explicit, grant-guarded event; no persistent host or scheduler.
use super::*;
use crate::protocol::Event;
use serde_json::{Value, json};

enum Selection<'a> {
    Review,
    TaskContinuation(&'a Event, &'a Target),
}

/// The local task owner supplies the immutable event selected from its existing
/// receipt-checked context. This path never scans or dispatches sibling queues.
pub fn run_scoped_task_continuation(
    store: &Store,
    event: &Event,
    target: &Target,
    maximum_seconds: u64,
    before_submit: &mut dyn FnMut(&Event, &Target) -> Result<()>,
    keep_running: &dyn Fn() -> Result<()>,
) -> Result<Value> {
    run_selected_with_adapter(
        store,
        &event.event_id,
        maximum_seconds,
        Selection::TaskContinuation(event, target),
        before_submit,
        keep_running,
        |deadline, guard| {
            let installed = installed_bootstrap_directory(store)?;
            Adapter::spawn_from_installed_guarded(
                store.root(),
                &installed,
                deadline.saturating_duration_since(Instant::now()),
                guard,
            )
        },
    )
}

pub fn run_scoped_review(
    store: &Store,
    event_id: &str,
    maximum_seconds: u64,
    before_submit: &mut dyn FnMut(&Event, &Target) -> Result<()>,
    keep_running: &dyn Fn() -> Result<()>,
) -> Result<Value> {
    run_with_adapter(
        store,
        event_id,
        maximum_seconds,
        before_submit,
        keep_running,
        |deadline, guard| {
            let installed = installed_bootstrap_directory(store)?;
            Adapter::spawn_from_installed_guarded(
                store.root(),
                &installed,
                deadline.saturating_duration_since(Instant::now()),
                guard,
            )
        },
    )
}

fn run_with_adapter<A: SubmissionAdapter>(
    store: &Store,
    event_id: &str,
    maximum_seconds: u64,
    before_submit: &mut dyn FnMut(&Event, &Target) -> Result<()>,
    keep_running: &dyn Fn() -> Result<()>,
    connect: impl FnOnce(Instant, &dyn Fn() -> Result<()>) -> Result<A>,
) -> Result<Value> {
    run_selected_with_adapter(
        store,
        event_id,
        maximum_seconds,
        Selection::Review,
        before_submit,
        keep_running,
        connect,
    )
}

fn run_selected_with_adapter<A: SubmissionAdapter>(
    store: &Store,
    event_id: &str,
    maximum_seconds: u64,
    selection: Selection<'_>,
    before_submit: &mut dyn FnMut(&Event, &Target) -> Result<()>,
    keep_running: &dyn Fn() -> Result<()>,
    connect: impl FnOnce(Instant, &dyn Fn() -> Result<()>) -> Result<A>,
) -> Result<Value> {
    if !crate::protocol::valid_id(event_id) || !(1..=180).contains(&maximum_seconds) {
        return Err("SCOPED_REVIEW_ARGUMENTS_INVALID".into());
    }
    let deadline = Instant::now() + Duration::from_secs(maximum_seconds);
    let guard = || {
        if Instant::now() >= deadline {
            return Err("SCOPED_REVIEW_TIMEOUT".into());
        }
        keep_running()?;
        if desired(store)? != "STOPPED" {
            return Err("SCOPED_REVIEW_HOST_NOT_STOPPED".into());
        }
        Ok(())
    };
    guard()?;
    let lease = store.host_lock()?;
    guard()?;
    if store
        .delivery(event_id)?
        .is_some_and(|delivery| delivery.phase != Phase::Claimed)
    {
        return Err("SCOPED_REVIEW_REPLAY_REFUSED".into());
    }
    // One validated key chooses one existing event. Never scan or dispatch a
    // sibling project's queue, and never accept a caller-provided message.
    let event: Event = read(&store.root().join("queue").join(format!("{event_id}.json")))?;
    event.validate()?;
    let review = event.event_type == "source_review"
        && event
            .audit_references
            .iter()
            .filter(|item| item.starts_with("catdesk-review-v1:"))
            .count()
            == 1
        && !event
            .audit_references
            .iter()
            .any(|item| item == "catdesk-review-v1:");
    let permitted = match selection {
        Selection::Review => review,
        Selection::TaskContinuation(expected, target) => {
            target.validate()?;
            &event == expected
                && event.event_type == "engineering_attention"
                && !event
                    .audit_references
                    .iter()
                    .any(|item| item.starts_with("catdesk-review-v1:"))
                && store.config()?.targets.get(&event.project_id) == Some(target)
                && event.wake_metadata()?.is_some_and(|metadata| {
                    metadata.purpose == crate::protocol::WakePurpose::NewTask
                        && metadata
                            .source_turn_id
                            .strip_prefix("initial-context-")
                            .is_some_and(|id| {
                                id.len() == 64
                                    && id
                                        .bytes()
                                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                            })
                })
        }
    };
    if event.event_id != event_id || !permitted {
        return Err("SCOPED_REVIEW_EVENT_INVALID".into());
    }
    if event.is_expired_at(now())? {
        return Err("QUEUE_EVENT_EXPIRED".into());
    }
    if store
        .timers_for_project(&event.project_id)?
        .iter()
        .any(|timer| timer.response_state != ResponseState::Complete)
    {
        return Err("SCOPED_REVIEW_PROJECT_BUSY".into());
    }
    let delivery = store.claim(&event, &lease)?;
    let mut status = local_status(&delivery);
    let result = (|| {
        guard()?;
        let mut adapter = connect(deadline, &guard)?;
        let mut scoped = ScopedReviewGuard {
            deadline,
            review_event: review,
            before_submit,
            keep_running: &guard,
        };
        let result = attempt_inner(
            store,
            &lease,
            &mut adapter,
            &delivery,
            &mut status,
            Some(&mut scoped),
        );
        adapter.finish(
            deadline
                .saturating_duration_since(Instant::now())
                .min(Duration::from_secs(5)),
        );
        result
    })();
    if let Err(reason) = result {
        match store.delivery(event_id)?.map(|delivery| delivery.phase) {
            Some(Phase::Claimed) => {
                store.transition(
                    event_id,
                    lease.owner(),
                    Phase::Attention,
                    None,
                    Some(reason.clone()),
                )?;
            }
            Some(Phase::Submitting) => {
                store.note_submitting_attention(event_id, &reason)?;
            }
            Some(Phase::Sent) => {
                let _ = store.set_timer_response_state(event_id, ResponseState::Attention);
            }
            _ => {}
        }
        return Err(reason);
    }
    let final_delivery = store
        .delivery(event_id)?
        .ok_or("SCOPED_REVIEW_RECEIPT_REQUIRED")?;
    if final_delivery.phase != Phase::Sent || final_delivery.receipt.is_none() {
        return Err("SCOPED_REVIEW_RECEIPT_REQUIRED".into());
    }
    Ok(
        json!({"action":if review {"scoped_review_run"} else {"scoped_task_continuation_run"},"eventId":event_id,"projectId":event.project_id,
        "targetGeneration":final_delivery.target.generation,"targetDigest":final_delivery.target.digest,
        "phase":final_delivery.phase,"receipt":final_delivery.receipt,"maximumSeconds":maximum_seconds,
        "hostStarted":false,"schedulerStarted":false,"desiredStateChanged":false}),
    )
}

fn local_status(delivery: &crate::store::Delivery) -> Status {
    Status {
        version: VERSION.into(),
        protocol_version: 1,
        host: "STOPPED".into(),
        pid: 0,
        browser_session_protocol: None,
        updated_utc: now(),
        targets: BTreeMap::from([(delivery.event.project_id.clone(), delivery.target.clone())]),
        queue_depth: 1,
        stale_count: 0,
        browser: "NOT_OBSERVED".into(),
        login: "NOT_OBSERVED".into(),
        submission: "CLAIMED".into(),
        last_event_id: Some(delivery.event.event_id.clone()),
        last_attempt_utc: Some(now()),
        last_success_utc: None,
        last_receipt: None,
        attention: None,
        host_recovery_count: 0,
        last_host_error: None,
        last_host_error_utc: None,
        turn_timers: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::Cell, collections::VecDeque, rc::Rc};

    struct FakeAdapter {
        frames: VecDeque<Value>,
        receipt: Receipt,
        admitted: Rc<Cell<bool>>,
        ack_count: Rc<Cell<u32>>,
        cancelled: Rc<Cell<bool>>,
        cancel_after_ack: bool,
        delay: Duration,
    }
    impl SubmissionAdapter for FakeAdapter {
        fn send(&mut self, value: Value) -> Result<()> {
            if value.get("submit") == Some(&json!(true)) {
                assert!(
                    self.admitted.get(),
                    "grant must be consumed before the submit ack"
                );
                self.ack_count.set(self.ack_count.get() + 1);
                if self.cancel_after_ack {
                    self.cancelled.set(true);
                }
                self.frames
                    .push_back(json!({"stage":"SENT_PENDING_RESPONSE","receipt":self.receipt}));
            }
            Ok(())
        }
        fn receive(
            &mut self,
            _: Duration,
        ) -> std::result::Result<Result<Value>, mpsc::RecvTimeoutError> {
            if !self.delay.is_zero() {
                thread::sleep(self.delay);
            }
            self.frames
                .pop_front()
                .map(Ok)
                .ok_or(mpsc::RecvTimeoutError::Disconnected)
        }
    }

    fn fixture(label: &str) -> (Store, Event, Target) {
        let (store, lease) = super::super::scheduler::tests::fixture(label);
        drop(lease);
        atomic(
            &store.root().join("control.json"),
            &json!({"schemaVersion":1,"desired":"STOPPED"}),
        )
        .unwrap();
        let target = store.config().unwrap().targets["alpha"].clone();
        let event = store
            .produce_expected_with_audit(
                "scoped-review",
                "alpha",
                "source_review",
                "review public source",
                &target,
                &["catdesk-review-v1:grant-one:entry-one".into()],
            )
            .unwrap();
        (store, event, target)
    }

    fn adapter(event: &Event, target: &Target) -> FakeAdapter {
        FakeAdapter {
            frames: VecDeque::from([
                json!({"stage":"CHAT_IDLE"}),
                json!({"stage":"BEFORE_SUBMIT"}),
            ]),
            receipt: Receipt {
                schema_version: 1,
                event_id: event.event_id.clone(),
                target_generation: target.generation,
                target_digest: target.digest.clone(),
                message_digest: crate::protocol::digest(&event.message),
                sent_utc: now(),
                evidence: "EXACT_USER_MESSAGE_APPENDED".into(),
            },
            admitted: Rc::new(Cell::new(false)),
            ack_count: Rc::new(Cell::new(0)),
            cancelled: Rc::new(Cell::new(false)),
            cancel_after_ack: false,
            delay: Duration::ZERO,
        }
    }

    fn continuation(store: &Store, target: &Target) -> Event {
        store
            .produce_expected_wake(
                "alpha",
                "Continue the exact already-granted task",
                target,
                crate::store::WakeIntent {
                    purpose: crate::protocol::WakePurpose::NewTask,
                    source_event_id: "existing-source-task",
                    source_turn_id: &format!("initial-context-{}", "a".repeat(64)),
                    priority: 0,
                    ttl_seconds: 600,
                },
            )
            .unwrap()
    }

    #[test]
    fn scoped_task_continuation_sends_only_exact_selection_while_foreign_queue_is_unreadable() {
        let (store, review, target) = fixture("task-continuation-isolation");
        let event = continuation(&store, &target);
        let foreign = store.root().join("queue/beta-unreadable.json");
        std::fs::write(&foreign, b"preserve unrelated invalid queue bytes").unwrap();
        let fake = adapter(&event, &target);
        let admitted = fake.admitted.clone();
        let ack = fake.ack_count.clone();
        let before_control = std::fs::read(store.root().join("control.json")).unwrap();
        let result = run_selected_with_adapter(
            &store,
            &event.event_id,
            120,
            Selection::TaskContinuation(&event, &target),
            &mut |actual, actual_target| {
                assert_eq!(actual, &event);
                assert_eq!(actual_target, &target);
                admitted.set(true);
                Ok(())
            },
            &|| Ok(()),
            |_, _| Ok(fake),
        )
        .unwrap();
        assert_eq!(result["action"], "scoped_task_continuation_run");
        assert_eq!(result["phase"], "SENT");
        assert_eq!(ack.get(), 1);
        assert!(store.delivery(&review.event_id).unwrap().is_none());
        assert!(store.delivery("beta-1").unwrap().is_none());
        assert_eq!(
            std::fs::read(&foreign).unwrap(),
            b"preserve unrelated invalid queue bytes"
        );
        assert_eq!(
            std::fs::read(store.root().join("control.json")).unwrap(),
            before_control
        );
    }

    #[test]
    fn scoped_task_continuation_refuses_foreign_changed_or_review_selection_before_browser() {
        for changed in ["body", "target", "review"] {
            let (store, review, target) = fixture(&format!("task-selection-{changed}"));
            let event = continuation(&store, &target);
            let mut expected = event.clone();
            let mut expected_target = target.clone();
            let selected = match changed {
                "body" => {
                    expected.message.push_str(" changed");
                    &event
                }
                "target" => {
                    expected_target =
                        Target::new(99, "https://chatgpt.com/c/foreign".into()).unwrap();
                    &event
                }
                _ => {
                    expected = review.clone();
                    &review
                }
            };
            let result = run_selected_with_adapter::<FakeAdapter>(
                &store,
                &selected.event_id,
                120,
                Selection::TaskContinuation(&expected, &expected_target),
                &mut |_, _| panic!("invalid selection cannot authorize submission"),
                &|| Ok(()),
                |_, _| panic!("invalid selection cannot start browser"),
            );
            assert_eq!(result.unwrap_err(), "SCOPED_REVIEW_EVENT_INVALID");
            assert!(store.delivery(&selected.event_id).unwrap().is_none());
        }
    }

    #[test]
    fn scoped_task_continuation_keeps_unknown_outcome_and_suppresses_completed_replay() {
        for unknown in [false, true] {
            let (store, _, target) = fixture(if unknown {
                "task-unknown"
            } else {
                "task-complete"
            });
            let event = continuation(&store, &target);
            let mut fake = adapter(&event, &target);
            if unknown {
                fake.receipt.message_digest = "0".repeat(64);
            }
            let admitted = fake.admitted.clone();
            let ack = fake.ack_count.clone();
            let result = run_selected_with_adapter(
                &store,
                &event.event_id,
                120,
                Selection::TaskContinuation(&event, &target),
                &mut |_, _| {
                    admitted.set(true);
                    Ok(())
                },
                &|| Ok(()),
                |_, _| Ok(fake),
            );
            assert_eq!(result.is_err(), unknown);
            assert_eq!(ack.get(), 1);
            let before = std::fs::read(
                store
                    .root()
                    .join("deliveries")
                    .join(format!("{}.json", event.event_id)),
            )
            .unwrap();
            let result = run_selected_with_adapter::<FakeAdapter>(
                &store,
                &event.event_id,
                120,
                Selection::TaskContinuation(&event, &target),
                &mut |_, _| panic!("replay cannot authorize submission"),
                &|| Ok(()),
                |_, _| panic!("replay cannot start browser"),
            );
            assert_eq!(result.unwrap_err(), "SCOPED_REVIEW_REPLAY_REFUSED");
            assert_eq!(
                std::fs::read(
                    store
                        .root()
                        .join("deliveries")
                        .join(format!("{}.json", event.event_id))
                )
                .unwrap(),
                before
            );
        }
    }

    #[test]
    fn scoped_task_continuation_expired_intent_is_refused_before_claim_or_browser() {
        let (store, _, target) = fixture("task-expired");
        let created = now() - 601;
        let event = Event {
            schema_version: 1,
            event_id: "expired-task-continuation".into(),
            project_id: "alpha".into(),
            event_type: "engineering_attention".into(),
            created_utc: created,
            message: "Continue exact task".into(),
            target_generation: target.generation,
            audit_references: vec![],
        }
        .with_wake_metadata(crate::protocol::WakeMetadata {
            purpose: crate::protocol::WakePurpose::NewTask,
            source_event_id: "existing-source-task".into(),
            source_turn_id: format!("initial-context-{}", "a".repeat(64)),
            priority: 0,
            expires_utc: created + 600,
        })
        .unwrap();
        store.publish(&event).unwrap();
        assert_eq!(
            run_selected_with_adapter::<FakeAdapter>(
                &store,
                &event.event_id,
                120,
                Selection::TaskContinuation(&event, &target),
                &mut |_, _| panic!(),
                &|| Ok(()),
                |_, _| panic!("expired intent cannot start browser")
            )
            .unwrap_err(),
            "QUEUE_EVENT_EXPIRED"
        );
        assert!(store.delivery(&event.event_id).unwrap().is_none());
    }

    #[test]
    fn scoped_review_consumes_immediately_before_one_ack_and_preserves_stopped_control() {
        let (store, event, target) = fixture("scoped-success");
        let fake = adapter(&event, &target);
        let admitted = fake.admitted.clone();
        let acknowledgments = fake.ack_count.clone();
        let before = std::fs::read(store.root().join("control.json")).unwrap();
        let result = run_with_adapter(
            &store,
            &event.event_id,
            180,
            &mut |actual, actual_target| {
                assert_eq!(actual, &event);
                assert_eq!(actual_target, &target);
                assert_eq!(
                    store.delivery(&event.event_id)?.unwrap().phase,
                    Phase::Claimed
                );
                admitted.set(true);
                Ok(())
            },
            &|| Ok(()),
            |_, _| Ok(fake),
        )
        .unwrap();
        assert_eq!(acknowledgments.get(), 1);
        assert_eq!(result["phase"], "SENT");
        assert_eq!(
            std::fs::read(store.root().join("control.json")).unwrap(),
            before
        );
        assert!(!store.root().join("status.json").exists());
        assert!(!store.root().join("readiness-history.json").exists());
        assert!(store.delivery("beta-1").unwrap().is_none());
    }

    #[test]
    fn scoped_review_denial_or_pre_ack_cancellation_never_submits() {
        for cancelled in [false, true] {
            let (store, event, target) = fixture("scoped-denial");
            let fake = adapter(&event, &target);
            let ack = fake.ack_count.clone();
            let stop = fake.cancelled.clone();
            let result = run_with_adapter(
                &store,
                &event.event_id,
                180,
                &mut |_, _| {
                    if cancelled {
                        stop.set(true);
                        Ok(())
                    } else {
                        Err("REVIEW_AUTH_REVOKED".into())
                    }
                },
                &|| {
                    if stop.get() {
                        Err("TEST_CANCELLED".into())
                    } else {
                        Ok(())
                    }
                },
                |_, _| Ok(fake),
            );
            assert!(result.is_err());
            assert_eq!(ack.get(), 0);
            assert_eq!(
                store.delivery(&event.event_id).unwrap().unwrap().phase,
                Phase::Attention
            );
            assert!(store.timer(&event.event_id).unwrap().is_none());
        }
    }

    #[test]
    fn scoped_review_duplicate_submit_is_refused_without_second_consumption() {
        let (store, event, target) = fixture("scoped-duplicate");
        let mut fake = adapter(&event, &target);
        fake.frames.push_back(json!({"stage":"BEFORE_SUBMIT"}));
        let admitted = fake.admitted.clone();
        let ack = fake.ack_count.clone();
        let consumes = Cell::new(0);
        let result = run_with_adapter(
            &store,
            &event.event_id,
            180,
            &mut |_, _| {
                consumes.set(consumes.get() + 1);
                admitted.set(true);
                Ok(())
            },
            &|| Ok(()),
            |_, _| Ok(fake),
        );
        assert_eq!(result.unwrap_err(), "SCOPED_REVIEW_DUPLICATE_SUBMIT");
        assert_eq!(consumes.get(), 1);
        assert_eq!(ack.get(), 1);
        assert_eq!(
            store.delivery(&event.event_id).unwrap().unwrap().phase,
            Phase::Submitting
        );
    }

    #[test]
    fn scoped_review_drains_exact_receipt_after_post_ack_cancellation_without_resend() {
        let (store, event, target) = fixture("scoped-after-ack");
        let mut fake = adapter(&event, &target);
        fake.cancel_after_ack = true;
        let admitted = fake.admitted.clone();
        let stop = fake.cancelled.clone();
        let ack = fake.ack_count.clone();
        run_with_adapter(
            &store,
            &event.event_id,
            180,
            &mut |_, _| {
                admitted.set(true);
                Ok(())
            },
            &|| {
                if stop.get() {
                    Err("TEST_CANCELLED".into())
                } else {
                    Ok(())
                }
            },
            |_, _| Ok(fake),
        )
        .unwrap();
        assert_eq!(ack.get(), 1);
        assert_eq!(
            store.delivery(&event.event_id).unwrap().unwrap().phase,
            Phase::Sent
        );
    }

    #[test]
    fn scoped_review_progress_cannot_extend_absolute_deadline() {
        let (store, event, target) = fixture("scoped-timeout");
        let mut fake = adapter(&event, &target);
        fake.frames = (0..100).map(|_| json!({"stage":"CHAT_IDLE"})).collect();
        fake.delay = Duration::from_millis(25);
        let ack = fake.ack_count.clone();
        let start = Instant::now();
        let result = run_with_adapter(
            &store,
            &event.event_id,
            1,
            &mut |_, _| panic!("no send stage"),
            &|| Ok(()),
            |_, _| Ok(fake),
        );
        assert_eq!(result.unwrap_err(), "SCOPED_REVIEW_TIMEOUT");
        assert!(start.elapsed() < Duration::from_secs(3));
        assert_eq!(ack.get(), 0);
    }

    #[test]
    fn scoped_review_rejects_replay_running_host_and_invalid_budget_before_adapter() {
        let (store, event, _) = fixture("scoped-preflight");
        for limit in [0, 181] {
            let result = run_with_adapter::<FakeAdapter>(
                &store,
                &event.event_id,
                limit,
                &mut |_, _| unreachable!(),
                &|| Ok(()),
                |_, _| panic!("no adapter"),
            );
            assert_eq!(result.unwrap_err(), "SCOPED_REVIEW_ARGUMENTS_INVALID");
        }
        let lease = store.host_lock().unwrap();
        let delivery = store.claim(&event, &lease).unwrap();
        store
            .transition(
                &event.event_id,
                lease.owner(),
                Phase::Submitting,
                None,
                None,
            )
            .unwrap();
        drop(lease);
        let result = run_with_adapter::<FakeAdapter>(
            &store,
            &event.event_id,
            180,
            &mut |_, _| unreachable!(),
            &|| Ok(()),
            |_, _| panic!("no adapter"),
        );
        assert_eq!(result.unwrap_err(), "SCOPED_REVIEW_REPLAY_REFUSED");
        assert_eq!(
            store.delivery(&event.event_id).unwrap().unwrap().target,
            delivery.target
        );
    }

    #[test]
    fn ordinary_attempt_refuses_scoped_event_before_adapter_request() {
        let (store, event, target) = fixture("scoped-ordinary");
        let lease = store.host_lock().unwrap();
        let delivery = store.claim(&event, &lease).unwrap();
        let mut fake = adapter(&event, &target);
        let mut status = local_status(&delivery);
        assert_eq!(
            attempt_inner(&store, &lease, &mut fake, &delivery, &mut status, None).unwrap_err(),
            "SCOPED_REVIEW_EXPLICIT_GUARD_REQUIRED"
        );
        assert_eq!(fake.ack_count.get(), 0);
        assert_eq!(fake.frames.len(), 2);
    }

    #[test]
    fn scoped_audit_reference_is_immutable_and_cannot_be_stripped_by_replay() {
        let (store, event, target) = fixture("scoped-audit");
        assert_eq!(
            store
                .produce_expected_with_audit(
                    &event.event_id,
                    "alpha",
                    "source_review",
                    &event.message,
                    &target,
                    &event.audit_references
                )
                .unwrap(),
            event
        );
        assert!(
            store
                .produce_expected(
                    &event.event_id,
                    "alpha",
                    "source_review",
                    &event.message,
                    &target
                )
                .is_err()
        );
        assert!(
            store
                .produce_expected_with_audit(
                    &event.event_id,
                    "alpha",
                    "source_review",
                    &event.message,
                    &target,
                    &["catdesk-review-v1:other".into()]
                )
                .is_err()
        );
    }

    #[test]
    fn scoped_review_wire_is_rejected_by_the_literal_legacy_event_allowlist() {
        let (_, event, _) = fixture("scoped-legacy-wire");
        event.validate().unwrap();
        let wire = serde_json::to_value(&event).unwrap();
        // This is the exact accepted-purpose set from the predecessor Event
        // reader. Audit references alone were ignored by that reader.
        let legacy_accepts = matches!(
            wire["eventType"].as_str(),
            Some("review_ready" | "engineering_attention" | "test")
        );
        assert_eq!(wire["eventType"], "source_review");
        assert!(
            !legacy_accepts,
            "an old host must not accept this queued review"
        );
    }

    #[test]
    fn scoped_review_type_without_audit_cannot_reach_either_sender() {
        let (store, _, target) = fixture("scoped-missing-audit");
        let event = store
            .produce_expected(
                "missing-review-audit",
                "alpha",
                "source_review",
                "source review without authorization",
                &target,
            )
            .unwrap();
        let result = run_with_adapter::<FakeAdapter>(
            &store,
            &event.event_id,
            180,
            &mut |_, _| panic!("missing audit must not consume a grant"),
            &|| Ok(()),
            |_, _| panic!("missing audit must not open a browser"),
        );
        assert_eq!(result.unwrap_err(), "SCOPED_REVIEW_EVENT_INVALID");
        let lease = store.host_lock().unwrap();
        let delivery = store.claim(&event, &lease).unwrap();
        let mut fake = adapter(&event, &target);
        assert_eq!(
            attempt_inner(
                &store,
                &lease,
                &mut fake,
                &delivery,
                &mut local_status(&delivery),
                None
            )
            .unwrap_err(),
            "SCOPED_REVIEW_EXPLICIT_GUARD_REQUIRED"
        );
        assert_eq!(fake.ack_count.get(), 0);
        assert_eq!(fake.frames.len(), 2);
    }

    #[test]
    fn scoped_review_can_be_retired_without_granting_native_work_or_sending() {
        let (store, event, target) = fixture("scoped-retirement");
        store
            .retire_stale_expected(
                &event.event_id,
                &event.project_id,
                &target,
                &crate::protocol::digest(&event.message),
            )
            .unwrap();
        let delivery = store.delivery(&event.event_id).unwrap().unwrap();
        assert_eq!(delivery.phase, Phase::Stale);
        assert_eq!(delivery.event.event_type, "source_review");
        assert!(delivery.receipt.is_none());
        assert!(store.timer(&event.event_id).unwrap().is_none());
    }
}
