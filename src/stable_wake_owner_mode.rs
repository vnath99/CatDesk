//! Closed, fail-closed wake-owner selection and fixture-only activation.
//!
//! The selector is the sole durable owner choice.  It is intentionally not a
//! general configuration interface: all paths below are fixed product paths
//! below the supplied workspace, and callers may supply only the expected
//! *current* owner for compare-and-swap.

use crate::{
    stable_wake_adapter_runtime::DurableAdapterRuntime,
    stable_wake_core::{
        canonical_inbox_records, canonical_wake_target, read_protected_wake_target,
    },
    stable_wake_delivery::StableWakeDelivery,
};
use serde::Deserialize;
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

const MAX_OWNER_CONFIG_BYTES: u64 = 1024;
const OWNER_SCHEMA_VERSION: u32 = 1;
static SELECTOR_TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);
#[cfg(test)]
thread_local! {
    static SELECTOR_FAIL_BEFORE_REPLACE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WakeOwnerMode {
    LegacyPython,
    Rust,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OwnerConfigV1 {
    schema_version: u32,
    owner: String,
}

/// Reads only the fixed project-owned selector. Missing means the documented
/// legacy-safe default; malformed selection authorizes neither owner.
pub(crate) fn selected_owner(workspace: &Path) -> Result<WakeOwnerMode, String> {
    let wake_bridge = wake_bridge_root(workspace)?;
    let selector = wake_bridge.join("owner.json");
    match fs::symlink_metadata(&selector) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(WakeOwnerMode::LegacyPython)
        }
        Err(_) => Err("stable wake owner selection unavailable".into()),
        Ok(_) => parse_selector(&selector),
    }
}

/// Atomically move the fixed project selector from the exact expected owner
/// to Rust after every stable prerequisite has been revalidated. This source
/// API is deliberately dormant: it is not wired to an MCP, browser, daemon,
/// or installation surface.
#[allow(dead_code)] // The no-argument activation binary owns the production call.
pub(crate) fn activate_reviewed_rust_owner(
    workspace: &Path,
    expected_old: WakeOwnerMode,
) -> Result<WakeOwnerMode, String> {
    if expected_old != WakeOwnerMode::LegacyPython && expected_old != WakeOwnerMode::Rust {
        return Err("stable wake owner expected selection invalid".into());
    }
    let workspace = canonical_directory(workspace)?;
    let delivery = StableWakeDelivery::open(&workspace)?;
    // Reuse the crash-recoverable kernel mutex already used by schema-4
    // transitions. Its release is tied to process death, not lock-file cleanup.
    let _selector_lock = StableWakeDelivery::acquire_lock_for_probe(&workspace)?;
    activation_preflight(&workspace, &delivery)?;
    let wake_bridge = wake_bridge_root(&workspace)?;
    let selector = wake_bridge.join("owner.json");
    let current = match fs::symlink_metadata(&selector) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => WakeOwnerMode::LegacyPython,
        Err(_) => return Err("stable wake owner selection unavailable".into()),
        Ok(_) => parse_selector(&selector)?,
    };
    if current != expected_old {
        return Err("stable wake owner selection compare-and-swap stale".into());
    }
    if current == WakeOwnerMode::Rust {
        // Idempotence is accepted only after the full preflight above and a
        // protected readback of the already-exact selection.
        return Ok(WakeOwnerMode::Rust);
    }
    atomic_write_selector(&wake_bridge)?;
    match parse_selector(&selector)? {
        WakeOwnerMode::Rust => Ok(WakeOwnerMode::Rust),
        WakeOwnerMode::LegacyPython => Err("stable wake owner selection readback mismatch".into()),
    }
}

