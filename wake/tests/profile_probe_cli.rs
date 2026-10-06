use catdesk_wake::{
    runtime,
    store::{Store, atomic},
};
use std::{fs, process::Command};

#[test]
fn probe_refuses_running_or_unacknowledged_pause_before_launch() {
    let base = std::env::temp_dir().join(format!("catdesk-probe-cli-{}", std::process::id()));
    fs::create_dir_all(&base).unwrap();
    let root = base.join("CatDeskWake");
    let store = Store::open_scoped_for_test(&root, &base).unwrap();
    store.initialize().unwrap();
    fs::create_dir_all(root.join("runtime")).unwrap();
    fs::create_dir_all(root.join("browser-profile")).unwrap();
    // Deliberately not executable: guards must return before attempting any launch.
    fs::write(root.join("runtime/python.exe"), b"not an executable").unwrap();
    let mut status = runtime::status(&store).unwrap();
    status.host = "RUNNING".into();
    atomic(&root.join("status.json"), &status).unwrap();
    for (desired, expected) in [
        ("RUNNING", "PROBE_REQUIRES_PAUSED_WAKE"),
        ("PAUSED", "PROBE_REQUIRES_PAUSE_ACKNOWLEDGEMENT"),
    ] {
        runtime::control(&store, desired).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_CatDeskWakeProfileProbe"))
            .env("LOCALAPPDATA", &base)
            .env("CATDESK_WAKE_TEST_ONLY_ROOT_PARENT", &base)
            .output()
            .unwrap();
        assert!(!output.status.success());
        let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["code"], expected);
        assert!(store.events().unwrap().is_empty());
    }
    fs::remove_dir_all(base).unwrap();
}
