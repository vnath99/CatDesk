//! One explicit, grant-guarded event; no persistent host or scheduler.
use super::*;
use crate::protocol::Event;
use serde_json::{Value, json};

enum Selection<'a> {
    Review,
    TaskContinuation(&'a Event, &'a Target),
    RolloverContinuation(&'a Event, &'a Target),
    RolloverHandoff(&'a Event, &'a Target),
    WorkRequest(&'a Event, &'a Target),
}

struct FencedAdapter<'a, A> {
    inner: A,
    handoff: &'a mut dyn FnMut(&Event, &Target) -> Result<()>,
}
impl<A: SubmissionAdapter> SubmissionAdapter for FencedAdapter<'_, A> {
    fn submit_fenced(&mut self, event: &Event, target: &Target) -> Result<()> {
        self.inner.submit_fenced(event, target)?;
        (self.handoff)(event, target)
    }
    fn send(&mut self, value: Value) -> Result<()> {
        self.inner.send(value)
    }
    fn receive(
        &mut self,
        timeout: Duration,
    ) -> std::result::Result<Result<Value>, mpsc::RecvTimeoutError> {
        self.inner.receive(timeout)
    }
    fn finish(&mut self, grace: Duration) {
        self.inner.finish(grace)
    }
}

/// Deliver one exact ordinary invitation chosen by its central work journal.
/// Uses the existing sender/receipt protocol without starting the shared host.
pub fn run_scoped_work_request(
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
        Selection::WorkRequest(event, target),
        before_submit,
        keep_running,
        |deadline, guard| {
            let installed = installed_ordinary_browser_directory(store)?;
            Adapter::spawn_from_installed_guarded(
                store.root(),
                &installed,
                deadline.saturating_duration_since(Instant::now()),
                guard,
            )
        },
    )
}

