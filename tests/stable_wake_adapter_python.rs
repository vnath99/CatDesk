//! Bounded bridge proving the durable offline adapter fixture executes under Cargo.
//!
//! This test intentionally has no caller-controlled command, script, arguments,
//! workspace, or environment.  It is not a generic shell facility.

use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

const MAX_OUTPUT_BYTES: usize = 32 * 1024;
const FIXTURE_TIMEOUT: Duration = Duration::from_secs(60);

fn require_fixed_regular_file(path: &Path) {
    let metadata = fs::symlink_metadata(path).expect("fixed adapter fixture path must exist");
    assert!(
        metadata.file_type().is_file(),
        "fixed path must be a regular file"
    );
    assert!(
        !metadata.file_type().is_symlink(),
        "fixed path must not be a link"
    );
}

fn read_limited(mut stream: impl Read + Send + 'static) -> thread::JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut bytes = Vec::with_capacity(MAX_OUTPUT_BYTES + 1);
        let mut chunk = [0_u8; 1024];
        while bytes.len() <= MAX_OUTPUT_BYTES {
            let remaining = MAX_OUTPUT_BYTES + 1 - bytes.len();
            let read_limit = chunk.len().min(remaining);
            match stream.read(&mut chunk[..read_limit]) {
                Ok(0) | Err(_) => break,
                Ok(read) => bytes.extend_from_slice(&chunk[..read]),
            }
        }
        bytes
    })
}

fn fixed_installed_wake_python() -> Option<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA")?;
    let python = PathBuf::from(local)
        .join("CatDeskWake")
        .join("runtime")
        .join("python.exe");
    python.is_file().then_some(python)
}

fn run_bounded_fixed_python_test(python: &Path, script: &Path, test_names: &[&str]) {
    require_fixed_regular_file(python);
    require_fixed_regular_file(script);

    let mut child = Command::new(python)
        .arg("-I")
        .arg(script)
        .args(test_names)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("fixed wake Python test must launch");
    let stdout = read_limited(child.stdout.take().expect("stdout pipe"));
    let stderr = read_limited(child.stderr.take().expect("stderr pipe"));
    let deadline = Instant::now() + FIXTURE_TIMEOUT;
    let status = loop {
        if let Some(status) = child.try_wait().expect("fixture status") {
            break status;
        }
        if Instant::now() >= deadline {
            child.kill().expect("kill timed out fixture");
            let _ = child.wait();
            panic!("fixed wake Python test timed out");
        }
        thread::sleep(Duration::from_millis(10));
    };
    let stdout = stdout.join().expect("stdout reader");
    let stderr = stderr.join().expect("stderr reader");
    assert!(
        stdout.len() <= MAX_OUTPUT_BYTES,
        "fixture stdout exceeded bound"
    );
    assert!(
        stderr.len() <= MAX_OUTPUT_BYTES,
        "fixture stderr exceeded bound"
    );
    assert!(
        status.success(),
        "fixed wake Python test failed: {}",
        String::from_utf8_lossy(&stderr)
    );
}

#[test]
fn t0430_active_generation_refresh_regression_runs_through_root_cargo_test() {
    let Some(python) = fixed_installed_wake_python() else {
        eprintln!("installed Wake Python runtime is not provisioned");
        return;
    };
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let response = workspace.join("tests").join("test_wake_bridge.py");
    run_bounded_fixed_python_test(
        &python,
        &response,
        &[
            "WakeBridgeTests.test_current_prosemirror_editor_fallback_is_bounded_and_used_for_text",
            "WakeBridgeTests.test_active_generation_periodically_refreshes_same_target_until_pause_clears",
            "WakeBridgeTests.test_response_timeout_retries_existing_turn_in_place_before_any_reload",
            "WakeBridgeTests.test_response_timeout_retry_does_not_reload_away_transient_retry_control",
            "WakeBridgeTests.test_response_completion_refuses_sequence_drift_before_retry",
            "WakeBridgeTests.test_submission_acceptance_refuses_same_document_append_without_server_turn_signal",
            "WakeBridgeTests.test_submission_acceptance_recovers_chrome_error_without_resubmitting",
            "WakeBridgeTests.test_submission_acceptance_does_not_reopen_non_chrome_target_drift",
            "WakeBridgeTests.test_turn_anchor_and_response_queries_accept_role_only_turn_dom",
            "WakeBridgeTests.test_post_submit_round_trip_target_drift_fails_closed",
            "WakeBrowserCleanupContractTests.test_every_wake_attempt_closes_only_its_owned_browser_in_finally",
            "WakeBrowserCleanupContractTests.test_owned_browser_cleanup_never_uses_browser_wide_quit_or_process_kill",
            "WakeBrowserCleanupContractTests.test_owned_browser_cleanup_failure_cannot_change_delivery_authority",
        ],
    );
}

#[test]
fn durable_reviewed_runtime_executes_exact_offline_adapter_fixture_when_provisioned() {
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let runtime = workspace.join(".catdesk/wake-bridge/stable-runtime-v1");
    if !runtime.exists() {
        // T-0264 deliberately does not provision the durable runtime on the
        // developer host. The isolated Rust runtime tests prove the complete
        // descriptor/identity contract; execution of this exact fixed Python
        // fixture becomes mandatory only after the separately gated host
        // provisioning procedure has installed the reviewed root.
        eprintln!("stable wake durable runtime is not provisioned");
        return;
    }
    let runtime_metadata =
        fs::symlink_metadata(&runtime).expect("durable runtime root metadata must be readable");
    assert!(
        runtime_metadata.is_dir() && !runtime_metadata.file_type().is_symlink(),
        "durable runtime root must be a non-link directory"
    );
    let python = runtime.join("python.exe");
    let fixture = workspace.join("tests/test_stable_wake_browser_adapter.py");
    require_fixed_regular_file(&python);
    require_fixed_regular_file(&fixture);

    let mut child = Command::new(&python)
        .arg(&fixture)
        .current_dir(&workspace)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("fixed wake venv must launch the exact offline fixture");
    let stdout = read_limited(child.stdout.take().expect("stdout pipe"));
    let stderr = read_limited(child.stderr.take().expect("stderr pipe"));
    let deadline = Instant::now() + FIXTURE_TIMEOUT;
    let status = loop {
        if let Some(status) = child.try_wait().expect("fixture status") {
            break status;
        }
        if Instant::now() >= deadline {
            child.kill().expect("kill timed out fixture");
            let _ = child.wait();
            panic!("fixed offline adapter fixture timed out");
        }
        thread::sleep(Duration::from_millis(10));
    };
    let stdout = stdout.join().expect("stdout reader");
    let stderr = stderr.join().expect("stderr reader");
    assert!(
        stdout.len() <= MAX_OUTPUT_BYTES,
        "fixture stdout exceeded bound"
    );
    assert!(
        stderr.len() <= MAX_OUTPUT_BYTES,
        "fixture stderr exceeded bound"
    );
    assert!(
        status.success(),
        "fixed offline adapter fixture failed: {}",
        String::from_utf8_lossy(&stderr)
    );
}
