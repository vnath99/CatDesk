use std::process::Command;

fn fixed_python() -> std::path::PathBuf {
    let local = std::env::var_os("LOCALAPPDATA").expect("LOCALAPPDATA");
    std::path::PathBuf::from(local)
        .join("CatDeskWake")
        .join("runtime")
        .join("python.exe")
}

#[test]
#[ignore = "manual Python bridge regression harness"]
fn bridge_python_regression_suite() {
    let python = fixed_python();
    let bridge = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("repo root")
        .join("scripts")
        .join("wake_bridge.py");
    let script = r#"
import importlib.util, pathlib, sys, tempfile
path = pathlib.Path(sys.argv[1])
spec = importlib.util.spec_from_file_location('wake_bridge_regression', path)
module = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = module
spec.loader.exec_module(module)
with tempfile.TemporaryDirectory() as td:
    profile = pathlib.Path(td)
    class Cdp:
        def __init__(self): self.reads = 0
        def get_current_url(self):
            self.reads += 1
            if self.reads == 1:
                return 'chrome-error://chromewebdata/'
            return 'https://chatgpt.com/c/review-one'
        def is_element_visible(self, _selector): return False
    cdp = Cdp()
    sink = module.CdpSink('https://chatgpt.com/c/review-one', profile, 1, 1)
    assert sink.readiness_reason(cdp) == 'BROWSER_NETWORK_ERROR'
    assert cdp.reads == 1
print('BRIDGE_CLASSIFIER_REGRESSION_OK')
"#;
    let output = Command::new(python)
        .arg("-I")
        .arg("-c")
        .arg(script)
        .arg(bridge)
        .output()
        .expect("run wake bridge classifier regression");
    assert!(
        output.status.success(),
        "python bridge regression failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("BRIDGE_CLASSIFIER_REGRESSION_OK"));
}

#[test]
fn adapter_and_response_python_regression_suite() {
    let python = fixed_python();
    if !python.is_file() {
        return;
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("repo root");

    let adapter = root.join("wake").join("tests").join("test_adapter.py");
    let adapter_output = Command::new(&python)
        .arg("-I")
        .arg(&adapter)
        .output()
        .expect("run independent adapter regression suite");
    assert!(
        adapter_output.status.success(),
        "Python adapter regression failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&adapter_output.stdout),
        String::from_utf8_lossy(&adapter_output.stderr)
    );

    let response = root.join("tests").join("test_wake_bridge.py");
    let output = Command::new(&python)
        .arg("-I")
        .arg(&response)
        .args([
            "WakeBridgeTests.test_submission_acceptance_uses_same_document_stop_signal_without_reload",
            "WakeBridgeTests.test_submission_acceptance_allows_response_that_completed_before_stop_poll",
            "WakeBridgeTests.test_submission_acceptance_refuses_same_document_append_without_server_turn_signal",
            "WakeBridgeTests.test_response_snapshot_accepts_hidden_but_present_completed_turn_action",
            "WakeBridgeTests.test_response_completion_waits_for_generation_to_finish_before_success",
            "WakeBridgeTests.test_active_generation_periodically_refreshes_same_target_until_pause_clears",
            "WakeBridgeTests.test_response_timeout_retries_existing_turn_in_place_before_any_reload",
            "WakeBridgeTests.test_response_timeout_retry_does_not_reload_away_transient_retry_control",
            "WakeBridgeTests.test_response_completion_refuses_sequence_drift_before_retry",
            "WakeBridgeTests.test_response_timeout_retry_control_is_fail_closed_when_ambiguous",
            "WakeBridgeTests.test_response_timeout_retry_budget_is_bounded",
            "WakeBridgeTests.test_response_retry_start_cannot_extend_total_deadline",
            "WakeBridgeTests.test_tool_pause_without_completion_controls_waits_for_resumed_generation",
            "WakeBridgeTests.test_production_waits_for_stop_to_clear_then_rechecks_exact_empty_unique_editor",
            "WakeBridgeTests.test_page_readiness_holds_slow_editor_stable_for_ten_seconds_before_accepting",
            "WakeBridgeTests.test_page_readiness_allows_transient_on_target_login_control_to_hydrate_to_editor",
            "WakeBridgeTests.test_page_readiness_blank_shell_reloads_once_then_stabilizes",
            "WakeBridgeTests.test_persistent_blank_shell_never_falls_through_to_second_reload",
            "WakeBridgeTests.test_readiness_resets_stability_after_editor_disappears",
            "WakeBridgeTests.test_network_diagnostics_export_only_fixed_codes",
            "WakeBridgeTests.test_network_diagnostics_preserve_bounded_recovery_and_terminal_reason",
            "WakeBridgeTests.test_outer_network_retry_reopens_only_before_first_browser_write",
            "WakeBridgeTests.test_outer_network_retry_stops_after_three_pre_write_sessions",
            "WakeBridgeTests.test_outer_retry_never_relaunches_for_non_network_attention",
            "WakeBridgeTests.test_outer_retry_never_relaunches_after_browser_write_starts",
            "WakeBridgeTests.test_missing_response_editor_obeys_generation_deadline",
            "WakeBridgeTests.test_missing_receipt_after_stop_hold_keeps_polling_without_busy_loop",
            "WakeBridgeTests.test_production_post_submit_rejects_unstable_final_digest_target_or_composer",
            "WakeBridgeTests.test_page_readiness_all_three_fail_keeps_specific_reason_and_only_retries_between_windows",
            "WakeBridgeTests.test_retry_page_readiness_waits_for_delayed_home_to_exact_transition",
            "WakeBridgeTests.test_retry_page_readiness_persistent_home_waits_full_settle_window",
        ])
        .output()
        .expect("run bounded response-completion regression suite");
    assert!(
        output.status.success(),
        "Python response-completion regression failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn dev54_post_submit_receipt_round_trip_is_editor_independent() {
    let python = fixed_python();
    if !python.is_file() {
        return;
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("repo root");
    let test_file = root.join("tests").join("test_wake_bridge.py");
    let output = Command::new(&python)
        .arg("-I")
        .arg(&test_file)
        .arg("WakeBridgeTests.test_post_submit_receipt_requires_fresh_document_round_trip")
        .arg("WakeBridgeTests.test_post_submit_round_trip_target_drift_fails_closed")
        .output()
        .expect("run dev54 post-submit receipt regression");
    assert!(
        output.status.success(),
        "dev54 post-submit receipt regression failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn dev54_adapter_reconciliation_remains_observe_only() {
    let python = fixed_python();
    if !python.is_file() {
        return;
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("repo root");
    let test_file = root.join("wake").join("tests").join("test_adapter.py");
    let output = Command::new(&python)
        .arg("-I")
        .arg(&test_file)
        .arg("AdapterTests.test_reconciliation_observes_only_and_never_submits")
        .output()
        .expect("run dev54 adapter reconciliation regression");
    assert!(
        output.status.success(),
        "dev54 adapter reconciliation regression failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn dev54_same_tab_post_submit_acceptance_and_persistence() {
    let python = fixed_python();
    if !python.is_file() {
        return;
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("repo root");

    let adapter = root.join("wake").join("tests").join("test_adapter.py");
    let adapter_output = Command::new(&python)
        .arg("-I")
        .arg(&adapter)
        .arg(
            "AdapterTests.test_success_preserves_sender_until_response_then_proves_durable_receipt",
        )
        .arg("AdapterTests.test_reconciliation_observes_only_and_never_submits")
        .output()
        .expect("run dev68 adapter ordering regression");
    assert!(
        adapter_output.status.success(),
        "dev68 adapter regression failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&adapter_output.stdout),
        String::from_utf8_lossy(&adapter_output.stderr)
    );

    let response = root.join("tests").join("test_wake_bridge.py");
    let response_output = Command::new(&python)
        .arg("-I")
        .arg(&response)
        .arg("WakeBridgeTests.test_submission_acceptance_uses_same_document_stop_signal_without_reload")
        .arg("WakeBridgeTests.test_submission_acceptance_allows_response_that_completed_before_stop_poll")
        .arg("WakeBridgeTests.test_submission_acceptance_refuses_same_document_append_without_server_turn_signal")
        .arg("WakeBrowserCleanupContractTests.test_every_wake_attempt_closes_only_its_owned_browser_in_finally")
        .arg("WakeBridgeTests.test_cdp_sink_valid_receipt_uses_smoke_compatible_direct_click")
        .output()
        .expect("run dev68 same-tab response regression");
    assert!(
        response_output.status.success(),
        "dev68 response regression failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&response_output.stdout),
        String::from_utf8_lossy(&response_output.stderr)
    );
}