/// Deliver only the source-handoff request selected by its retained project
/// journal owner. The shared host stays stopped and sibling queues are unread.
pub fn run_scoped_rollover_handoff(
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
        Selection::RolloverHandoff(event, target),
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

/// Deliver only the rollover event selected and revalidated by its journal
/// owner. This uses the same bounded sender and never drains sibling queues.
pub fn run_scoped_rollover_continuation(
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
        Selection::RolloverContinuation(event, target),
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
    run_scoped_task_continuation_handoff(
        store,
        event,
        target,
        maximum_seconds,
        before_submit,
        keep_running,
        &mut |_, _| Ok(()),
    )
}

/// Handoff follows durable SUBMITTING and precedes the sole native ACK.
pub fn run_scoped_task_continuation_handoff(
    store: &Store,
    event: &Event,
    target: &Target,
    maximum_seconds: u64,
    before_submit: &mut dyn FnMut(&Event, &Target) -> Result<()>,
    keep_running: &dyn Fn() -> Result<()>,
    after_fence: &mut dyn FnMut(&Event, &Target) -> Result<()>,
) -> Result<Value> {
    run_selected_with_adapter(
        store,
        &event.event_id,
        maximum_seconds,
        Selection::TaskContinuation(event, target),
        before_submit,
        keep_running,
        |deadline, guard| {
            let installed = installed_ordinary_browser_directory(store)?;
            let inner = Adapter::spawn_from_installed_guarded(
                store.root(),
                &installed,
                deadline.saturating_duration_since(Instant::now()),
                guard,
            )?;
            Ok(FencedAdapter {
                inner,
                handoff: after_fence,
            })
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
    let rollover = matches!(&selection, Selection::RolloverContinuation(..));
    let handoff = matches!(&selection, Selection::RolloverHandoff(..));
    let work = matches!(&selection, Selection::WorkRequest(..));
    let permitted = match selection {
        Selection::Review => review,
        Selection::WorkRequest(expected, target) => {
            target.validate()?;
            &event == expected
                && event.event_type == "engineering_attention"
                && event.audit_references.is_empty()
                && event.event_id.strip_prefix("web-work-").is_some_and(|id| {
                    id.len() == 64
                        && id
                            .bytes()
                            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                })
                && store.config()?.targets.get(&event.project_id) == Some(target)
        }
        Selection::RolloverHandoff(expected, target) => {
            target.validate()?;
            &event == expected
                && event.event_type == "engineering_attention"
                && event.audit_references.is_empty()
                && event
                    .event_id
                    .strip_prefix("web-handoff-")
                    .is_some_and(crate::protocol::valid_id)
                && store.config()?.targets.get(&event.project_id) == Some(target)
        }
        Selection::RolloverContinuation(expected, target) => {
            target.validate()?;
            &event == expected
                && event.event_type == "engineering_attention"
                && event.audit_references.is_empty()
                && event
                    .event_id
                    .strip_prefix("web-continue-")
                    .is_some_and(crate::protocol::valid_id)
                && store.config()?.targets.get(&event.project_id) == Some(target)
        }
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
        json!({"action":if review {"scoped_review_run"} else if handoff {"scoped_rollover_handoff_run"} else if rollover {"scoped_rollover_continuation_run"} else if work {"scoped_work_request_run"} else {"scoped_task_continuation_run"},"eventId":event_id,"projectId":event.project_id,
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
        terminal_refusal: Option<Value>,
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
                let terminal = self.terminal_refusal.clone().unwrap_or_else(
                    || json!({"stage":"SENT_PENDING_RESPONSE","receipt":self.receipt}),
                );
                self.frames.push_back(terminal);
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

    fn ordinary_package(store: &Store) -> PathBuf {
        let directory = store.root().join("versions/ordinary-v1");
        std::fs::create_dir_all(&directory).unwrap();
        let host = b"synthetic host bytes; never executed";
        let adapter = b"synthetic ordinary acknowledged-submit adapter; never executed";
        let bridge = b"synthetic ordinary receipt primitives; never executed";
        for (name, bytes) in [
            ("CatDeskWakeHost.exe", host.as_slice()),
            ("adapter.py", adapter.as_slice()),
            ("wake_bridge.py", bridge.as_slice()),
        ] {
            std::fs::write(directory.join(name), bytes).unwrap();
        }
        let host_hash = crate::protocol::digest(std::str::from_utf8(host).unwrap());
        let adapter_hash = crate::protocol::digest(std::str::from_utf8(adapter).unwrap());
        let bridge_hash = crate::protocol::digest(std::str::from_utf8(bridge).unwrap());
        atomic(
            &store.root().join("current.json"),
            &json!({
                "schemaVersion":1,"directory":"ordinary-v1","hostSha256":host_hash,
                "adapterSha256":adapter_hash,"wakeBridgeSha256":bridge_hash
            }),
        )
        .unwrap();
        atomic(&directory.join("manifest.json"), &json!({
            "schemaVersion":1,"protocolVersion":1,"artifacts":{
                "CatDeskWakeHost.exe":host_hash,"adapter.py":adapter_hash,"wake_bridge.py":bridge_hash
            }
        })).unwrap();
        directory
    }

    #[test]
    fn ordinary_package_does_not_require_new_chat_but_bootstrap_still_does() {
        let (store, event, _) = fixture("ordinary-v1-capability");
        let directory = ordinary_package(&store);
        let paths = [
            store.root().join("current.json"),
            store.root().join("config.json"),
            store.root().join("control.json"),
            store
                .root()
                .join("queue")
                .join(format!("{}.json", event.event_id)),
        ];
        let before: Vec<_> = paths.iter().map(|p| std::fs::read(p).unwrap()).collect();
        assert_eq!(
            installed_ordinary_browser_directory(&store).unwrap(),
            directory
        );
        assert_eq!(
            installed_bootstrap_directory(&store).unwrap_err(),
            "BOOTSTRAP_INSTALLED_PACKAGE_UNSUPPORTED"
        );
        assert!(store.delivery(&event.event_id).unwrap().is_none());
        for (path, bytes) in paths.iter().zip(before) {
            assert_eq!(std::fs::read(path).unwrap(), bytes);
        }
    }

    #[test]
    fn ordinary_package_keeps_host_adapter_and_manifest_hash_checks() {
        for changed in [
            "CatDeskWakeHost.exe",
            "adapter.py",
            "wake_bridge.py",
            "manifest.json",
        ] {
            let (store, _, _) = fixture(&format!("ordinary-integrity-{changed}"));
            let directory = ordinary_package(&store);
            if changed == "manifest.json" {
                let mut manifest: Value = read(&directory.join(changed)).unwrap();
                manifest["artifacts"]["adapter.py"] = json!("0".repeat(64));
                atomic(&directory.join(changed), &manifest).unwrap();
            } else {
                std::fs::write(directory.join(changed), b"unexpected bytes").unwrap();
            }
            assert!(
                installed_ordinary_browser_directory(&store).is_err(),
                "{changed}"
            );
        }
    }

    #[test]
    fn ordinary_package_rejects_unknown_or_missing_protocol() {
        for protocol in [Value::Null, json!(2), json!("1")] {
            let (store, _, _) = fixture("ordinary-protocol");
            let directory = ordinary_package(&store);
            let mut manifest: Value = read(&directory.join("manifest.json")).unwrap();
            manifest["protocolVersion"] = protocol;
            atomic(&directory.join("manifest.json"), &manifest).unwrap();
            assert_eq!(
                installed_ordinary_browser_directory(&store).unwrap_err(),
                "ORDINARY_INSTALLED_PACKAGE_UNSUPPORTED"
            );
        }
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
            terminal_refusal: None,
        }
    }

    fn work_event(store: &Store, project: &str, target: &Target) -> Event {
        store
            .produce_expected(
                &format!("web-work-{}", crate::protocol::digest(project)),
                project,
                "engineering_attention",
                "Existing journal-owned work invitation",
                target,
            )
            .unwrap()
    }

    #[test]
    fn scoped_readiness_failure_is_retained_for_its_event_without_blocking_a_sibling() {
        let (store, event, target) = fixture("scoped-readiness-failure");
        let beta_target = store.config().unwrap().targets["beta"].clone();
        let beta = work_event(&store, "beta", &beta_target);
        let control = std::fs::read(store.root().join("control.json")).unwrap();
        let status_before = std::fs::read(store.root().join("status.json")).ok();
        let mut fake = adapter(&event, &target);
        let ack = fake.ack_count.clone();
        fake.frames = VecDeque::from([
            json!({"stage":"READINESS_OBSERVED","route":"AUTH","reason":"LOGIN_OR_PROFILE_REQUIRED"}),
            json!({"stage":"ATTENTION","reason":"LOGIN_OR_PROFILE_REQUIRED"}),
        ]);
        assert_eq!(
            run_selected_with_adapter(
                &store,
                &event.event_id,
                120,
                Selection::Review,
                &mut |_, _| panic!("readiness failure cannot authorize input"),
                &|| Ok(()),
                |_, _| Ok(fake),
            )
            .unwrap_err(),
            "LOGIN_OR_PROFILE_REQUIRED"
        );
        assert_eq!(ack.get(), 0);
        let delivery = store.delivery(&event.event_id).unwrap().unwrap();
        assert_eq!(delivery.phase, Phase::Attention);
        assert_eq!(delivery.target, target);
        assert_eq!(delivery.event, event);
        assert!(delivery.receipt.is_none());
        assert!(store.timer(&event.event_id).unwrap().is_none());
        let history = readiness_history(&store).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(
            history[0].event_id.as_deref(),
            Some(event.event_id.as_str())
        );
        assert_eq!(history[0].route, "AUTH");
        assert_eq!(history[0].reason, "LOGIN_OR_PROFILE_REQUIRED");

        let mut sibling = adapter(&beta, &beta_target);
        let admitted = sibling.admitted.clone();
        sibling
            .frames
            .push_front(json!({"stage":"READINESS_OBSERVED",
            "route":"SAME_CONVERSATION","reason":"READY"}));
        assert_eq!(
            run_selected_with_adapter(
                &store,
                &beta.event_id,
                120,
                Selection::WorkRequest(&beta, &beta_target),
                &mut |_, _| {
                    admitted.set(true);
                    Ok(())
                },
                &|| Ok(()),
                |_, _| Ok(sibling),
            )
            .unwrap()["phase"],
            "SENT"
        );
        let history = readiness_history(&store).unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history[1].event_id.as_deref(), Some(beta.event_id.as_str()));
        assert_eq!(history[1].route, "SAME_CONVERSATION");
        assert_eq!(history[1].reason, "READY");
        assert_eq!(store.delivery(&event.event_id).unwrap().unwrap(), delivery);
        assert_eq!(
            std::fs::read(store.root().join("control.json")).unwrap(),
            control
        );
        assert_eq!(
            std::fs::read(store.root().join("status.json")).ok(),
            status_before
        );
        let bytes = std::fs::read_to_string(readiness_history_path(&store)).unwrap();
        assert!(!bytes.contains(&event.message));
        assert!(!bytes.contains(&target.url));
    }

    #[test]
    fn scoped_readiness_rejects_untrusted_or_corrupt_history_before_input() {
        for corrupt in [false, true] {
            let (store, event, target) = fixture("scoped-readiness-invalid");
            let mut fake = adapter(&event, &target);
            let ack = fake.ack_count.clone();
            if corrupt {
                std::fs::write(readiness_history_path(&store), b"preserved invalid journal")
                    .unwrap();
            }
            let old_history = std::fs::read(readiness_history_path(&store)).ok();
            fake.frames = VecDeque::from([json!({"stage":"READINESS_OBSERVED",
                "route":if corrupt {"AUTH"} else {"https://private.example/route"},
                "reason":"LOGIN_OR_PROFILE_REQUIRED"})]);
            assert!(
                run_selected_with_adapter(
                    &store,
                    &event.event_id,
                    120,
                    Selection::Review,
                    &mut |_, _| panic!("untrusted readiness cannot authorize input"),
                    &|| Ok(()),
                    |_, _| Ok(fake),
                )
                .is_err()
            );
            assert_eq!(ack.get(), 0);
            assert_eq!(
                std::fs::read(readiness_history_path(&store)).ok(),
                old_history
            );
            assert!(
                store
                    .delivery(&event.event_id)
                    .unwrap()
                    .unwrap()
                    .receipt
                    .is_none()
            );
            assert!(store.timer(&event.event_id).unwrap().is_none());
        }
    }

    #[test]
    fn scoped_work_request_unknown_alpha_does_not_block_beta_and_never_replays() {
        let (store, review, alpha) = fixture("work-independent");
        let beta = store.config().unwrap().targets["beta"].clone();
        let alpha_event = work_event(&store, "alpha", &alpha);
        let beta_event = work_event(&store, "beta", &beta);
        let foreign = store.root().join("queue/gamma-unreadable.json");
        std::fs::write(&foreign, b"unrelated queue must stay unread").unwrap();
        let control = std::fs::read(store.root().join("control.json")).unwrap();
        for (event, target, unknown) in [(&alpha_event, &alpha, true), (&beta_event, &beta, false)]
        {
            let mut fake = adapter(event, target);
            if unknown {
                fake.receipt.message_digest = "0".repeat(64);
            }
            let admitted = fake.admitted.clone();
            let acks = fake.ack_count.clone();
            let result = run_selected_with_adapter(
                &store,
                &event.event_id,
                120,
                Selection::WorkRequest(event, target),
                &mut |actual, actual_target| {
                    assert_eq!(actual, event);
                    assert_eq!(actual_target, target);
                    admitted.set(true);
                    Ok(())
                },
                &|| Ok(()),
                |_, _| Ok(fake),
            );
            assert_eq!(result.is_err(), unknown);
            if !unknown {
                assert_eq!(result.unwrap()["action"], "scoped_work_request_run");
            }
            assert_eq!(acks.get(), 1);
            assert_eq!(
                store.delivery(&event.event_id).unwrap().unwrap().phase,
                if unknown {
                    Phase::Submitting
                } else {
                    Phase::Sent
                }
            );
            assert_eq!(
                run_selected_with_adapter::<FakeAdapter>(
                    &store,
                    &event.event_id,
                    120,
                    Selection::WorkRequest(event, target),
                    &mut |_, _| panic!("no replay admission"),
                    &|| Ok(()),
                    |_, _| panic!("no replay browser")
                )
                .unwrap_err(),
                "SCOPED_REVIEW_REPLAY_REFUSED"
            );
        }
        assert!(store.delivery(&review.event_id).unwrap().is_none());
        assert_eq!(
            std::fs::read(&foreign).unwrap(),
            b"unrelated queue must stay unread"
        );
        assert_eq!(
            std::fs::read(store.root().join("control.json")).unwrap(),
            control
        );
    }

    #[test]
    fn scoped_work_request_refuses_changed_roles_witness_and_target_before_claim() {
        for change in ["role", "prefix", "audit", "message", "project", "target"] {
            let (store, _, target) = fixture(&format!("work-reject-{change}"));
            let mut event = work_event(&store, "alpha", &target);
            let mut expected = event.clone();
            let mut selected_target = target.clone();
            match change {
                "role" => event.event_type = "source_review".into(),
                "prefix" => event.event_id = "web-work-not-a-digest".into(),
                "audit" => event
                    .audit_references
                    .push("catdesk-review-v1:other".into()),
                "message" => expected.message.push_str(" changed"),
                "project" => expected.project_id = "beta".into(),
                "target" => selected_target.generation += 1,
                _ => unreachable!(),
            }
            if matches!(change, "role" | "prefix" | "audit") {
                expected = event.clone();
                std::fs::write(
                    store
                        .root()
                        .join("queue")
                        .join(format!("{}.json", event.event_id)),
                    serde_json::to_vec(&event).unwrap(),
                )
                .unwrap();
            }
            assert_eq!(
                run_selected_with_adapter::<FakeAdapter>(
                    &store,
                    &event.event_id,
                    120,
                    Selection::WorkRequest(&expected, &selected_target),
                    &mut |_, _| panic!("no invalid admission"),
                    &|| Ok(()),
                    |_, _| panic!("no invalid browser")
                )
                .unwrap_err(),
                "SCOPED_REVIEW_EVENT_INVALID",
                "{change}"
            );
            assert!(store.delivery(&event.event_id).unwrap().is_none());
        }
    }

    #[test]
    fn scoped_work_request_lost_owner_before_submit_never_acknowledges() {
        let (store, _, target) = fixture("work-owner-lost");
        let event = work_event(&store, "alpha", &target);
        let fake = adapter(&event, &target);
        let acks = fake.ack_count.clone();
        assert_eq!(
            run_selected_with_adapter(
                &store,
                &event.event_id,
                120,
                Selection::WorkRequest(&event, &target),
                &mut |_, _| Err("WEB_WORK_PROJECT_REGISTRATION_CHANGED".into()),
                &|| Ok(()),
                |_, _| Ok(fake)
            )
            .unwrap_err(),
            "WEB_WORK_PROJECT_REGISTRATION_CHANGED"
        );
        assert_eq!(acks.get(), 0);
        assert_ne!(
            store.delivery(&event.event_id).unwrap().unwrap().phase,
            Phase::Sent
        );
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
    fn scoped_pre_edit_refusal_records_only_fresh_bound_before_input_evidence() {
        for case in ["valid", "legacy", "foreign", "before-ack", "accepted"] {
            let (store, event, target) = fixture(case);
            let mut frame = json!({"stage":"ATTENTION","reason":"CONVERSATION_DRAFT_INSERT_UNVERIFIED_PRE_STRUCTURE",
                "preEditRefusal":{"schemaVersion":1,"eventId":event.event_id,"projectId":event.project_id,
                    "targetGeneration":target.generation,"targetDigest":target.digest,
                    "messageDigest":crate::protocol::digest(&event.message),"nativeInputAttempted":false,
                    "composerStructure":{"schemaVersion":1,"reason":"ATTRIBUTE_STYLE","nodesExamined":2}}});
            if case == "legacy" {
                frame.as_object_mut().unwrap().remove("preEditRefusal");
            }
            if case == "foreign" {
                frame["preEditRefusal"]["projectId"] = json!("beta");
            }
            let mut fake = adapter(&event, &target);
            if case == "before-ack" {
                fake.frames = VecDeque::from([frame.clone()]);
            }
            if case == "accepted" {
                fake.frames
                    .push_back(json!({"stage":"SUBMISSION_ACCEPTED"}));
            }
            fake.terminal_refusal = Some(frame);
            let admitted = fake.admitted.clone();
            let acks = fake.ack_count.clone();
            let result = run_selected_with_adapter(
                &store,
                &event.event_id,
                10,
                Selection::Review,
                &mut |_, _| {
                    admitted.set(true);
                    Ok(())
                },
                &|| Ok(()),
                |_, _| Ok(fake),
            );
            assert!(result.is_err());
            assert_eq!(acks.get(), u32::from(case != "before-ack"));
            let proof = store
                .root()
                .join("logs")
                .join(format!("pre-edit-refusal-{}.json", event.event_id));
            assert_eq!(proof.exists(), case == "valid");
            let delivery = store.delivery(&event.event_id).unwrap().unwrap();
            assert!(delivery.receipt.is_none());
            assert_eq!(
                delivery.phase,
                if case == "before-ack" {
                    Phase::Attention
                } else {
                    Phase::Submitting
                }
            );
            if case == "valid" {
                store
                    .retire_stale_expected(
                        &event.event_id,
                        "alpha",
                        &target,
                        &crate::protocol::digest(&event.message),
                    )
                    .unwrap();
                assert_eq!(
                    store
                        .delivery(&event.event_id)
                        .unwrap()
                        .unwrap()
                        .reason
                        .as_deref(),
                    Some("PRE_EDIT_REFUSED_NO_SEND")
                );
                assert!(store.retry_pre_submit(&event.event_id).is_err());
            } else if case != "before-ack" {
                assert!(
                    store
                        .retire_stale_expected(
                            &event.event_id,
                            "alpha",
                            &target,
                            &crate::protocol::digest(&event.message)
                        )
                        .is_err()
                );
            }
        }
    }

    fn rollover_selection<'a>(
        handoff: bool,
        event: &'a Event,
        target: &'a Target,
    ) -> Selection<'a> {
        if handoff {
            Selection::RolloverHandoff(event, target)
        } else {
            Selection::RolloverContinuation(event, target)
        }
    }

    #[test]
    fn scoped_rollover_events_isolated_delivery_and_unknown_outcome_never_replay() {
        for handoff in [false, true] {
            for unknown in [false, true] {
                let (store, review, target) = fixture(if unknown {
                    "rollover-unknown"
                } else {
                    "rollover-exact"
                });
                let event = store
                    .produce_expected(
                        if handoff {
                            "web-handoff-stable-intent"
                        } else {
                            "web-continue-stable-intent"
                        },
                        "alpha",
                        "engineering_attention",
                        "Continue only the journal-owned rollover",
                        &target,
                    )
                    .unwrap();
                let foreign = store.root().join("queue/beta-unreadable.json");
                std::fs::write(&foreign, b"preserve unrelated queue").unwrap();
                let control = std::fs::read(store.root().join("control.json")).unwrap();
                let mut fake = adapter(&event, &target);
                if unknown {
                    fake.receipt.message_digest = "0".repeat(64);
                }
                let admitted = fake.admitted.clone();
                let acknowledgments = fake.ack_count.clone();
                let result = run_selected_with_adapter(
                    &store,
                    &event.event_id,
                    120,
                    rollover_selection(handoff, &event, &target),
                    &mut |actual, actual_target| {
                        assert_eq!(actual, &event);
                        assert_eq!(actual_target, &target);
                        admitted.set(true);
                        Ok(())
                    },
                    &|| Ok(()),
                    |_, _| Ok(fake),
                );
                assert_eq!(result.is_err(), unknown);
                if !unknown {
                    assert_eq!(
                        result.unwrap()["action"],
                        if handoff {
                            "scoped_rollover_handoff_run"
                        } else {
                            "scoped_rollover_continuation_run"
                        }
                    );
                }
                assert_eq!(acknowledgments.get(), 1);
                assert!(store.delivery(&review.event_id).unwrap().is_none());
                assert_eq!(
                    std::fs::read(&foreign).unwrap(),
                    b"preserve unrelated queue"
                );
                assert_eq!(
                    std::fs::read(store.root().join("control.json")).unwrap(),
                    control
                );
                assert_eq!(
                    run_selected_with_adapter::<FakeAdapter>(
                        &store,
                        &event.event_id,
                        120,
                        rollover_selection(handoff, &event, &target),
                        &mut |_, _| panic!("no replay authorization"),
                        &|| Ok(()),
                        |_, _| panic!("no replay browser"),
                    )
                    .unwrap_err(),
                    "SCOPED_REVIEW_REPLAY_REFUSED"
                );
                assert_eq!(acknowledgments.get(), 1);
            }
        }
    }

    #[test]
    fn scoped_rollover_events_refuse_changed_selection_and_owner_before_ack() {
        for handoff in [false, true] {
            for changed in ["body", "target", "owner"] {
                let (store, _, target) = fixture(&format!("rollover-{changed}"));
                let event = store
                    .produce_expected(
                        if handoff {
                            "web-handoff-intent"
                        } else {
                            "web-continue-intent"
                        },
                        "alpha",
                        "engineering_attention",
                        "Continue exact rollover",
                        &target,
                    )
                    .unwrap();
                let mut expected = event.clone();
                let mut selected_target = target.clone();
                if changed == "body" {
                    expected.message.push_str(" altered");
                }
                if changed == "target" {
                    selected_target.generation += 1;
                }
                let fake = adapter(&event, &target);
                let acknowledgments = fake.ack_count.clone();
                let result = run_selected_with_adapter(
                    &store,
                    &event.event_id,
                    120,
                    rollover_selection(handoff, &expected, &selected_target),
                    &mut |_, _| Err("JOURNAL_OWNER_CHANGED".into()),
                    &|| Ok(()),
                    |_, _| {
                        assert_eq!(changed, "owner");
                        Ok(fake)
                    },
                );
                assert!(result.is_err());
                assert_eq!(acknowledgments.get(), 0);
            }
        }
    }

    #[test]
    fn scoped_handoff_refuses_other_event_roles_before_browser_or_claim() {
        for changed in ["continuation", "review", "audit"] {
            let (store, review, target) = fixture(&format!("handoff-role-{changed}"));
            let mut event = if changed == "review" {
                review
            } else {
                store
                    .produce_expected(
                        if changed == "continuation" {
                            "web-continue-intent"
                        } else {
                            "web-handoff-intent"
                        },
                        "alpha",
                        "engineering_attention",
                        "Frozen handoff",
                        &target,
                    )
                    .unwrap()
            };
            if changed == "audit" {
                event
                    .audit_references
                    .push("catdesk-review-v1:foreign".into());
                atomic(
                    &store
                        .root()
                        .join("queue")
                        .join(format!("{}.json", event.event_id)),
                    &event,
                )
                .unwrap();
            }
            assert_eq!(
                run_selected_with_adapter::<FakeAdapter>(
                    &store,
                    &event.event_id,
                    120,
                    Selection::RolloverHandoff(&event, &target),
                    &mut |_, _| panic!("invalid selection must not authorize"),
                    &|| Ok(()),
                    |_, _| panic!("invalid selection must not open browser"),
                )
                .unwrap_err(),
                "SCOPED_REVIEW_EVENT_INVALID"
            );
            assert!(store.delivery(&event.event_id).unwrap().is_none());
        }
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
    fn task_continuation_handoff_follows_durable_fence_and_precedes_the_only_ack() {
        for deny in [false, true] {
            let (store, _, target) = fixture(if deny {
                "handoff-denied"
            } else {
                "handoff-admitted"
            });
            let event = continuation(&store, &target);
            let fake = adapter(&event, &target);
            let admitted = fake.admitted.clone();
            let ack = fake.ack_count.clone();
            let handoffs = Cell::new(0);
            let mut handoff = |actual: &Event, actual_target: &Target| {
                assert!(admitted.get());
                assert_eq!(ack.get(), 0);
                assert_eq!(actual, &event);
                assert_eq!(actual_target, &target);
                let delivery = store.delivery(&event.event_id)?.unwrap();
                store.require_submitting_witness(&delivery)?;
                handoffs.set(handoffs.get() + 1);
                if deny {
                    Err("TEST_HANDOFF_REFUSED".into())
                } else {
                    Ok(())
                }
            };
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
                |_, _| {
                    Ok(FencedAdapter {
                        inner: fake,
                        handoff: &mut handoff,
                    })
                },
            );
            assert_eq!(handoffs.get(), 1);
            assert_eq!(ack.get(), u32::from(!deny));
            let delivery = store.delivery(&event.event_id).unwrap().unwrap();
            if deny {
                assert_eq!(result.unwrap_err(), "TEST_HANDOFF_REFUSED");
                assert_eq!(delivery.phase, Phase::Submitting);
                assert!(delivery.receipt.is_none());
                assert!(store.timer(&event.event_id).unwrap().is_none());
                assert_eq!(
                    run_selected_with_adapter::<FakeAdapter>(
                        &store,
                        &event.event_id,
                        120,
                        Selection::TaskContinuation(&event, &target),
                        &mut |_, _| panic!("cannot authorize replay"),
                        &|| Ok(()),
                        |_, _| panic!("cannot start replay"),
                    )
                    .unwrap_err(),
                    "SCOPED_REVIEW_REPLAY_REFUSED"
                );
            } else {
                assert_eq!(result.unwrap()["phase"], "SENT");
                assert!(delivery.receipt.is_some());
            }
        }
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
