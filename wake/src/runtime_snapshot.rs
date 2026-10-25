//! Shared-deadline, read-only snapshots with content-free local diagnostics.
use super::*;
use serde_json::Value;
use std::cell::Cell;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SnapshotReadPhase {
    Input,
    OwnerCheck,
    ScopeCheck,
    PackageCheck,
    BrowserLock,
    AdapterStart,
    AdapterReadyWait,
    BrowserReady,
    SnapshotReply,
    TargetOpen,
    FrameValidation,
    Cleanup,
    Complete,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotDiagnostic {
    pub phase: SnapshotReadPhase,
    pub elapsed_millis: u64,
    pub budget_millis: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotReadReport {
    pub snapshot: Option<Value>,
    pub failure: Option<String>,
    pub diagnostic: SnapshotDiagnostic,
}

struct SnapshotOutcome {
    result: Result<Value>,
    diagnostic: SnapshotDiagnostic,
}

fn diagnostic_failure(reason: &str) -> String {
    let code = reason.split_once(':').map_or(reason, |(code, _)| code);
    match code {
        "SOURCE_SNAPSHOT_TIMEOUT"
        | "SOURCE_SNAPSHOT_REQUEST_INVALID"
        | "SOURCE_SNAPSHOT_INSTALLED_PACKAGE_UNSUPPORTED"
        | "SOURCE_SNAPSHOT_SCOPE_INVALID"
        | "SOURCE_SNAPSHOT_INVALID"
        | "SOURCE_SNAPSHOT_PROTOCOL_INVALID"
        | "SOURCE_SNAPSHOT_ATTENTION"
        | "SOURCE_SNAPSHOT_SOURCE_DRIFT"
        | "SOURCE_SNAPSHOT_QUERY_FAILED"
        | "SOURCE_SNAPSHOT_TOO_LARGE"
        | "SOURCE_REVIEW_RECEIPT_REQUIRED"
        | "BOOTSTRAP_SOURCE_BINDING_CHANGED"
        | "BOOTSTRAP_SOURCE_QUEUE_PENDING"
        | "BOOTSTRAP_SOURCE_TURN_PENDING"
        | "BOOTSTRAP_INSTALLED_PACKAGE_UNSUPPORTED"
        | "HOST_LOCK_UNAVAILABLE"
        | "HOST_ALREADY_RUNNING"
        | "HOST_EXECUTABLE_UNAVAILABLE"
        | "HOST_EXECUTABLE_HASH_MISMATCH"
        | "COOPERATIVE_HOST_PROTOCOL_REQUIRED"
        | "BROWSER_ADAPTER_UNAVAILABLE"
        | "BROWSER_ADAPTER_HASH_MISMATCH"
        | "BROWSER_PRIMITIVES_UNAVAILABLE"
        | "BROWSER_PRIMITIVES_HASH_MISMATCH"
        | "BROWSER_SESSION_LOCK_UNSAFE"
        | "BROWSER_SESSION_LOCK_UNAVAILABLE"
        | "BROWSER_SESSION_BUSY"
        | "BROWSER_RUNTIME_NOT_INSTALLED"
        | "BROWSER_PROFILE_PATH_UNSUPPORTED"
        | "BROWSER_LOG_UNAVAILABLE"
        | "BROWSER_ADAPTER_START_FAILED"
        | "BROWSER_ADAPTER_PIPE_FAILED"
        | "BROWSER_ADAPTER_REQUEST_INVALID"
        | "BROWSER_ADAPTER_RESPONSE_INVALID"
        | "BROWSER_ADAPTER_EXITED"
        | "BROWSER_SESSION_START_TIMEOUT"
        | "BROWSER_PROFILE_ACCESS_DENIED"
        | "BROWSER_BINARY_OR_DRIVER_MISSING"
        | "BROWSER_SESSION_NOT_CREATED"
        | "BROWSER_DRIVER_START_FAILED"
        | "BROWSER_PROCESS_START_FAILED"
        | "BROWSER_SESSION_UNAVAILABLE"
        | "BROWSER_PRIMITIVES_LOAD_FAILED"
        | "CHATGPT_NOT_IDLE"
        | "EDITOR_SELECTOR"
        | "CAPTCHA_OR_SECURITY_CHECK"
        | "SELENIUMBASE_IMPORT_FAILED"
        | "LOGIN_OR_PROFILE_REQUIRED"
        | "RESPONSE_STATE_QUERY_FAILED"
        | "RESPONSE_STATE_UNPROVEN"
        | "WEB_REVIEW_READ_TIMEOUT"
        | "WEB_REVIEW_TARGET_CHANGED"
        | "WEB_AUTO_OWNER_STOPPED" => code.into(),
        _ => "SOURCE_SNAPSHOT_FAILED".into(),
    }
}

impl SnapshotOutcome {
    fn report(self) -> SnapshotReadReport {
        let (snapshot, failure) = match self.result {
            Ok(snapshot) => (Some(snapshot), None),
            Err(reason) => (None, Some(diagnostic_failure(&reason))),
        };
        SnapshotReadReport {
            snapshot,
            failure,
            diagnostic: self.diagnostic,
        }
    }
}

pub fn diagnosed_sent_snapshot(
    store: &Store,
    project_id: &str,
    source: &Target,
    event_id: &str,
    keep_running: &dyn Fn() -> Result<()>,
) -> SnapshotReadReport {
    snapshot(
        store,
        project_id,
        source,
        Some(event_id),
        keep_running,
        Duration::from_secs(90),
    )
    .report()
}

pub(super) fn snapshot_result(
    store: &Store,
    project_id: &str,
    source: &Target,
    delivered_review: Option<&str>,
    keep_running: &dyn Fn() -> Result<()>,
) -> Result<Value> {
    snapshot(
        store,
        project_id,
        source,
        delivered_review,
        keep_running,
        Duration::from_secs(180),
    )
    .result
}

fn snapshot(
    store: &Store,
    project_id: &str,
    source: &Target,
    delivered_review: Option<&str>,
    keep_running: &dyn Fn() -> Result<()>,
    maximum: Duration,
) -> SnapshotOutcome {
    snapshot_with_adapter(
        store,
        project_id,
        source,
        delivered_review,
        keep_running,
        maximum,
        |deadline, guard, phase| {
            let installed = installed_bootstrap_directory(store)?;
            let adapter_bytes = std::fs::read(installed.join("adapter.py"))
                .map_err(|_| "BROWSER_ADAPTER_UNAVAILABLE")?;
            let bridge_bytes = std::fs::read(installed.join("wake_bridge.py"))
                .map_err(|_| "BROWSER_PRIMITIVES_UNAVAILABLE")?;
            if !adapter_bytes
                .windows(b"def source_snapshot(".len())
                .any(|v| v == b"def source_snapshot(")
                || !bridge_bytes
                    .windows(b"roleTurnHash".len())
                    .any(|v| v == b"roleTurnHash")
            {
                return Err("SOURCE_SNAPSHOT_INSTALLED_PACKAGE_UNSUPPORTED".into());
            }
            guard()?;
            Adapter::spawn_from_installed_monitored(
                store.root(),
                &installed,
                deadline
                    .saturating_duration_since(Instant::now())
                    .min(Duration::from_secs(30)),
                guard,
                phase,
            )
        },
    )
}

#[allow(clippy::too_many_arguments)]
fn snapshot_with_adapter<A: SubmissionAdapter>(
    store: &Store,
    project_id: &str,
    source: &Target,
    delivered_review: Option<&str>,
    keep_running: &dyn Fn() -> Result<()>,
    maximum: Duration,
    connect: impl FnOnce(Instant, &dyn Fn() -> Result<()>, &dyn Fn(AdapterStartPhase)) -> Result<A>,
) -> SnapshotOutcome {
    let started = Instant::now();
    let deadline = started + maximum;
    let phase = Cell::new(SnapshotReadPhase::Input);
    let guard = || {
        if Instant::now() >= deadline {
            return Err("SOURCE_SNAPSHOT_TIMEOUT".into());
        }
        keep_running()?;
        if Instant::now() >= deadline {
            return Err("SOURCE_SNAPSHOT_TIMEOUT".into());
        }
        Ok(())
    };
    let result = (|| {
        guard()?;
        source.validate()?;
        if !crate::protocol::valid_id(project_id) {
            return Err("SOURCE_SNAPSHOT_REQUEST_INVALID".into());
        }
        phase.set(SnapshotReadPhase::OwnerCheck);
        let _owner = browser_owner_guard(store)?;
        guard()?;
        phase.set(SnapshotReadPhase::ScopeCheck);
        ensure_source_snapshot_available(store, project_id, source, delivered_review)?;
        guard()?;
        phase.set(SnapshotReadPhase::PackageCheck);
        let monitor = |value| {
            phase.set(match value {
                AdapterStartPhase::BrowserLock => SnapshotReadPhase::BrowserLock,
                AdapterStartPhase::AdapterStart => SnapshotReadPhase::AdapterStart,
                AdapterStartPhase::AdapterReadyWait => SnapshotReadPhase::AdapterReadyWait,
            })
        };
        let mut adapter = connect(deadline, &guard, &monitor)?;
        phase.set(SnapshotReadPhase::BrowserReady);
        guard()?;
        adapter
            .send(serde_json::json!({"snapshot":true,"projectId":project_id,"source":source}))?;
        phase.set(SnapshotReadPhase::SnapshotReply);
        loop {
            guard()?;
            let value = adapter.receive(
                deadline
                    .saturating_duration_since(Instant::now())
                    .min(Duration::from_millis(250)),
            );
            guard()?;
            match value {
                Ok(Ok(value)) => match value.get("stage").and_then(Value::as_str) {
                    Some("SOURCE_SNAPSHOT") => {
                        phase.set(SnapshotReadPhase::FrameValidation);
                        ensure_source_snapshot_available(
                            store,
                            project_id,
                            source,
                            delivered_review,
                        )?;
                        let snapshot = source_snapshot_payload(value, project_id, source)?;
                        guard()?;
                        phase.set(SnapshotReadPhase::Cleanup);
                        adapter.finish(
                            deadline
                                .saturating_duration_since(Instant::now())
                                .min(Duration::from_secs(5)),
                        );
                        guard()?;
                        phase.set(SnapshotReadPhase::Complete);
                        return Ok(snapshot);
                    }
                    Some(
                        "CDP_REUSE_BEGIN" | "CDP_REUSE_DONE" | "CDP_ACTIVATE_BEGIN"
                        | "CDP_ACTIVATE_DONE",
                    ) => {
                        phase.set(SnapshotReadPhase::TargetOpen);
                    }
                    Some("ATTENTION") => {
                        return Err(value
                            .get("reason")
                            .and_then(Value::as_str)
                            .filter(|value| crate::protocol::valid_id(value))
                            .unwrap_or("SOURCE_SNAPSHOT_ATTENTION")
                            .into());
                    }
                    _ => return Err("SOURCE_SNAPSHOT_PROTOCOL_INVALID".into()),
                },
                Ok(Err(error)) => return Err(error),
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err("BROWSER_ADAPTER_EXITED".into());
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
        }
    })();
    SnapshotOutcome {
        result,
        diagnostic: SnapshotDiagnostic {
            phase: phase.get(),
            elapsed_millis: started.elapsed().as_millis().min(u64::MAX as u128) as u64,
            budget_millis: maximum.as_millis().min(u64::MAX as u128) as u64,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::{cell::RefCell, collections::VecDeque, rc::Rc};

    struct FakeAdapter {
        frames: VecDeque<Value>,
        delay: Duration,
        sent: Rc<RefCell<Vec<Value>>>,
        cleanup_budget: Rc<Cell<Duration>>,
        consume_cleanup_budget: bool,
    }
    impl SubmissionAdapter for FakeAdapter {
        fn send(&mut self, value: Value) -> Result<()> {
            self.sent.borrow_mut().push(value);
            Ok(())
        }
        fn receive(
            &mut self,
            _: Duration,
        ) -> std::result::Result<Result<Value>, mpsc::RecvTimeoutError> {
            thread::sleep(self.delay);
            self.frames
                .pop_front()
                .map(Ok)
                .ok_or(mpsc::RecvTimeoutError::Disconnected)
        }
        fn finish(&mut self, grace: Duration) {
            self.cleanup_budget.set(grace);
            if self.consume_cleanup_budget {
                thread::sleep(grace);
            }
        }
    }

    fn fixture() -> (Store, Target) {
        let base = std::env::temp_dir().join(format!(
            "snapshot-budget-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&base).unwrap();
        let store = Store::open_scoped_for_test(&base.join("wake"), &base).unwrap();
        store.initialize().unwrap();
        let target = store
            .set_target("alpha", 0, "https://chatgpt.com/c/snapshot-budget")
            .unwrap();
        (store, target)
    }

    fn frame(target: &Target) -> Value {
        json!({"stage":"SOURCE_SNAPSHOT","projectId":"alpha","sourceGeneration":target.generation,
            "sourceDigest":target.digest,"snapshot":{"messages":[],"truncated":false,"coverage":{}}})
    }

    fn adapter(frames: Vec<Value>, delay: Duration) -> FakeAdapter {
        FakeAdapter {
            frames: frames.into(),
            delay,
            sent: Rc::new(RefCell::new(vec![])),
            cleanup_budget: Rc::new(Cell::new(Duration::ZERO)),
            consume_cleanup_budget: false,
        }
    }

    #[test]
    fn snapshot_success_sends_only_one_snapshot_and_bounds_cleanup_to_remaining_time() {
        let (store, target) = fixture();
        let fake = adapter(vec![frame(&target)], Duration::ZERO);
        let sent = fake.sent.clone();
        let cleanup = fake.cleanup_budget.clone();
        let outcome = snapshot_with_adapter(
            &store,
            "alpha",
            &target,
            None,
            &|| Ok(()),
            Duration::from_millis(100),
            |_, _, phase| {
                phase(AdapterStartPhase::BrowserLock);
                phase(AdapterStartPhase::AdapterStart);
                phase(AdapterStartPhase::AdapterReadyWait);
                Ok(fake)
            },
        );
        let report = outcome.report();
        assert!(report.snapshot.is_some());
        assert!(report.failure.is_none());
        assert_eq!(report.diagnostic.phase, SnapshotReadPhase::Complete);
        assert_eq!(report.diagnostic.budget_millis, 100);
        assert_eq!(sent.borrow().len(), 1);
        assert_eq!(sent.borrow()[0]["snapshot"], true);
        assert!(sent.borrow()[0].get("submit").is_none());
        assert!(cleanup.get() <= Duration::from_millis(100));
        assert!(!store.root().join("status.json").exists());
        assert_eq!(
            std::fs::read_dir(store.root().join("logs"))
                .unwrap()
                .count(),
            0
        );
    }

    #[test]
    fn snapshot_startup_timeout_reports_the_exact_fixed_startup_phase() {
        for (startup, expected) in [
            (
                AdapterStartPhase::BrowserLock,
                SnapshotReadPhase::BrowserLock,
            ),
            (
                AdapterStartPhase::AdapterStart,
                SnapshotReadPhase::AdapterStart,
            ),
            (
                AdapterStartPhase::AdapterReadyWait,
                SnapshotReadPhase::AdapterReadyWait,
            ),
        ] {
            let (store, target) = fixture();
            let outcome = snapshot_with_adapter::<FakeAdapter>(
                &store,
                "alpha",
                &target,
                None,
                &|| Ok(()),
                Duration::from_millis(40),
                |_, guard, phase| {
                    phase(startup);
                    thread::sleep(Duration::from_millis(55));
                    guard()?;
                    unreachable!()
                },
            );
            let report = outcome.report();
            assert_eq!(report.failure.as_deref(), Some("SOURCE_SNAPSHOT_TIMEOUT"));
            assert_eq!(report.diagnostic.phase, expected);
            assert!(report.diagnostic.elapsed_millis >= 40);
            assert!(report.snapshot.is_none());
        }
    }

    #[test]
    fn snapshot_startup_and_reply_share_one_deadline_instead_of_resetting_it() {
        let (store, target) = fixture();
        let fake = adapter(vec![frame(&target)], Duration::from_millis(65));
        let sent = fake.sent.clone();
        let outcome = snapshot_with_adapter(
            &store,
            "alpha",
            &target,
            None,
            &|| Ok(()),
            Duration::from_millis(100),
            |_, guard, phase| {
                phase(AdapterStartPhase::AdapterReadyWait);
                thread::sleep(Duration::from_millis(60));
                guard()?;
                Ok(fake)
            },
        );
        let report = outcome.report();
        assert_eq!(report.failure.as_deref(), Some("SOURCE_SNAPSHOT_TIMEOUT"));
        assert_eq!(report.diagnostic.phase, SnapshotReadPhase::SnapshotReply);
        assert!(report.diagnostic.elapsed_millis >= 100 && report.diagnostic.elapsed_millis < 500);
        assert!(report.snapshot.is_none());
        assert_eq!(sent.borrow().len(), 1);
    }

    #[test]
    fn snapshot_progress_does_not_renew_the_reply_budget() {
        let (store, target) = fixture();
        let fake = adapter(
            (0..100)
                .map(|_| json!({"stage":"CDP_ACTIVATE_BEGIN"}))
                .collect(),
            Duration::from_millis(15),
        );
        let outcome = snapshot_with_adapter(
            &store,
            "alpha",
            &target,
            None,
            &|| Ok(()),
            Duration::from_millis(80),
            |_, _, _| Ok(fake),
        );
        let report = outcome.report();
        assert_eq!(report.failure.as_deref(), Some("SOURCE_SNAPSHOT_TIMEOUT"));
        assert_eq!(report.diagnostic.phase, SnapshotReadPhase::TargetOpen);
        assert!(report.diagnostic.elapsed_millis < 400);
    }

    #[test]
    fn snapshot_cleanup_consumes_the_same_deadline_before_success() {
        let (store, target) = fixture();
        let mut fake = adapter(vec![frame(&target)], Duration::ZERO);
        fake.consume_cleanup_budget = true;
        let report = snapshot_with_adapter(
            &store,
            "alpha",
            &target,
            None,
            &|| Ok(()),
            Duration::from_millis(80),
            |_, _, _| Ok(fake),
        )
        .report();
        assert_eq!(report.failure.as_deref(), Some("SOURCE_SNAPSHOT_TIMEOUT"));
        assert_eq!(report.diagnostic.phase, SnapshotReadPhase::Cleanup);
        assert!(report.snapshot.is_none());
    }

    #[test]
    fn snapshot_report_redacts_private_error_but_raw_compatibility_result_does_not_change() {
        let (store, target) = fixture();
        let private = "private https://chatgpt.com/c/secret error";
        let outcome = snapshot_with_adapter::<FakeAdapter>(
            &store,
            "alpha",
            &target,
            None,
            &|| Err(private.into()),
            Duration::from_millis(100),
            |_, _, _| unreachable!(),
        );
        assert_eq!(outcome.result.as_ref().unwrap_err(), private);
        let report = outcome.report();
        assert_eq!(report.failure.as_deref(), Some("SOURCE_SNAPSHOT_FAILED"));
        assert_eq!(report.diagnostic.phase, SnapshotReadPhase::Input);
        assert!(!serde_json::to_string(&report).unwrap().contains("secret"));
    }

    #[test]
    fn snapshot_diagnostic_error_vocabulary_matches_bounded_component_codes() {
        for reason in [
            "RESPONSE_STATE_QUERY_FAILED",
            "SOURCE_SNAPSHOT_TIMEOUT",
            "CHATGPT_NOT_IDLE",
            "SELENIUMBASE_IMPORT_FAILED",
        ] {
            assert_eq!(diagnostic_failure(reason), reason);
        }
        assert_eq!(
            diagnostic_failure("SOURCE_SNAPSHOT_TIMEOUT: private text"),
            "SOURCE_SNAPSHOT_TIMEOUT"
        );
        for reason in [
            "SECRET_TOKEN",
            "ARBITRARY_UPPERCASE",
            "BROWSER_PRIVATE_TOKEN_VALUE",
            "PRIVATE",
            "mixed_case",
            "",
            &"SOURCE_".repeat(12),
        ] {
            assert_eq!(diagnostic_failure(reason), "SOURCE_SNAPSHOT_FAILED");
        }
    }

    #[test]
    fn snapshot_invalid_frame_stays_unverified_with_content_free_diagnostic() {
        let (store, target) = fixture();
        let mut invalid = frame(&target);
        invalid["projectId"] = json!("wrong-project");
        invalid["snapshot"]["privateBody"] = json!("secret body");
        let fake = adapter(vec![invalid], Duration::ZERO);
        let report = snapshot_with_adapter(
            &store,
            "alpha",
            &target,
            None,
            &|| Ok(()),
            Duration::from_millis(100),
            |_, _, _| Ok(fake),
        )
        .report();
        assert_eq!(
            report.failure.as_deref(),
            Some("SOURCE_SNAPSHOT_SCOPE_INVALID")
        );
        assert_eq!(report.diagnostic.phase, SnapshotReadPhase::FrameValidation);
        assert!(report.snapshot.is_none());
        assert!(!serde_json::to_string(&report).unwrap().contains("secret"));
    }
}