fn activation_preflight(workspace: &Path, delivery: &StableWakeDelivery) -> Result<(), String> {
    // Canonical event and protected target authority remain their accepted,
    // independent parsers; neither is rewritten or acknowledged here.
    let _ = canonical_inbox_records(workspace, "catdesk")?;
    let _ = read_protected_wake_target(workspace)?;
    delivery.validate_cutover_safe()?;

    // The reviewed owner, interpreter, adapter, browser primitives, and
    // descriptor are all bound from the fixed durable runtime root.  This
    // intentionally has no current release-directory, source-script, or
    // repository venv prerequisite.
    let _runtime = DurableAdapterRuntime::open(workspace)?;
    Ok(())
}

fn reconcile_independent_target(
    wake: &catdesk_wake::store::Store,
    authoritative_url: &str,
    apply: bool,
) -> Result<Option<catdesk_wake::protocol::Target>, String> {
    let config = match wake.config() {
        Ok(config) => Some(config),
        Err(error) if error == "STATE_UNAVAILABLE" && !apply => None,
        Err(error) if error == "STATE_UNAVAILABLE" => {
            wake.initialize()?;
            Some(wake.config()?)
        }
        Err(error) => return Err(error),
    };
    let existing = config
        .as_ref()
        .and_then(|config| config.targets.get("catdesk"))
        .cloned();
    if let Some(target) = existing {
        target.validate()?;
        if target.url != authoritative_url {
            return Err("MIGRATION_TARGET_MISMATCH".into());
        }
        return Ok(Some(target));
    }
    if !apply {
        return Ok(None);
    }
    wake.set_target("catdesk", 0, authoritative_url).map(Some)
}

