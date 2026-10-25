//! One no-send reconciliation of an already-dispatched source review.
use super::*;
use crate::store::Delivery;
use serde_json::{Value, json};

/// The caller holds its project registration lock while checking historical
/// grant dispatch and running `commit`. Neither check grants another submit.
pub fn reconcile_scoped_review_guarded<F>(
    store: &Store,
    project: &str,
    target: &Target,
    event: &str,
    keep_running: &dyn Fn() -> Result<()>,
    with_authority: F,
) -> Result<Value>
where
    F: FnMut(&Delivery, &mut dyn FnMut() -> Result<()>) -> Result<()>,
{
    reconcile_with_adapter(
        store,
        project,
        target,
        event,
        keep_running,
        with_authority,
        Duration::from_secs(90),
        |deadline, guard| {
            let installed = installed_bootstrap_directory(store)?;
            let bytes = std::fs::read(installed.join("adapter.py"))
                .map_err(|_| "BROWSER_ADAPTER_UNAVAILABLE")?;
            if !bytes
                .windows(b"def reconcile_submission(".len())
                .any(|part| part == b"def reconcile_submission(")
                || !bytes
                    .windows(b"SENT_PENDING_RESPONSE".len())
                    .any(|part| part == b"SENT_PENDING_RESPONSE")
            {
                return Err("SCOPED_RECONCILE_PACKAGE_UNSUPPORTED".into());
            }
            Adapter::spawn_from_installed_guarded(
                store.root(),
                &installed,
                deadline.saturating_duration_since(Instant::now()),
                guard,
            )
        },
    )
}

#[allow(clippy::too_many_arguments)]
fn reconcile_with_adapter<A, F>(
    store: &Store,
    project: &str,
    target: &Target,
    event: &str,
    keep_running: &dyn Fn() -> Result<()>,
    mut with_authority: F,
    maximum: Duration,
    connect: impl FnOnce(Instant, &dyn Fn() -> Result<()>) -> Result<A>,
) -> Result<Value>
where
    A: SubmissionAdapter,
    F: FnMut(&Delivery, &mut dyn FnMut() -> Result<()>) -> Result<()>,
{
    let started = Instant::now();
    let deadline = started + maximum;
    let owner_guard = || {
        if Instant::now() >= deadline {
            return Err("SCOPED_RECONCILE_TIMEOUT".into());
        }
        if desired(store)? != "STOPPED" {
            return Err("SCOPED_RECONCILE_HOST_NOT_STOPPED".into());
        }
        Ok(())
    };
    let guard = || {
        owner_guard()?;
        keep_running()?;
        owner_guard()
    };
    guard()?;
    target.validate()?;
    if !crate::protocol::valid_id(project) || !crate::protocol::valid_id(event) {
        return Err("SCOPED_RECONCILE_ARGUMENTS_INVALID".into());
    }
    let _owner = store.host_lock()?;
    guard()?;
    let delivery = store
        .delivery(event)?
        .ok_or("SCOPED_RECONCILE_DELIVERY_MISSING")?;
    delivery.event.validate()?;
    if delivery.phase != Phase::Submitting
        || delivery.receipt.is_some()
        || delivery.event.event_id != event
        || delivery.event.project_id != project
        || delivery.event.event_type != "source_review"
        || delivery.target != *target
        || delivery.event.target_generation != target.generation
        || delivery.message_digest != crate::protocol::digest(&delivery.event.message)
        || delivery.event.audit_references.len() != 1
        || !delivery.event.audit_references[0].starts_with("catdesk-review-v1:")
        || delivery.event.audit_references[0] == "catdesk-review-v1:"
    {
        return Err("SCOPED_RECONCILE_DELIVERY_INVALID".into());
    }
    let unchanged = || -> Result<()> {
        if store.config()?.targets.get(project) != Some(target)
            || store.delivery(event)?.as_ref() != Some(&delivery)
        {
            return Err("SCOPED_RECONCILE_SCOPE_CHANGED".into());
        }
        Ok(())
    };
    unchanged()?;
    with_authority(&delivery, &mut || Ok(()))?;
    guard()?;
    let mut adapter = connect(deadline, &guard)?;
    let observed = read_receipt(&mut adapter, &delivery, deadline, &guard);
    // Cleanup shares the same total budget. Even a valid receipt does not
    // authorize mutation after cancellation or a deadline reached in cleanup.
    adapter.finish(
        deadline
            .saturating_duration_since(Instant::now())
            .min(Duration::from_secs(5)),
    );
    drop(adapter);
    let receipt = observed?;
    guard()?;
    unchanged()?;
    with_authority(&delivery, &mut || {
        // Registration lock is held; do not call the paired-target reader here.
        owner_guard()?;
        unchanged()?;
        // Both writes require positive receipt evidence. An I/O failure can
        // retain a partial positive commit; never clear it or replay the USER.
        store.record_exact_receipt_timer(event, &receipt)?;
        store.reconcile_submitting_sent(event, receipt.clone())?;
        Ok(())
    })?;
    Ok(json!({"reconciled":true,"phase":"SENT","receipt":receipt,
        "browserReadOnly":true,"sendAttempted":false,"workResolved":false,
        "hostStarted":false,"schedulerStarted":false,"desiredStateChanged":false,
        "diagnostic":{"phase":"COMPLETE","elapsedMillis":started.elapsed().as_millis(),
        "budgetMillis":maximum.as_millis()}}))
}

