use std::process::Command;

fn main() -> Result<(), String> {
    let local = std::env::var_os("LOCALAPPDATA").ok_or("LOCALAPPDATA")?;
    let python = std::path::PathBuf::from(local)
        .join("CatDeskWake")
        .join("runtime")
        .join("python.exe");
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("REPO_ROOT")?;
    let test_file = repo.join("tests").join("test_wake_bridge.py");
    let script = r#"
import importlib.util, pathlib, sys, unittest
path = pathlib.Path(sys.argv[1])
spec = importlib.util.spec_from_file_location('selected_wake_bridge_tests', path)
mod = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = mod
spec.loader.exec_module(mod)
names = [
    'test_bounded_route_class_never_returns_identifiers',
    'test_readiness_classifies_chrome_error_from_one_authoritative_url_snapshot',
    'test_page_readiness_reopens_exact_target_when_reload_leaves_chrome_error_document',
    'test_page_readiness_reopens_exact_target_when_cdp_shell_falls_back_home',
    'test_retry_page_readiness_prefers_active_cdp_open_for_home_recovery',
    'test_home_login_control_is_route_drift_not_auth_loss',
    'test_production_post_submit_requires_two_stable_receipt_polls_and_honors_stop_hold',
    'test_submitting_reconciliation_requires_two_exact_persisted_observations_and_never_submits',
    'test_submitting_reconciliation_rejects_duplicate_or_target_drift',
    'test_response_completion_waits_for_generation_to_finish_before_success',
    'test_active_generation_periodically_refreshes_same_target_until_pause_clears',
    'test_response_timeout_retries_existing_turn_in_place_before_any_reload',
    'test_response_timeout_retry_does_not_reload_away_transient_retry_control',
    'test_response_completion_refuses_sequence_drift_before_retry',
    'test_response_timeout_retry_control_is_fail_closed_when_ambiguous',
    'test_response_timeout_retry_budget_is_bounded',
]
suite = unittest.TestSuite(mod.WakeBridgeTests(name) for name in names)
result = unittest.TextTestRunner(verbosity=1).run(suite)
raise SystemExit(0 if result.wasSuccessful() else 1)
"#;
    let out = Command::new(python)
        .current_dir(repo)
        .arg("-I")
        .arg("-c")
        .arg(script)
        .arg(test_file)
        .output()
        .map_err(|_| "PYTHON_SELECTED_TEST_START_FAILED")?;
    println!("PYTHON_EXIT={}", out.status);
    print!("{}", String::from_utf8_lossy(&out.stdout));
    eprint!("{}", String::from_utf8_lossy(&out.stderr));
    if out.status.success() {
        Ok(())
    } else {
        Err("PYTHON_SELECTED_TEST_FAILED".into())
    }
}