/// Supported fixed-path migration. No historical event is replayed or rewritten.
/// Earlier CatDesk generations reject the new selector, including after rollback.
#[allow(dead_code)] // Only the dedicated migration executable invokes this operation.
pub(crate) fn migrate_independent_owner(
    workspace: &Path,
    preflight: bool,
) -> Result<String, String> {
    use catdesk_wake::{
        runtime::{Activation, default_root},
        store::{Store, atomic, now, read},
    };
    use sha2::{Digest, Sha256};
    let workspace = canonical_directory(workspace)?;
    let bridge = wake_bridge_root(&workspace)?;
    let _selector_lock = StableWakeDelivery::acquire_lock_for_probe(&workspace)?;
    let legacy_path = bridge.join("wake-bridge.lock");
    let metadata = fs::symlink_metadata(&legacy_path).map_err(|_| "LEGACY_LOCK_UNAVAILABLE")?;
    if !metadata.is_file() || unsafe_link(&metadata) {
        return Err("LEGACY_LOCK_UNSAFE".into());
    }
    let legacy_lock = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&legacy_path)
        .map_err(|_| "LEGACY_LOCK_UNAVAILABLE")?;
    legacy_lock
        .try_lock()
        .map_err(|_| "LEGACY_OWNER_STILL_RUNNING")?;
    let protected_target = read_protected_wake_target(&workspace)
        .map_err(|_| "MIGRATION_TARGET_AUTHORITY_MISMATCH".to_string())?;
    let config_bytes =
        read_safe_regular_file(&bridge.join("config.json"), 16 * 1024, "legacy config")?;
    let config_value: serde_json::Value = serde_json::from_slice(&config_bytes)
        .map_err(|_| "MIGRATION_TARGET_AUTHORITY_MISMATCH".to_string())?;
    let configured_url = config_value
        .get("conversation_url")
        .and_then(|value| value.as_str())
        .ok_or_else(|| "MIGRATION_TARGET_AUTHORITY_MISMATCH".to_string())?;
    let authoritative_url = canonical_wake_target(configured_url)
        .map_err(|_| "MIGRATION_TARGET_AUTHORITY_MISMATCH".to_string())?;
    if format!("{:x}", Sha256::digest(authoritative_url.as_bytes()))
        != protected_target.target_sha256
    {
        return Err("MIGRATION_TARGET_AUTHORITY_MISMATCH".into());
    }
    let root = default_root()?;
    let wake = Store::open(&root)?;
    let staged_target = reconcile_independent_target(&wake, &authoritative_url, !preflight)?;
    let current: Option<serde_json::Value> = match fs::symlink_metadata(bridge.join("owner.json")) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => return Err("LEGACY_SELECTOR_UNAVAILABLE".into()),
        Ok(_) => Some(read(&bridge.join("owner.json"))?),
    };
    let independent = current
        .as_ref()
        .is_some_and(|v| v == &serde_json::json!({"schemaVersion":1,"owner":"independent_v1"}));
    if current.as_ref().is_some_and(|v| {
        v != &serde_json::json!({"schemaVersion":1,"owner":"legacy_python"}) && !independent
    }) {
        return Err("LEGACY_OWNER_CAS_CONFLICT".into());
    }
    let state_bytes =
        read_safe_regular_file(&bridge.join("state.json"), 128 * 1024, "legacy state")?;
    let state_hash = format!("{:x}", Sha256::digest(&state_bytes));
    let config_hash = format!("{:x}", Sha256::digest(&config_bytes));
    let _state: serde_json::Value =
        serde_json::from_slice(&state_bytes).map_err(|_| "LEGACY_STATE_MALFORMED")?;
    if preflight {
        let target_state = if staged_target.is_some() {
            "CONFIGURED"
        } else {
            "STAGING_REQUIRED"
        };
        return Ok(format!(
            "PREFLIGHT_READY independentTarget={target_state} legacyStateSha256={state_hash} legacyConfigSha256={config_hash}"
        ));
    }
    let target = staged_target.ok_or("TARGET_NOT_CONFIGURED")?;
    target.validate()?;
    let audit_path = root.join("migration-v1.json");
    let audit = if audit_path.exists() {
        let audit: serde_json::Value = read(&audit_path)?;
        if audit.get("legacyStateSha256").and_then(|v| v.as_str()) != Some(state_hash.as_str())
            || audit.get("legacyConfigSha256").and_then(|v| v.as_str())
                != Some(config_hash.as_str())
        {
            return Err("MIGRATION_PRESTATE_DRIFT".into());
        }
        audit
    } else {
        if independent {
            return Err("MIGRATION_AUDIT_MISSING".into());
        }
        let audit = serde_json::json!({"schemaVersion":1,"acceptAfterUtc":now(),
            "legacyStateSha256":state_hash,"legacyConfigSha256":config_hash,
            "legacyWorkspace":workspace,"target":target,"owner":"CatDeskWake",
            "policy":"historical-events-retained-no-replay"});
        atomic(&audit_path, &audit)?;
        audit
    };
    let accept_after_utc = audit
        .get("acceptAfterUtc")
        .and_then(|v| v.as_u64())
        .filter(|v| *v > 0)
        .ok_or("MIGRATION_AUDIT_INVALID")?;
    if !independent {
        atomic(
            &bridge.join("owner.json"),
            &serde_json::json!({"schemaVersion":1,"owner":"independent_v1"}),
        )?;
    }
    // Predecessor is disabled first. An interruption leaves no browser owner;
    // re-entry completes only with the exact historical state still preserved.
    let selector: serde_json::Value = read(&bridge.join("owner.json"))?;
    if selector != serde_json::json!({"schemaVersion":1,"owner":"independent_v1"}) {
        return Err("MIGRATION_SELECTOR_READBACK_FAILED".into());
    }
    atomic(
        &root.join("activation.json"),
        &Activation {
            schema_version: 1,
            owner: "CatDeskWake".into(),
            migration_audit: "migration-v1.json".into(),
            accept_after_utc,
        },
    )?;
    Ok("INDEPENDENT_OWNER_ACTIVATED".into())
}

fn parse_selector(path: &Path) -> Result<WakeOwnerMode, String> {
    let bytes =
        read_safe_regular_file(path, MAX_OWNER_CONFIG_BYTES, "stable wake owner selection")?;
    let config: OwnerConfigV1 = serde_json::from_slice(&bytes)
        .map_err(|_| "stable wake owner selection invalid".to_string())?;
    if config.schema_version != OWNER_SCHEMA_VERSION {
        return Err("stable wake owner selection invalid".into());
    }
    match config.owner.as_str() {
        "legacy_python" => Ok(WakeOwnerMode::LegacyPython),
        "rust" => Ok(WakeOwnerMode::Rust),
        _ => Err("stable wake owner selection invalid".into()),
    }
}