fn read_receipt<A: SubmissionAdapter>(
    adapter: &mut A,
    delivery: &Delivery,
    deadline: Instant,
    guard: &dyn Fn() -> Result<()>,
) -> Result<Receipt> {
    guard()?;
    // This request has no submit acknowledgment and selects the adapter's
    // existing exact persisted-receipt reader, never its typing path.
    adapter.send(json!({"reconcile":true,"event":delivery.event,
        "target":delivery.target,"cooperative":true}))?;
    let mut preliminary: Option<Receipt> = None;
    loop {
        guard()?;
        let frame = adapter.receive(
            deadline
                .saturating_duration_since(Instant::now())
                .min(Duration::from_millis(250)),
        );
        guard()?;
        match frame {
            Ok(Ok(value)) => match value.get("stage").and_then(Value::as_str) {
                Some(
                    "CDP_REUSE_BEGIN" | "CDP_REUSE_DONE" | "CDP_ACTIVATE_BEGIN"
                    | "CDP_ACTIVATE_DONE" | "TARGET_OPEN" | "PAGE_READY",
                ) => {}
                Some("USER_MESSAGE_APPENDED" | "SENT_PENDING_RESPONSE") => {
                    let receipt: Receipt = serde_json::from_value(
                        value
                            .get("receipt")
                            .ok_or("SCOPED_RECONCILE_RECEIPT_INVALID")?
                            .clone(),
                    )
                    .map_err(|_| "SCOPED_RECONCILE_RECEIPT_INVALID")?;
                    if receipt.schema_version != 1
                        || receipt.event_id != delivery.event.event_id
                        || receipt.target_generation != delivery.target.generation
                        || receipt.target_digest != delivery.target.digest
                        || receipt.message_digest != delivery.message_digest
                        || receipt.evidence != "EXACT_USER_MESSAGE_APPENDED"
                        || receipt.sent_utc == 0
                        || preliminary.as_ref().is_some_and(|prior| prior != &receipt)
                    {
                        return Err("SCOPED_RECONCILE_RECEIPT_INVALID".into());
                    }
                    if value["stage"] == "SENT_PENDING_RESPONSE" {
                        return Ok(receipt);
                    }
                    preliminary = Some(receipt);
                }
                Some("ATTENTION") => return Err("SCOPED_RECONCILE_RECEIPT_UNPROVEN".into()),
                // In particular, never answer BEFORE_SUBMIT or retry frames.
                _ => return Err("SCOPED_RECONCILE_FRAME_INVALID".into()),
            },
            Ok(Err(_)) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err("SCOPED_RECONCILE_ADAPTER_FAILED".into());
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        cell::{Cell, RefCell},
        collections::VecDeque,
        rc::Rc,
    };

    struct Reader {
        frames: VecDeque<Value>,
        requests: Rc<RefCell<Vec<Value>>>,
        finished: Rc<Cell<bool>>,
        cleanup_delay: Duration,
    }
    impl SubmissionAdapter for Reader {
        fn send(&mut self, value: Value) -> Result<()> {
            self.requests.borrow_mut().push(value);
            Ok(())
        }
        fn receive(
            &mut self,
            _: Duration,
        ) -> std::result::Result<Result<Value>, mpsc::RecvTimeoutError> {
            self.frames
                .pop_front()
                .map(Ok)
                .ok_or(mpsc::RecvTimeoutError::Disconnected)
        }
        fn finish(&mut self, _: Duration) {
            thread::sleep(self.cleanup_delay);
            self.finished.set(true);
        }
    }
    fn fixture() -> (Store, Delivery, Receipt) {
        let (store, lease) = super::super::scheduler::tests::fixture("scoped-reconcile");
        drop(lease);
        atomic(
            &store.root().join("control.json"),
            &json!({"schemaVersion":1,"desired":"STOPPED"}),
        )
        .unwrap();
        let target = store.config().unwrap().targets["alpha"].clone();
        let event = store
            .produce_expected_with_audit(
                "review-reconcile",
                "alpha",
                "source_review",
                "review source",
                &target,
                &["catdesk-review-v1:grant".into()],
            )
            .unwrap();
        let lease = store.host_lock().unwrap();
        store.claim(&event, &lease).unwrap();
        let delivery = store
            .transition(
                &event.event_id,
                lease.owner(),
                Phase::Submitting,
                None,
                None,
            )
            .unwrap();
        let receipt = Receipt {
            schema_version: 1,
            event_id: event.event_id,
            target_generation: target.generation,
            target_digest: target.digest,
            message_digest: crate::protocol::digest("review source"),
            sent_utc: now(),
            evidence: "EXACT_USER_MESSAGE_APPENDED".into(),
        };
        (store, delivery, receipt)
    }
    fn reader(frames: Vec<Value>) -> Reader {
        Reader {
            frames: frames.into(),
            requests: Rc::new(RefCell::new(vec![])),
            finished: Rc::new(Cell::new(false)),
            cleanup_delay: Duration::ZERO,
        }
    }
    fn receipt_frames(receipt: &Receipt) -> Vec<Value> {
        vec![
            json!({"stage":"USER_MESSAGE_APPENDED","receipt":receipt}),
            json!({"stage":"SENT_PENDING_RESPONSE","receipt":receipt}),
        ]
    }
    fn preserved(store: &Store, delivery: &Delivery, control: &[u8]) {
        assert_eq!(
            store.delivery(&delivery.event.event_id).unwrap().as_ref(),
            Some(delivery)
        );
        assert!(store.timer(&delivery.event.event_id).unwrap().is_none());
        assert_eq!(
            std::fs::read(store.root().join("control.json")).unwrap(),
            control
        );
    }
    #[test]
    fn exact_persisted_receipt_reconciles_once_without_submit_or_host_changes() {
        let (store, d, r) = fixture();
        let fake = reader(receipt_frames(&r));
        let requests = fake.requests.clone();
        let finished = fake.finished.clone();
        let control = std::fs::read(store.root().join("control.json")).unwrap();
        let checks = Cell::new(0);
        let result = reconcile_with_adapter(
            &store,
            "alpha",
            &d.target,
            &d.event.event_id,
            &|| Ok(()),
            |_, commit| {
                checks.set(checks.get() + 1);
                commit()
            },
            Duration::from_secs(1),
            |_, _| Ok(fake),
        )
        .unwrap();
        assert_eq!(checks.get(), 2);
        assert!(finished.get());
        assert_eq!(requests.borrow().len(), 1);
        assert_eq!(
            requests.borrow()[0],
            json!({"reconcile":true,"event":d.event,"target":d.target,"cooperative":true})
        );
        assert_eq!(result["sendAttempted"], false);
        assert_eq!(
            store.delivery(&d.event.event_id).unwrap().unwrap().phase,
            Phase::Sent
        );
        assert_eq!(
            store
                .timer(&d.event.event_id)
                .unwrap()
                .unwrap()
                .response_state,
            ResponseState::Observing
        );
        assert_eq!(
            std::fs::read(store.root().join("control.json")).unwrap(),
            control
        );
        assert_eq!(
            reconcile_with_adapter::<Reader, _>(
                &store,
                "alpha",
                &d.target,
                &d.event.event_id,
                &|| Ok(()),
                |_, _| Ok(()),
                Duration::from_secs(1),
                |_, _| panic!("already sent must not open browser")
            )
            .unwrap_err(),
            "SCOPED_RECONCILE_DELIVERY_INVALID"
        );
    }
    #[test]
    fn unknown_before_submit_and_wrong_receipt_preserve_original_delivery_and_timer() {
        for kind in ["attention", "submit", "wrong"] {
            let (store, d, mut r) = fixture();
            let control = std::fs::read(store.root().join("control.json")).unwrap();
            let frames = match kind {
                "attention" => vec![json!({"stage":"ATTENTION","reason":"PRIVATE_UNKNOWN"})],
                "submit" => vec![json!({"stage":"BEFORE_SUBMIT"})],
                _ => {
                    r.message_digest = "f".repeat(64);
                    receipt_frames(&r)
                }
            };
            let fake = reader(frames);
            let requests = fake.requests.clone();
            let finished = fake.finished.clone();
            let result = reconcile_with_adapter(
                &store,
                "alpha",
                &d.target,
                &d.event.event_id,
                &|| Ok(()),
                |_, commit| commit(),
                Duration::from_secs(1),
                |_, _| Ok(fake),
            );
            assert!(result.is_err());
            preserved(&store, &d, &control);
            assert!(finished.get());
            assert_eq!(requests.borrow().len(), 1);
            assert!(requests.borrow().iter().all(|v| v.get("submit").is_none()));
        }
    }
    #[test]
    fn wrong_target_and_denied_dispatch_never_open_the_browser() {
        let (store, d, _) = fixture();
        let control = std::fs::read(store.root().join("control.json")).unwrap();
        let wrong = Target::new(2, "https://chatgpt.com/c/wrong".into()).unwrap();
        assert!(
            reconcile_with_adapter::<Reader, _>(
                &store,
                "alpha",
                &wrong,
                &d.event.event_id,
                &|| Ok(()),
                |_, _| Ok(()),
                Duration::from_secs(1),
                |_, _| panic!("wrong target")
            )
            .is_err()
        );
        assert_eq!(
            reconcile_with_adapter::<Reader, _>(
                &store,
                "alpha",
                &d.target,
                &d.event.event_id,
                &|| Ok(()),
                |_, _| Err("DISPATCH_MISSING".into()),
                Duration::from_secs(1),
                |_, _| panic!("missing dispatch")
            )
            .unwrap_err(),
            "DISPATCH_MISSING"
        );
        preserved(&store, &d, &control);
    }
    #[test]
    fn cancellation_and_cleanup_deadline_cannot_commit_a_positive_receipt() {
        for timed in [false, true] {
            let (store, d, r) = fixture();
            let control = std::fs::read(store.root().join("control.json")).unwrap();
            let mut fake = reader(receipt_frames(&r));
            let cancelled = Cell::new(false);
            if timed {
                fake.cleanup_delay = Duration::from_millis(20)
            };
            let finished = fake.finished.clone();
            let result = reconcile_with_adapter(
                &store,
                "alpha",
                &d.target,
                &d.event.event_id,
                &|| {
                    if cancelled.get() {
                        Err("CANCELLED".into())
                    } else {
                        Ok(())
                    }
                },
                |_, commit| commit(),
                if timed {
                    Duration::from_millis(10)
                } else {
                    Duration::from_secs(1)
                },
                |_, _| {
                    if !timed {
                        cancelled.set(true)
                    };
                    Ok(fake)
                },
            );
            assert!(result.is_err());
            assert!(finished.get());
            preserved(&store, &d, &control);
        }
    }
}
