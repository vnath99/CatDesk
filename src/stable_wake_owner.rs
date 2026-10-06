//! Dormant Rust single-writer browser handoff.
//!
//! This module owns only delivery-state transitions. `BrowserAdapter` is
//! deliberately stateless: it receives opaque digests, has no workspace path,
//! and cannot read or mutate the canonical inbox or schema-4 state.

use crate::stable_wake_adapter_runtime::DurableAdapterRuntime;
use crate::stable_wake_delivery::{
    DeliveryClassification, DeliveryReceiptV1, StableWakeDelivery, SubmissionBoundary,
};
use crate::stable_wake_owner_mode::{WakeOwnerMode, selected_owner};
use serde::Deserialize;
use std::{
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};

const MAX_ADAPTER_OUTPUT_BYTES: usize = 256;
const FIXED_ADAPTER_SCHEMA: u32 = 1;
const FIXED_ADAPTER_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AdapterRequest {
    pub(crate) record_id: String,
    pub(crate) message_sha256: String,
    pub(crate) target_sha256: String,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum AdapterResult {
    DefiniteSuccess { sent_at_unix: f64 },
    DefinitePreSubmitFailure,
    TargetNotReady,
    AuthenticationRequired,
    CaptchaChallenge,
    ChatUnavailable,
    AmbiguousPostSubmit,
}

/// The adapter has no path/config/state parameters. A real implementation is
/// deferred to T-0254 and must remain a one-attempt process boundary.
pub(crate) trait BrowserAdapter {
    fn attempt(&self, request: &AdapterRequest) -> Result<AdapterResult, String>;
}

/// Fixed durable-runtime process adapter. It receives no caller-selected
/// executable, workspace, profile, conversation URL, or state/inbox path.
/// The script itself has only one browser attempt and emits a bounded result.
pub(crate) struct FixedScriptBrowserAdapter {
    runtime: DurableAdapterRuntime,
    workspace: PathBuf,
}

impl FixedScriptBrowserAdapter {
    pub(crate) fn open(workspace: &Path) -> Result<Self, String> {
        let workspace = std::fs::canonicalize(workspace)
            .map_err(|_| "stable wake adapter workspace unavailable".to_string())?;
        let runtime = DurableAdapterRuntime::open(&workspace)?;
        Ok(Self { runtime, workspace })
    }
}

impl BrowserAdapter for FixedScriptBrowserAdapter {
    fn attempt(&self, request: &AdapterRequest) -> Result<AdapterResult, String> {
        // All values originate at the durable Rust submission boundary.  No
        // adapter caller can add an executable, target, profile, or pathname.
        let lease = self.runtime.prepare_launch()?;
        let child = Command::new(lease.interpreter_path())
            // Isolated mode refuses PYTHONPATH, user-site, and incidental
            // per-user interpreter state. The reviewed durable runtime is the
            // only interpreter/module authority.
            .arg("-I")
            .arg(lease.adapter_path())
            .arg("--record-id")
            .arg(&request.record_id)
            .arg("--message-sha256")
            .arg(&request.message_sha256)
            .arg("--target-sha256")
            .arg(&request.target_sha256)
            .current_dir(&self.workspace)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| "stable wake adapter process unavailable".to_string())?;
        let mut child = RetainedAdapterChild::new(child, lease);
        let deadline = Instant::now() + FIXED_ADAPTER_TIMEOUT;
        loop {
            match child.poll() {
                Ok(Some(_)) => break,
                Ok(None) if Instant::now() >= deadline => {
                    child.terminate_and_reap();
                    return Err("stable wake adapter timeout".into());
                }
                Ok(None) => thread::sleep(Duration::from_millis(25)),
                Err(()) => {
                    child.terminate_and_reap();
                    return Err("stable wake adapter process uncertain".into());
                }
            }
        }
        let output = child.reap_after_terminal()?;
        if !output.status.success() || output.stdout.len() > MAX_ADAPTER_OUTPUT_BYTES {
            Err("stable wake adapter process uncertain".into())
        } else {
            parse_adapter_result(&output.stdout)
        }
    }
}

/// Owns both a successfully spawned child and its launch-bound reviewed file
/// lease. `Child` drop alone does not kill or wait; this guard makes the only
/// lease-release path explicit: a terminal status observed by `try_wait`, or a
/// successful terminate-and-reap. If the OS cannot confirm reaping, `Drop`
/// intentionally leaks the child handle and reviewed lease rather than permit
/// a possibly-live pathname-based Python process to outlive its authority.
struct RetainedAdapterChild<'lease> {
    child: Option<Child>,
    lease: Option<crate::stable_wake_adapter_runtime::RuntimeLaunchLease<'lease>>,
    terminal_observed: bool,
}