fn atomic_write_selector(wake_bridge: &Path) -> Result<(), String> {
    let wake_bridge = canonical_directory(wake_bridge)?;
    let selector = wake_bridge.join("owner.json");
    if selector.exists() {
        let _ = read_safe_regular_file(
            &selector,
            MAX_OWNER_CONFIG_BYTES,
            "stable wake owner selection",
        )?;
    }
    let temp = wake_bridge.join(format!(
        ".owner-{}.tmp",
        SELECTOR_TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let payload = br#"{"schemaVersion":1,"owner":"rust"}"#;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .map_err(|_| "stable wake owner selection unavailable".to_string())?;
    file.write_all(payload)
        .and_then(|_| file.sync_all())
        .map_err(|_| "stable wake owner selection unavailable".to_string())?;
    drop(file);
    #[cfg(test)]
    if SELECTOR_FAIL_BEFORE_REPLACE.with(std::cell::Cell::get) {
        let _ = fs::remove_file(&temp);
        return Err("stable wake owner selection injected pre-replace failure".into());
    }
    atomic_replace(&temp, &selector)?;
    Ok(())
}

#[cfg(windows)]
fn atomic_replace(temp: &Path, destination: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    unsafe extern "system" {
        fn MoveFileExW(existing: *const u16, new: *const u16, flags: u32) -> i32;
    }
    const MOVEFILE_REPLACE_EXISTING: u32 = 0x1;
    const MOVEFILE_WRITE_THROUGH: u32 = 0x8;
    let existing: Vec<u16> = temp.as_os_str().encode_wide().chain(Some(0)).collect();
    let new: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    if unsafe {
        MoveFileExW(
            existing.as_ptr(),
            new.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    } == 0
    {
        let _ = fs::remove_file(temp);
        return Err("stable wake owner selection atomic replace failed".into());
    }
    Ok(())
}

#[cfg(not(windows))]
fn atomic_replace(temp: &Path, destination: &Path) -> Result<(), String> {
    fs::rename(temp, destination)
        .map_err(|_| "stable wake owner selection atomic replace failed".to_string())
}

fn wake_bridge_root(workspace: &Path) -> Result<PathBuf, String> {
    let workspace = canonical_directory(workspace)?;
    let catdesk = regular_directory_child(&workspace, ".catdesk")?;
    regular_directory_child(&catdesk, "wake-bridge")
}

fn canonical_directory(path: &Path) -> Result<PathBuf, String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|_| "stable wake owner selection unavailable".to_string())?;
    if unsafe_link(&metadata) || !metadata.is_dir() {
        return Err("stable wake owner selection unsafe".into());
    }
    fs::canonicalize(path).map_err(|_| "stable wake owner selection unavailable".to_string())
}

fn regular_directory_child(parent: &Path, name: &str) -> Result<PathBuf, String> {
    let child = parent.join(name);
    let metadata = fs::symlink_metadata(&child)
        .map_err(|_| "stable wake owner selection unavailable".to_string())?;
    if unsafe_link(&metadata) || !metadata.is_dir() {
        return Err("stable wake owner selection unsafe".into());
    }
    let canonical = fs::canonicalize(&child)
        .map_err(|_| "stable wake owner selection unavailable".to_string())?;
    if canonical.parent() != Some(parent) {
        return Err("stable wake owner selection unsafe".into());
    }
    Ok(canonical)
}

fn read_safe_regular_file(path: &Path, max: u64, label: &str) -> Result<Vec<u8>, String> {
    let metadata = fs::symlink_metadata(path).map_err(|_| format!("{label} unavailable"))?;
    if unsafe_link(&metadata) || !metadata.is_file() || metadata.len() > max {
        return Err(format!("{label} unsafe"));
    }
    let canonical = fs::canonicalize(path).map_err(|_| format!("{label} unavailable"))?;
    if canonical != path {
        return Err(format!("{label} unsafe"));
    }
    fs::read(path).map_err(|_| format!("{label} unavailable"))
}

fn unsafe_link(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    use std::{
        fs,
        sync::{Arc, Barrier},
        thread,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn digest(bytes: &[u8]) -> String {
        format!("{:x}", Sha256::digest(bytes))
    }

    fn fixture(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("catdesk-t0260-r1-{name}-{nonce}"));
        fs::create_dir_all(root.join(".catdesk/autonomy")).expect("inbox fixture");
        fs::create_dir_all(root.join(".catdesk/wake-bridge/stable-runtime-v1"))
            .expect("durable runtime fixture");
        fs::create_dir_all(root.join(".catdesk/wake-bridge/browser-profile"))
            .expect("profile fixture");
        fs::create_dir_all(root.join(".catdesk/reviewed-build-control/wake-owner-artifacts"))
            .expect("reviewed evidence fixture");

        let runtime = root.join(".catdesk/wake-bridge/stable-runtime-v1");
        let owner = b"reviewed stable owner fixture";
        let interpreter = b"reviewed durable interpreter fixture";
        let adapter = b"reviewed stable adapter fixture";
        let bridge = b"reviewed browser primitives fixture";
        fs::write(runtime.join("catdesk-stable-wake-owner.exe"), owner).expect("owner fixture");
        fs::write(runtime.join("python.exe"), interpreter).expect("interpreter fixture");
        fs::write(runtime.join("stable_wake_browser_adapter.py"), adapter)
            .expect("adapter fixture");
        fs::write(runtime.join("wake_bridge.py"), bridge).expect("bridge fixture");
        fs::write(
            root.join(".catdesk/wake-bridge/config.json"),
            br#"{"conversation_url":"https://chatgpt.com/c/fixed-thread","profile_dir":".catdesk/wake-bridge/browser-profile"}"#,
        )
        .expect("config fixture");
        fs::write(
            root.join(".catdesk/autonomy/review-inbox.json"),
            br#"[{"schemaVersion":1,"recordId":"review_1","projectId":"catdesk","sessionId":"session_1","state":"COMPLETED_VERIFIED","nextAction":"independent_final_review","reference":"artifacts/completion.json","createdAtUnix":1,"unread":true}]"#,
        )
        .expect("inbox fixture");
        let descriptor = format!(
            r#"{{"schemaVersion":1,"runtimeVersion":1,"ownerSha256":"{}","interpreterSha256":"{}","adapterSha256":"{}","browserPrimitivesSha256":"{}"}}"#,
            digest(owner),
            digest(interpreter),
            digest(adapter),
            digest(bridge),
        );
        fs::write(runtime.join("adapter-runtime.json"), &descriptor).expect("descriptor fixture");
        let evidence = format!(
            r#"{{"schemaVersion":2,"runtimeDescriptorSha256":"{}","provenanceSha256":"{}"}}"#,
            digest(descriptor.as_bytes()),
            "a".repeat(64)
        );
        fs::write(
            root.join(
                ".catdesk/reviewed-build-control/wake-owner-artifacts/reviewed-artifacts.json",
            ),
            evidence,
        )
        .expect("reviewed evidence");
        root
    }

    fn cleanup(root: PathBuf) {
        let _ = fs::remove_dir_all(root);
    }

    fn seven_legacy_pre_submit_entries() -> String {
        let entries = [
            ("legacy_idle_1", "CHATGPT_NOT_IDLE"),
            ("legacy_login_1", "LOGIN_OR_PROFILE_REQUIRED"),
            ("legacy_idle_2", "CHATGPT_NOT_IDLE"),
            ("legacy_login_2", "LOGIN_OR_PROFILE_REQUIRED"),
            ("legacy_idle_3", "CHATGPT_NOT_IDLE"),
            ("legacy_login_3", "LOGIN_OR_PROFILE_REQUIRED"),
            ("legacy_idle_4", "CHATGPT_NOT_IDLE"),
        ]
        .into_iter()
        .map(|(record_id, attention)| {
            format!(
                r#"{{"record_id":"{record_id}","status":"OPERATOR_ATTENTION","claimed_at_unix":1,"browser_sent_at_unix":null,"message_sha256":null,"target_sha256":null,"receipt_schema_version":null,"attention":"{attention}"}}"#
            )
        })
        .collect::<Vec<_>>();
        format!(
            r#"{{"schema_version":4,"deliveries":[{}],"operator_attention":null}}"#,
            entries.join(",")
        )
    }
    #[test]
    fn independent_target_staging_is_exact_and_preflight_is_non_authoritative() {
        let fixture_parent = std::env::temp_dir().join(format!(
            "catdesk-independent-wake-target-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        fs::create_dir_all(&fixture_parent).expect("fixture parent");
        let root = fixture_parent.join("store");
        let store = catdesk_wake::store::Store::open_scoped_for_test(&root, &fixture_parent)
            .expect("store");
        let canonical = "https://chatgpt.com/c/exact-thread";
        assert_eq!(
            reconcile_independent_target(&store, canonical, false).expect("preflight"),
            None
        );
        assert!(!root.join("config.json").exists());
        let staged = reconcile_independent_target(&store, canonical, true)
            .expect("stage")
            .expect("target");
        assert_eq!(staged.generation, 1);
        assert_eq!(staged.url, canonical);
        assert_eq!(
            reconcile_independent_target(&store, canonical, false).expect("readback"),
            Some(staged.clone())
        );
        assert_eq!(
            reconcile_independent_target(&store, "https://chatgpt.com/c/drift", true),
            Err("MIGRATION_TARGET_MISMATCH".into())
        );
        assert_eq!(
            store
                .config()
                .expect("config")
                .targets
                .get("catdesk")
                .expect("catdesk target"),
            &staged
        );
        cleanup(fixture_parent);
    }

    #[test]
    fn selector_has_exact_two_owner_vocabulary_and_no_legacy_alias() {
        let source = include_str!("stable_wake_owner_mode.rs")
            .split("\nmod tests {")
            .next()
            .expect("production source");
        assert!(source.contains("\"legacy_python\""));
        assert!(source.contains("\"rust\""));
        assert!(!source.contains("\"legacy\" =>"));
    }

    #[test]
    fn activation_is_not_a_browser_or_inbox_mutation_surface() {
        let source = include_str!("stable_wake_owner_mode.rs")
            .split("\nmod tests {")
            .next()
            .expect("production source");
        assert!(!source.contains("Command::new"));
        assert!(!source.contains("wake_bridge.py"));
        assert!(source.contains("canonical_inbox_records"));
        assert!(source.contains("DurableAdapterRuntime::open"));
        assert!(!source.contains("target/release"));
        assert!(!source.contains("venv/Scripts"));
    }

    #[test]
    fn fixed_owner_entrypoints_allow_no_direct_selector_or_path_authority() {
        let activation = include_str!("bin/catdesk-stable-wake-activate.rs");
        assert!(activation.contains("env::args_os().count() != 1"));
        assert!(
            activation
                .contains("activate_reviewed_rust_owner(&workspace, WakeOwnerMode::LegacyPython)")
        );
        assert!(activation.contains("RUST_OWNER_SELECTED"));
        for forbidden in [
            "--workspace",
            "owner.json",
            "Command::new",
            "wake_bridge.py",
            "state.json",
            "review-inbox.json",
        ] {
            assert!(
                !activation.contains(forbidden),
                "activation must not expose direct selector or path authority: {forbidden}"
            );
        }

        let owner = include_str!("bin/catdesk-stable-wake-owner.rs");
        assert!(owner.contains("env::args_os().count() != 1"));
        assert!(owner.contains("FixedScriptBrowserAdapter::open(&workspace)"));
        assert!(owner.contains("dispatch_if_rust_selected(&workspace"));
        for forbidden in [
            "--workspace",
            "owner.json",
            "Command::new",
            "catdesk_wake_bridge_run_once",
            "wake_bridge.py",
        ] {
            assert!(
                !owner.contains(forbidden),
                "Rust owner entrypoint must not expose alternate owner authority: {forbidden}"
            );
        }

        let legacy_bridge = include_str!("../scripts/wake_bridge.py");
        assert!(legacy_bridge.contains("def legacy_owner_selected(root: Path) -> bool:"));
        assert!(legacy_bridge.contains("value[\"owner\"] == \"legacy_python\""));
        assert!(
            !legacy_bridge.contains("value[\"owner\"] == \"rust\""),
            "legacy bridge must be ineligible once Rust is selected"
        );
    }

    #[test]
    fn activation_cas_readback_stale_replay_and_idempotence_are_executable() {
        let root = fixture("cas");
        let inbox = root.join(".catdesk/autonomy/review-inbox.json");
        let config = root.join(".catdesk/wake-bridge/config.json");
        let inbox_before = fs::read(&inbox).expect("inbox before");
        let config_before = fs::read(&config).expect("config before");
        assert_eq!(selected_owner(&root), Ok(WakeOwnerMode::LegacyPython));

        assert_eq!(
            activate_reviewed_rust_owner(&root, WakeOwnerMode::LegacyPython),
            Ok(WakeOwnerMode::Rust)
        );
        assert_eq!(selected_owner(&root), Ok(WakeOwnerMode::Rust));
        assert_eq!(
            activate_reviewed_rust_owner(&root, WakeOwnerMode::LegacyPython),
            Err("stable wake owner selection compare-and-swap stale".into())
        );
        assert_eq!(
            activate_reviewed_rust_owner(&root, WakeOwnerMode::Rust),
            Ok(WakeOwnerMode::Rust)
        );
        assert_eq!(fs::read(&inbox).expect("inbox after"), inbox_before);
        assert_eq!(fs::read(&config).expect("config after"), config_before);
        assert!(!root.join(".catdesk/wake-bridge/state.json").exists());
        cleanup(root);
    }

    #[test]
    fn activation_accepts_only_immutable_exact_legacy_pre_submit_history() {
        let root = fixture("legacy-pre-submit-history");
        let state = root.join(".catdesk/wake-bridge/state.json");
        fs::write(&state, seven_legacy_pre_submit_entries()).expect("seven-entry state");
        let inbox = root.join(".catdesk/autonomy/review-inbox.json");
        let config = root.join(".catdesk/wake-bridge/config.json");
        let before_state = fs::read(&state).expect("state before");
        let before_inbox = fs::read(&inbox).expect("inbox before");
        let before_config = fs::read(&config).expect("config before");

        assert_eq!(
            activate_reviewed_rust_owner(&root, WakeOwnerMode::LegacyPython),
            Ok(WakeOwnerMode::Rust)
        );
        assert_eq!(selected_owner(&root), Ok(WakeOwnerMode::Rust));
        assert_eq!(fs::read(&state).expect("state after"), before_state);
        assert_eq!(fs::read(&inbox).expect("inbox after"), before_inbox);
        assert_eq!(fs::read(&config).expect("config after"), before_config);
        assert_eq!(
            StableWakeDelivery::open(&root)
                .expect("delivery")
                .classify("legacy_idle_1"),
            Ok(crate::stable_wake_delivery::DeliveryClassification::SubmittingAmbiguous)
        );
        assert_eq!(
            activate_reviewed_rust_owner(&root, WakeOwnerMode::Rust),
            Ok(WakeOwnerMode::Rust)
        );
        cleanup(root);
    }

    #[test]
    fn malformed_selector_and_preflight_failures_preserve_fixture_inputs() {
        let root = fixture("negative");
        let selector = root.join(".catdesk/wake-bridge/owner.json");
        fs::write(&selector, br#"{"schemaVersion":1,"owner":"both"}"#).expect("bad selector");
        assert!(selected_owner(&root).is_err());
        assert!(activate_reviewed_rust_owner(&root, WakeOwnerMode::LegacyPython).is_err());

        fs::remove_file(&selector).expect("remove selector");
        let config = root.join(".catdesk/wake-bridge/config.json");
        let inbox = root.join(".catdesk/autonomy/review-inbox.json");
        let config_before = fs::read(&config).expect("config before");
        let inbox_before = fs::read(&inbox).expect("inbox before");
        fs::write(&config, b"not-json").expect("bad config");
        assert!(activate_reviewed_rust_owner(&root, WakeOwnerMode::LegacyPython).is_err());
        assert!(!selector.exists());
        fs::write(&config, &config_before).expect("restore config");
        fs::write(&inbox, b"not-json").expect("replace inbox");
        assert!(activate_reviewed_rust_owner(&root, WakeOwnerMode::LegacyPython).is_err());
        assert!(!selector.exists());
        fs::write(&inbox, &inbox_before).expect("restore inbox");

        let state = root.join(".catdesk/wake-bridge/state.json");
        fs::write(&state, br#"{"schemaVersion":4,"deliveries":[{"record_id":"review_1","status":"SUBMITTING","claimed_at_unix":1}],"operator_attention":null}"#).expect("submitting state");
        assert!(activate_reviewed_rust_owner(&root, WakeOwnerMode::LegacyPython).is_err());
        assert!(!selector.exists());
        cleanup(root);
    }

    #[test]
    fn two_contenders_have_one_legacy_to_rust_transition() {
        let root = fixture("concurrent");
        let gate = Arc::new(Barrier::new(3));
        let mut joins = Vec::new();
        for _ in 0..2 {
            let root = root.clone();
            let gate = Arc::clone(&gate);
            joins.push(thread::spawn(move || {
                gate.wait();
                activate_reviewed_rust_owner(&root, WakeOwnerMode::LegacyPython)
            }));
        }
        gate.wait();
        let outcomes = joins
            .into_iter()
            .map(|join| join.join().expect("contender"))
            .collect::<Vec<_>>();
        assert_eq!(
            outcomes.iter().filter(|outcome| outcome.is_ok()).count(),
            1,
            "exactly one CAS contender may cross legacy to rust: {outcomes:?}"
        );
        assert_eq!(selected_owner(&root), Ok(WakeOwnerMode::Rust));
        cleanup(root);
    }

    #[test]
    fn durable_runtime_reviewed_artifact_replacement_blocks_activation() {
        let root = fixture("artifact");
        let path =
            root.join(".catdesk/wake-bridge/stable-runtime-v1/stable_wake_browser_adapter.py");
        fs::write(&path, b"replaced adapter bytes").expect("replace durable adapter");
        assert!(
            activate_reviewed_rust_owner(&root, WakeOwnerMode::LegacyPython).is_err(),
            "replaced durable artifact must block selector CAS"
        );
        assert!(!root.join(".catdesk/wake-bridge/owner.json").exists());
        cleanup(root);
    }

    #[test]
    fn selector_fault_before_replace_preserves_parseable_old_or_new_state() {
        let root = fixture("atomic");
        let selector = root.join(".catdesk/wake-bridge/owner.json");
        fs::write(&selector, br#"{"schemaVersion":1,"owner":"legacy_python"}"#)
            .expect("legacy selector");
        SELECTOR_FAIL_BEFORE_REPLACE.with(|fault| fault.set(true));
        let result = activate_reviewed_rust_owner(&root, WakeOwnerMode::LegacyPython);
        SELECTOR_FAIL_BEFORE_REPLACE.with(|fault| fault.set(false));
        assert!(result.is_err());
        assert_eq!(selected_owner(&root), Ok(WakeOwnerMode::LegacyPython));
        assert_eq!(
            activate_reviewed_rust_owner(&root, WakeOwnerMode::LegacyPython),
            Ok(WakeOwnerMode::Rust)
        );
        assert_eq!(selected_owner(&root), Ok(WakeOwnerMode::Rust));
        cleanup(root);
    }
}
