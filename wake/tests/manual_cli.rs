#![cfg(feature = "test-support")]

use catdesk_wake::store::Store;
use std::{fs, process::Command};

#[test]
fn manual_cli_requires_matching_expected_target_before_publication_or_start() {
    let base = std::env::temp_dir().join(format!("catdesk-manual-cli-{}", std::process::id()));
    fs::create_dir_all(&base).unwrap();
    let root = base.join("CatDeskWake");
    let store = Store::open_scoped_for_test(&root, &base).unwrap();
    store.initialize().unwrap();
    let target = store
        .set_target("catdesk", 0, "https://chatgpt.com/c/fixture")
        .unwrap();
    for args in [
        vec!["test-event", "manual-no-target"],
        vec![
            "test-event",
            "manual-wrong-generation",
            "16",
            target.digest.as_str(),
        ],
        vec!["test-event", "manual-wrong-digest", "1", "wrong"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_CatDeskWakeHost"))
            .args(args)
            .env("LOCALAPPDATA", &base)
            .env("CATDESK_WAKE_TEST_ONLY_ROOT_PARENT", &base)
            .output()
            .unwrap();
        assert!(!output.status.success());
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(
            text.contains("CLI_ARGUMENTS_INVALID") || text.contains("TARGET_EXPECTATION_MISMATCH"),
            "{text}"
        );
        assert!(store.events().unwrap().is_empty());
        assert!(!root.join("control.json").exists());
        assert!(!root.join("status.json").exists());
    }
    let output = Command::new(env!("CARGO_BIN_EXE_CatDeskWakeHost"))
        .args(["event-status", "manual-not-published"])
        .env("LOCALAPPDATA", &base)
        .env("CATDESK_WAKE_TEST_ONLY_ROOT_PARENT", &base)
        .output()
        .unwrap();
    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(value["delivery"].is_null());
    assert!(value["timer"].is_null());
    assert!(store.events().unwrap().is_empty());
    let output = Command::new(env!("CARGO_BIN_EXE_CatDeskWakeHost"))
        .args(["event-status", "../config"])
        .env("LOCALAPPDATA", &base)
        .env("CATDESK_WAKE_TEST_ONLY_ROOT_PARENT", &base)
        .output()
        .unwrap();
    assert!(!output.status.success());

    let output = Command::new(env!("CARGO_BIN_EXE_CatDeskWakeHost"))
        .arg("readiness-history")
        .env("LOCALAPPDATA", &base)
        .env("CATDESK_WAKE_TEST_ONLY_ROOT_PARENT", &base)
        .output()
        .unwrap();
    assert!(output.status.success());
    let history: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(history, serde_json::json!([]));
    assert!(!root.join("status.json").exists());

    let output = Command::new(env!("CARGO_BIN_EXE_CatDeskWakeHost"))
        .arg("store-provenance")
        .env("LOCALAPPDATA", &base)
        .env("CATDESK_WAKE_TEST_ONLY_ROOT_PARENT", &base)
        .output()
        .unwrap();
    assert!(output.status.success());
    let provenance: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(provenance["schemaVersion"], 1);
    assert!(provenance["rootPathSha256"].as_str().is_some());
    assert!(provenance["configSha256"].as_str().is_some());
    assert!(provenance["currentSha256"].is_null());
    assert!(provenance["statusSha256"].is_null());
    assert!(!root.join("status.json").exists());

    let missing_parent = base.join("provenance-absent");
    let output = Command::new(env!("CARGO_BIN_EXE_CatDeskWakeHost"))
        .arg("store-provenance")
        .env("LOCALAPPDATA", &missing_parent)
        .env("CATDESK_WAKE_TEST_ONLY_ROOT_PARENT", &missing_parent)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(!missing_parent.exists());

    fs::remove_dir_all(base).unwrap();
}