impl<'lease> RetainedAdapterChild<'lease> {
    fn new(
        child: Child,
        lease: crate::stable_wake_adapter_runtime::RuntimeLaunchLease<'lease>,
    ) -> Self {
        Self {
            child: Some(child),
            lease: Some(lease),
            terminal_observed: false,
        }
    }

    /// `Err(())` is deliberately not propagated: the caller must finalise the
    /// child before the guard, and therefore the reviewed lease, can drop.
    fn poll(&mut self) -> Result<Option<()>, ()> {
        #[cfg(test)]
        if INJECT_POLL_ERROR.swap(false, std::sync::atomic::Ordering::SeqCst) {
            return Err(());
        }
        let child = self.child.as_mut().expect("child retained until terminal");
        match child.try_wait() {
            Ok(Some(_)) => {
                self.terminal_observed = true;
                Ok(Some(()))
            }
            Ok(None) => Ok(None),
            Err(_) => Err(()),
        }
    }

    fn reap_after_terminal(mut self) -> Result<Output, String> {
        if !self.terminal_observed {
            self.terminate_and_reap();
            return Err("stable wake adapter process uncertain".into());
        }
        // `try_wait` has already proved this exact child exited. Any output
        // collection error therefore occurs after terminal state, not while a
        // pathname-based interpreter could still execute.
        let child = self.child.take().expect("terminal child retained");
        let output = child.wait_with_output();
        self.release_after_terminal();
        output.map_err(|_| "stable wake adapter process uncertain".to_string())
    }

    fn terminate_and_reap(&mut self) {
        let Some(child) = self.child.as_mut() else {
            return;
        };
        let _ = child.kill();
        if child.wait().is_ok() {
            self.child.take();
            self.terminal_observed = true;
            self.release_after_terminal();
        }
    }

    fn release_after_terminal(&mut self) {
        if let Some(lease) = self.lease.take() {
            lease.release_after_child_exit();
        }
    }

    fn retain_fail_closed(&mut self) {
        if let Some(child) = self.child.take() {
            std::mem::forget(child);
        }
        if let Some(lease) = self.lease.take() {
            std::mem::forget(lease);
        }
    }
}

#[cfg(test)]
static INJECT_POLL_ERROR: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

