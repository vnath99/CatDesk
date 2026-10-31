//! Private IPC for an operator-granted task context in its existing chat.
//! The caller owns the durable task journal and grant checks. No queue record,
//! status file or diagnostic here retains the message or its execution key.
use super::*;
use serde_json::{Value, json};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskContextOperation {
    Submit,
    /// An uncertain submission may only inspect the original exact USER turn.
    Reconcile,
}

pub struct TaskContextSubmission<'a> {
    pub project_id: &'a str,
    pub event_id: &'a str,
    pub target: &'a Target,
    pub message: &'a str,
    pub operation: TaskContextOperation,
    pub maximum_seconds: u64,
}

/// The caller must durably fence this exact task/payload before entering this
/// function and persist SUBMITTING in before_submit before it returns success.
/// A returned error never grants a replacement submission. Reconcile sends no
/// submit acknowledgement, edits no composer and cannot retry an assistant.
pub fn run_private_task_context(
    store: &Store,
    request: &TaskContextSubmission<'_>,
    before_submit: &mut dyn FnMut() -> Result<()>,
    keep_running: &dyn Fn() -> Result<()>,
) -> Result<Receipt> {
    run_with_adapter(
        store,
        request,
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
    request: &TaskContextSubmission<'_>,
    before_submit: &mut dyn FnMut() -> Result<()>,
    keep_running: &dyn Fn() -> Result<()>,
    connect: impl FnOnce(Instant, &dyn Fn() -> Result<()>) -> Result<A>,
) -> Result<Receipt> {
    let TaskContextSubmission {
        project_id,
        event_id,
        target,
        message,
        operation,
        maximum_seconds,
    } = *request;
    if !crate::protocol::valid_id(project_id)
        || !crate::protocol::valid_id(event_id)
        || !event_id.starts_with("task-context-")
        || !(1..=180).contains(&maximum_seconds)
        || message.is_empty()
        || message.len() > 16 * 1024
        || message
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
    {
        return Err("TASK_CONTEXT_ARGUMENTS_INVALID".into());
    }
    target.validate()?;
    let message_digest =
        crate::protocol::digest(&message.split_whitespace().collect::<Vec<_>>().join(" "));
    let mut wire = json!({"cooperative":true,"event":{"eventId":event_id,
        "projectId":project_id,"targetGeneration":target.generation,"message":message},"target":target});
    if operation == TaskContextOperation::Reconcile {
        wire["reconcile"] = json!(true);
    }
    if serde_json::to_vec(&wire)
        .map_err(|_| "TASK_CONTEXT_ARGUMENTS_INVALID")?
        .len()
        + 1
        > 32 * 1024
    {
        return Err("TASK_CONTEXT_ARGUMENTS_INVALID".into());
    }
    let deadline = Instant::now() + Duration::from_secs(maximum_seconds);
    let guard = || {
        if Instant::now() >= deadline {
            return Err("TASK_CONTEXT_TIMEOUT".into());
        }
        keep_running()?;
        if desired(store)? != "STOPPED" {
            return Err("TASK_CONTEXT_HOST_NOT_STOPPED".into());
        }
        if store.config()?.targets.get(project_id) != Some(target) {
            return Err("TASK_CONTEXT_TARGET_CHANGED".into());
        }
        crate::development_runtime::validate_target_isolation_for_store(store.root(), &target.url)
    };
    guard()?;
    let _host = store.host_lock()?;
    guard()?;
    if operation == TaskContextOperation::Submit
        && store
            .timers_for_project(project_id)?
            .iter()
            .any(|timer| timer.response_state != ResponseState::Complete)
    {
        return Err("TASK_CONTEXT_PROJECT_BUSY".into());
    }
    let mut adapter = connect(deadline, &guard)?;
    let mut submitted = false;
    let mut witnessed = None;
    let result: Result<Receipt> = (|| {
        guard()?;
        adapter.send(wire)?;
        loop {
            guard()?;
            let frame = match adapter.receive(Duration::from_millis(100)) {
                Ok(value) => value?,
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err("TASK_CONTEXT_ADAPTER_EXITED".into());
                }
            };
            match frame.get("stage").and_then(Value::as_str) {
                Some("BEFORE_SUBMIT")
                    if operation == TaskContextOperation::Submit && !submitted =>
                {
                    if frame.as_object().is_none_or(|v| v.len() != 1) {
                        return Err("TASK_CONTEXT_FRAME_INVALID".into());
                    }
                    guard()?;
                    before_submit()?;
                    // Fence before ACK, including a failed/ambiguous pipe write.
                    submitted = true;
                    guard()?;
                    adapter.send(json!({"submit":true}))?;
                }
                Some(stage @ ("USER_MESSAGE_APPENDED" | "SENT_PENDING_RESPONSE"))
                    if submitted || operation == TaskContextOperation::Reconcile =>
                {
                    if frame.as_object().is_none_or(|v| v.len() != 2) {
                        return Err("TASK_CONTEXT_RECEIPT_INVALID".into());
                    }
                    let receipt: Receipt = serde_json::from_value(frame["receipt"].clone())
                        .map_err(|_| "TASK_CONTEXT_RECEIPT_INVALID")?;
                    if receipt.schema_version != 1
                        || receipt.event_id != event_id
                        || receipt.target_generation != target.generation
                        || receipt.target_digest != target.digest
                        || receipt.message_digest != message_digest
                        || receipt.sent_utc == 0
                        || receipt.evidence != "EXACT_USER_MESSAGE_APPENDED"
                    {
                        return Err("TASK_CONTEXT_RECEIPT_INVALID".into());
                    }
                    if let Some(prior) = &witnessed
                        && prior != &frame["receipt"]
                    {
                        return Err("TASK_CONTEXT_RECEIPT_INVALID".into());
                    }
                    if stage == "SENT_PENDING_RESPONSE" && witnessed.is_none() {
                        return Err("TASK_CONTEXT_RECEIPT_INVALID".into());
                    }
                    witnessed = Some(frame["receipt"].clone());
                    if stage == "SENT_PENDING_RESPONSE" {
                        return Ok(receipt);
                    }
                }
                Some("READINESS_OBSERVED") => {
                    readiness_observation(&frame)?;
                }
                Some("ATTENTION") => {
                    let reason = match frame.get("reason").and_then(Value::as_str) {
                        Some("LOGIN_OR_PROFILE_REQUIRED") => "LOGIN_OR_PROFILE_REQUIRED",
                        Some("EXISTING_DRAFT") => "EXISTING_DRAFT",
                        Some("CHATGPT_NOT_IDLE") => "CHATGPT_NOT_IDLE",
                        Some("EDITOR_SELECTOR") => "EDITOR_SELECTOR",
                        _ => "TASK_CONTEXT_BROWSER_ATTENTION",
                    };
                    return Err(reason.into());
                }
                Some(
                    "CDP_ACTIVATE_BEGIN"
                    | "CDP_ACTIVATE_DONE"
                    | "CDP_REUSE_BEGIN"
                    | "CDP_REUSE_DONE"
                    | "TARGET_OPEN"
                    | "PAGE_READY"
                    | "CHAT_IDLE"
                    | "PREWRITE_READY"
                    | "RECEIPT_BASELINE_READY"
                    | "PREWRITE_CONFIRMED"
                    | "SUBMISSION_ACCEPTED",
                ) => {}
                _ => return Err("TASK_CONTEXT_FRAME_INVALID".into()),
            }
        }
    })();
    adapter.finish(
        deadline
            .saturating_duration_since(Instant::now())
            .min(Duration::from_secs(5)),
    );
    result.map_err(|reason| {
        if submitted {
            return "TASK_CONTEXT_OUTCOME_UNKNOWN".into();
        }
        if operation == TaskContextOperation::Reconcile {
            return "TASK_CONTEXT_RECONCILIATION_UNPROVEN".into();
        }
        match reason.as_str() {
            "TASK_CONTEXT_TIMEOUT"
            | "TASK_CONTEXT_HOST_NOT_STOPPED"
            | "TASK_CONTEXT_TARGET_CHANGED"
            | "TASK_CONTEXT_RECEIPT_INVALID"
            | "TASK_CONTEXT_FRAME_INVALID"
            | "TASK_CONTEXT_BROWSER_ATTENTION"
            | "TASK_CONTEXT_ADAPTER_EXITED"
            | "LOGIN_OR_PROFILE_REQUIRED"
            | "EXISTING_DRAFT"
            | "CHATGPT_NOT_IDLE"
            | "EDITOR_SELECTOR"
            | "WEB_TASK_GRANT_REVOKED"
            | "WEB_TASK_GRANT_SCOPE_MISMATCH"
            | "WEB_WORK_PROJECT_REGISTRATION_CHANGED"
            | "WEB_TASK_CONTEXT_SOURCE_CHANGED" => reason,
            _ => "TASK_CONTEXT_PRE_SUBMIT_UNPROVEN".into(),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        cell::{Cell, RefCell},
        collections::VecDeque,
        rc::Rc,
    };

    struct Fake {
        frames: VecDeque<Value>,
        sent: Rc<RefCell<Vec<Value>>>,
        fail_ack: bool,
        admitted: Rc<Cell<bool>>,
        after_ack: Option<Rc<Cell<bool>>>,
    }
    impl SubmissionAdapter for Fake {
        fn send(&mut self, frame: Value) -> Result<()> {
            if frame == json!({"submit":true}) {
                assert!(self.admitted.get(), "durable caller fence precedes ACK");
                if let Some(flag) = &self.after_ack {
                    flag.set(true);
                }
                if self.fail_ack {
                    return Err("PRIVATE_PIPE_ERROR_MUST_NOT_EXPORT".into());
                }
            }
            self.sent.borrow_mut().push(frame);
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
    }
    fn fixture(name: &str) -> (Store, Target) {
        let (store, lease) = super::super::scheduler::tests::fixture(name);
        drop(lease);
        atomic(
            &store.root().join("control.json"),
            &json!({"schemaVersion":1,"desired":"STOPPED"}),
        )
        .unwrap();
        let target = store.config().unwrap().targets["alpha"].clone();
        (store, target)
    }
    fn request(target: &Target) -> TaskContextSubmission<'_> {
        TaskContextSubmission {
            project_id: "alpha",
            event_id: "task-context-immutable-one",
            target,
            message: "CatDesk private assigned-task context. PRIVATE_EXECUTION_KEY",
            operation: TaskContextOperation::Submit,
            maximum_seconds: 5,
        }
    }
    fn receipt(request: &TaskContextSubmission<'_>) -> Value {
        json!({"schemaVersion":1,"eventId":request.event_id,"targetGeneration":request.target.generation,
            "targetDigest":request.target.digest,"messageDigest":crate::protocol::digest(request.message),
            "sentUtc":now(),"evidence":"EXACT_USER_MESSAGE_APPENDED"})
    }
    fn fake(frames: Vec<Value>) -> Fake {
        Fake {
            frames: frames.into(),
            sent: Rc::new(RefCell::new(vec![])),
            fail_ack: false,
            admitted: Rc::new(Cell::new(false)),
            after_ack: None,
        }
    }
    fn acknowledgments(sent: &Rc<RefCell<Vec<Value>>>) -> usize {
        sent.borrow()
            .iter()
            .filter(|v| *v == &json!({"submit":true}))
            .count()
    }
    #[test]
    fn context_uses_existing_adapter_one_durable_fence_and_no_shared_message_file() {
        let (store, target) = fixture("task-context-private");
        let request = request(&target);
        let proof = receipt(&request);
        let adapter = fake(vec![
            json!({"stage":"BEFORE_SUBMIT"}),
            json!({"stage":"USER_MESSAGE_APPENDED","receipt":proof}),
            json!({"stage":"SENT_PENDING_RESPONSE","receipt":proof}),
        ]);
        let sent = adapter.sent.clone();
        let admitted = adapter.admitted.clone();
        let before = store.events().unwrap();
        let result = run_with_adapter(
            &store,
            &request,
            &mut || {
                admitted.set(true);
                Ok(())
            },
            &|| Ok(()),
            |_, _| Ok(adapter),
        )
        .unwrap();
        assert_eq!(result.event_id, request.event_id);
        assert_eq!(acknowledgments(&sent), 1);
        assert_eq!(sent.borrow()[0]["event"]["message"], request.message);
        assert_eq!(store.events().unwrap(), before);
        assert!(store.delivery(request.event_id).unwrap().is_none());
        assert!(store.timer(request.event_id).unwrap().is_none());
        assert_eq!(desired(&store).unwrap(), "STOPPED");
    }
    #[test]
    fn context_reconciliation_never_runs_submit_callback_or_acknowledges_submit() {
        let (store, target) = fixture("task-context-observe");
        let mut request = request(&target);
        request.operation = TaskContextOperation::Reconcile;
        let proof = receipt(&request);
        let adapter = fake(vec![
            json!({"stage":"USER_MESSAGE_APPENDED","receipt":proof}),
            json!({"stage":"SENT_PENDING_RESPONSE","receipt":proof}),
        ]);
        let sent = adapter.sent.clone();
        run_with_adapter(
            &store,
            &request,
            &mut || panic!("read-only reconciliation cannot submit"),
            &|| Ok(()),
            |_, _| Ok(adapter),
        )
        .unwrap();
        assert_eq!(acknowledgments(&sent), 0);
        assert_eq!(sent.borrow()[0]["reconcile"], true);
    }
    #[test]
    fn context_ambiguous_ack_and_disconnect_never_send_again() {
        for fail_ack in [true, false] {
            let (store, target) = fixture(if fail_ack {
                "context-ack"
            } else {
                "context-eof"
            });
            let request = request(&target);
            let mut adapter = fake(vec![json!({"stage":"BEFORE_SUBMIT"})]);
            adapter.fail_ack = fail_ack;
            let admitted = adapter.admitted.clone();
            let sent = adapter.sent.clone();
            assert_eq!(
                run_with_adapter(
                    &store,
                    &request,
                    &mut || {
                        admitted.set(true);
                        Ok(())
                    },
                    &|| Ok(()),
                    |_, _| Ok(adapter)
                )
                .unwrap_err(),
                "TASK_CONTEXT_OUTCOME_UNKNOWN"
            );
            assert!(acknowledgments(&sent) <= 1);
        }
    }
    #[test]
    fn context_wrong_or_changed_receipts_remain_unknown_after_ack() {
        for field in [
            "eventId",
            "targetGeneration",
            "targetDigest",
            "messageDigest",
            "evidence",
            "sentUtc",
            "extra",
        ] {
            let (store, target) = fixture(&format!("context-bad-{field}"));
            let request = request(&target);
            let mut bad = receipt(&request);
            bad[field] = json!("foreign");
            let adapter = fake(vec![
                json!({"stage":"BEFORE_SUBMIT"}),
                json!({"stage":"SENT_PENDING_RESPONSE","receipt":bad}),
            ]);
            let admitted = adapter.admitted.clone();
            assert_eq!(
                run_with_adapter(
                    &store,
                    &request,
                    &mut || {
                        admitted.set(true);
                        Ok(())
                    },
                    &|| Ok(()),
                    |_, _| Ok(adapter)
                )
                .unwrap_err(),
                "TASK_CONTEXT_OUTCOME_UNKNOWN"
            );
        }
    }
    #[test]
    fn context_duplicate_ack_and_reconcile_submit_frame_are_refused() {
        for operation in [
            TaskContextOperation::Submit,
            TaskContextOperation::Reconcile,
        ] {
            let (store, target) = fixture(&format!("context-frame-{operation:?}"));
            let mut request = request(&target);
            request.operation = operation;
            let adapter = fake(vec![
                json!({"stage":"BEFORE_SUBMIT"}),
                json!({"stage":"BEFORE_SUBMIT"}),
            ]);
            let sent = adapter.sent.clone();
            let admitted = adapter.admitted.clone();
            let error = run_with_adapter(
                &store,
                &request,
                &mut || {
                    admitted.set(true);
                    Ok(())
                },
                &|| Ok(()),
                |_, _| Ok(adapter),
            )
            .unwrap_err();
            assert_eq!(
                error,
                if operation == TaskContextOperation::Submit {
                    "TASK_CONTEXT_OUTCOME_UNKNOWN"
                } else {
                    "TASK_CONTEXT_RECONCILIATION_UNPROVEN"
                }
            );
            assert_eq!(
                acknowledgments(&sent),
                usize::from(operation == TaskContextOperation::Submit)
            );
        }
    }
    #[test]
    fn context_caller_revocation_before_write_sends_no_ack_or_private_error() {
        let (store, target) = fixture("context-revoked");
        let request = request(&target);
        let adapter = fake(vec![json!({"stage":"BEFORE_SUBMIT"})]);
        let sent = adapter.sent.clone();
        assert_eq!(
            run_with_adapter(
                &store,
                &request,
                &mut || Err("PRIVATE_FAILURE".into()),
                &|| Ok(()),
                |_, _| Ok(adapter)
            )
            .unwrap_err(),
            "TASK_CONTEXT_PRE_SUBMIT_UNPROVEN"
        );
        assert_eq!(acknowledgments(&sent), 0);
    }
    #[test]
    fn context_cancellation_after_write_is_unknown_not_retry_permission() {
        let (store, target) = fixture("context-cancelled");
        let request = request(&target);
        let mut adapter = fake(vec![json!({"stage":"BEFORE_SUBMIT"})]);
        let cancelled = Rc::new(Cell::new(false));
        adapter.after_ack = Some(cancelled.clone());
        let admitted = adapter.admitted.clone();
        assert_eq!(
            run_with_adapter(
                &store,
                &request,
                &mut || {
                    admitted.set(true);
                    Ok(())
                },
                &|| if cancelled.get() {
                    Err("PRIVATE_CANCELLED".into())
                } else {
                    Ok(())
                },
                |_, _| Ok(adapter)
            )
            .unwrap_err(),
            "TASK_CONTEXT_OUTCOME_UNKNOWN"
        );
    }
    #[test]
    fn context_receipt_sequence_requires_matching_persistence_and_final_frames() {
        for missing in [true, false] {
            let (store, target) = fixture(if missing {
                "context-proof-missing"
            } else {
                "context-proof-changed"
            });
            let request = request(&target);
            let proof = receipt(&request);
            let mut frames = vec![json!({"stage":"BEFORE_SUBMIT"})];
            let mut final_proof = proof.clone();
            if !missing {
                frames.push(json!({"stage":"USER_MESSAGE_APPENDED","receipt":proof}));
                final_proof["sentUtc"] = json!(now() + 1);
            }
            frames.push(json!({"stage":"SENT_PENDING_RESPONSE","receipt":final_proof}));
            let adapter = fake(frames);
            let admitted = adapter.admitted.clone();
            assert_eq!(
                run_with_adapter(
                    &store,
                    &request,
                    &mut || {
                        admitted.set(true);
                        Ok(())
                    },
                    &|| Ok(()),
                    |_, _| Ok(adapter)
                )
                .unwrap_err(),
                "TASK_CONTEXT_OUTCOME_UNKNOWN"
            );
        }
    }
    #[test]
    fn context_closed_browser_failures_preserve_the_layer_without_private_error_data() {
        for reason in [
            "LOGIN_OR_PROFILE_REQUIRED",
            "EXISTING_DRAFT",
            "CHATGPT_NOT_IDLE",
            "EDITOR_SELECTOR",
            "PRIVATE_CAPABILITY_VALUE",
        ] {
            let (store, target) = fixture(&format!("context-reason-{reason}"));
            let request = request(&target);
            let adapter = fake(vec![json!({"stage":"ATTENTION","reason":reason})]);
            let sent = adapter.sent.clone();
            let error = run_with_adapter(
                &store,
                &request,
                &mut || panic!("no submission"),
                &|| Ok(()),
                |_, _| Ok(adapter),
            )
            .unwrap_err();
            assert_eq!(
                error,
                if reason == "PRIVATE_CAPABILITY_VALUE" {
                    "TASK_CONTEXT_BROWSER_ATTENTION"
                } else {
                    reason
                }
            );
            assert_eq!(acknowledgments(&sent), 0);
        }
    }
    #[test]
    fn context_expired_connection_never_receives_the_private_message() {
        let (store, target) = fixture("context-connect-budget");
        let mut request = request(&target);
        request.maximum_seconds = 1;
        let adapter = fake(vec![]);
        let sent = adapter.sent.clone();
        assert_eq!(
            run_with_adapter(
                &store,
                &request,
                &mut || panic!("no submission"),
                &|| Ok(()),
                |_, _| {
                    thread::sleep(Duration::from_millis(1050));
                    Ok(adapter)
                }
            )
            .unwrap_err(),
            "TASK_CONTEXT_TIMEOUT"
        );
        assert!(sent.borrow().is_empty());
    }
    #[test]
    fn context_bounds_and_target_drift_refuse_before_adapter_start() {
        let (store, target) = fixture("context-bounds");
        for maximum_seconds in [0, 181] {
            let mut request = request(&target);
            request.maximum_seconds = maximum_seconds;
            assert!(
                run_with_adapter::<Fake>(
                    &store,
                    &request,
                    &mut || panic!(),
                    &|| Ok(()),
                    |_, _| panic!("no browser")
                )
                .is_err()
            );
        }
        let foreign = Target::new(target.generation + 1, target.url.clone()).unwrap();
        assert_eq!(
            run_with_adapter::<Fake>(
                &store,
                &request(&foreign),
                &mut || panic!(),
                &|| Ok(()),
                |_, _| panic!("no browser")
            )
            .unwrap_err(),
            "TASK_CONTEXT_TARGET_CHANGED"
        );
        let large = "x".repeat(16 * 1024 + 1);
        let mut request = request(&target);
        request.message = &large;
        assert!(
            run_with_adapter::<Fake>(
                &store,
                &request,
                &mut || panic!(),
                &|| Ok(()),
                |_, _| panic!("no browser")
            )
            .is_err()
        );
    }
}
