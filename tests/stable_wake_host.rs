use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

const HOST: &str = env!("CARGO_BIN_EXE_catdesk-stable-wake-host");

fn workspace(name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("catdesk-stable-host-{name}-{nonce}"));
    fs::create_dir_all(root.join(".catdesk/autonomy")).expect("autonomy root");
    fs::create_dir_all(root.join(".catdesk/wake-bridge")).expect("wake root");
    root
}

fn inbox_path(root: &Path) -> PathBuf {
    root.join(".catdesk/autonomy/review-inbox.json")
}

fn config_path(root: &Path) -> PathBuf {
    root.join(".catdesk/wake-bridge/config.json")
}

fn record(id: &str) -> Value {
    json!({
        "schemaVersion": 1,
        "recordId": id,
        "projectId": "catdesk",
        "sessionId": "session_1",
        "state": "COMPLETED_VERIFIED",
        "nextAction": "independent_final_review",
        "reference": "artifacts/completion.json",
        "createdAtUnix": 1,
        "unread": true,
    })
}

fn write_ready_inputs(root: &Path) {
    fs::write(
        inbox_path(root),
        serde_json::to_vec(&json!([record("review_1")])).expect("inbox"),
    )
    .expect("write inbox");
    fs::write(
        config_path(root),
        r#"{"conversation_url":"https://chatgpt.com/c/exact-thread","profile_dir":".catdesk/wake-bridge/browser-profile"}"#,
    )
    .expect("write config");
}

fn run_host(root: &Path, expected_digest: Option<&str>) -> (i32, Value, String) {
    let mut command = Command::new(HOST);
    command.arg("--workspace").arg(root);
    if let Some(digest) = expected_digest {
        command.arg("--expected-target-sha256").arg(digest);
    }
    let output = command.output().expect("host child process");
    let stdout = String::from_utf8(output.stdout).expect("utf8 output");
    let payload: Value = serde_json::from_str(stdout.trim()).expect("bounded JSON output");
    (output.status.code().unwrap_or(-1), payload, stdout)
}

fn assert_refused(root: &Path) {
    let (code, payload, _) = run_host(root, None);
    assert_eq!(code, 2);
    assert_eq!(
        payload.get("status").and_then(Value::as_str),
        Some("REFUSED")
    );
    assert!(payload.get("reason").and_then(Value::as_str).is_some());
}

