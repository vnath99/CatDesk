//! Entry authority regressions; these never launch a host or browser.
use super::*;
use serde_json::Value;
use std::collections::BTreeMap;

fn package(root: &Path, id: &str) -> (PathBuf, Value) {
    let directory = root.join("versions").join(id);
    std::fs::create_dir_all(&directory).unwrap();
    let mut hashes = BTreeMap::new();
    for name in [
        "CatDeskWakeHost.exe",
        "CatDeskBinagotchy.exe",
        "adapter.py",
        "wake_bridge.py",
    ] {
        let bytes = format!("inert {id} {name}; never executed");
        std::fs::write(directory.join(name), bytes.as_bytes()).unwrap();
        hashes.insert(name, crate::protocol::digest(&bytes));
    }
    atomic(
        &directory.join("manifest.json"),
        &serde_json::json!({
            "schemaVersion":1,"protocolVersion":1,"artifacts":hashes
        }),
    )
    .unwrap();
    (
        directory.join("CatDeskWakeHost.exe"),
        serde_json::json!({
            "schemaVersion":1,"directory":id,"hostSha256":hashes["CatDeskWakeHost.exe"],
            "binagotchySha256":hashes["CatDeskBinagotchy.exe"],"adapterSha256":hashes["adapter.py"],
            "wakeBridgeSha256":hashes["wake_bridge.py"]
        }),
    )
}

fn fixture() -> (Store, PathBuf, Value, PathBuf, Value) {
    let (store, lease) = scheduler::tests::fixture("host-entry-selected-image");
    drop(lease);
    control(&store, "STOPPED").unwrap();
    // Existing status/journal bytes make accidental startup writes observable.
    atomic(
        &store.root().join("status.json"),
        &serde_json::json!({"preserved":"prior status"}),
    )
    .unwrap();
    std::fs::write(
        store.root().join("held-task.json"),
        b"UNKNOWN remains UNKNOWN",
    )
    .unwrap();
    let (a, pa) = package(store.root(), "fixture-A");
    let (b, pb) = package(store.root(), "fixture-B");
    (store, a, pa, b, pb)
}

#[derive(Debug, PartialEq, Eq)]
enum SnapshotEntry {
    Data(Vec<u8>),
    HostLock {
        is_file: bool,
        is_symlink: bool,
        length: u64,
    },
}

fn snapshot(root: &Path) -> BTreeMap<PathBuf, SnapshotEntry> {
    let mut found = BTreeMap::new();
    let mut pending = vec![root.to_owned()];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            let relative = path.strip_prefix(root).unwrap().to_owned();
            if relative == Path::new("host.lock") {
                // Windows refuses body reads while our real singleton lease
                // owns its byte range. Preserve that lease and compare the
                // existing lock object's presence/type/length, not its body.
                let metadata = std::fs::symlink_metadata(&path).unwrap();
                found.insert(
                    relative,
                    SnapshotEntry::HostLock {
                        is_file: metadata.is_file(),
                        is_symlink: metadata.file_type().is_symlink(),
                        length: metadata.len(),
                    },
                );
                continue;
            }
            if path.is_dir() {
                pending.push(path);
            } else {
                found.insert(relative, SnapshotEntry::Data(std::fs::read(path).unwrap()));
            }
        }
    }
    found
}

#[test]
fn stale_captured_a_refuses_after_lease_acquisition_when_current_is_b() {
    let (store, a, pa, b, pb) = fixture();
    atomic(&store.root().join("current.json"), &pa).unwrap();
    // The supported launcher captured A before the selection transaction.
    let captured = a;
    atomic(&store.root().join("current.json"), &pb).unwrap();
    let _root = crate::install_authority::HostRootPin::acquire(store.root()).unwrap();
    let lease = store.host_lock().unwrap();
    let before = snapshot(store.root());
    assert_eq!(
        verify_current_host_authority_at(&store, &captured).unwrap_err(),
        "HOST_INSTALL_POINTER_MISMATCH"
    );
    verify_current_host_authority_at(&store, &b).unwrap();
    assert_eq!(snapshot(store.root()), before);
    drop(lease);
    assert!(store.host_lock().is_ok());
}

#[test]
fn actual_host_entries_refuse_foreign_image_before_owner_status_or_journal_effects() {
    let (store, _, _, _, pb) = fixture();
    atomic(&store.root().join("current.json"), &pb).unwrap();
    // Precreate the existing singleton object so a first lock-file creation is
    // not confused with a status, task or dispatch mutation.
    drop(store.host_lock().unwrap());
    let before = snapshot(store.root());
    assert_eq!(run(&store).unwrap_err(), "HOST_INSTALL_POINTER_MISMATCH");
    assert_eq!(snapshot(store.root()), before);
    assert_eq!(
        run_resilient(&store).unwrap_err(),
        "HOST_INSTALL_POINTER_MISMATCH"
    );
    assert_eq!(snapshot(store.root()), before);
    assert!(
        store.host_lock().is_ok(),
        "failed admission releases its singleton lease"
    );
}

#[test]
fn missing_or_corrupt_pointer_and_tampered_selected_host_fail_without_state_writes() {
    let (store, _, _, b, pb) = fixture();
    drop(store.host_lock().unwrap());
    let before = snapshot(store.root());
    assert!(run_resilient(&store).is_err());
    assert_eq!(snapshot(store.root()), before);
    std::fs::write(store.root().join("current.json"), b"{not json").unwrap();
    let before = snapshot(store.root());
    assert!(run_resilient(&store).is_err());
    assert_eq!(snapshot(store.root()), before);
    atomic(&store.root().join("current.json"), &pb).unwrap();
    std::fs::write(&b, b"damaged selected image").unwrap();
    let lease = store.host_lock().unwrap();
    let before = snapshot(store.root());
    assert_eq!(
        verify_current_host_authority_at(&store, &b).unwrap_err(),
        "HOST_EXECUTABLE_HASH_MISMATCH"
    );
    assert_eq!(snapshot(store.root()), before);
    drop(lease);
    assert!(store.host_lock().is_ok());
}

#[test]
fn already_owned_host_refuses_before_selection_or_runtime_effects() {
    let (store, _, _, _, pb) = fixture();
    atomic(&store.root().join("current.json"), &pb).unwrap();
    let _lease = store.host_lock().unwrap();
    let before = snapshot(store.root());
    assert_eq!(run_resilient(&store).unwrap_err(), "HOST_ALREADY_RUNNING");
    assert_eq!(snapshot(store.root()), before);
}