impl Drop for RetainedAdapterChild<'_> {
    fn drop(&mut self) {
        if self.lease.is_none() {
            return;
        }
        self.terminate_and_reap();
        if self.lease.is_some() {
            // `Child::drop` does not reap. Do not let Rust's ordinary field
            // drop release reviewed handles after an unconfirmed finalization.
            self.retain_fail_closed();
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AdapterWireResult {
    schema_version: u32,
    outcome: String,
    #[serde(default)]
    sent_at_unix: Option<f64>,
}

fn parse_adapter_result(bytes: &[u8]) -> Result<AdapterResult, String> {
    if bytes.is_empty() || bytes.len() > MAX_ADAPTER_OUTPUT_BYTES {
        return Err("stable wake adapter result invalid".into());
    }
    let wire: AdapterWireResult = serde_json::from_slice(bytes)
        .map_err(|_| "stable wake adapter result invalid".to_string())?;
    if wire.schema_version != FIXED_ADAPTER_SCHEMA {
        return Err("stable wake adapter result invalid".into());
    }
    let simple = |result| {
        if wire.sent_at_unix.is_none() {
            Ok(result)
        } else {
            Err("stable wake adapter result invalid".into())
        }
    };
    match wire.outcome.as_str() {
        "DEFINITE_SUCCESS" => match wire.sent_at_unix {
            Some(value) if value.is_finite() && value > 0.0 => Ok(AdapterResult::DefiniteSuccess {
                sent_at_unix: value,
            }),
            _ => Err("stable wake adapter result invalid".into()),
        },
        "DEFINITE_PRE_SUBMIT_FAILURE" => simple(AdapterResult::DefinitePreSubmitFailure),
        "TARGET_NOT_READY" => simple(AdapterResult::TargetNotReady),
        "AUTH_REQUIRED" => simple(AdapterResult::AuthenticationRequired),
        "CAPTCHA_CHALLENGE" => simple(AdapterResult::CaptchaChallenge),
        "CHAT_UNAVAILABLE" => simple(AdapterResult::ChatUnavailable),
        "AMBIGUOUS_POST_SUBMIT" => simple(AdapterResult::AmbiguousPostSubmit),
        _ => Err("stable wake adapter result invalid".into()),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum OwnerOutcome {
    Sent,
    PreSubmitRetryable,
    AlreadyOwned(DeliveryClassification),
    OperatorAttention,
}

pub(crate) struct StableWakeOwner {
    delivery: StableWakeDelivery,
}

impl StableWakeOwner {
    pub(crate) fn open(workspace: &Path) -> Result<Self, String> {
        Ok(Self {
            delivery: StableWakeDelivery::open(workspace)?,
        })
    }

    /// Executes at most one adapter attempt. No adapter outcome can select a
    /// target, acknowledge an inbox record, or mutate durable delivery state.
    pub(crate) fn dispatch_one<A: BrowserAdapter>(
        &self,
        record_id: &str,
        claimed_at_unix: f64,
        adapter: &A,
    ) -> Result<OwnerOutcome, String> {
        // Capture the protected binding before obtaining/continuing a claim.
        // `begin_submitting` compares this snapshot with a fresh protected
        // read, so a config replacement in the transition window fails closed.
        let target = self.delivery.target_binding()?;
        let claimed = self.delivery.claim(record_id, claimed_at_unix)?;
        if claimed != DeliveryClassification::ClaimedRetryable {
            return Ok(OwnerOutcome::AlreadyOwned(claimed));
        }
        // This revalidates canonical actionability and target identity, then
        // makes SUBMITTING durable before any adapter side effect.
        let boundary = self.delivery.begin_submitting(record_id, &target)?;
        let request = request_from_boundary(&boundary);
        let result = adapter.attempt(&request);
        match result {
            Ok(AdapterResult::DefiniteSuccess { sent_at_unix }) => {
                match self
                    .delivery
                    .record_receipt(&receipt_from_boundary(&boundary, sent_at_unix))
                {
                    Ok(_) => Ok(OwnerOutcome::Sent),
                    // A claimed success with an invalid or unprovable receipt
                    // is post-submit ambiguity, never a retryable error.
                    Err(_) => {
                        self.delivery.note_operator_attention(
                            record_id,
                            "ADAPTER_SUCCESS_RECEIPT_INVALID",
                        )?;
                        Ok(OwnerOutcome::OperatorAttention)
                    }
                }
            }
            Ok(AdapterResult::DefinitePreSubmitFailure) => {
                self.delivery.return_to_clean_claim(record_id)?;
                Ok(OwnerOutcome::PreSubmitRetryable)
            }
            Ok(other) => {
                self.delivery
                    .note_operator_attention(record_id, attention_for(&other))?;
                Ok(OwnerOutcome::OperatorAttention)
            }
            Err(_) => {
                self.delivery
                    .note_operator_attention(record_id, "ADAPTER_CRASH_OR_TIMEOUT")?;
                Ok(OwnerOutcome::OperatorAttention)
            }
        }
    }
}

/// The only Rust dispatch entrypoint used by the production owner binary.
/// Selector evaluation happens before opening a claim, so Rust cannot become a
/// second writer while the documented legacy-safe selector state is present.
pub(crate) fn dispatch_if_rust_selected<A: BrowserAdapter>(
    workspace: &Path,
    record_id: &str,
    claimed_at_unix: f64,
    adapter: &A,
) -> Result<OwnerOutcome, String> {
    if selected_owner(workspace)? != WakeOwnerMode::Rust {
        return Err("stable wake Rust owner is not selected".into());
    }
    StableWakeOwner::open(workspace)?.dispatch_one(record_id, claimed_at_unix, adapter)
}

fn request_from_boundary(boundary: &SubmissionBoundary) -> AdapterRequest {
    AdapterRequest {
        record_id: boundary.record_id.clone(),
        message_sha256: boundary.message_sha256.clone(),
        target_sha256: boundary.target_sha256.clone(),
    }
}

fn receipt_from_boundary(boundary: &SubmissionBoundary, sent_at_unix: f64) -> DeliveryReceiptV1 {
    DeliveryReceiptV1 {
        record_id: boundary.record_id.clone(),
        browser_sent_at_unix: sent_at_unix,
        message_sha256: boundary.message_sha256.clone(),
        target_sha256: boundary.target_sha256.clone(),
        receipt_schema_version: 1,
    }
}

fn attention_for(result: &AdapterResult) -> &'static str {
    match result {
        AdapterResult::TargetNotReady => "ADAPTER_TARGET_NOT_READY",
        AdapterResult::AuthenticationRequired => "ADAPTER_AUTH_REQUIRED",
        AdapterResult::CaptchaChallenge => "ADAPTER_CAPTCHA_CHALLENGE",
        AdapterResult::ChatUnavailable => "ADAPTER_CHAT_UNAVAILABLE",
        AdapterResult::AmbiguousPostSubmit => "ADAPTER_POST_SUBMIT_AMBIGUOUS",
        AdapterResult::DefiniteSuccess { .. } | AdapterResult::DefinitePreSubmitFailure => {
            "ADAPTER_RESULT_UNSAFE"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stable_wake_core::{ReviewInboxRecord, ReviewState};
    use sha2::{Digest, Sha256};
    use std::{
        fs,
        path::PathBuf,
        process::Command,
        sync::{Arc, Mutex},
        thread,
        time::{SystemTime, UNIX_EPOCH},
    };

    #[derive(Clone)]
    struct Fake {
        result: Result<AdapterResult, String>,
        calls: Arc<Mutex<usize>>,
    }
    impl BrowserAdapter for Fake {
        fn attempt(&self, request: &AdapterRequest) -> Result<AdapterResult, String> {
            assert!(request.record_id.len() <= 128);
            assert_eq!(request.message_sha256.len(), 64);
            assert_eq!(request.target_sha256.len(), 64);
            *self.calls.lock().unwrap() += 1;
            self.result.clone()
        }
    }
    fn root(name: &str, unread: bool) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("catdesk-owner-{name}-{nonce}"));
        fs::create_dir_all(root.join(".catdesk/autonomy")).unwrap();
        fs::create_dir_all(root.join(".catdesk/wake-bridge")).unwrap();
        fs::write(root.join(".catdesk/wake-bridge/config.json"), r#"{"conversation_url":"https://chatgpt.com/c/exact-thread","profile_dir":".catdesk/wake-bridge/browser-profile"}"#).unwrap();
        let record = ReviewInboxRecord {
            schema_version: 1,
            record_id: "review_1".into(),
            project_id: "catdesk".into(),
            session_id: "session_1".into(),
            state: ReviewState::CompletedVerified,
            next_action: "independent_final_review".into(),
            reference: "artifacts/completion.json".into(),
            created_at_unix: 1,
            unread,
        };
        fs::write(
            root.join(".catdesk/autonomy/review-inbox.json"),
            serde_json::to_vec(&vec![record]).unwrap(),
        )
        .unwrap();
        root
    }
    fn fake(result: Result<AdapterResult, String>) -> Fake {
        Fake {
            result,
            calls: Arc::new(Mutex::new(0)),
        }
    }

    fn digest(bytes: &[u8]) -> String {
        format!("{:x}", Sha256::digest(bytes))
    }

    fn runtime_fixture(name: &str) -> PathBuf {
        let root = root(name, true);
        let runtime = root.join(".catdesk/wake-bridge/stable-runtime-v1");
        fs::create_dir_all(&runtime).unwrap();
        let owner = b"owner";
        let interpreter = b"interpreter";
        let adapter = b"adapter";
        let primitive = b"primitive";
        fs::write(runtime.join("catdesk-stable-wake-owner.exe"), owner).unwrap();
        fs::write(runtime.join("python.exe"), interpreter).unwrap();
        fs::write(runtime.join("stable_wake_browser_adapter.py"), adapter).unwrap();
        fs::write(runtime.join("wake_bridge.py"), primitive).unwrap();
        let descriptor = format!(
            r#"{{"schemaVersion":1,"runtimeVersion":1,"ownerSha256":"{}","interpreterSha256":"{}","adapterSha256":"{}","browserPrimitivesSha256":"{}"}}"#,
            digest(owner),
            digest(interpreter),
            digest(adapter),
            digest(primitive),
        );
        fs::write(runtime.join("adapter-runtime.json"), &descriptor).unwrap();
        let evidence_dir = root.join(".catdesk/reviewed-build-control/wake-owner-artifacts");
        fs::create_dir_all(&evidence_dir).unwrap();
        fs::write(
            evidence_dir.join("reviewed-artifacts.json"),
            format!(
                r#"{{"schemaVersion":2,"runtimeDescriptorSha256":"{}","provenanceSha256":"{}"}}"#,
                digest(descriptor.as_bytes()),
                "a".repeat(64),
            ),
        )
        .unwrap();
        root
    }

    #[test]
    #[cfg(windows)]
    fn injected_poll_error_terminates_reaps_before_releasing_launch_lease() {
        const CHILD: &str = "CATDESK_RETAINED_CHILD_SLEEP";
        if std::env::var_os(CHILD).is_some() {
            thread::sleep(std::time::Duration::from_secs(30));
            return;
        }

        let root = runtime_fixture("post-spawn-wait-error");
        let runtime_root = root.join(".catdesk/wake-bridge/stable-runtime-v1");
        let runtime = DurableAdapterRuntime::open(&root).unwrap();
        let lease = runtime.prepare_launch().unwrap();
        let child = Command::new(std::env::current_exe().unwrap())
            .arg("--exact")
            .arg("stable_wake_owner::tests::injected_poll_error_terminates_reaps_before_releasing_launch_lease")
            .env(CHILD, "1")
            .spawn()
            .unwrap();
        let mut guard = RetainedAdapterChild::new(child, lease);

        INJECT_POLL_ERROR.store(true, std::sync::atomic::Ordering::SeqCst);
        assert_eq!(guard.poll(), Err(()));
        for name in [
            "python.exe",
            "stable_wake_browser_adapter.py",
            "wake_bridge.py",
        ] {
            assert!(fs::write(runtime_root.join(name), b"replacement").is_err());
        }
        guard.terminate_and_reap();
        drop(guard);

        fs::write(runtime_root.join("python.exe"), b"after-reap").unwrap();
        drop(runtime);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn success_pre_submit_and_ambiguous_paths_are_single_attempt_and_bounded() {
        let workspace = root("paths", true);
        let owner = StableWakeOwner::open(&workspace).unwrap();
        let success = fake(Ok(AdapterResult::DefiniteSuccess { sent_at_unix: 2.0 }));
        assert_eq!(
            owner.dispatch_one("review_1", 1.0, &success).unwrap(),
            OwnerOutcome::Sent
        );
        assert_eq!(*success.calls.lock().unwrap(), 1);
        assert_eq!(
            owner.delivery.classify("review_1").unwrap(),
            DeliveryClassification::AlreadySent
        );
        fs::remove_dir_all(workspace).unwrap();

        let workspace = root("pre", true);
        let owner = StableWakeOwner::open(&workspace).unwrap();
        let pre = fake(Ok(AdapterResult::DefinitePreSubmitFailure));
        assert_eq!(
            owner.dispatch_one("review_1", 1.0, &pre).unwrap(),
            OwnerOutcome::PreSubmitRetryable
        );
        assert_eq!(
            owner.delivery.classify("review_1").unwrap(),
            DeliveryClassification::ClaimedRetryable
        );
        fs::remove_dir_all(workspace).unwrap();

        let workspace = root("ambiguous", true);
        let owner = StableWakeOwner::open(&workspace).unwrap();
        let ambiguous = fake(Ok(AdapterResult::AmbiguousPostSubmit));
        assert_eq!(
            owner.dispatch_one("review_1", 1.0, &ambiguous).unwrap(),
            OwnerOutcome::OperatorAttention
        );
        assert!(owner.delivery.classify("review_1").is_err());
        assert_eq!(*ambiguous.calls.lock().unwrap(), 1);
        fs::remove_dir_all(workspace).unwrap();

        let workspace = root("bad-receipt", true);
        let owner = StableWakeOwner::open(&workspace).unwrap();
        let bad_receipt = fake(Ok(AdapterResult::DefiniteSuccess { sent_at_unix: 0.5 }));
        assert_eq!(
            owner.dispatch_one("review_1", 1.0, &bad_receipt).unwrap(),
            OwnerOutcome::OperatorAttention
        );
        assert!(owner.delivery.classify("review_1").is_err());
        fs::remove_dir_all(workspace).unwrap();
    }

    #[test]
    fn crashes_restart_stale_target_and_concurrency_never_repeat_adapter() {
        let workspace = root("restart", true);
        let owner = StableWakeOwner::open(&workspace).unwrap();
        let binding = owner.delivery.target_binding().unwrap();
        owner.delivery.claim("review_1", 1.0).unwrap();
        owner
            .delivery
            .begin_submitting("review_1", &binding)
            .unwrap();
        let fake = fake(Ok(AdapterResult::DefiniteSuccess { sent_at_unix: 2.0 }));
        assert_eq!(
            owner.dispatch_one("review_1", 3.0, &fake).unwrap(),
            OwnerOutcome::AlreadyOwned(DeliveryClassification::SubmittingAmbiguous)
        );
        assert_eq!(*fake.calls.lock().unwrap(), 0);
        fs::remove_dir_all(workspace).unwrap();

        let root = Arc::new(root("concurrent", true));
        let calls = Arc::new(Mutex::new(0));
        let a = root.clone();
        let b = root.clone();
        let ca = calls.clone();
        let cb = calls.clone();
        let one = thread::spawn(move || {
            let fake = Fake {
                result: Ok(AdapterResult::AmbiguousPostSubmit),
                calls: ca,
            };
            StableWakeOwner::open(&a)
                .unwrap()
                .dispatch_one("review_1", 1.0, &fake)
        });
        let two = thread::spawn(move || {
            let fake = Fake {
                result: Ok(AdapterResult::AmbiguousPostSubmit),
                calls: cb,
            };
            StableWakeOwner::open(&b)
                .unwrap()
                .dispatch_one("review_1", 1.0, &fake)
        });
        let _ = one.join().unwrap();
        let _ = two.join().unwrap();
        assert_eq!(*calls.lock().unwrap(), 1);
        fs::remove_dir_all(&*root).unwrap();
    }

    #[test]
    fn stale_records_and_all_uncertain_adapter_results_fail_closed_without_retries() {
        let workspace = root("stale", false);
        let owner = StableWakeOwner::open(&workspace).unwrap();
        let stale = fake(Ok(AdapterResult::DefiniteSuccess { sent_at_unix: 2.0 }));
        assert!(owner.dispatch_one("review_1", 1.0, &stale).is_err());
        assert_eq!(*stale.calls.lock().unwrap(), 0);
        fs::remove_dir_all(workspace).unwrap();

        for (name, result) in [
            ("target", Ok(AdapterResult::TargetNotReady)),
            ("auth", Ok(AdapterResult::AuthenticationRequired)),
            ("captcha", Ok(AdapterResult::CaptchaChallenge)),
            ("chat", Ok(AdapterResult::ChatUnavailable)),
            ("crash", Err("adapter crashed".into())),
        ] {
            let workspace = root(name, true);
            let owner = StableWakeOwner::open(&workspace).unwrap();
            let adapter = fake(result);
            assert_eq!(
                owner.dispatch_one("review_1", 1.0, &adapter).unwrap(),
                OwnerOutcome::OperatorAttention
            );
            assert_eq!(*adapter.calls.lock().unwrap(), 1);
            assert!(owner.delivery.classify("review_1").is_err());
            fs::remove_dir_all(workspace).unwrap();
        }
    }

    #[test]
    fn adapter_only_has_no_file_authority_and_source_has_no_live_owner_cutover() {
        let root = root("adapter-only", true);
        let inbox = fs::read(root.join(".catdesk/autonomy/review-inbox.json")).unwrap();
        let config = fs::read(root.join(".catdesk/wake-bridge/config.json")).unwrap();
        let fake = fake(Ok(AdapterResult::ChatUnavailable));
        let _ = fake.attempt(&AdapterRequest {
            record_id: "review_1".into(),
            message_sha256: "0".repeat(64),
            target_sha256: "0".repeat(64),
        });
        assert_eq!(
            fs::read(root.join(".catdesk/autonomy/review-inbox.json")).unwrap(),
            inbox
        );
        assert_eq!(
            fs::read(root.join(".catdesk/wake-bridge/config.json")).unwrap(),
            config
        );
        let source = include_str!("stable_wake_owner.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap();
        for forbidden in [
            "wake_bridge.py",
            "selenium",
            "catdesk_wake_bridge_run_once",
            "review-events",
        ] {
            assert!(!source.contains(forbidden));
        }
        assert!(source.contains("DurableAdapterRuntime::open"));
        assert!(source.contains("self.runtime.prepare_launch()?"));
        assert!(source.contains("RetainedAdapterChild::new(child, lease)"));
        assert!(source.contains("child.reap_after_terminal()?"));
        assert!(source.contains("child.terminate_and_reap();"));
        assert!(!source.contains("self.runtime.revalidate()?"));
        assert!(!source.contains("self.runtime.interpreter_path()"));
        assert!(!source.contains("self.runtime.adapter_path()"));
        assert!(!source.contains("venv/Scripts"));
        assert!(!source.contains("target/release"));
        assert!(!source.contains("--workspace"));
        assert!(!source.contains("--conversation-url"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn adapter_wire_parser_is_bounded_fixed_vocabulary_and_secret_free() {
        assert_eq!(
            parse_adapter_result(
                br#"{"schemaVersion":1,"outcome":"DEFINITE_SUCCESS","sentAtUnix":2.0}"#
            )
            .unwrap(),
            AdapterResult::DefiniteSuccess { sent_at_unix: 2.0 }
        );
        assert_eq!(
            parse_adapter_result(br#"{"schemaVersion":1,"outcome":"AUTH_REQUIRED"}"#).unwrap(),
            AdapterResult::AuthenticationRequired
        );
        for invalid in [
            br#"{"schemaVersion":1,"outcome":"DEFINITE_SUCCESS"}"#.as_slice(),
            br#"{"schemaVersion":1,"outcome":"UNKNOWN"}"#.as_slice(),
            br#"{"schemaVersion":1,"outcome":"AUTH_REQUIRED","token":"secret"}"#.as_slice(),
            br#"{"schemaVersion":1,"outcome":"AUTH_REQUIRED","outcome":"DEFINITE_SUCCESS"}"#
                .as_slice(),
        ] {
            assert!(parse_adapter_result(invalid).is_err());
        }
        assert!(parse_adapter_result(&vec![b'x'; MAX_ADAPTER_OUTPUT_BYTES + 1]).is_err());
    }

    #[test]
    fn fixed_adapter_source_has_no_delivery_or_canonical_inbox_write_authority() {
        let source = include_str!("../scripts/stable_wake_browser_adapter.py");
        for forbidden in [
            "state.json",
            "review-inbox.json",
            ".write_text(",
            "os.replace(",
            ".unlink(",
            "--workspace",
            "--conversation-url",
        ] {
            assert!(
                !source.contains(forbidden),
                "forbidden adapter authority: {forbidden}"
            );
        }
        assert!(source.contains("RUNTIME_ROOT = Path(__file__).resolve().parent"));
        assert!(source.contains("LEGACY_SOURCE = RUNTIME_ROOT / \"wake_bridge.py\""));
        assert!(source.contains("root = _safe_directory_identity(Path.cwd())"));
        assert!(source.contains("MAX_CONFIG_BYTES = 16 * 1024"));
        assert!(source.contains("def validate_exact_profile"));
        assert!(source.contains("os.path.samefile(candidate_identity, expected)"));
        assert!(source.contains("normalized == FIXED_PROFILE"));
        assert!(source.contains("def _assert_directory_component_is_safe"));
        assert!(source.contains("os.path.isjunction"));
        assert!(source.contains("stat.FILE_ATTRIBUTE_REPARSE_POINT"));
        assert!(source.contains("os.lstat(current)"));
        assert!(source.contains("os.lstat(resolved)"));
    }

    #[test]
    fn production_rust_dispatch_is_ineligible_under_legacy_and_terminal_sent_is_not_resent() {
        let workspace = root("mode-gate", true);
        let adapter = fake(Ok(AdapterResult::DefiniteSuccess { sent_at_unix: 2.0 }));
        assert!(dispatch_if_rust_selected(&workspace, "review_1", 1.0, &adapter).is_err());
        assert_eq!(*adapter.calls.lock().unwrap(), 0);

        fs::write(
            workspace.join(".catdesk/wake-bridge/owner.json"),
            br#"{"schemaVersion":1,"owner":"rust"}"#,
        )
        .unwrap();
        assert_eq!(
            dispatch_if_rust_selected(&workspace, "review_1", 1.0, &adapter).unwrap(),
            OwnerOutcome::Sent
        );
        assert_eq!(*adapter.calls.lock().unwrap(), 1);
        assert!(matches!(
            dispatch_if_rust_selected(&workspace, "review_1", 3.0, &adapter).unwrap(),
            OwnerOutcome::AlreadyOwned(DeliveryClassification::AlreadySent)
        ));
        assert_eq!(*adapter.calls.lock().unwrap(), 1);
        fs::remove_dir_all(workspace).unwrap();
    }
}
