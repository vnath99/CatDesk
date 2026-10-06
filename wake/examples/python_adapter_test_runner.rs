use std::process::Command;

fn emit(label: &str, output: &std::process::Output) {
    println!("{label}_EXIT={}", output.status);
    println!("{label}_STDOUT_BEGIN");
    print!("{}", String::from_utf8_lossy(&output.stdout));
    println!("{label}_STDOUT_END");
    println!("{label}_STDERR_BEGIN");
    print!("{}", String::from_utf8_lossy(&output.stderr));
    println!("{label}_STDERR_END");
}

fn main() -> Result<(), String> {
    let local = std::env::var_os("LOCALAPPDATA").ok_or("LOCALAPPDATA")?;
    let python = std::path::PathBuf::from(local)
        .join("CatDeskWake")
        .join("runtime")
        .join("python.exe");
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("REPO_ROOT")?;

    let adapter = Command::new(&python)
        .current_dir(repo)
        .arg("wake/tests/test_adapter.py")
        .output()
        .map_err(|_| "PYTHON_ADAPTER_TEST_START_FAILED")?;
    emit("ADAPTER", &adapter);
    if !adapter.status.success() {
        return Err("PYTHON_ADAPTER_TEST_FAILED".into());
    }

    let bridge_test = repo.join("tests").join("test_wake_bridge.py");
    let script = r#"
import importlib.util, pathlib, sys, unittest
path = pathlib.Path(sys.argv[1])
spec = importlib.util.spec_from_file_location('wake_bridge_dev54_tests', path)
mod = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = mod
spec.loader.exec_module(mod)
names = [
    'test_post_submit_receipt_requires_fresh_document_round_trip',
    'test_post_submit_round_trip_target_drift_fails_closed',
    'test_production_post_submit_requires_two_stable_receipt_polls_and_honors_stop_hold',
    'test_response_completion_waits_for_generation_to_finish_before_success',
    'test_response_timeout_reopens_exact_target_and_retries_existing_turn_only',
    'test_response_timeout_reopen_can_prove_completion_without_duplicate_retry',
    'test_response_completion_refuses_sequence_drift_before_retry',
    'test_response_timeout_retry_control_is_fail_closed_when_ambiguous',
    'test_response_timeout_retry_budget_is_bounded',
]
suite = unittest.TestSuite(mod.WakeBridgeTests(name) for name in names)
result = unittest.TextTestRunner(verbosity=1).run(suite)
raise SystemExit(0 if result.wasSuccessful() else 1)
"#;
    let bridge = Command::new(&python)
        .current_dir(repo)
        .arg("-I")
        .arg("-c")
        .arg(script)
        .arg(bridge_test)
        .output()
        .map_err(|_| "PYTHON_BRIDGE_TEST_START_FAILED")?;
    emit("BRIDGE", &bridge);
    if !bridge.status.success() {
        return Err("PYTHON_BRIDGE_TEST_FAILED".into());
    }

    Ok(())
}