#[test]
fn child_process_is_independent_of_actual_release_daemon_and_lkg_fixture_states() {
    let root = workspace("runtime-independence");
    write_ready_inputs(&root);
    let inbox_before = fs::read(inbox_path(&root)).expect("inbox before");
    let config_before = fs::read(config_path(&root)).expect("config before");

    // Condition one: the release binary is actually absent; no listener or
    // tunnel artifact is created anywhere in this fixture.
    assert!(!root.join("target/release/catdesk.exe").exists());
    let (code_absent, ready_absent, stdout_absent) = run_host(&root, None);
    assert_eq!(code_absent, 0);
    assert_eq!(
        ready_absent.get("status").and_then(Value::as_str),
        Some("READY")
    );
    assert_eq!(
        ready_absent.get("actionableCount").and_then(Value::as_u64),
        Some(1)
    );
    assert!(!stdout_absent.contains("https://chatgpt.com"));

    // Condition two: an unrelated replacement binary is present.  It is never
    // read or executed by the child host.
    fs::create_dir_all(root.join("target/release")).expect("replacement root");
    fs::write(
        root.join("target/release/catdesk.exe"),
        b"replacement-not-catdesk",
    )
    .expect("replacement");
    let (code_replaced, ready_replaced, _) = run_host(&root, None);
    assert_eq!(code_replaced, 0);
    assert_eq!(ready_replaced, ready_absent);

    // These are actual malformed/stale durable release artifacts.  The host
    // does not inspect them because they are not R1A inputs.
    fs::create_dir_all(root.join(".catdesk/reviewed-release")).expect("manifest root");
    fs::write(
        root.join(".catdesk/reviewed-release/manifest.json"),
        b"not-json",
    )
    .expect("bad manifest");
    fs::create_dir_all(root.join(".catdesk/promotion")).expect("promotion root");
    fs::write(root.join(".catdesk/promotion/lkg.json"), b"stale-lkg").expect("stale lkg");
    let (code_artifacts, ready_artifacts, _) = run_host(&root, None);
    assert_eq!(code_artifacts, 0);
    assert_eq!(ready_artifacts, ready_absent);

    assert_eq!(fs::read(inbox_path(&root)).unwrap(), inbox_before);
    assert_eq!(fs::read(config_path(&root)).unwrap(), config_before);
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn child_process_refuses_target_integrity_failures_without_mutating_inputs() {
    let root = workspace("target-refusal");
    write_ready_inputs(&root);
    let (_, ready, _) = run_host(&root, None);
    let original_digest = ready["targetSha256"].as_str().expect("digest").to_owned();
    let inbox_before = fs::read(inbox_path(&root)).unwrap();

    fs::remove_file(config_path(&root)).unwrap();
    assert_refused(&root);
    fs::write(
        config_path(&root),
        r#"{"conversation_url":"https://chatgpt.com/c/exact-thread","profile_dir":".catdesk/wake-bridge/browser-profile"}"#,
    )
    .unwrap();

    fs::write(
        config_path(&root),
        r#"{"conversation_url":"https://chatgpt.com/c/other-thread","profile_dir":".catdesk/wake-bridge/browser-profile"}"#,
    )
    .unwrap();
    let config_before_drift_check = fs::read(config_path(&root)).unwrap();
    let (code, payload, _) = run_host(&root, Some(&original_digest));
    assert_eq!(code, 2);
    assert_eq!(payload["reason"], "wake target binding drifted");
    assert_eq!(
        fs::read(config_path(&root)).unwrap(),
        config_before_drift_check
    );

    fs::write(config_path(&root), b"not-json").unwrap();
    assert_refused(&root);
    fs::write(
        config_path(&root),
        r#"{"conversation_url":"https://chatgpt.com/c/one","conversation_url":"https://chatgpt.com/c/two","profile_dir":".catdesk/wake-bridge/browser-profile"}"#,
    )
    .unwrap();
    assert_refused(&root);
    fs::write(config_path(&root), vec![b'x'; 16 * 1024 + 1]).unwrap();
    assert_refused(&root);
    assert_eq!(fs::read(inbox_path(&root)).unwrap(), inbox_before);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn child_process_refuses_canonical_inbox_attacks_and_conflicts_read_only() {
    let root = workspace("inbox-refusal");
    write_ready_inputs(&root);
    let config_before = fs::read(config_path(&root)).unwrap();

    fs::write(inbox_path(&root), b"not-json").unwrap();
    assert_refused(&root);
    fs::write(inbox_path(&root), vec![b'x'; 2 * 1024 * 1024 + 1]).unwrap();
    assert_refused(&root);

    let mut wrong_project = record("review_1");
    wrong_project["projectId"] = json!("other");
    fs::write(
        inbox_path(&root),
        serde_json::to_vec(&json!([wrong_project])).unwrap(),
    )
    .unwrap();
    assert_refused(&root);

    let mut wrong_schema = record("review_1");
    wrong_schema["schemaVersion"] = json!(2);
    fs::write(
        inbox_path(&root),
        serde_json::to_vec(&json!([wrong_schema])).unwrap(),
    )
    .unwrap();
    assert_refused(&root);

    let mut invalid_identity = record("review_1");
    invalid_identity["sessionId"] = json!("bad identity");
    fs::write(
        inbox_path(&root),
        serde_json::to_vec(&json!([invalid_identity])).unwrap(),
    )
    .unwrap();
    assert_refused(&root);

    let mut oversized_reference = record("review_1");
    oversized_reference["reference"] = json!("a".repeat(1025));
    fs::write(
        inbox_path(&root),
        serde_json::to_vec(&json!([oversized_reference])).unwrap(),
    )
    .unwrap();
    assert_refused(&root);

    let mut traversal = record("review_1");
    traversal["reference"] = json!("../outside.json");
    fs::write(
        inbox_path(&root),
        serde_json::to_vec(&json!([traversal])).unwrap(),
    )
    .unwrap();
    assert_refused(&root);

    let mut rooted = record("review_1");
    rooted["reference"] = json!("/rooted.json");
    fs::write(
        inbox_path(&root),
        serde_json::to_vec(&json!([rooted])).unwrap(),
    )
    .unwrap();
    assert_refused(&root);

    let mut conflict = record("review_1");
    conflict["createdAtUnix"] = json!(2);
    fs::write(
        inbox_path(&root),
        serde_json::to_vec(&json!([record("review_1"), conflict])).unwrap(),
    )
    .unwrap();
    let before_conflict = fs::read(inbox_path(&root)).unwrap();
    assert_refused(&root);
    assert_eq!(fs::read(inbox_path(&root)).unwrap(), before_conflict);
    assert_eq!(fs::read(config_path(&root)).unwrap(), config_before);

    let records: Vec<_> = (0..=512)
        .map(|index| record(&format!("review_{index}")))
        .collect();
    fs::write(inbox_path(&root), serde_json::to_vec(&records).unwrap()).unwrap();
    assert_refused(&root);

    fs::remove_file(inbox_path(&root)).unwrap();
    fs::create_dir(inbox_path(&root)).unwrap();
    assert_refused(&root);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn standalone_source_has_no_runtime_or_control_plane_dependency() {
    let binary = include_str!("../src/bin/catdesk-stable-wake-host.rs");
    let core = include_str!("../src/stable_wake_core.rs");
    let binary_production = binary
        .split("#[cfg(test)]")
        .next()
        .expect("binary production");
    let core_production = core.split("#[cfg(test)]").next().expect("core production");
    for forbidden in ["crate::", "super::", "mod server", "mod mcp", "AppState"] {
        assert!(
            !binary_production.contains(forbidden) && !core_production.contains(forbidden),
            "forbidden standalone dependency: {forbidden}"
        );
    }
    assert!(core_production.contains("review-inbox.json"));
    assert!(!core_production.contains("stable-wake/review-events"));
}
